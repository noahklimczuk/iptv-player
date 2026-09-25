//! The refresh pipeline: fetch → parse → classify → reconcile → EPG match → index.
//!
//! README §4.6 is the governing constraint: a refresh must never blow away user data.
//! Reconciliation happens by stable provider key in `aurora_db`, and channels that
//! vanish from the provider are *reported*, not deleted.

use std::collections::HashMap;

use aurora_core::epg_match::{self, EpgIndex};
use aurora_core::model::{EpgChannel, MediaKind, PlaylistEntry, Programme};
use aurora_core::neterr::{ErrorAction, ErrorCode, NetFailure};
use aurora_core::rules::RuleSet;
use aurora_core::{series, title};
use aurora_db::repo::{channels, epg as epg_repo, library, search};
use aurora_db::rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::epg::{self, BatchingSink, BATCH_SIZE};
use crate::http::HttpClient;
use crate::playlist;
use crate::source::SourceKind;
use crate::xtream::XtreamClient;

/// Progress, so a 40,000-entry import is not a frozen spinner (README §C8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub phase: Phase,
    pub done: usize,
    /// Zero when the total is not knowable yet, e.g. while streaming an EPG.
    pub total: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Authenticating,
    FetchingPlaylist,
    ImportingChannels,
    ImportingMovies,
    ImportingSeries,
    FetchingEpg,
    MatchingEpg,
    Indexing,
    Done,
}

/// README §4.6: "142 new movies, 6 new episodes... 3 channels removed".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub channels: usize,
    pub movies: usize,
    pub series: usize,
    pub episodes: usize,
    pub epg_channels: usize,
    pub epg_programmes: usize,
    /// Channels the provider stopped listing. Kept in the library, flagged, not deleted.
    pub channels_missing: usize,
    pub epg_matched: usize,
    pub epg_unmatched: Vec<String>,
    /// Non-fatal problems worth surfacing after the refresh.
    pub warnings: Vec<String>,
    /// What the import saw and did not keep. Zero everywhere is the claim that
    /// nothing was lost; anything else says exactly what went and why.
    pub dropped: Dropped,
}

/// Everything an import saw and did not keep, and why.
///
/// A refresh that reports "22,000 channels" and an app that shows none is a gap
/// nobody can close from the outside, because the places an entry can vanish are all
/// inside this module and none of them used to leave a trace. Each field here is one
/// of those places. The rule is that every entry the provider listed is either
/// imported or counted below — never neither.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dropped {
    /// A playlist rule matched it and said hide.
    pub hidden_by_rule: usize,
    /// Its content type was switched off for this import.
    pub kind_excluded: usize,
    /// The panel listed it with no stream id, so there is no URL to play.
    pub no_stream_id: usize,
    /// An episode entry whose name carries no season/episode marker, so it cannot be
    /// placed in a show.
    pub no_episode_marker: usize,
}

impl Dropped {
    pub fn total(&self) -> usize {
        self.hidden_by_rule + self.kind_excluded + self.no_stream_id + self.no_episode_marker
    }
}

pub struct SyncOptions {
    pub provider_id: i64,
    pub source: SourceKind,
    /// Supplied by the caller from the credential store; never persisted here.
    pub password: Option<String>,
    /// Extra XMLTV URLs beyond whatever the source advertises.
    pub extra_epg_urls: Vec<String>,
    pub import_live: bool,
    pub import_vod: bool,
    pub import_series: bool,
    pub now_unix: i64,
}

impl SyncOptions {
    pub fn new(provider_id: i64, source: SourceKind, now_unix: i64) -> Self {
        Self {
            provider_id,
            source,
            password: None,
            extra_epg_urls: Vec::new(),
            import_live: true,
            import_vod: true,
            import_series: true,
            now_unix,
        }
    }
}

/// Run a full refresh. `on_progress` is called often enough to drive a progress bar.
/// Everything the network gave us, before a single row is written.
///
/// The point of this type is the boundary it draws. A refresh downloads a playlist and
/// a guide that can run to tens of megabytes, and it used to do that while holding the
/// one database connection — which froze every other command, and, because the DVR
/// scheduler takes the same lock every ten seconds to decide whether a recording is
/// due, meant a recording falling inside a long refresh simply did not start.
///
/// Splitting the phases makes that impossible to reintroduce by accident: [`fetch`]
/// cannot touch the database because it is never handed one, and [`apply`] cannot touch
/// the network for the same reason.
#[derive(Debug, Default)]
pub struct Fetched {
    live: Vec<PlaylistEntry>,
    movies: Vec<PlaylistEntry>,
    episodes: Vec<PlaylistEntry>,
    /// Shows a panel listed outright, rather than ones inferred from flat episode
    /// entries. An Xtream `get_series` call returns all of them with their artwork and
    /// plot; only the per-episode listing needs a request each.
    series: Vec<FetchedSeries>,
    epg: Vec<FetchedEpg>,
    warnings: Vec<String>,
    dropped: Dropped,
}

/// One show as the provider listed it, owned so it can outlive the HTTP client.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FetchedSeries {
    provider_key: String,
    title: String,
    year: Option<i32>,
    poster: Option<String>,
    group: Option<String>,
}

/// One guide source, parsed and waiting to be written.
#[derive(Debug, Default)]
struct FetchedEpg {
    channels: Vec<EpgChannel>,
    /// Kept batched exactly as they will be inserted, so `apply` does no regrouping.
    programmes: Vec<Vec<Programme>>,
    counted_channels: usize,
    counted_programmes: usize,
}

/// Download and parse everything, touching no database.
///
/// Rules are applied here rather than in [`apply`]: an entry the user has hidden should
/// never reach the library at all, and deciding that before the write means there is no
/// window in which it exists.
pub fn fetch(
    http: &HttpClient,
    options: &SyncOptions,
    rules: &RuleSet,
    mut on_progress: impl FnMut(Progress),
) -> Result<Fetched, NetFailure> {
    let mut out = Fetched::default();
    let mut epg_urls: Vec<String> = options.extra_epg_urls.clone();
    let mut series: Vec<FetchedSeries> = Vec::new();

    let entries = match &options.source {
        SourceKind::M3u { url } => {
            on_progress(Progress {
                phase: Phase::FetchingPlaylist,
                done: 0,
                total: 0,
            });
            let parsed = playlist::fetch(http, url)?;
            epg_urls.extend(parsed.header.epg_urls.clone());
            for w in parsed.result.warnings.iter().take(20) {
                out.warnings.push(format!("line {}: {}", w.line, w.message));
            }
            parsed.result.entries
        }
        SourceKind::Xtream { base_url, username } => {
            on_progress(Progress {
                phase: Phase::Authenticating,
                done: 0,
                total: 0,
            });
            let password = options.password.as_deref().unwrap_or_default();
            missing_sign_in(username, password)?;
            let client = XtreamClient::new(http, base_url, username, password);
            client.authenticate(options.now_unix)?;
            epg_urls.push(client.xmltv_url());

            on_progress(Progress {
                phase: Phase::FetchingPlaylist,
                done: 0,
                total: 0,
            });
            xtream_entries(
                &client,
                options,
                &mut out.warnings,
                &mut series,
                &mut out.dropped,
            )?
        }
    };

    out.series = series;

    for mut entry in entries {
        let outcome = rules.apply(&mut entry);
        if outcome.hidden {
            out.dropped.hidden_by_rule += 1;
            continue;
        }
        // Every arm accounts for its entry. The catch-all used to be `_ => {}`, which
        // is the difference between "your provider sent nothing" and "your provider
        // sent twenty thousand channels and this import threw them away" — two
        // sentences that looked identical from the sofa.
        match entry.kind {
            MediaKind::Live if options.import_live => out.live.push(entry),
            MediaKind::Movie if options.import_vod => out.movies.push(entry),
            MediaKind::Episode if options.import_series => out.episodes.push(entry),
            MediaKind::Live | MediaKind::Movie | MediaKind::Episode => {
                out.dropped.kind_excluded += 1;
            }
        }
    }

    epg_urls.sort();
    epg_urls.dedup();
    for url in &epg_urls {
        on_progress(Progress {
            phase: Phase::FetchingEpg,
            done: 0,
            total: 0,
        });
        match fetch_epg(http, url) {
            Ok(epg) => out.epg.push(epg),
            // A missing guide must not fail the whole refresh — the library is still
            // usable without it.
            Err(e) => out.warnings.push(format!(
                "EPG source failed: {} ({})",
                e.message,
                crate::http::redact(url)
            )),
        }
    }

    Ok(out)
}

