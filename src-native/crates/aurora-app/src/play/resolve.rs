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

/// Work out how to play `what` for the person watching: [`plan`], once the parental
/// gate has agreed.
///
/// The player and the mosaic come here; the recorder does not, because a recording a
/// parent scheduled must not fail because a kids profile happens to be on screen when it
/// starts. Watching that recording is judged here, by its channel.
pub fn plan_for_viewer(
    db: &Connection,
    credentials: &dyn CredentialStore,
    what: What,
    start_secs: Option<f64>,
    now: i64,
) -> Result<Plan> {
    allowed(db, what)?;
    plan(db, credentials, what, start_secs, now)
}

/// Refuse what the profile watching may not play (`aurora_db::repo::parental`).
pub fn allowed(db: &Connection, what: What) -> Result<()> {
    use aurora_db::repo::parental::{self, Target, Verdict};
    let target = match what {
        What::Channel(id) | What::Catchup { channel: id, .. } => Target::Channel(id),
        What::Movie(id) => Target::Movie(id),
        What::Episode(id) => Target::Episode(id),
        What::Recording(id) => Target::Recording(id),
    };
    match parental::check(db, target)? {
        Verdict::Allowed => Ok(()),
        Verdict::NotAllowed => Err(AppError::NotAllowed),
        Verdict::Locked => Err(AppError::Locked),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_db::rusqlite::params;
    use aurora_ingest::credentials::MemoryStore;

    const NOW: i64 = 1_760_000_000;

    fn library() -> Connection {
        let db = aurora_db::open_memory().unwrap();
        db.execute(
            "INSERT INTO providers (id,name,kind,base_url,user_agent,referrer,created_at)
             VALUES (1,'P','m3u','https://example.com','  Kodi/21  ','',0)",
            [],
        )
        .unwrap();
        db
    }

    fn channel(db: &Connection, id: i64, name: &str, url: Option<&str>) {
        db.execute(
            "INSERT INTO channels (id,provider_id,provider_key,name,match_key,last_seen_at)
             VALUES (?1,1,?2,?2,?2,0)",
            params![id, name],
        )
        .unwrap();
        if let Some(url) = url {
            db.execute(
                "INSERT INTO channel_sources (channel_id,url) VALUES (?1,?2)",
                params![id, url],
            )
            .unwrap();
        }
    }

    fn plan_of(db: &Connection, what: What) -> Result<Plan> {
        plan(db, &MemoryStore::default(), what, None, NOW)
    }

    /// The player and the mosaic ask the gate; the recorder plans without it, because a
    /// recording a parent scheduled must still run while a kids profile is on screen.
    #[test]
    fn a_locked_channel_is_refused_to_the_viewer_but_not_to_the_recorder() {
        use aurora_db::repo::parental::{self, LockKind};
        let db = library();
        channel(&db, 10, "News", Some("http://x/news.ts"));
        aurora_db::repo::profiles::set_master_pin(&db, Some("2468"), 0).unwrap();
        parental::set_lock(&db, LockKind::Channel, "10", true).unwrap();

        let viewer = plan_for_viewer(&db, &MemoryStore::default(), What::Channel(10), None, NOW);
        assert!(matches!(viewer, Err(AppError::Locked)), "{viewer:?}");
        let catchup = What::Catchup {
            channel: 10,
            start: 0,
            stop: 60,
        };
        assert!(matches!(allowed(&db, catchup), Err(AppError::Locked)));
        plan_of(&db, What::Channel(10)).expect("the recorder's plan ignores the gate");

        parental::unlock(&db, crate::now_unix()).unwrap();
        plan_for_viewer(&db, &MemoryStore::default(), What::Channel(10), None, NOW)
            .expect("the PIN opened it");
    }

    fn refusal(db: &Connection, what: What) -> String {
        match plan_of(db, what) {
            Ok(p) => panic!("planned {:?} when it should have been refused", p.what),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn a_channel_is_live_named_and_sent_with_its_providers_headers() {
        let db = library();
        channel(&db, 1, "BBC One", Some("http://example.com/1.ts"));
        let p = plan_of(&db, What::Channel(1)).unwrap();
        assert!(p.live);
        assert_eq!(p.buffer_secs, LIVE_BUFFER_SECS);
        assert_eq!(p.label.title.as_deref(), Some("BBC One"));
        assert_eq!(p.label.channel_id, Some(1));
        assert_eq!(p.candidates[0].url, "http://example.com/1.ts");
        assert!(p.candidates[0].source_id.is_some());
        // Trimmed, and a blank Referer is no Referer.
        assert_eq!(p.user_agent.as_deref(), Some("Kodi/21"));
        assert_eq!(p.referrer, None);
    }

    #[test]
    fn a_radio_channel_and_a_hidden_one_are_still_playable() {
        let db = library();
        channel(&db, 1, "Jazz FM", Some("http://example.com/j.mp3"));
        db.execute("UPDATE channels SET is_radio = 1 WHERE id = 1", [])
            .unwrap();
        channel(&db, 2, "Hidden", Some("http://example.com/h.ts"));
        db.execute("UPDATE channels SET hidden = 1 WHERE id = 2", [])
            .unwrap();
        assert!(plan_of(&db, What::Channel(1)).is_ok());
        assert!(plan_of(&db, What::Channel(2)).is_ok());
    }

    #[test]
    fn every_source_is_a_candidate_and_blank_addresses_are_left_out() {
        let db = library();
        channel(&db, 1, "Two Feeds", Some("http://example.com/a.ts"));
        for url in ["http://example.com/b.ts", "   "] {
            db.execute(
                "INSERT INTO channel_sources (channel_id,url,priority) VALUES (1,?1,5)",
                [url],
            )
            .unwrap();
        }
        let urls: Vec<String> = plan_of(&db, What::Channel(1))
            .unwrap()
            .candidates
            .into_iter()
            .map(|c| c.url)
            .collect();
        assert_eq!(urls.len(), 2, "{urls:?}");
        assert!(urls.iter().all(|u| !u.trim().is_empty()));
    }

    #[test]
    fn a_channel_with_no_address_or_none_at_all_says_which() {
        let db = library();
        channel(&db, 1, "No Source", None);
        let said = refusal(&db, What::Channel(1));
        assert!(said.contains("No Source has no stream address"), "{said}");
        let said = refusal(&db, What::Channel(99));
        assert!(said.contains("no longer in your library"), "{said}");
    }

    #[test]
    fn a_film_starts_where_asked_and_says_when_it_is_gone_or_has_no_address() {
        let db = library();
        db.execute(
            "INSERT INTO movies (id,provider_id,provider_key,title,match_key,url,last_seen_at)
             VALUES (1,1,'m1','A Film','afilm','http://example.com/f.mkv',0),
                    (2,1,'m2','No Address','noaddress','   ',0)",
            [],
        )
        .unwrap();
        let p = plan(
            &db,
            &MemoryStore::default(),
            What::Movie(1),
            Some(754.5),
            NOW,
        )
        .unwrap();
        assert!(!p.live);
        assert_eq!(p.start_secs, Some(754.5));
        assert_eq!(p.buffer_secs, FILM_BUFFER_SECS);
        assert_eq!(p.label.title.as_deref(), Some("A Film"));
        assert_eq!(p.user_agent.as_deref(), Some("Kodi/21"));
        // A nonsense start is no start.
        for bad in [f64::NAN, -3.0, 0.0] {
            let p = plan(&db, &MemoryStore::default(), What::Movie(1), Some(bad), NOW).unwrap();
            assert_eq!(p.start_secs, None, "{bad}");
        }
        assert!(refusal(&db, What::Movie(42)).contains("film"));
        assert!(!refusal(&db, What::Movie(2)).is_empty());
    }

    #[test]
    fn an_episode_is_labelled_with_its_show_and_number() {
        let db = library();
        db.execute(
            "INSERT INTO series (id,provider_id,provider_key,title,match_key,last_seen_at)
             VALUES (1,1,'s1','The Show','theshow',0)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO episodes (id,series_id,season,episode,title,url)
             VALUES (1,1,2,5,'The Fifth','http://example.com/e.mp4'),
                    (2,1,2,6,NULL,'http://example.com/e6.mp4')",
            [],
        )
        .unwrap();
        let p = plan_of(&db, What::Episode(1)).unwrap();
        assert_eq!(p.label.title.as_deref(), Some("The Show"));
        assert_eq!(p.label.subtitle.as_deref(), Some("S2 E5 · The Fifth"));
        assert_eq!(
            plan_of(&db, What::Episode(2))
                .unwrap()
                .label
                .subtitle
                .as_deref(),
            Some("S2 E6")
        );
        assert!(refusal(&db, What::Episode(9)).contains("episode"));
    }

    fn recording(db: &Connection, id: i64, state: &str, path: Option<&str>) {
        db.execute(
            "INSERT INTO recordings (id,channel_id,title,air_start,air_stop,start,stop,
                                     state,file_path,created_at)
             VALUES (?1,1,'The News',?1 * 1000,?1 * 1000 + 100,?1 * 1000,?1 * 1000 + 100,?2,?3,0)",
            params![id, state, path],
        )
        .unwrap();
    }

    #[test]
    fn a_finished_recording_plays_its_file_and_one_that_cannot_says_why() {
        let db = library();
        channel(&db, 1, "BBC One", Some("http://example.com/1.ts"));
        let file = std::env::temp_dir().join(format!("aurora-resolve-{}.ts", std::process::id()));
        std::fs::write(&file, b"ts").unwrap();
        let path = file.to_string_lossy().into_owned();
        recording(&db, 1, "completed", Some(&path));
        recording(&db, 2, "scheduled", None);
        recording(&db, 3, "recording", None);
        recording(&db, 4, "failed", None);
        recording(&db, 5, "completed", Some("/nowhere/at/all.ts"));

        let p = plan_of(&db, What::Recording(1)).unwrap();
        assert_eq!(p.candidates[0].url, path);
        assert!(!p.live);
        // A local file: no provider headers.
        assert_eq!(p.user_agent, None);

        assert!(refusal(&db, What::Recording(2)).contains("not started yet"));
        assert!(refusal(&db, What::Recording(3)).contains("still being written"));
        assert!(refusal(&db, What::Recording(4)).contains("no file on disk"));
        assert!(refusal(&db, What::Recording(5)).contains("is gone"));
        assert!(refusal(&db, What::Recording(6)).contains("recording"));
        let _ = std::fs::remove_file(file);
    }

    fn catchup_channel(db: &Connection, id: i64, mode: Option<&str>, days: i64, url: Option<&str>) {
        channel(db, id, &format!("Channel {id}"), url);
        db.execute(
            "UPDATE channels SET catchup_mode = ?2, catchup_days = ?3 WHERE id = ?1",
            params![id, mode, days],
        )
        .unwrap();
    }

    #[test]
    fn catch_up_plays_like_a_film_under_the_channels_name() {
        let db = library();
        catchup_channel(&db, 1, Some("shift"), 7, Some("http://example.com/1.ts"));
        let start = NOW - 3_600;
        let p = plan_of(
            &db,
            What::Catchup {
                channel: 1,
                start,
                stop: start + 1_800,
            },
        )
        .unwrap();
        assert!(!p.live, "a past programme is seekable end to end");
        assert_eq!(p.label.title.as_deref(), Some("Channel 1"));
        assert!(p.candidates[0].url.starts_with("http://example.com/1.ts"));
    }

    #[test]
    fn catch_up_refusals_say_which_one_it_is() {
        let db = library();
        catchup_channel(&db, 1, None, 0, Some("http://example.com/1.ts"));
        catchup_channel(&db, 2, Some("shift"), 1, Some("http://example.com/2.ts"));
        catchup_channel(&db, 3, Some("shift"), 7, None);
        let ask = |channel, start| What::Catchup {
            channel,
            start,
            stop: start + 600,
        };
        assert!(refusal(&db, ask(1, NOW - 60)).contains("does not offer catch-up"));
        assert!(refusal(&db, ask(2, NOW + 600)).contains("has not aired that yet"));
        assert!(refusal(&db, ask(2, NOW - 3 * 86_400)).contains("1-day catch-up window"));
        assert!(refusal(&db, ask(3, NOW - 60)).contains("not how to ask for it"));
        assert!(refusal(&db, ask(99, NOW - 60)).contains("no longer in your library"));
    }
}
