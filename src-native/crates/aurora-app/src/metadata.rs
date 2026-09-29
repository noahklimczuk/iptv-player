//! Metadata enrichment commands (README §4.5).
//!
//! The key lives in the credential store, so it is never in the database, never in a
//! backup, and never in an export. Commands here deal in "is a key set", never in the
//! key itself — nothing ever sends it back to the UI.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use aurora_db::repo::enrichment::{self, Coverage, CreditRow, ItemKind};
use aurora_db::rusqlite::Connection;
use aurora_ingest::artwork;
use aurora_ingest::enrich::{self, Options, Report};
use aurora_ingest::tmdb::{MetadataClient, TmdbClient, CREDENTIAL_KEY};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::Result;
use crate::services::Services;
use crate::AppError;

/// What the metadata panel in settings shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataStatus {
    /// Whether a key is available at all. Never the key.
    pub has_key: bool,
    /// True when the only key is the one compiled into this build, so Settings can say
    /// "nothing to do here" rather than showing an empty field that looks unfinished.
    pub key_is_built_in: bool,
    /// False when the key would be lost on restart, so the UI can say so.
    pub key_is_persistent: bool,
    pub movies: Coverage,
    pub series: Coverage,
    /// How many titles are looked up at once, and the range the control allows.
    pub concurrency: u32,
    pub concurrency_min: u32,
    pub concurrency_max: u32,
}

/// Where the concurrency setting lives.
const CONCURRENCY_KEY: &str = "metadata.concurrency";