/// Write what [`fetch`] downloaded, touching no network.
///
/// The caller holds the database for the whole of this, which is deliberate: these
/// writes are local and bounded, and an import that let other commands see it halfway
/// Refuse a panel import that has no sign-in to send.
///
/// Worth its own refusal because of what a panel does with a blank one. The first real
/// subscription this was pointed at answers `player_api.php` with HTTP 200 and *zero
/// bytes* when the username or the password is empty — which arrives here as "the
/// response to the account check was not valid JSON", and reads on screen as the
/// provider having sent something broken. The provider sent exactly what it was asked
/// for; the request had no password in it.
///
/// So the check is here, before the request, where the difference between "this panel is
/// misbehaving" and "Aurora has nothing to sign in with" is still known. Public because
/// the wizard's validate step needs the same refusal in the same words, one screen
/// earlier.
pub fn missing_sign_in(username: &str, password: &str) -> Result<(), NetFailure> {
    let missing = match (username.trim().is_empty(), password.is_empty()) {
        (false, false) => return Ok(()),
        (true, true) => "username or password",
        (true, false) => "username",
        (false, true) => "password",
    };
    Err(NetFailure {
        code: ErrorCode::Unauthorized,
        message: format!("Aurora has no {missing} saved for this provider"),
        cause: "Nothing was sent to sign in with, so the panel would answer with \
                nothing. Open Settings, edit the provider and enter it again — on \
                Windows the password lives in Credential Manager, and a provider added \
                before it could be written there has none."
            .into(),
        actions: vec![ErrorAction::OpenSettings],
        retryable: false,
    })
}

/// through would show a library missing its search index.
pub fn apply(
    db: &mut Connection,
    fetched: Fetched,
    options: &SyncOptions,
    mut on_progress: impl FnMut(Progress),
) -> Result<SyncReport, NetFailure> {
    // Warnings from the download carry through; everything else is counted here.
    let mut report = SyncReport {
        warnings: fetched.warnings,
        dropped: fetched.dropped,
        ..Default::default()
    };

    // ── Channels ────────────────────────────────────────────────────────────────
    on_progress(Progress {
        phase: Phase::ImportingChannels,
        done: 0,
        total: fetched.live.len(),
    });
    let keyed: Vec<(String, &PlaylistEntry)> =
        fetched.live.iter().map(|e| (provider_key(e), e)).collect();
    report.channels = channels::upsert_batch(db, options.provider_id, &keyed, options.now_unix)
        .map_err(db_failure)?;
    report.channels_missing = channels::stale(db, options.provider_id, options.now_unix)
        .map_err(db_failure)?
        .len();

    // Channel sources, so playback can resolve a URL and fail over (README §7.14).
    write_channel_sources(db, options.provider_id, &keyed).map_err(db_failure)?;
    on_progress(Progress {
        phase: Phase::ImportingChannels,
        done: fetched.live.len(),
        total: fetched.live.len(),
    });

    // ── Movies ──────────────────────────────────────────────────────────────────
    on_progress(Progress {
        phase: Phase::ImportingMovies,
        done: 0,
        total: fetched.movies.len(),
    });
    let new_movies: Vec<library::NewMovie> = fetched
        .movies
        .iter()
        .map(|e| {
            let cleaned = title::clean_movie_title(&e.name);
            library::NewMovie {
                provider_key: provider_key(e),
                title: cleaned.title.clone(),
                match_key: title::match_key(&cleaned.title),
                year: cleaned.year,
                quality: cleaned.quality.clone(),
                group: e.group.clone(),
                url: e.url.clone(),
                poster: e.logo.clone(),
                added_at: None,
            }
        })
        .collect();
    report.movies = library::upsert_movies(db, options.provider_id, &new_movies, options.now_unix)
        .map_err(db_failure)?;

    // ── Series ──────────────────────────────────────────────────────────────────
    on_progress(Progress {
        phase: Phase::ImportingSeries,
        done: 0,
        total: fetched.episodes.len() + fetched.series.len(),
    });

    // Shows the provider listed outright. These used to be fetched and dropped on the
    // floor — 28,693 of them on the subscription in docs/ROADMAP.md — so the Series
    // screen was empty on every real Xtream panel while Phase 7 was marked Done. Their
    // episodes still need one request per show and are still deferred; the rows
    // themselves come from the one call already made.
    for show in &fetched.series {
        let match_key = title::match_key(&show.title);
        library::upsert_series(
            db,
            options.provider_id,
            &library::NewSeries {
                provider_key: &show.provider_key,
                title: &show.title,
                match_key: &match_key,
                year: show.year,
                poster: show.poster.as_deref(),
                group: show.group.as_deref(),
                quality: None,
            },
            options.now_unix,
        )
        .map_err(db_failure)?;
    }
    report.series += fetched.series.len();

    let (groups, ungrouped) = series::group_series(&fetched.episodes);
    if !ungrouped.is_empty() {
        report.dropped.no_episode_marker += ungrouped.len();
        report.warnings.push(format!(
            "{} episode entries had no recognisable season/episode marker and were skipped",
            ungrouped.len()
        ));
    }
    for group in &groups {
        let match_key = title::match_key(&group.title);
        let series_id = library::upsert_series(
            db,
            options.provider_id,
            &library::NewSeries {
                provider_key: &format!("series:{match_key}"),
                title: &group.title,
                match_key: &match_key,
                year: group.year,
                poster: None,
                group: group.group.as_deref(),
                quality: group.quality.as_deref(),
            },
            options.now_unix,
        )
        .map_err(db_failure)?;

        let eps: Vec<library::NewEpisode> = group
            .seasons
            .iter()
            .flat_map(|s| {
                s.episodes.iter().map(move |e| library::NewEpisode {
                    season: s.number,
                    episode: e.number,
                    title: e.title.clone(),
                    url: e.url.clone(),
                    still: e.logo.clone(),
                })
            })
            .collect();
        report.episodes +=
            library::upsert_episodes(db, series_id, &eps, options.now_unix).map_err(db_failure)?;
    }
    report.series += groups.len();

    // ── EPG ─────────────────────────────────────────────────────────────────────
    let mut epg_channels: Vec<EpgChannel> = Vec::new();
    for source in fetched.epg {
        epg_repo::upsert_channels(db, &source.channels).map_err(db_failure)?;
        for batch in &source.programmes {
            epg_repo::insert_batch(db, batch).map_err(db_failure)?;
        }
        report.epg_channels += source.counted_channels;
        report.epg_programmes += source.counted_programmes;
        epg_channels.extend(source.channels);
    }

    // ── Matching ────────────────────────────────────────────────────────────────
    on_progress(Progress {
        phase: Phase::MatchingEpg,
        done: 0,
        total: 0,
    });
    if !epg_channels.is_empty() {
        let index = EpgIndex::build(&epg_channels);
        let rows = channels::list(
            db,
            &channels::ChannelFilter {
                include_hidden: true,
                ..Default::default()
            },
        )
        .map_err(db_failure)?;

        let manual = manual_mappings(db).map_err(db_failure)?;
        for row in &rows {
            let hit = index.resolve(
                row.tvg_id.as_deref(),
                &row.name,
                manual.get(&row.id).map(String::as_str),
            );
            match hit {
                Some(m) => {
                    channels::set_epg_mapping(
                        db,
                        row.id,
                        Some(&m.epg_channel_id),
                        Some(&format!("{:?}", m.method).to_lowercase()),
                    )
                    .map_err(db_failure)?;
                    report.epg_matched += 1;
                }
                None => {
                    if report.epg_unmatched.len() < 200 {
                        report.epg_unmatched.push(row.name.clone());
                    }
                }
            }
        }
        let coverage = epg_match::coverage(
            &index,
            rows.iter().map(|r| (r.name.as_str(), r.tvg_id.as_deref())),
        );
        tracing::info!(
            matched = coverage.matched,
            total = coverage.total,
            "EPG coverage {:.1}%",
            coverage.percent()
        );
    }

    // ── Search index ────────────────────────────────────────────────────────────
    on_progress(Progress {
        phase: Phase::Indexing,
        done: 0,
        total: 0,
    });
    reindex(db, options.provider_id).map_err(db_failure)?;
    // Language and quality are columns the list queries filter on, so they are derived
    // once here rather than per paint (README §7.3). Doing it after everything is
    // written, rather than per upsert, means one pass and one definition of the answer.
    aurora_db::repo::filtering::reclassify(db).map_err(db_failure)?;

    on_progress(Progress {
        phase: Phase::Done,
        done: 1,
        total: 1,
    });
    Ok(report)
}

