//! From "play this" to addresses mpv can open (docs/PLAYER.md H1–H3).
//!
//! Pure lookups: nothing here plays anything, so all of it is tested against an
//! in-memory library.

use std::collections::HashMap;

use aurora_core::catchup;
use aurora_core::model::MediaKind as Kind;
use aurora_db::repo::{channels, sources};
use aurora_db::rusqlite::{Connection, OptionalExtension};
use aurora_ingest::credentials::CredentialStore;
use aurora_video::ItemKind;

use crate::error::{AppError, Result};
use crate::streams::{self, StreamRef};

/// The network buffer, seconds (README §6.1): shallow for live, where it is latency,
/// deep for a film, where it is insurance.
pub const LIVE_BUFFER_SECS: u32 = 8;
pub const FILM_BUFFER_SECS: u32 = 30;

/// What is being asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum What {
    Channel(i64),
    Movie(i64),
    Episode(i64),
    Recording(i64),
    /// A past programme, replayed from the provider's archive (README §7.5).
    Catchup {
        channel: i64,
        start: i64,
        stop: i64,
    },
}

/// What the controls say about what is playing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Label {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub kind: Option<ItemKind>,
    /// The library row: the film, the episode, the recording, or the channel.
    pub item_id: Option<i64>,
    pub channel_id: Option<i64>,
}

/// One address to try.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The channel source row, for live: its health is recorded against it.
    pub source_id: Option<i64>,
    pub url: String,
}

/// Everything needed to start, and to start again.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub what: What,
    pub label: Label,
    /// Best first. Several only for a channel with backup sources.
    pub candidates: Vec<Candidate>,
    pub live: bool,
    pub start_secs: Option<f64>,
    pub buffer_secs: u32,
    /// The provider's User-Agent and Referer, which some providers insist on (E6).
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
}

fn gone(noun: &str, more: &str) -> AppError {
    AppError::Other(format!("This {noun} is no longer in your library. {more}"))
}

fn address(url: String, noun: &str) -> Result<String> {
    if url.trim().is_empty() {
        return Err(AppError::Other(format!(
            "Your provider listed this {noun} without a playable address, so there is \
             nothing to open. A library refresh may fill it in."
        )));
    }
    Ok(url)
}

/// Work out how to play `what`.
pub fn plan(
    db: &Connection,
    credentials: &dyn CredentialStore,
    what: What,
    start_secs: Option<f64>,
    now: i64,
) -> Result<Plan> {
    let mut plan = plan_without_headers(db, credentials, what, start_secs, now)?;
    let (user_agent, referrer) = provider_headers(db, what)?;
    plan.user_agent = user_agent;
    plan.referrer = referrer;
    Ok(plan)
}