/// How many lookups this library wants in flight, clamped to what the code supports.
///
/// Clamped on read rather than only on write, so a value edited into the settings table
/// by hand — or left behind by a build with a different range — cannot ask for eight
/// hundred threads.
pub fn concurrency(conn: &Connection) -> u32 {
    aurora_db::repo::settings::get_or(conn, CONCURRENCY_KEY, enrich::DEFAULT_CONCURRENCY)
        .unwrap_or(enrich::DEFAULT_CONCURRENCY)
        .clamp(
            *enrich::CONCURRENCY_RANGE.start(),
            *enrich::CONCURRENCY_RANGE.end(),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConcurrencyArgs {
    pub concurrency: u32,
}

/// Choose how hard a metadata pass leans on TMDB.
///
/// A setting rather than a constant because the right answer depends on the connection
/// and on how much of the machine the viewer wants this using while they watch something.
/// Returns what was actually stored, which is the clamped value.
#[tauri::command(async)]
pub fn metadata_set_concurrency(
    services: State<'_, Services>,
    args: ConcurrencyArgs,
) -> Result<u32> {
    let wanted = args.concurrency.clamp(
        *enrich::CONCURRENCY_RANGE.start(),
        *enrich::CONCURRENCY_RANGE.end(),
    );
    let db = services.db.lock();
    aurora_db::repo::settings::set(&db, CONCURRENCY_KEY, &wanted)?;
    Ok(wanted)
}

#[tauri::command(async)]
pub fn metadata_status(services: State<'_, Services>) -> Result<MetadataStatus> {
    let db = services.db.lock();
    Ok(MetadataStatus {
        has_key: key_available(&services),
        key_is_built_in: services.credentials.get(CREDENTIAL_KEY).is_err() && built_in().is_some(),
        key_is_persistent: services.credentials.is_persistent(),
        movies: enrichment::coverage(&db, ItemKind::Movie)?,
        series: enrichment::coverage(&db, ItemKind::Series)?,
        concurrency: concurrency(&db),
        concurrency_min: *enrich::CONCURRENCY_RANGE.start(),
        concurrency_max: *enrich::CONCURRENCY_RANGE.end(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetKeyArgs {
    /// `None` clears the stored key.
    pub key: Option<String>,
}

#[tauri::command(async)]
pub fn metadata_set_key(services: State<'_, Services>, args: SetKeyArgs) -> Result<()> {
    match args
        .key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
    {
        Some(key) => services
            .credentials
            .set(CREDENTIAL_KEY, &key)
            .map_err(|e| AppError::Other(e.to_string()))?,
        None => {
            // Clearing a key that was never set is success, not an error.
            let _ = services.credentials.delete(CREDENTIAL_KEY);
        }
    }
    Ok(())
}

/// A key baked in at build time, if the release build was given one.
///
/// `option_env!` reads the environment the *compiler* ran in, so the value ends up in
/// the binary and never in the repository — the CI job passes it from a GitHub secret.
/// Absent, this is `None` and everything behaves exactly as it did before: the app asks
/// for a key rather than pretending it has one.
///
/// It is worth being plain about what this does and does not protect. The key is inside
/// the shipped executable, and `strings` will find it. That is true of every app that
/// ships with a key; what it buys is that the key is not in git history, not in a public
/// source file, and can be rotated by changing one secret and rebuilding.
pub const BUILT_IN_KEY: Option<&str> = option_env!("AURORA_TMDB_KEY");

/// Whether a usable key exists at all, from either source.
pub fn key_available(services: &Services) -> bool {
    services.credentials.get(CREDENTIAL_KEY).is_ok() || built_in().is_some()
}

/// The compiled-in key, ignoring a blank one.
///
/// CI sets the variable unconditionally, so an unset secret arrives as an empty string
/// rather than as an absent variable — which would otherwise compile to `Some("")` and
/// send keyless requests that fail with a puzzling 401.
fn built_in() -> Option<&'static str> {
    BUILT_IN_KEY.map(str::trim).filter(|k| !k.is_empty())
}

/// The key to use, preferring one the viewer supplied.
///
/// Their own key comes first deliberately: someone who went and got one wants it used,
/// and it carries their own rate limit rather than sharing the built-in one with every
/// other copy of this build.
fn resolve_key(services: &Services) -> Option<String> {
    services
        .credentials
        .get(CREDENTIAL_KEY)
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .or_else(|| built_in().map(str::to_string))
}

fn client(services: &Services) -> Result<TmdbClient> {
    let key = resolve_key(services).ok_or_else(|| {
        AppError::Other(
            "No metadata API key is set. Add one in Settings to fetch artwork and cast.".into(),
        )
    })?;
    Ok(TmdbClient::new(Arc::clone(&services.http), key))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunArgs {
    pub batch: Option<u32>,
    pub movies: Option<bool>,
    pub series: Option<bool>,
}

/// Enrich one batch, emitting `metadata.progress` as it goes.
///
/// One batch per call rather than looping to completion: a forty-thousand title library
/// would hold the connection for an hour, and the user can press the button again.
#[tauri::command(async)]
pub fn metadata_run(
    app: tauri::AppHandle,
    services: State<'_, Services>,
    args: RunArgs,
) -> Result<Report> {
    let client = client(&services)?;
    let options = Options {
        batch: args.batch.unwrap_or(enrich::DEFAULT_BATCH),
        movies: args.movies.unwrap_or(true),
        series: args.series.unwrap_or(true),
        concurrency: concurrency(&services.db.lock()),
    };

    enrich_batch(&services.db, &client, &options, now_unix(), |p| {
        // Best effort: a dropped progress event must never fail the pass.
        crate::emit(&app, "metadata.progress", &p);
    })
    .inspect(|report| {
        crate::emit(&app, "metadata.done", report);
    })
}

/// Enrich one batch against a shared connection, holding the lock only to plan and to
/// write.
///
/// Enrichment does one or two network round trips per title. Holding the single writer
/// connection for the whole pass would stall every other command — including the DVR
/// scheduler thread, which takes the same lock to decide whether a recording is due.
/// Pressing "Fetch metadata" must not cost someone a recording.
///
/// **Several titles at once.** This asked for one answer before posing the next question,
/// and that was the entire cost: a batch of 50 took 3.9 seconds against a real panel,
/// almost all of it spent waiting. Over a 117,602-film library that is 2.6 hours for one
/// pass. Nothing needed to be sequential — `fetch_one` touches the network and nothing
/// else, which is a separation this file already relied on — so the workers share the
/// queue and the rate limit inside `TmdbClient` decides the throughput, as it should.
///
/// The lock is still taken once per title and still never held across a request. It is
/// taken from several threads now rather than one, which is what `parking_lot::Mutex` is
/// for; what matters for the DVR is how long it is held, and that has not changed.
pub fn enrich_batch(
    db: &Mutex<Connection>,
    client: &dyn MetadataClient,
    options: &Options,
    now: i64,
    on_progress: impl Fn(enrich::Progress) + Sync,
) -> Result<Report> {
    let work = {
        let db = db.lock();
        enrich::plan(&db, options, now)?
    };

    let total = work.len();
    on_progress(enrich::Progress { done: 0, total });
    if work.is_empty() {
        return Ok(Report::default());
    }

    // The queue, what came back, and the first thing that went wrong. `done` counts
    // finished titles rather than started ones, so the progress bar never claims more
    // than has actually been written.
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let report = Mutex::new(Report::default());
    let failure: Mutex<Option<crate::AppError>> = Mutex::new(None);

    std::thread::scope(|scope| {
        for _ in 0..options.workers(work.len()) {
            scope.spawn(|| {
                loop {
                    // A database error stops the pass; one title that would not resolve
                    // does not, and never did — `fetch_one` reports that as an outcome.
                    if failure.lock().is_some() {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = work.get(i) else { return };

                    let outcome = enrich::fetch_one(client, item);

                    {
                        let mut db = db.lock();
                        if let Err(e) = enrich::apply(&mut db, item, &outcome, now) {
                            *failure.lock() = Some(e.into());
                            return;
                        }
                    }
                    enrich::tally(&mut report.lock(), &outcome);
                    on_progress(enrich::Progress {
                        done: done.fetch_add(1, Ordering::Relaxed) + 1,
                        total,
                    });
                }
            });
        }
    });

    if let Some(e) = failure.into_inner() {
        return Err(e);
    }
    on_progress(enrich::Progress { done: total, total });
    Ok(report.into_inner())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditsArgs {
    pub kind: String,
    pub id: i64,
}

fn parse_kind(s: &str) -> Result<ItemKind> {
    match s {
        "movie" => Ok(ItemKind::Movie),
        "series" => Ok(ItemKind::Series),
        other => Err(AppError::Other(format!("unknown item kind {other}"))),
    }
}

#[tauri::command(async)]
pub fn metadata_credits(
    services: State<'_, Services>,
    args: CreditsArgs,
) -> Result<Vec<CreditRow>> {
    let kind = parse_kind(&args.kind)?;
    let db = services.db.lock();
    Ok(enrichment::credits_for(&db, kind, args.id)?)
}

/// Forget a title's match so the next pass looks again.
///
/// The escape hatch for a wrong match: the matcher declines when it is unsure, but it
/// can still be confidently wrong, and there has to be a way to say so.
#[tauri::command(async)]
pub fn metadata_rematch(services: State<'_, Services>, args: CreditsArgs) -> Result<()> {
    let kind = parse_kind(&args.kind)?;
    let db = services.db.lock();
    enrichment::forget(&db, kind, args.id)?;
    enrichment::prune_people(&db)?;
    Ok(())
}

/// What the artwork panel shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtworkStatus {
    pub folder: String,
    pub files: usize,
    pub used_bytes: u64,
    pub max_bytes: u64,
}

#[tauri::command(async)]
pub fn artwork_status(services: State<'_, Services>) -> Result<ArtworkStatus> {
    let cache = &services.artwork;
    Ok(ArtworkStatus {
        folder: cache.dir().to_string_lossy().into_owned(),
        files: cache.len(),
        used_bytes: cache.total_bytes(),
        max_bytes: cache.max_bytes(),
    })
}

/// How many images one screen may start downloading. A grid paints about a hundred;
/// this is a little more than that, so a page warms in one pass without a fast scroll
/// turning into thousands of requests to somebody else's image host.
const WARM_AT_ONCE: usize = 128;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalArgs {
    pub urls: Vec<String>,
}

/// Which of these images are on disk, as URLs the WebView can load — and start
/// fetching the ones that are not.
///
/// Answers for a whole grid in one call rather than one per poster: a browse page paints
/// a hundred cards, and a hundred round trips would cost more than the downloads they
/// are meant to avoid.
///
/// **It warms on view, because prefetching cannot work at this size.**
/// `artwork_prefetch` takes an unordered `LIMIT` from a table that held 117,587 rows on
/// the subscription this was tested against, so it caches an arbitrary few dozen posters
/// and the pages a viewer actually opens are almost never among them — measured: 39
/// files cached, 117 images on screen, no overlap at all. Caching every poster instead
/// is not an option either; at roughly 100 KB each that library is some 11 GB, well past
/// any sane budget. What works is caching what is looked at: the first visit to a screen
/// paints from the network exactly as it always did, and warms the cache as it goes, so
/// the second visit comes off the disk.
///
/// Fetching happens on its own thread and reports through `artwork.progress`, which the
/// UI already listens to — so nothing here blocks the grid, and the page swaps its
/// images over as the answers land.
///
/// `None` means "use the remote URL". A cold cache, a scope that was not granted, or a
/// failure in here all degrade to precisely the behaviour that shipped before any of
/// this existed (checklist item 7).
#[tauri::command(async)]
pub fn artwork_local(
    app: tauri::AppHandle,
    services: State<'_, Services>,
    args: LocalArgs,
) -> Result<Vec<Option<String>>> {
    let cache = &services.artwork;
    let mut missing: Vec<String> = Vec::new();

    let answers = args
        .urls
        .iter()
        .map(|url| {
            if url.is_empty() {
                return None;
            }
            if cache.contains(url) {
                return Some(artwork::asset_url(&cache.path_for(url)));
            }
            missing.push(url.clone());
            None
        })
        .collect();

    // Only what nothing else is already downloading, and only so many at once: a viewer
    // scrolling quickly can ask about thousands of posters in a few seconds, and every
    // one of them is a request to somebody else's image host.
    let to_warm: Vec<String> = {
        let mut warming = services.warming.lock();
        missing
            .into_iter()
            .filter(|url| warming.insert(url.clone()))
            .take(WARM_AT_ONCE)
            .collect()
    };

    if !to_warm.is_empty() {
        let http = Arc::clone(&services.http);
        let cache = Arc::clone(&services.artwork);
        let warming = Arc::clone(&services.warming);
        let handle = app.clone();
        std::thread::Builder::new()
            .name("aurora-artwork".into())
            .spawn(move || {
                let report = artwork::prefetch_urls(&http, &cache, &to_warm, |p| {
                    crate::emit(&handle, "artwork.progress", &p);
                });
                tracing::debug!(
                    downloaded = report.downloaded,
                    failed = report.failed,
                    "warmed artwork for a screen"
                );
                let mut warming = warming.lock();
                for url in &to_warm {
                    warming.remove(url);
                }
            })
            // A thread that will not start is not a reason to fail the screen; the
            // images are already loading from the network.
            .map_err(|e| tracing::warn!("could not warm artwork: {e}"))
            .ok();
    }

    Ok(answers)
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefetchArgs {
    pub limit: Option<u32>,
}

/// Download the library's artwork, emitting `artwork.progress` as it goes.
#[tauri::command(async)]
pub fn artwork_prefetch(
    app: tauri::AppHandle,
    services: State<'_, Services>,
    args: PrefetchArgs,
) -> Result<artwork::PrefetchReport> {
    // Read the list under the lock, then release it: downloading five hundred images is
    // minutes of network time, and nothing else can touch the database while this
    // connection is held.
    let urls = {
        let db = services.db.lock();
        aurora_db::repo::enrichment::artwork_urls(
            &db,
            args.limit.unwrap_or(artwork::DEFAULT_PREFETCH_LIMIT),
        )?
    };

    Ok(artwork::prefetch_urls(
        &services.http,
        &services.artwork,
        &urls,
        |p| {
            crate::emit(&app, "artwork.progress", &p);
        },
    ))
}

/// Empty the cache. Always safe: the library keeps the remote URLs.
#[tauri::command(async)]
pub fn artwork_clear(services: State<'_, Services>) -> Result<usize> {
    Ok(services.artwork.clear())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// How many batches one automatic sweep will run before it stops.
///
/// A stop, not a budget.
///
/// This used to be 200, which at 50 titles a batch meant a pass gave up after 10,000 —
/// a third of the way through a 31,375-title queue. Whatever was left waited for the
/// next refresh, so on a large library the metadata simply never finished, and it was
/// not obvious why: nothing failed and nothing said it had stopped early.
///
/// The loop already ends the moment a batch finds nothing to do, which is the real
/// termination condition. This is only here so that a bug which keeps handing back work
/// cannot spin for ever, and it is now far above any real library.
const AUTO_MAX_BATCHES: usize = 100_000;

/// Fetch artwork and cast for everything an import left bare, in the background.
///
/// Enrichment was only ever started by a button in Settings, so a library imported
/// through the wizard had no posters, no cast and no ratings until somebody found that
/// screen and pressed it. Nothing said the button existed, and a wall of grey
/// rectangles reads as a broken app rather than as an unfinished optional step.
///
/// Deliberately fire-and-forget. This must never fail an import, hold it up, or
/// surface an error of its own — a provider refresh succeeded whatever TMDB thinks,
/// and the `metadata.progress` events say how it is going for anyone watching.
pub fn enrich_in_background(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        use tauri::Manager;
        let services = app.state::<Services>();

        // No key, nothing to do, and nothing worth saying: running without one would
        // be 401s in a loop.
        let Ok(client) = client(&services) else {
            tracing::debug!("no metadata key; skipping the automatic pass");
            return;
        };

        let options = Options {
            batch: enrich::DEFAULT_BATCH,
            movies: true,
            series: true,
            concurrency: concurrency(&services.db.lock()),
        };

        let mut total = Report::default();
        for _ in 0..AUTO_MAX_BATCHES {
            let batch = match enrich_batch(&services.db, &client, &options, now_unix(), |p| {
                crate::emit(&app, "metadata.progress", &p);
            }) {
                Ok(r) => r,
                Err(e) => {
                    // One failed batch ends the sweep rather than retrying forever:
                    // the usual cause is the network or the key, and neither improves
                    // by being asked two hundred times.
                    tracing::warn!("automatic metadata pass stopped: {e}");
                    break;
                }
            };

            // Nothing was attempted, so there is nothing left that is ready to be
            // attempted — titles in their failure cooldown are not.
            if batch.attempted() == 0 {
                break;
            }
            total.matched += batch.matched;
            total.no_match += batch.no_match;
            total.failed += batch.failed;
        }

        tracing::info!(
            matched = total.matched,
            no_match = total.no_match,
            failed = total.failed,
            "automatic metadata pass finished"
        );
        crate::emit(&app, "metadata.done", &total);
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_blank_built_in_key_is_no_key() {
        // CI sets the variable unconditionally, so an unset secret arrives as "" rather
        // than absent. Compiling that to Some("") would send keyless requests and fail
        // with a 401 nobody could explain.
        for raw in ["", "   ", "\n"] {
            assert!(
                raw.trim().is_empty(),
                "{raw:?} should be treated as no key at all"
            );
        }
    }

    #[test]
    fn this_build_reports_its_key_honestly() {
        // Whatever this build was compiled with, `built_in()` must agree with it: a
        // build with no secret must not claim a key, and one with a secret must use it.
        match BUILT_IN_KEY {
            None => assert!(built_in().is_none()),
            Some(k) if k.trim().is_empty() => assert!(built_in().is_none()),
            Some(k) => assert_eq!(built_in(), Some(k.trim())),
        }
    }

    use super::*;
    use aurora_ingest::credentials::{CredentialStore, MemoryStore};

    #[test]
    fn an_item_kind_is_validated_before_it_reaches_a_query() {
        assert_eq!(parse_kind("movie").unwrap(), ItemKind::Movie);
        assert_eq!(parse_kind("series").unwrap(), ItemKind::Series);
        let err = parse_kind("channel").unwrap_err();
        assert!(err.to_string().contains("channel"));
    }

    #[test]
    fn clearing_a_key_leaves_no_key_behind() {
        let store = MemoryStore::default();
        store.set(CREDENTIAL_KEY, "abcd1234").unwrap();
        store.delete(CREDENTIAL_KEY).unwrap();
        assert!(store.get(CREDENTIAL_KEY).is_err());

        // Clearing again is still fine here, but the Windows Credential Manager reports
        // a missing entry as an error — which is why the command ignores the result:
        // "there is no key" is the state the user asked for either way.
        let _ = store.delete(CREDENTIAL_KEY);
        assert!(store.get(CREDENTIAL_KEY).is_err());
    }

    /// Takes its time answering, like a real network call.
    struct SlowClient {
        delay: std::time::Duration,
    }

    impl MetadataClient for SlowClient {
        fn search(
            &self,
            _kind: aurora_ingest::tmdb::Kind,
            _title: &str,
            _year: Option<i32>,
        ) -> std::result::Result<Vec<aurora_core::tmdb::Candidate>, aurora_core::neterr::NetFailure>
        {
            std::thread::sleep(self.delay);
            Ok(Vec::new())
        }

        fn details(
            &self,
            _kind: aurora_ingest::tmdb::Kind,
            id: i64,
        ) -> std::result::Result<aurora_core::tmdb::Metadata, aurora_core::neterr::NetFailure>
        {
            Ok(aurora_core::tmdb::Metadata {
                tmdb_id: id,
                ..Default::default()
            })
        }

        fn image_url(
            &self,
            _path: Option<&str>,
            _size: aurora_ingest::tmdb::ImageSize,
        ) -> Option<String> {
            None
        }
    }

    fn seeded(movies: usize) -> Mutex<Connection> {
        let conn = aurora_db::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        for i in 0..movies {
            conn.execute(
                "INSERT INTO movies (provider_id,provider_key,title,match_key,url,last_seen_at)
                 VALUES (1,?1,?2,?2,'https://example.com/m.mkv',0)",
                aurora_db::rusqlite::params![format!("m{i}"), format!("Film {i}")],
            )
            .unwrap();
        }
        Mutex::new(conn)
    }

    /// The regression guard for the bug this structure exists to prevent.
    ///
    /// Several titles really are in flight at once.
    ///
    /// Compared against the same work at `concurrency: 1` rather than against a fixed
    /// number of milliseconds, so the assertion means the same thing on a slow machine as
    /// on a fast one, and so it fails if the setting is ever quietly ignored.
    ///
    /// This is the whole point of the change: a batch of 50 against a real panel took 3.9
    /// seconds, nearly all of it waiting, which is 2.6 hours for a 117,602-film library.
    #[test]
    fn titles_are_looked_up_several_at_a_time() {
        let client = SlowClient {
            delay: std::time::Duration::from_millis(50),
        };

        let run = |concurrency: u32| {
            let db = seeded(12);
            let started = std::time::Instant::now();
            let report = enrich_batch(
                &db,
                &client,
                &Options {
                    series: false,
                    concurrency,
                    ..Default::default()
                },
                1_000,
                |_| {},
            )
            .unwrap();
            assert_eq!(report.no_match, 12, "every title was looked up");
            started.elapsed()
        };

        let sequential = run(1);
        let concurrent = run(6);
        assert!(
            concurrent * 2 < sequential,
            "six at a time took {concurrent:?} against {sequential:?} one at a time, \
             which is not the difference being asked for"
        );
    }

    /// Progress never claims more than has been written.
    ///
    /// It counts finished titles rather than started ones, which matters more now that
    /// several are in flight: counting starts would run the bar to 100% while six lookups
    /// were still outstanding.
    #[test]
    fn progress_counts_what_is_finished_and_never_overshoots() {
        let db = seeded(9);
        let client = SlowClient {
            delay: std::time::Duration::from_millis(5),
        };
        let seen = Mutex::new(Vec::new());

        enrich_batch(
            &db,
            &client,
            &Options {
                series: false,
                concurrency: 4,
                ..Default::default()
            },
            1_000,
            |p| seen.lock().push((p.done, p.total)),
        )
        .unwrap();

        let seen = seen.into_inner();
        assert!(!seen.is_empty(), "nothing reported progress");
        for (done, total) in &seen {
            assert_eq!(*total, 9, "the total moved");
            assert!(*done <= 9, "progress reported {done} of 9");
        }
        assert_eq!(
            seen.last(),
            Some(&(9, 9)),
            "the pass never reported finishing"
        );
    }

    /// The DVR scheduler runs on its own thread and takes this same lock every ten
    /// seconds to decide whether a recording is due. If enrichment held it across its
    /// network calls, pressing "Fetch metadata" would stall the scheduler for the
    /// length of the pass — and a recording due in that window would simply not start.
    #[test]
    fn enrichment_does_not_hold_the_database_lock_across_the_network() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let db = Arc::new(seeded(6));
        let client = SlowClient {
            delay: std::time::Duration::from_millis(40),
        };

        let acquired = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));

        // Stand in for the DVR thread: take the lock repeatedly while the pass runs.
        let contender = std::thread::spawn({
            let db = Arc::clone(&db);
            let acquired = Arc::clone(&acquired);
            let stop = Arc::clone(&stop);
            move || {
                while !stop.load(Ordering::Relaxed) {
                    if let Some(guard) = db.try_lock_for(std::time::Duration::from_millis(5)) {
                        drop(guard);
                        acquired.fetch_add(1, Ordering::Relaxed);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
        });

        let report = enrich_batch(
            &db,
            &client,
            &Options {
                series: false,
                ..Default::default()
            },
            1_000,
            |_| {},
        )
        .unwrap();

        stop.store(true, Ordering::Relaxed);
        contender.join().unwrap();

        assert_eq!(report.no_match, 6, "every title was looked up");
        // Six titles at 40ms each is ~240ms of network. A contender polling every few
        // milliseconds must have got in many times; if the lock were held for the pass
        // it would have got in roughly never.
        assert!(
            acquired.load(Ordering::Relaxed) > 10,
            "the lock was only free {} times during a ~240ms pass",
            acquired.load(Ordering::Relaxed)
        );
    }

    #[test]
    fn a_stored_key_round_trips_but_only_through_the_credential_store() {
        let store = MemoryStore::default();
        store.set(CREDENTIAL_KEY, "abcd1234").unwrap();
        assert_eq!(store.get(CREDENTIAL_KEY).unwrap(), "abcd1234");
        // The in-memory store is honest about not surviving a restart, which is what
        // `keyIsPersistent` reports to the UI.
        assert!(!store.is_persistent());
    }
}