/// Fetch then apply, holding the database only for the second half.
///
/// Kept as one call for tests and for any caller that has no lock to release; the host
/// calls the halves separately so a refresh never blocks the DVR (README §23).
pub fn run(
    db: &mut Connection,
    http: &HttpClient,
    options: &SyncOptions,
    rules: &RuleSet,
    mut on_progress: impl FnMut(Progress),
) -> Result<SyncReport, NetFailure> {
    let fetched = fetch(http, options, rules, &mut on_progress)?;
    apply(db, fetched, options, on_progress)
}

fn xtream_entries(
    client: &XtreamClient<'_>,
    options: &SyncOptions,
    warnings: &mut Vec<String>,
    series: &mut Vec<FetchedSeries>,
    dropped: &mut Dropped,
) -> Result<Vec<PlaylistEntry>, NetFailure> {
    let mut out = Vec::new();

    let category_names = |cats: Vec<aurora_core::xtream::Category>| {
        cats.into_iter()
            .filter_map(|c| Some((c.category_id?, c.category_name.unwrap_or_default())))
            .collect::<HashMap<String, String>>()
    };

    if options.import_live {
        let cats = category_names(client.live_categories()?);
        for s in client.live_streams()? {
            let Some(id) = s.stream_id else {
                dropped.no_stream_id += 1;
                continue;
            };
            let mut entry = PlaylistEntry::new(
                s.name.clone().unwrap_or_else(|| format!("Channel {id}")),
                client.stream_url(MediaKind::Live, id, None),
            );
            entry.kind = MediaKind::Live;
            entry.tvg_id = s.epg_channel_id.clone();
            entry.logo = s.stream_icon.clone();
            entry.number = s.num;
            entry.group = s.category_id.as_ref().and_then(|c| cats.get(c).cloned());
            if s.has_catchup() {
                entry.catchup = Some(aurora_core::model::Catchup {
                    mode: "xc".into(),
                    source: None,
                    days: s.tv_archive_duration.unwrap_or(7) as u16,
                });
            }
            out.push(entry);
        }
    }

    if options.import_vod {
        let cats = category_names(client.vod_categories()?);
        for s in client.vod_streams()? {
            let Some(id) = s.stream_id else {
                dropped.no_stream_id += 1;
                continue;
            };
            let mut entry = PlaylistEntry::new(
                s.name.clone().unwrap_or_else(|| format!("Movie {id}")),
                client.stream_url(MediaKind::Movie, id, s.container_extension.as_deref()),
            );
            entry.kind = MediaKind::Movie;
            entry.logo = s.stream_icon.clone();
            entry.group = s.category_id.as_ref().and_then(|c| cats.get(c).cloned());
            out.push(entry);
        }
    }

    if options.import_series {
        let cats = category_names(client.series_categories()?);
        for listing in client.series()? {
            let Some(id) = listing.series_id else {
                dropped.no_stream_id += 1;
                continue;
            };
            let raw = listing
                .name
                .clone()
                .unwrap_or_else(|| format!("Series {id}"));
            // Same cleaning films get: a panel's titles carry release tags and a year,
            // and the cleaned form is what enrichment sends to TMDB.
            let cleaned = title::clean_movie_title(&raw);
            series.push(FetchedSeries {
                provider_key: format!("series:{id}"),
                title: cleaned.title,
                year: cleaned
                    .year
                    .or_else(|| year_from(listing.release_date.as_deref())),
                poster: listing.cover.clone().filter(|c| !c.trim().is_empty()),
                group: listing
                    .category_id
                    .as_ref()
                    .and_then(|c| cats.get(c).cloned()),
            });
        }
        if !series.is_empty() {
            warnings.push(format!(
                "{} series imported; their episode listings need one request each and \
                 are fetched when a show is opened",
                series.len()
            ));
        }
    }

    Ok(out)
}

/// The year out of an Xtream `releaseDate`, which panels write as `2019-04-14`,
/// `2019` or nothing at all.
fn year_from(release_date: Option<&str>) -> Option<i32> {
    let raw = release_date?.trim();
    let head: String = raw.chars().take_while(|c| c.is_ascii_digit()).collect();
    let year: i32 = head.parse().ok()?;
    (1880..=2200).contains(&year).then_some(year)
}

/// Download and parse one guide source. Writes nothing.
fn fetch_epg(http: &HttpClient, url: &str) -> Result<FetchedEpg, NetFailure> {
    // Two passes would mean two downloads, so channels are collected in memory (a few
    // thousand small rows) while programmes stream straight through to SQLite.
    let mut channel_buf: Vec<EpgChannel> = Vec::new();
    let mut programme_batches: Vec<Vec<Programme>> = Vec::new();

    let mut sink = BatchingSink::new(
        |chans: &[EpgChannel]| {
            channel_buf.extend_from_slice(chans);
            Ok(())
        },
        |progs: &[Programme]| {
            programme_batches.push(progs.to_vec());
            Ok(())
        },
        BATCH_SIZE,
    );

    let stats = epg::fetch_into(http, url, &mut sink)?;
    sink.finish();
    if let Some(e) = sink.error.clone() {
        return Err(NetFailure::classify(&e));
    }
    drop(sink);

    Ok(FetchedEpg {
        channels: channel_buf,
        programmes: programme_batches,
        counted_channels: stats.channels,
        counted_programmes: stats.programmes,
    })
}

