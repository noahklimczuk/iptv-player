//! The refresh pipeline: fetch → parse → classify → reconcile → EPG match → index.
//!
//! README §4.6 is the governing constraint: a refresh must never blow away user data.
//! Reconciliation happens by stable provider key in `aurora_db`, and channels that
//! vanish from the provider are *reported*, not deleted.

use std::collections::HashMap;

use aurora_core::epg_match::{self, EpgIndex};
use aurora_core::model::{EpgChannel, MediaKind, PlaylistEntry, Programme};
use aurora_core::neterr::NetFailure;
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
pub fn run(
    db: &mut Connection,
    http: &HttpClient,
    options: &SyncOptions,
    rules: &RuleSet,
    mut on_progress: impl FnMut(Progress),
) -> Result<SyncReport, NetFailure> {
    let mut report = SyncReport::default();
    let mut epg_urls: Vec<String> = options.extra_epg_urls.clone();

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
                report
                    .warnings
                    .push(format!("line {}: {}", w.line, w.message));
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
            let client = XtreamClient::new(http, base_url, username, password);
            client.authenticate(options.now_unix)?;
            epg_urls.push(client.xmltv_url());

            on_progress(Progress {
                phase: Phase::FetchingPlaylist,
                done: 0,
                total: 0,
            });
            xtream_entries(&client, options, &mut report)?
        }
    };

    // Apply the user's rules before anything is written, so hidden entries never
    // reach the library in the first place.
    let mut live = Vec::new();
    let mut movies = Vec::new();
    let mut episodes = Vec::new();
    for mut entry in entries {
        let outcome = rules.apply(&mut entry);
        if outcome.hidden {
            continue;
        }
        match entry.kind {
            MediaKind::Live if options.import_live => live.push(entry),
            MediaKind::Movie if options.import_vod => movies.push(entry),
            MediaKind::Episode if options.import_series => episodes.push(entry),
            _ => {}
        }
    }

    // ── Channels ────────────────────────────────────────────────────────────────
    on_progress(Progress {
        phase: Phase::ImportingChannels,
        done: 0,
        total: live.len(),
    });
    let keyed: Vec<(String, &PlaylistEntry)> = live.iter().map(|e| (provider_key(e), e)).collect();
    report.channels = channels::upsert_batch(db, options.provider_id, &keyed, options.now_unix)
        .map_err(db_failure)?;
    report.channels_missing = channels::stale(db, options.provider_id, options.now_unix)
        .map_err(db_failure)?
        .len();

    // Channel sources, so playback can resolve a URL and fail over (README §7.14).
    write_channel_sources(db, options.provider_id, &keyed).map_err(db_failure)?;
    on_progress(Progress {
        phase: Phase::ImportingChannels,
        done: live.len(),
        total: live.len(),
    });

    // ── Movies ──────────────────────────────────────────────────────────────────
    on_progress(Progress {
        phase: Phase::ImportingMovies,
        done: 0,
        total: movies.len(),
    });
    let new_movies: Vec<library::NewMovie> = movies
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
        total: episodes.len(),
    });
    let (groups, ungrouped) = series::group_series(&episodes);
    if !ungrouped.is_empty() {
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
    report.series = groups.len();

    // ── EPG ─────────────────────────────────────────────────────────────────────
    epg_urls.sort();
    epg_urls.dedup();
    let mut epg_channels: Vec<EpgChannel> = Vec::new();

    for url in &epg_urls {
        on_progress(Progress {
            phase: Phase::FetchingEpg,
            done: 0,
            total: 0,
        });
        match import_epg(db, http, url, &mut epg_channels) {
            Ok((chans, progs)) => {
                report.epg_channels += chans;
                report.epg_programmes += progs;
            }
            Err(e) => {
                // A missing guide must not fail the whole refresh — the library is
                // still usable without it.
                report.warnings.push(format!(
                    "EPG source failed: {} ({})",
                    e.message,
                    crate::http::redact(url)
                ));
            }
        }
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

fn xtream_entries(
    client: &XtreamClient<'_>,
    options: &SyncOptions,
    report: &mut SyncReport,
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
            let Some(id) = s.stream_id else { continue };
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
            let Some(id) = s.stream_id else { continue };
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
        // The per-series episode listing needs one request each; that is a lot of
        // round trips, so it is deferred to a follow-up pass rather than blocking
        // the first refresh.
        let listings = client.series()?;
        if !listings.is_empty() {
            report.warnings.push(format!(
                "{} series found; episode listings are fetched on demand",
                listings.len()
            ));
        }
    }

    Ok(out)
}

fn import_epg(
    db: &mut Connection,
    http: &HttpClient,
    url: &str,
    collected: &mut Vec<EpgChannel>,
) -> Result<(usize, usize), NetFailure> {
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

    epg_repo::upsert_channels(db, &channel_buf).map_err(db_failure)?;
    for batch in &programme_batches {
        epg_repo::insert_batch(db, batch).map_err(db_failure)?;
    }
    collected.extend(channel_buf);

    Ok((stats.channels, stats.programmes))
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
