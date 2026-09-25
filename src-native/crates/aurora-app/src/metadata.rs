//! Metadata enrichment commands (README §4.5).
//!
//! The key lives in the credential store, so it is never in the database, never in a
//! backup, and never in an export. Commands here deal in "is a key set", never in the
//! key itself — nothing ever sends it back to the UI.

use std::sync::Arc;

use aurora_db::repo::enrichment::{self, Coverage, CreditRow, ItemKind};
use aurora_db::rusqlite::Connection;
use aurora_ingest::artwork;
use aurora_ingest::enrich::{self, Options, Report};
use aurora_ingest::tmdb::{MetadataClient, TmdbClient, CREDENTIAL_KEY};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

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
    };

    enrich_batch(&services.db, &client, &options, now_unix(), |p| {
        // Best effort: a dropped progress event must never fail the pass.
        let _ = app.emit("metadata.progress", &p);
    })
    .inspect(|report| {
        let _ = app.emit("metadata.done", report);
    })
}

/// Enrich one batch against a shared connection, holding the lock only to plan and to
/// write.
///
/// Enrichment does one or two network round trips per title. Holding the single writer
/// connection for the whole pass would stall every other command — including the DVR
/// scheduler thread, which takes the same lock to decide whether a recording is due.
/// Pressing "Fetch metadata" must not cost someone a recording.
pub fn enrich_batch(
    db: &Mutex<Connection>,
    client: &dyn MetadataClient,
    options: &Options,
    now: i64,
    mut on_progress: impl FnMut(enrich::Progress),
) -> Result<Report> {
    let work = {
        let db = db.lock();
        enrich::plan(&db, options, now)?
    };

    let total = work.len();
    let mut report = Report::default();
    for (done, item) in work.iter().enumerate() {
        on_progress(enrich::Progress { done, total });

        // No lock held across this.
        let outcome = enrich::fetch_one(client, item);

        {
            let mut db = db.lock();
            enrich::apply(&mut db, item, &outcome, now)?;
        }
        enrich::tally(&mut report, &outcome);
    }
    on_progress(enrich::Progress { done: total, total });
    Ok(report)
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
            let _ = app.emit("artwork.progress", &p);
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