/// The provider's User-Agent and Referer for `what`. A recording is a local file.
fn provider_headers(db: &Connection, what: What) -> Result<(Option<String>, Option<String>)> {
    let (sql, id) = match what {
        What::Channel(id) | What::Catchup { channel: id, .. } => {
            ("SELECT provider_id FROM channels WHERE id = ?1", id)
        }
        What::Movie(id) => ("SELECT provider_id FROM movies WHERE id = ?1", id),
        What::Episode(id) => (
            "SELECT s.provider_id FROM episodes e JOIN series s ON s.id = e.series_id
             WHERE e.id = ?1",
            id,
        ),
        What::Recording(_) => return Ok((None, None)),
    };
    let found = db
        .query_row(
            &format!("SELECT p.user_agent, p.referrer FROM providers p WHERE p.id = ({sql})"),
            [id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()
        .map_err(aurora_db::DbError::from)?;
    let clean = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    Ok(found.map_or((None, None), |(ua, re)| (clean(ua), clean(re))))
}

fn plan_without_headers(
    db: &Connection,
    credentials: &dyn CredentialStore,
    what: What,
    start_secs: Option<f64>,
    now: i64,
) -> Result<Plan> {
    let mut access = HashMap::new();
    let start = start_secs.filter(|s| s.is_finite() && *s > 0.0);
    match what {
        What::Channel(id) => {
            let ch = channels::get(db, id)?.ok_or_else(|| {
                AppError::Other("That channel is no longer in your library.".into())
            })?;
            let mut candidates = Vec::new();
            let mut last_error = None;
            for s in sources::for_channel(db, id, now)? {
                match streams::playable(
                    db,
                    credentials,
                    &mut access,
                    Kind::Live,
                    &StreamRef::from(&s),
                ) {
                    Ok(url) if !url.trim().is_empty() => candidates.push(Candidate {
                        source_id: Some(s.id),
                        url,
                    }),
                    Ok(_) => {}
                    Err(e) => last_error = Some(e),
                }
            }
            if candidates.is_empty() {
                return Err(last_error.unwrap_or_else(|| {
                    AppError::Other(format!(
                        "{} has no stream address. A library refresh may fill it in.",
                        ch.name
                    ))
                }));
            }
            Ok(Plan {
                what,
                label: Label {
                    title: Some(ch.name),
                    subtitle: None,
                    kind: Some(ItemKind::Live),
                    item_id: Some(id),
                    channel_id: Some(id),
                },
                candidates,
                live: true,
                start_secs: None,
                buffer_secs: LIVE_BUFFER_SECS,
                user_agent: None,
                referrer: None,
            })
        }
        What::Movie(id) => {
            let row = db
                .query_row(
                    "SELECT url, shown_title(COALESCE(custom_title, tmdb_title, title)),
                            stream_id, container, provider_id
                     FROM movies WHERE id = ?1",
                    [id],
                    |r| {
                        Ok((
                            StreamRef {
                                url: r.get(0)?,
                                stream_id: r.get(2)?,
                                container: r.get(3)?,
                                provider_id: r.get(4)?,
                            },
                            r.get::<_, String>(1)?,
                        ))
                    },
                )
                .optional()
                .map_err(aurora_db::DbError::from)?;
            let (stream, title) =
                row.ok_or_else(|| gone("film", "A refresh may have removed it."))?;
            let url = address(
                streams::playable(db, credentials, &mut access, Kind::Movie, &stream)?,
                "film",
            )?;
            Ok(Plan {
                what,
                label: Label {
                    title: Some(title),
                    subtitle: None,
                    kind: Some(ItemKind::Movie),
                    item_id: Some(id),
                    channel_id: None,
                },
                candidates: vec![Candidate {
                    source_id: None,
                    url,
                }],
                live: false,
                start_secs: start,
                buffer_secs: FILM_BUFFER_SECS,
                user_agent: None,
                referrer: None,
            })
        }
        What::Episode(id) => {
            let row = db
                .query_row(
                    "SELECT e.url, e.stream_id, e.container, s.provider_id,
                            shown_title(COALESCE(s.custom_title, s.tmdb_title, s.title)),
                            e.season, e.episode, shown_title(e.title)
                     FROM episodes e JOIN series s ON s.id = e.series_id
                     WHERE e.id = ?1",
                    [id],
                    |r| {
                        Ok((
                            StreamRef {
                                url: r.get(0)?,
                                stream_id: r.get(1)?,
                                container: r.get(2)?,
                                provider_id: r.get(3)?,
                            },
                            r.get::<_, Option<String>>(4)?,
                            r.get::<_, i64>(5)?,
                            r.get::<_, i64>(6)?,
                            r.get::<_, Option<String>>(7)?,
                        ))
                    },
                )
                .optional()
                .map_err(aurora_db::DbError::from)?;
            let (stream, show, season, number, name) =
                row.ok_or_else(|| gone("episode", "A refresh may have removed it."))?;
            let url = address(
                streams::playable(db, credentials, &mut access, Kind::Episode, &stream)?,
                "episode",
            )?;
            let name = name.filter(|n| !n.trim().is_empty());
            let numbered = format!("S{season} E{number}");
            Ok(Plan {
                what,
                label: Label {
                    title: show.or_else(|| name.clone()),
                    subtitle: Some(match &name {
                        Some(n) => format!("{numbered} · {n}"),
                        None => numbered,
                    }),
                    kind: Some(ItemKind::Episode),
                    item_id: Some(id),
                    channel_id: None,
                },
                candidates: vec![Candidate {
                    source_id: None,
                    url,
                }],
                live: false,
                start_secs: start,
                buffer_secs: FILM_BUFFER_SECS,
                user_agent: None,
                referrer: None,
            })
        }
        What::Recording(id) => {
            let row: Option<(Option<String>, String, String)> = db
                .query_row(
                    "SELECT file_path, title, state FROM recordings WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(aurora_db::DbError::from)?;
            let (path, title, state) = row.ok_or_else(|| {
                gone(
                    "recording",
                    "It may have been deleted to stay under the storage quota.",
                )
            })?;
            let path = match (path.as_deref().map(str::trim), state.as_str()) {
                (Some(p), _) if !p.is_empty() => p.to_string(),
                (_, "scheduled") => {
                    return Err(AppError::Other(
                        "This recording has not started yet, so there is nothing to play.".into(),
                    ))
                }
                (_, "recording") => {
                    return Err(AppError::Other(
                        "This recording is still being written. It can be played once it \
                         finishes."
                            .into(),
                    ))
                }
                _ => {
                    return Err(AppError::Other(
                        "This recording has no file on disk. The Recordings screen says why."
                            .into(),
                    ))
                }
            };
            if !std::path::Path::new(&path).exists() {
                return Err(AppError::Other(format!(
                    "The file for this recording is gone: {path}"
                )));
            }
            Ok(Plan {
                what,
                label: Label {
                    title: Some(title),
                    subtitle: None,
                    kind: Some(ItemKind::Recording),
                    item_id: Some(id),
                    channel_id: None,
                },
                candidates: vec![Candidate {
                    source_id: None,
                    url: path,
                }],
                live: false,
                start_secs: start,
                buffer_secs: FILM_BUFFER_SECS,
                user_agent: None,
                referrer: None,
            })
        }
        What::Catchup {
            channel,
            start: from,
            stop,
        } => {
            let row = db
                .query_row(
                    "SELECT c.name, c.catchup_mode, c.catchup_source, c.catchup_days,
                            s.url, s.stream_id, s.container, c.provider_id
                     FROM channels c
                     LEFT JOIN channel_sources s ON s.channel_id = c.id
                     WHERE c.id = ?1
                     ORDER BY s.priority, s.fail_count
                     LIMIT 1",
                    [channel],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, i64>(3)?,
                            StreamRef {
                                url: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                                stream_id: r.get(5)?,
                                container: r.get(6)?,
                                provider_id: r.get(7)?,
                            },
                        ))
                    },
                )
                .optional()
                .map_err(aurora_db::DbError::from)?;
            let (name, mode, source, days, stream) = row.ok_or_else(|| {
                AppError::Other("That channel is no longer in your library.".into())
            })?;
            let Some(mode) = mode.as_deref().and_then(catchup::Mode::parse) else {
                return Err(AppError::Other(format!("{name} does not offer catch-up")));
            };
            let days = days.clamp(0, i64::from(u16::MAX)) as u16;
            if !catchup::is_available(from, now, days) {
                return Err(AppError::Other(if from > now {
                    format!("{name} has not aired that yet")
                } else {
                    format!("That programme is outside {name}'s {days}-day catch-up window")
                }));
            }
            let stream_url = if stream.url.is_empty() && stream.stream_id.is_none() {
                String::new()
            } else {
                streams::playable(db, credentials, &mut access, Kind::Live, &stream)?
            };
            let url = catchup::url_for(&catchup::Request {
                stream_url: &stream_url,
                mode,
                source: source.as_deref(),
                start: from,
                stop,
                now,
            })
            .ok_or_else(|| {
                AppError::Other(format!(
                    "{name} says it has catch-up but not how to ask for it"
                ))
            })?;
            Ok(Plan {
                what,
                label: Label {
                    title: Some(name),
                    subtitle: None,
                    kind: Some(ItemKind::Live),
                    item_id: Some(channel),
                    channel_id: Some(channel),
                },
                candidates: vec![Candidate {
                    source_id: None,
                    url,
                }],
                // A past programme has a beginning and an end: it is played like a film,
                // seekable end to end (H3).
                live: false,
                start_secs: None,
                buffer_secs: FILM_BUFFER_SECS,
                user_agent: None,
                referrer: None,
            })
        }
    }
}