fn write_channel_sources(
    db: &mut Connection,
    provider_id: i64,
    entries: &[(String, &PlaylistEntry)],
) -> aurora_db::Result<()> {
    let tx = db.transaction()?;
    {
        let mut lookup =
            tx.prepare("SELECT id FROM channels WHERE provider_id = ?1 AND provider_key = ?2")?;
        let mut insert = tx.prepare(
            "INSERT INTO channel_sources (channel_id, url, priority, quality)
             SELECT ?1, ?2, 0, ?3
             WHERE NOT EXISTS (
                 SELECT 1 FROM channel_sources WHERE channel_id = ?1 AND url = ?2
             )",
        )?;
        for (key, entry) in entries {
            let id: i64 = match lookup
                .query_row(aurora_db::rusqlite::params![provider_id, key], |r| r.get(0))
            {
                Ok(id) => id,
                Err(_) => continue,
            };
            insert.execute(aurora_db::rusqlite::params![
                id,
                entry.url,
                title::detect_quality(&entry.name)
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn manual_mappings(db: &Connection) -> aurora_db::Result<HashMap<i64, String>> {
    let mut stmt = db.prepare("SELECT channel_id, epg_channel_id FROM epg_manual_map")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .collect::<std::result::Result<HashMap<_, _>, _>>()?;
    Ok(rows)
}

/// Rebuild the FTS rows for this provider's content.
fn reindex(db: &mut Connection, _provider_id: i64) -> aurora_db::Result<()> {
    for kind in ["channel", "movie", "series"] {
        search::clear_kind(db, kind)?;
    }

    let mut rows: Vec<(String, i64, String, Option<String>)> = Vec::new();
    {
        let mut stmt = db.prepare(
            "SELECT id, COALESCE(custom_name, name), group_title FROM channels WHERE hidden = 0",
        )?;
        for row in stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })? {
            let (id, name, group) = row?;
            rows.push(("channel".into(), id, name, group));
        }

        let mut stmt = db.prepare("SELECT id, title, year FROM movies")?;
        for row in stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
            ))
        })? {
            let (id, t, year) = row?;
            rows.push(("movie".into(), id, t, year.map(|y| y.to_string())));
        }

        let mut stmt = db.prepare("SELECT id, title, year FROM series")?;
        for row in stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
            ))
        })? {
            let (id, t, year) = row?;
            rows.push(("series".into(), id, t, year.map(|y| y.to_string())));
        }
    }
    search::index(db, &rows)?;
    Ok(())
}

/// A stable identity for an entry across refreshes. Xtream stream ids are embedded in
/// the URL; for plain playlists the URL itself is the only stable thing there is.
fn provider_key(entry: &PlaylistEntry) -> String {
    let path = entry.url.split(['?', '#']).next().unwrap_or(&entry.url);
    for marker in ["/live/", "/movie/", "/series/"] {
        if let Some(idx) = path.find(marker) {
            if let Some(last) = path[idx..].rsplit('/').next() {
                let id = last.split('.').next().unwrap_or(last);
                if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
                    return format!("{}{id}", marker.trim_matches('/'));
                }
            }
        }
    }
    path.to_string()
}

fn db_failure(e: aurora_db::DbError) -> NetFailure {
    let mut failure = NetFailure::classify("database");
    failure.message = "Aurora could not write to its library".into();
    failure.cause = e.to_string();
    failure.retryable = false;
    failure
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::HttpConfig;
    use crate::testserver::{Reply, TestServer};
    use aurora_core::rules::{Action, Field, Match, Rule};

    fn http() -> HttpClient {
        HttpClient::new(HttpConfig {
            max_attempts: 1,
            ..Default::default()
        })
        .unwrap()
    }

    fn db() -> Connection {
        let conn = aurora_db::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO profiles (id,name,created_at) VALUES (1,'Me',0)",
            [],
        )
        .unwrap();
        conn
    }

    const PLAYLIST: &str = "#EXTM3U\n\
        #EXTINF:-1 tvg-id=\"cnn.us\" tvg-chno=\"202\" group-title=\"News\",CNN HD\n\
        http://example.com/live/u/p/101.ts\n\
        #EXTINF:-1 tvg-chno=\"101\" group-title=\"News\",BBC One\n\
        http://example.com/live/u/p/102.ts\n\
        #EXTINF:-1 group-title=\"Movies\",Inception (2010) 1080p\n\
        http://example.com/movie/u/p/900.mkv\n\
        #EXTINF:-1 group-title=\"Series\",Breaking Bad S01E01\n\
        http://example.com/series/u/p/500.mkv\n\
        #EXTINF:-1 group-title=\"Series\",Breaking Bad S01E02\n\
        http://example.com/series/u/p/501.mkv\n\
        #EXTINF:-1 group-title=\"XXX Adult\",Blocked Channel\n\
        http://example.com/live/u/p/999.ts\n";

    /// Shaped like a real subscription: the same channel twice at two qualities, a
    /// tagged foreign one, and a film whose quality is only in its name.
    const MIXED: &str = "#EXTM3U\n\
        #EXTINF:-1 group-title=\"News\",CNN HD\n\
        http://example.com/live/u/p/1.ts\n\
        #EXTINF:-1 group-title=\"News\",CNN FHD\n\
        http://example.com/live/u/p/2.ts\n\
        #EXTINF:-1 tvg-language=\"French\" group-title=\"France\",TF1\n\
        http://example.com/live/u/p/3.ts\n\
        #EXTINF:-1 group-title=\"AR | Arabic\",MBC 1\n\
        http://example.com/live/u/p/4.ts\n\
        #EXTINF:-1 group-title=\"Movies\",Dune (2021) 2160p\n\
        http://example.com/movie/u/p/9.mkv\n\
        #EXTINF:-1 group-title=\"Series | FHD\",Severance S01E01 1080p\n\
        http://example.com/series/u/p/7.mkv\n";

    const EPG: &str = r#"<tv>
        <channel id="cnn.us"><display-name>CNN</display-name></channel>
        <channel id="bbcone.uk"><display-name>BBC One</display-name></channel>
        <programme start="20240115120000 +0000" stop="20240115130000 +0000" channel="cnn.us">
          <title>World News</title></programme>
        <programme start="20240115130000 +0000" stop="20240115140000 +0000" channel="bbcone.uk">
          <title>The News</title></programme>
      </tv>"#;

    fn options(server: &TestServer) -> SyncOptions {
        SyncOptions::new(
            1,
            SourceKind::M3u {
                url: server.url("/list.m3u"),
            },
            1_705_320_000,
        )
    }

    /// Serve the playlist on /list.m3u and the guide on /epg.xml.
    fn combined_server() -> TestServer {
        TestServer::start(|_, req| {
            if req.path.starts_with("/epg") {
                Reply::ok(EPG)
            } else {
                Reply::ok(PLAYLIST)
            }
        })
    }

    fn no_rules() -> RuleSet {
        RuleSet::compile(&[]).unwrap()
    }

    fn gzip(data: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    /// One Xtream panel, answering however the test says.
    fn xtream_options(server: &TestServer) -> SyncOptions {
        let mut opts = SyncOptions::new(
            1,
            SourceKind::Xtream {
                base_url: server.url(""),
                username: "u".into(),
            },
            1_705_320_000,
        );
        opts.password = Some("p".into());
        opts
    }

    /* ── What a panel sends when something is wrong ──────────────────────────── */

    /// The classic: a panel behind a reverse proxy answering HTTP 200 with an HTML
    /// error page. `serde_json` on that produces something unreadable; the viewer
    /// needs to be told the provider is misbehaving, not shown a parser message.
    #[test]
    fn an_html_error_page_served_as_200_is_reported_as_the_provider_misbehaving() {
        let server = TestServer::always(
            Reply::ok("<!DOCTYPE html><html><body><h1>502 Bad Gateway</h1></body></html>")
                .with_header("Content-Type", "text/html"),
        );
        let mut conn = db();
        let opts = xtream_options(&server);

        let err = run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap_err();
        assert!(
            !err.message.to_lowercase().contains("expected value"),
            "a serde message reached the viewer: {}",
            err.message
        );
        assert!(!err.message.is_empty());
        // Nothing was written on the way to failing.
        assert_eq!(
            conn.query_row("SELECT count(*) FROM channels", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    /// Every HTTP status a panel is known to answer with, none of which may write a
    /// partial library or produce a message with the password in it.
    #[test]
    fn every_provider_error_status_is_refused_cleanly() {
        for status in [401, 403, 404, 429, 500, 502, 503] {
            let server = TestServer::always(Reply::status(status));
            let mut conn = db();
            let opts = xtream_options(&server);

            let err = run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap_err();
            assert!(
                !err.message.contains('p') || !err.message.contains("password=p"),
                "HTTP {status} leaked the password: {}",
                err.message
            );
            assert!(
                !err.cause.contains("password=p"),
                "HTTP {status}: {}",
                err.cause
            );
            assert_eq!(
                conn.query_row("SELECT count(*) FROM channels", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                0,
                "HTTP {status} wrote something"
            );
        }
    }

    /// An expired account answers 200 with a perfectly valid body saying no.
    #[test]
    fn an_expired_account_is_refused_before_anything_is_written() {
        let server = TestServer::always(Reply::ok(
            r#"{"user_info":{"username":"u","status":"Expired","exp_date":"1700000000"}}"#,
        ));
        let mut conn = db();
        let err = run(
            &mut conn,
            &http(),
            &xtream_options(&server),
            &no_rules(),
            |_| {},
        )
        .unwrap_err();
        assert!(!err.message.is_empty());
        assert_eq!(
            conn.query_row("SELECT count(*) FROM channels", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    /// A panel that answers the account check and then hands back nonsense for the
    /// catalogue must not take the library down with it.
    #[test]
    fn a_broken_catalogue_response_does_not_destroy_what_is_there() {
        let server = TestServer::start(|_, req| {
            if req.path.contains("action=get_live_streams") {
                Reply::ok("{ this is not json at all")
            } else if req.path.contains("action=") {
                Reply::ok("[]")
            } else {
                Reply::ok(r#"{"user_info":{"username":"u","status":"Active"}}"#)
            }
        });
        let mut conn = db();
        // Something already imported, which a failed refresh must leave alone.
        conn.execute(
            "INSERT INTO channels (provider_id, provider_key, name, match_key, last_seen_at)
             VALUES (1, 'existing', 'Already Here', 'alreadyhere', 0)",
            [],
        )
        .unwrap();

        assert!(run(
            &mut conn,
            &http(),
            &xtream_options(&server),
            &no_rules(),
            |_| {}
        )
        .is_err());
        let name: String = conn
            .query_row("SELECT name FROM channels", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "Already Here", "a failed refresh emptied the library");
    }

    /// The fields a panel gets wrong: numbers as strings, nulls where a string is
    /// expected, arrays where a scalar is. The lenient deserializers exist for this;
    /// this is the test that says so at the pipeline level.
    #[test]
    fn a_panel_that_types_its_json_loosely_still_imports() {
        let server = TestServer::start(|_, req| {
            let p = &req.path;
            if p.contains("action=get_live_categories") {
                Reply::ok(r#"[{"category_id":1,"category_name":null}]"#)
            } else if p.contains("action=get_live_streams") {
                Reply::ok(
                    r#"[{"stream_id":"101","name":"CNN","num":"202","category_id":1,
                         "tv_archive":"1","tv_archive_duration":"3"},
                        {"stream_id":102,"name":null},
                        {"name":"No id at all"}]"#,
                )
            } else if p.contains("action=") {
                Reply::ok("[]")
            } else if p.contains("xmltv.php") {
                Reply::ok(EPG)
            } else {
                Reply::ok(
                    r#"{"user_info":{"username":"u","status":"Active",
                              "max_connections":"2","active_cons":"0"}}"#,
                )
            }
        });
        let mut conn = db();

        let report = run(
            &mut conn,
            &http(),
            &xtream_options(&server),
            &no_rules(),
            |_| {},
        )
        .unwrap();
        // The two with a stream id arrive; the one without is skipped.
        assert_eq!(report.channels, 2);

        let rows = channels::list(&conn, &channels::ChannelFilter::default()).unwrap();
        let cnn = rows.iter().find(|c| c.name == "CNN").unwrap();
        assert_eq!(cnn.number, Some(202), "a stringy number did not survive");
        assert!(cnn.has_catchup, "a stringy tv_archive did not survive");
    }

    /* ── Guides, as they are actually served ────────────────────────────────── */

    /// Providers serve guides gzipped, and say so in two different ways: a `.gz`
    /// filename, or a content type on a `.php` endpoint. Both have to inflate.
    #[test]
    fn a_gzipped_guide_imports_whichever_way_the_provider_announces_it() {
        for (path, header) in [
            ("/epg.xml.gz", None),
            ("/xmltv.php", Some("application/gzip")),
        ] {
            let body = gzip(EPG.as_bytes());
            let server = TestServer::always(match header {
                Some(h) => Reply::ok(body).with_header("Content-Type", h),
                None => Reply::ok(body),
            });

            let mut conn = db();
            let mut opts = SyncOptions::new(
                1,
                SourceKind::M3u {
                    url: server.url("/playlist.m3u"),
                },
                1_705_320_000,
            );
            // The playlist is the same server, so it answers gzip too — which is fine,
            // since the EPG url is what this is about.
            opts.extra_epg_urls = vec![server.url(path)];

            let report = run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();
            assert!(
                report.epg_programmes > 0,
                "{path} produced no programmes: {report:?}"
            );
        }
    }

    /// Concatenated gzip members are legal, and some providers build their dumps that
    /// way — one member per source, appended.
    #[test]
    fn a_guide_built_from_concatenated_gzip_members_imports_whole() {
        let mut body = gzip(b"<tv><channel id=\"a\"><display-name>A</display-name></channel>");
        body.extend(gzip(
            b"<programme channel=\"a\" start=\"20240115120000 +0000\" \
              stop=\"20240115130000 +0000\"><title>Split across members</title></programme></tv>",
        ));

        let server = TestServer::always(Reply::ok(body));
        let mut conn = db();
        let mut opts = SyncOptions::new(
            1,
            SourceKind::M3u {
                url: server.url("/playlist.m3u"),
            },
            1_705_320_000,
        );
        opts.extra_epg_urls = vec![server.url("/epg.xml.gz")];

        let report = run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();
        assert_eq!(report.epg_programmes, 1, "{report:?}");
    }

    /// A guide that fails must not fail the refresh: the library is still usable
    /// without one, and a channel list that vanished because an EPG 404'd would be a
    /// much worse outcome than a missing guide.
    #[test]
    fn a_guide_that_will_not_load_costs_a_warning_not_the_import() {
        let server = TestServer::start(|_, req| {
            if req.path.contains("epg") {
                Reply::status(404)
            } else {
                Reply::ok("#EXTM3U\n#EXTINF:-1,CNN\nhttp://example.com/1.ts\n")
            }
        });
        let mut conn = db();
        let mut opts = SyncOptions::new(
            1,
            SourceKind::M3u {
                url: server.url("/playlist.m3u"),
            },
            1_705_320_000,
        );
        opts.extra_epg_urls = vec![server.url("/epg.xml")];

        let report = run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();
        assert_eq!(report.channels, 1);
        assert_eq!(report.epg_programmes, 0);
        assert!(
            report.warnings.iter().any(|w| w.contains("EPG")),
            "{:?}",
            report.warnings
        );
    }

    #[test]
    fn fetching_is_not_given_a_database_and_applying_is_not_given_a_network() {
        // The guarantee is structural, not a habit: `fetch` never receives a Connection
        // and `apply` never receives an HttpClient, so a future edit cannot put a
        // download back under the lock without changing a signature and reading why.
        let server = combined_server();
        let mut opts = options(&server);
        opts.extra_epg_urls = vec![server.url("/epg.xml")];

        let fetched = fetch(&http(), &opts, &no_rules(), |_| {}).expect("fetch");
        assert!(!fetched.live.is_empty(), "the playlist was downloaded");
        assert!(!fetched.epg.is_empty(), "the guide was downloaded");

        // Everything below happens with the server already shut down, which is the
        // proof that applying needs no network at all.
        drop(server);

        let mut conn = db();
        let report = apply(&mut conn, fetched, &opts, |_| {}).expect("apply");
        assert!(report.channels > 0);
        assert!(report.epg_programmes > 0, "the guide reached the database");
    }

    #[test]
    fn a_refresh_leaves_the_database_alone_while_it_downloads() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        // A guide that takes its time, so there is a window to compete for.
        let server = TestServer::start(|_, req| {
            std::thread::sleep(std::time::Duration::from_millis(30));
            if req.path.starts_with("/epg") {
                Reply::ok(EPG)
            } else {
                Reply::ok(PLAYLIST)
            }
        });
        let mut opts = options(&server);
        opts.extra_epg_urls = vec![server.url("/epg.xml")];

        // Stand in for the DVR thread, which takes this lock every ten seconds to ask
        // whether a recording is due.
        let conn = Arc::new(std::sync::Mutex::new(db()));
        let acquired = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let contender = std::thread::spawn({
            let conn = Arc::clone(&conn);
            let acquired = Arc::clone(&acquired);
            let stop = Arc::clone(&stop);
            move || {
                while !stop.load(Ordering::Relaxed) {
                    if let Ok(guard) = conn.try_lock() {
                        drop(guard);
                        acquired.fetch_add(1, Ordering::Relaxed);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
        });

        // The shape the host uses: download with nothing held, then write.
        let fetched = fetch(&http(), &opts, &no_rules(), |_| {}).expect("fetch");
        {
            let mut guard = conn.lock().expect("lock");
            apply(&mut guard, fetched, &opts, |_| {}).expect("apply");
        }

        stop.store(true, Ordering::Relaxed);
        contender.join().unwrap();

        // Several requests at 30ms each is a long window; a contender polling every few
        // milliseconds must have got in repeatedly. Holding the lock for the download
        // would have shut it out almost entirely.
        assert!(
            acquired.load(Ordering::Relaxed) > 3,
            "the DVR would have been locked out: only {} acquisitions",
            acquired.load(Ordering::Relaxed)
        );
    }

    #[test]
    fn an_import_classifies_language_and_quality_for_every_list() {
        let server = TestServer::start(|_, _| Reply::ok(MIXED));
        let mut conn = db();
        let opts = options(&server);

        run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();

        // Live: the reported language wins, the group answers for the untagged one, and
        // an English-looking name stays unknown rather than being guessed at.
        let rows: Vec<(String, Option<String>, i64)> = {
            let mut stmt = conn
                .prepare("SELECT name, lang_code, quality_rank FROM channels ORDER BY name")
                .unwrap();
            let out = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap();
            out
        };
        let by_name = |n: &str| rows.iter().find(|r| r.0 == n).cloned().unwrap();
        assert_eq!(by_name("CNN HD").1, None);
        assert_eq!(by_name("CNN HD").2, 2);
        assert_eq!(by_name("CNN FHD").2, 3);
        assert_eq!(by_name("TF1").1.as_deref(), Some("fr"));
        assert_eq!(by_name("MBC 1").1.as_deref(), Some("ar"));

        // Movies: the quality is in the title, and the year is not mistaken for one.
        let (quality, rank): (Option<String>, i64) = conn
            .query_row("SELECT quality, quality_rank FROM movies", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(quality.as_deref(), Some("4K"));
        assert_eq!(rank, 4);

        // Series: the quality comes off the episode entries, the group off the category.
        let (group, quality, rank): (Option<String>, Option<String>, i64) = conn
            .query_row(
                "SELECT group_title, quality, quality_rank FROM series",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(group.as_deref(), Some("Series | FHD"));
        assert_eq!(quality.as_deref(), Some("FHD"));
        assert_eq!(rank, 3);
    }

    #[test]
    fn the_filters_work_on_what_an_import_produced() {
        use aurora_db::repo::filtering::LibraryFilter;
        let server = TestServer::start(|_, _| Reply::ok(MIXED));
        let mut conn = db();
        let opts = options(&server);
        run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();

        let both = LibraryFilter {
            english_only: true,
            hide_duplicates: true,
        };
        let rows = channels::list(
            &conn,
            &channels::ChannelFilter {
                library: both,
                ..Default::default()
            },
        )
        .unwrap();
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        // One CNN, the better one; no TF1, no MBC.
        assert_eq!(names, vec!["CNN FHD"], "{names:?}");
    }

    #[test]
    fn imports_a_playlist_end_to_end() {
        let server = combined_server();
        let mut conn = db();
        let mut opts = options(&server);
        opts.extra_epg_urls = vec![server.url("/epg.xml")];

        let report = run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();

        assert_eq!(
            report.channels, 3,
            "three live entries including the adult one"
        );
        assert_eq!(report.movies, 1);
        assert_eq!(report.series, 1);
        assert_eq!(report.episodes, 2);
        assert_eq!(report.epg_programmes, 2);

        let rows = channels::list(&conn, &channels::ChannelFilter::default()).unwrap();
        assert_eq!(rows.len(), 3);
        // Ordered by channel number.
        assert_eq!(rows[0].name, "BBC One");
        assert_eq!(rows[0].number, Some(101));
    }

    #[test]
    fn epg_is_matched_onto_channels() {
        let server = combined_server();
        let mut conn = db();
        let mut opts = options(&server);
        opts.extra_epg_urls = vec![server.url("/epg.xml")];

        let report = run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();

        // CNN matches by tvg-id, BBC One by normalized name.
        assert_eq!(
            report.epg_matched, 2,
            "unmatched: {:?}",
            report.epg_unmatched
        );

        let rows = channels::list(&conn, &channels::ChannelFilter::default()).unwrap();
        let cnn = rows.iter().find(|r| r.name == "CNN HD").unwrap();
        assert_eq!(cnn.epg_channel_id.as_deref(), Some("cnn.us"));
    }

    #[test]
    fn rules_are_applied_before_anything_is_written() {
        let server = combined_server();
        let mut conn = db();
        let rules = RuleSet::compile(&[Rule::new(
            Field::Group,
            Match::Contains,
            "adult",
            Action::Hide,
        )])
        .unwrap();

        let report = run(&mut conn, &http(), &options(&server), &rules, |_| {}).unwrap();
        assert_eq!(
            report.channels, 2,
            "the adult channel must never reach the library"
        );

        let rows = channels::list(
            &conn,
            &channels::ChannelFilter {
                include_hidden: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!rows.iter().any(|r| r.name == "Blocked Channel"));
    }

    #[test]
    fn a_refresh_preserves_user_edits() {
        let server = combined_server();
        let mut conn = db();
        run(&mut conn, &http(), &options(&server), &no_rules(), |_| {}).unwrap();

        let id = channels::list(&conn, &channels::ChannelFilter::default())
            .unwrap()
            .iter()
            .find(|r| r.name == "CNN HD")
            .unwrap()
            .id;
        channels::rename(&conn, id, Some("My News")).unwrap();
        channels::renumber(&conn, id, Some(7)).unwrap();
        channels::set_hidden(&conn, id, true).unwrap();

        // Second refresh, same provider.
        let mut opts = options(&server);
        opts.now_unix += 3600;
        run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();

        let rows = channels::list(
            &conn,
            &channels::ChannelFilter {
                include_hidden: true,
                ..Default::default()
            },
        )
        .unwrap();
        let mine = rows.iter().find(|r| r.id == id).unwrap();
        assert_eq!(mine.name, "My News", "rename must survive a refresh");
        assert_eq!(mine.number, Some(7), "number must survive a refresh");
        assert!(mine.hidden, "hidden flag must survive a refresh");
        assert_eq!(rows.len(), 3, "refresh must not duplicate channels");
    }

    #[test]
    fn channels_dropped_by_the_provider_are_reported_not_deleted() {
        let shrunk = "#EXTM3U\n#EXTINF:-1,CNN HD\nhttp://example.com/live/u/p/101.ts\n";
        let server = TestServer::start(move |i, _| {
            if i == 0 {
                Reply::ok(PLAYLIST)
            } else {
                Reply::ok(shrunk)
            }
        });
        let mut conn = db();
        run(&mut conn, &http(), &options(&server), &no_rules(), |_| {}).unwrap();

        let mut opts = options(&server);
        opts.now_unix += 3600;
        let report = run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();

        assert!(
            report.channels_missing >= 2,
            "vanished channels should be counted"
        );
        let rows = channels::list(
            &conn,
            &channels::ChannelFilter {
                include_hidden: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            rows.len(),
            3,
            "they stay in the library so favorites survive"
        );
    }

    #[test]
    fn a_failing_epg_source_does_not_fail_the_whole_refresh() {
        let server = TestServer::start(|_, req| {
            if req.path.starts_with("/epg") {
                Reply::status(500)
            } else {
                Reply::ok(PLAYLIST)
            }
        });
        let mut conn = db();
        let mut opts = options(&server);
        opts.extra_epg_urls = vec![server.url("/epg.xml")];

        let report = run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();
        assert_eq!(report.channels, 3, "the library still imports");
        assert_eq!(report.epg_programmes, 0);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("EPG source failed")),
            "{:?}",
            report.warnings
        );
    }

    #[test]
    fn a_failing_playlist_aborts_with_a_readable_error() {
        let server = TestServer::always(Reply::status(401));
        let mut conn = db();
        let err = run(&mut conn, &http(), &options(&server), &no_rules(), |_| {}).unwrap_err();
        assert_eq!(err.code, aurora_core::neterr::ErrorCode::Unauthorized);
    }

    #[test]
    fn progress_runs_through_the_phases_and_ends_done() {
        let server = combined_server();
        let mut conn = db();
        let mut phases = Vec::new();
        run(&mut conn, &http(), &options(&server), &no_rules(), |p| {
            phases.push(p.phase)
        })
        .unwrap();

        assert_eq!(phases.first(), Some(&Phase::FetchingPlaylist));
        assert_eq!(phases.last(), Some(&Phase::Done));
        for expected in [
            Phase::ImportingChannels,
            Phase::ImportingMovies,
            Phase::Indexing,
        ] {
            assert!(
                phases.contains(&expected),
                "missing {expected:?} in {phases:?}"
            );
        }
    }

    #[test]
    fn search_finds_imported_content() {
        let server = combined_server();
        let mut conn = db();
        run(&mut conn, &http(), &options(&server), &no_rules(), |_| {}).unwrap();

        assert!(!search::query(&conn, "cnn", 10).unwrap().is_empty());
        assert!(!search::query(&conn, "inception", 10).unwrap().is_empty());
        assert!(!search::query(&conn, "breaking", 10).unwrap().is_empty());
    }

    #[test]
    fn reindexing_does_not_duplicate_search_rows() {
        let server = combined_server();
        let mut conn = db();
        run(&mut conn, &http(), &options(&server), &no_rules(), |_| {}).unwrap();
        let first = search::query(&conn, "cnn", 10).unwrap().len();

        let mut opts = options(&server);
        opts.now_unix += 3600;
        run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();
        assert_eq!(search::query(&conn, "cnn", 10).unwrap().len(), first);
    }

    #[test]
    fn channel_sources_are_written_so_playback_can_resolve_a_url() {
        let server = combined_server();
        let mut conn = db();
        run(&mut conn, &http(), &options(&server), &no_rules(), |_| {}).unwrap();

        let n: i64 = conn
            .query_row("SELECT count(*) FROM channel_sources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 3);

        // A second refresh must not pile up duplicate source rows.
        let mut opts = options(&server);
        opts.now_unix += 3600;
        run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();
        let again: i64 = conn
            .query_row("SELECT count(*) FROM channel_sources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(again, 3, "sources should be deduplicated by URL");
    }

    #[test]
    fn content_types_can_be_excluded() {
        let server = combined_server();
        let mut conn = db();
        let mut opts = options(&server);
        opts.import_vod = false;
        opts.import_series = false;

        let report = run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();
        assert_eq!(report.channels, 3);
        assert_eq!(report.movies, 0);
        assert_eq!(report.episodes, 0);

        // What was excluded is counted, not silently gone. This used to be `_ => {}`,
        // which is the difference between "your provider sent nothing" and "your
        // provider sent everything and the import threw it away".
        assert!(
            report.dropped.kind_excluded > 0,
            "excluded entries vanished without being counted"
        );
    }

    /// A clean import loses nothing, and says so.
    ///
    /// The strongest claim this module can make: every entry the provider listed was
    /// either imported or counted with a reason. A zero here is that claim; anything
    /// else names what went.
    #[test]
    fn an_ordinary_import_drops_nothing_at_all() {
        let server = combined_server();
        let mut conn = db();
        let report = run(&mut conn, &http(), &options(&server), &no_rules(), |_| {}).unwrap();

        assert_eq!(
            report.dropped,
            Dropped::default(),
            "an import with nothing wrong with it lost something: {:?}",
            report.dropped
        );
        assert_eq!(report.dropped.total(), 0);
    }

    /// A rule that hides things is a choice the viewer made, and still has to be
    /// visible: "my channels are missing" and "I hid them last week" are the same
    /// screen otherwise.
    #[test]
    fn what_a_rule_hides_is_counted_rather_than_silently_gone() {
        let server = combined_server();
        let mut conn = db();

        let kept = run(&mut conn, &http(), &options(&server), &no_rules(), |_| {})
            .unwrap()
            .channels;
        assert!(kept > 0, "nothing was imported to hide");

        // An empty `contains` matches every name, so this hides the lot.
        let rules =
            RuleSet::compile(&[Rule::new(Field::Name, Match::Contains, "", Action::Hide)]).unwrap();

        let mut opts = options(&server);
        opts.now_unix += 3600;
        let report = run(&mut conn, &http(), &opts, &rules, |_| {}).unwrap();

        assert!(
            report.dropped.hidden_by_rule > 0,
            "a rule hid entries and nothing counted them"
        );
    }

    /// A panel that lists a stream with no id has listed something unplayable. It is
    /// dropped — there is no URL to build — but never in silence.
    #[test]
    fn a_stream_with_no_id_is_counted_as_dropped() {
        let server = TestServer::start(|_, req| {
            let p = &req.path;
            if p.contains("action=get_live_streams") {
                // One usable, one with no id at all.
                Reply::ok(r#"[{"stream_id":101,"name":"CNN"},{"name":"Nameless"}]"#)
            } else if p.contains("action=get_vod_streams")
                || p.contains("action=get_series")
                || p.contains("categories")
            {
                Reply::ok("[]")
            } else if p.contains("xmltv.php") {
                Reply::ok(EPG)
            } else {
                Reply::ok(
                    r#"{"user_info":{"username":"u","status":"Active",
                          "max_connections":"2","active_cons":0}}"#,
                )
            }
        });

        let mut conn = db();
        let mut opts = SyncOptions::new(
            1,
            SourceKind::Xtream {
                base_url: server.url(""),
                username: "u".into(),
            },
            1_705_320_000,
        );
        opts.password = Some("p".into());

        let report = run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();
        assert_eq!(report.channels, 1, "the usable stream was imported");
        assert_eq!(
            report.dropped.no_stream_id, 1,
            "the stream with no id was not counted"
        );
    }

    /// The failure a real subscription produced, and what it looked like from the
    /// outside: a panel that answers HTTP 200 with zero bytes to a blank sign-in.
    #[test]
    fn a_panel_import_with_no_password_refuses_before_it_asks() {
        let server = TestServer::always(Reply::ok(Vec::new()));
        let mut conn = db();
        let mut opts = SyncOptions::new(
            1,
            SourceKind::Xtream {
                base_url: server.url(""),
                username: "u".into(),
            },
            1_705_320_000,
        );
        opts.password = None;

        let err = run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap_err();

        assert!(
            err.message.contains("no password saved"),
            "the message must name the missing password, got {:?}",
            err.message
        );
        assert_eq!(err.code, aurora_core::neterr::ErrorCode::Unauthorized);
        assert!(!err.retryable, "retrying cannot invent a password");
        assert!(
            server.requests().is_empty(),
            "nothing should be sent: a blank sign-in is answered with an empty body, \
             and the provider then gets blamed for it"
        );
    }

    #[test]
    fn a_panel_import_with_no_username_refuses_too() {
        let server = TestServer::always(Reply::ok(Vec::new()));
        let mut conn = db();
        let mut opts = SyncOptions::new(
            1,
            SourceKind::Xtream {
                base_url: server.url(""),
                username: "   ".into(),
            },
            1_705_320_000,
        );
        opts.password = Some("p".into());

        let err = run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap_err();
        assert!(err.message.contains("no username saved"), "{}", err.message);
        assert!(server.requests().is_empty());
    }

    #[test]
    fn an_m3u_import_needs_no_sign_in() {
        // The guard is for panels. An M3U URL carries whatever it needs in the URL, and
        // a password-less one is the normal case rather than a broken one.
        let server = TestServer::always(Reply::ok(
            b"#EXTM3U\n#EXTINF:-1,One\nhttp://example.com/1.ts\n".to_vec(),
        ));
        let mut conn = db();
        let opts = SyncOptions::new(
            1,
            SourceKind::M3u {
                url: server.url("/get.php"),
            },
            1_705_320_000,
        );
        assert_eq!(opts.password, None);
        let report = run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();
        assert_eq!(report.channels, 1);
    }

    #[test]
    fn an_xtream_source_imports_through_the_api() {
        let server = TestServer::start(|_, req| {
            let p = &req.path;
            if p.contains("action=get_live_categories") {
                Reply::ok(r#"[{"category_id":"1","category_name":"News"}]"#)
            } else if p.contains("action=get_live_streams") {
                Reply::ok(
                    r#"[{"stream_id":101,"name":"CNN","category_id":"1","num":202,
                         "epg_channel_id":"cnn.us","tv_archive":1}]"#,
                )
            } else if p.contains("action=get_vod_categories") {
                Reply::ok("[]")
            } else if p.contains("action=get_vod_streams") {
                Reply::ok(
                    r#"[{"stream_id":900,"name":"Inception (2010)",
                               "container_extension":"mkv"}]"#,
                )
            } else if p.contains("action=get_series") {
                Reply::ok("[]")
            } else if p.contains("xmltv.php") {
                Reply::ok(EPG)
            } else {
                Reply::ok(
                    r#"{"user_info":{"username":"u","status":"Active",
                              "max_connections":"2","active_cons":0}}"#,
                )
            }
        });

        let mut conn = db();
        let mut opts = SyncOptions::new(
            1,
            SourceKind::Xtream {
                base_url: server.url(""),
                username: "u".into(),
            },
            1_705_320_000,
        );
        opts.password = Some("p".into());

        let report = run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();
        assert_eq!(report.channels, 1);
        assert_eq!(report.movies, 1);
        assert_eq!(report.epg_programmes, 2);

        let rows = channels::list(&conn, &channels::ChannelFilter::default()).unwrap();
        assert_eq!(rows[0].name, "CNN");
        assert_eq!(rows[0].number, Some(202));
        assert!(rows[0].has_catchup);
        assert_eq!(rows[0].epg_channel_id.as_deref(), Some("cnn.us"));
    }

    /// The gap a real import found: 28,693 series downloaded and thrown away.
    ///
    /// `xtream_entries` fetched `get_series` and then discarded the result with a
    /// warning saying episode listings were deferred — but the *series* were deferred
    /// too, so nothing was written at all. The wizard offered Series as something to
    /// import, Phase 7 was marked Done, and the Series screen was empty on every real
    /// panel.
    #[test]
    fn xtream_series_listings_are_imported_not_discarded() {
        let server = TestServer::start(|_, req| {
            let p = &req.path;
            if p.contains("action=get_series_categories") {
                Reply::ok(r#"[{"category_id":"7","category_name":"Drama"}]"#)
            } else if p.contains("action=get_series") {
                Reply::ok(
                    r#"[{"series_id":11,"name":"Ratched (2020)","category_id":"7",
                         "cover":"https://example.com/ratched.jpg",
                         "releaseDate":"2020-09-18"},
                        {"series_id":12,"name":"The Office","category_id":"7",
                         "releaseDate":"2005"},
                        {"series_id":null,"name":"Broken row"}]"#,
                )
            } else if p.contains("action=get_live_categories")
                || p.contains("action=get_live_streams")
                || p.contains("action=get_vod_categories")
                || p.contains("action=get_vod_streams")
            {
                Reply::ok("[]")
            } else if p.contains("xmltv.php") {
                Reply::ok(EPG)
            } else {
                Reply::ok(r#"{"user_info":{"username":"u","status":"Active"}}"#)
            }
        });

        let mut conn = db();
        let mut opts = SyncOptions::new(
            1,
            SourceKind::Xtream {
                base_url: server.url(""),
                username: "u".into(),
            },
            1_705_320_000,
        );
        opts.password = Some("p".into());

        let report = run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();
        assert_eq!(
            report.series, 2,
            "a listing with no id is skipped, not counted"
        );

        let browse = library::BrowseQuery {
            limit: 50,
            ..Default::default()
        };
        let rows = library::list_series(&conn, &browse).unwrap();
        let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
        assert!(titles.contains(&"Ratched"), "{titles:?}");
        assert!(titles.contains(&"The Office"), "{titles:?}");

        let ratched = rows.iter().find(|r| r.title == "Ratched").unwrap();
        assert_eq!(ratched.year, Some(2020), "the year comes off the title");
        assert_eq!(
            ratched.poster.as_deref(),
            Some("https://example.com/ratched.jpg")
        );

        // A bare year in `releaseDate` is the other form panels use.
        let office = rows.iter().find(|r| r.title == "The Office").unwrap();
        assert_eq!(office.year, Some(2005));

        // Re-importing must not duplicate them.
        let report = run(&mut conn, &http(), &opts, &no_rules(), |_| {}).unwrap();
        assert_eq!(report.series, 2);
        assert_eq!(library::list_series(&conn, &browse).unwrap().len(), 2);
    }

    #[test]
    fn a_release_date_that_is_not_a_year_is_left_alone() {
        assert_eq!(year_from(Some("2019-04-14")), Some(2019));
        assert_eq!(year_from(Some("2019")), Some(2019));
        assert_eq!(year_from(Some("  1999-01-01 ")), Some(1999));
        assert_eq!(year_from(Some("")), None);
        assert_eq!(year_from(Some("n/a")), None);
        assert_eq!(year_from(Some("0000-00-00")), None);
        assert_eq!(year_from(None), None);
    }

    #[test]
    fn bad_xtream_credentials_abort_before_writing_anything() {
        let server = TestServer::always(Reply::ok(
            r#"{"user_info":{"username":"u","status":"Expired"}}"#,
        ));
        let mut conn = db();
        let mut opts = SyncOptions::new(
            1,
            SourceKind::Xtream {
                base_url: server.url(""),
                username: "u".into(),
            },
            0,
        );
        opts.password = Some("p".into());

        assert!(run(&mut conn, &http(), &opts, &no_rules(), |_| {}).is_err());
        assert!(channels::list(&conn, &channels::ChannelFilter::default())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn provider_keys_are_stable_across_url_changes() {
        // The token changes on every refresh; the stream id does not.
        let a = PlaylistEntry::new("CNN", "http://example.com/live/u/p/101.ts?token=aaa");
        let b = PlaylistEntry::new("CNN", "http://example.com/live/u/p/101.ts?token=bbb");
        assert_eq!(provider_key(&a), provider_key(&b));
        assert_eq!(provider_key(&a), "live101");

        let movie = PlaylistEntry::new("Film", "http://example.com/movie/u/p/900.mkv");
        assert_eq!(provider_key(&movie), "movie900");

        // A plain playlist has nothing else stable to key on.
        let plain = PlaylistEntry::new("X", "http://example.com/stream/abc.m3u8");
        assert_eq!(provider_key(&plain), "http://example.com/stream/abc.m3u8");
    }
}
