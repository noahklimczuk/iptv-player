//! Film and series metadata from TMDB: posters, backdrops, cast, ratings and genres.
//!
//! Windows' `aurora-app/src/metadata.rs`, on the shared `aurora_ingest::enrich` pass. The
//! key is the viewer's own when they set one (kept in the Keychain), otherwise the one
//! compiled into the build from `AURORA_TMDB_KEY`, which CI fills from the repository's
//! `TMDB_API_KEY` secret as it does for Windows. Without either, nothing is fetched and
//! Settings says a key is needed.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use aurora_db::repo::enrichment::{self, ItemKind};
use aurora_db::rusqlite::Connection;
use aurora_ingest::enrich::{self, Options};
use aurora_ingest::tmdb::{MetadataClient, TmdbClient, CREDENTIAL_KEY, DEFAULT_IMAGE_BASE_URL};
use parking_lot::Mutex;

use crate::error::{AuroraError, Result};
use crate::mylist::ListKind;
use crate::{now_unix, AuroraCore};

/// A key baked in at build time: `option_env!` reads the compiler's environment, so the
/// value is in the binary and never in the repository (Windows' `BUILT_IN_KEY`).
const BUILT_IN_KEY: Option<&str> = option_env!("AURORA_TMDB_KEY");

/// Where the concurrency setting lives: the same row Windows reads.
const CONCURRENCY_KEY: &str = "metadata.concurrency";

/// How many batches one sweep runs before it stops: a guard against a bug that keeps
/// handing back work, far above any real library (Windows' `AUTO_MAX_BATCHES`).
const MAX_BATCHES: usize = 100_000;

/// The compiled-in key, ignoring a blank one: CI sets the variable whether or not the
/// secret exists, so an unset secret arrives as "" and would otherwise send keyless
/// requests that fail with a 401.
fn built_in() -> Option<&'static str> {
    BUILT_IN_KEY.map(str::trim).filter(|k| !k.is_empty())
}

/// How far enrichment has got for one kind of title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct Coverage {
    pub total: i64,
    pub matched: i64,
    /// Searched, and nothing was a confident match.
    pub no_match: i64,
    /// The lookup failed; tried again after a cooldown.
    pub failed: i64,
    /// Not looked up yet.
    pub pending: i64,
}

impl From<enrichment::Coverage> for Coverage {
    fn from(c: enrichment::Coverage) -> Self {
        Self {
            total: c.total,
            matched: c.matched,
            no_match: c.no_match,
            failed: c.failed,
            pending: c.pending(),
        }
    }
}

/// What the metadata section of Settings shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MetadataStatus {
    /// Whether a key is available at all. Never the key.
    pub has_key: bool,
    /// The only key is the one in this build, so there is nothing for the viewer to do.
    pub key_is_built_in: bool,
    pub movies: Coverage,
    pub series: Coverage,
}

/// What one sweep did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, uniffi::Record)]
pub struct MetadataReport {
    pub matched: u64,
    pub no_match: u64,
    pub failed: u64,
}

/// Told how a sweep is going. Returning false stops it after the titles in flight.
#[uniffi::export(with_foreign)]
pub trait MetadataListener: Send + Sync {
    fn progress(&self, done: u64, total: u64) -> bool;
}

/// One person on a title's page.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Credit {
    pub person_id: i64,
    pub name: String,
    /// Their photo, when TMDB has one.
    pub photo_url: Option<String>,
    /// The character for cast, the job for crew.
    pub role: Option<String>,
    pub is_cast: bool,
}

fn item_kind(kind: ListKind) -> ItemKind {
    match kind {
        ListKind::Movie => ItemKind::Movie,
        ListKind::Series => ItemKind::Series,
    }
}

/// How many lookups are in flight, clamped on read so a hand-edited row cannot ask for
/// hundreds of threads.
fn concurrency(conn: &Connection) -> u32 {
    aurora_db::repo::settings::get_or(conn, CONCURRENCY_KEY, enrich::DEFAULT_CONCURRENCY)
        .unwrap_or(enrich::DEFAULT_CONCURRENCY)
        .clamp(*enrich::CONCURRENCY_RANGE.start(), *enrich::CONCURRENCY_RANGE.end())
}

impl AuroraCore {
    /// The viewer's key first: someone who went and got one wants it used, with its own
    /// rate limit rather than the build's shared one.
    fn tmdb_key(&self) -> Option<String> {
        self.credentials
            .get(CREDENTIAL_KEY)
            .ok()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .or_else(|| built_in().map(str::to_string))
    }

    /// Enrich everything that is due, one batch after another, until a batch finds
    /// nothing to do. The database is locked only to plan and to write each title, never
    /// across a request, so the library stays usable while this runs.
    pub(crate) fn enrich_with(
        &self,
        client: &dyn MetadataClient,
        listener: &dyn MetadataListener,
    ) -> Result<MetadataReport> {
        let options = Options {
            concurrency: concurrency(&self.db.lock()),
            ..Options::default()
        };
        let mut total = MetadataReport::default();
        for _ in 0..MAX_BATCHES {
            let (batch, stopped) = self.enrich_batch(client, &options, listener)?;
            total.matched += batch.matched as u64;
            total.no_match += batch.no_match as u64;
            total.failed += batch.failed as u64;
            // Nothing attempted means nothing is ready: titles in their failure
            // cooldown are not.
            if stopped || batch.attempted() == 0 {
                break;
            }
        }
        Ok(total)
    }

    /// One batch, several titles at once: the rate limit inside `TmdbClient` decides the
    /// throughput, as on Windows (`enrich_batch`).
    fn enrich_batch(
        &self,
        client: &dyn MetadataClient,
        options: &Options,
        listener: &dyn MetadataListener,
    ) -> Result<(enrich::Report, bool)> {
        let now = now_unix();
        let work = enrich::plan(&self.db.lock(), options, now)?;
        let total = work.len() as u64;
        if work.is_empty() {
            return Ok((enrich::Report::default(), false));
        }
        let stop = AtomicBool::new(!listener.progress(0, total));
        let next = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        let report = Mutex::new(enrich::Report::default());
        let failure: Mutex<Option<AuroraError>> = Mutex::new(None);

        std::thread::scope(|scope| {
            for _ in 0..options.workers(work.len()) {
                scope.spawn(|| loop {
                    if stop.load(Ordering::Relaxed) || failure.lock().is_some() {
                        return;
                    }
                    let Some(item) = work.get(next.fetch_add(1, Ordering::Relaxed)) else {
                        return;
                    };
                    // A title that will not resolve is an outcome, not an error; only
                    // the database failing stops the pass.
                    let outcome = enrich::fetch_one(client, item);
                    if let Err(e) = enrich::apply(&mut self.db.lock(), item, &outcome, now) {
                        *failure.lock() = Some(e.into());
                        return;
                    }
                    enrich::tally(&mut report.lock(), &outcome);
                    let n = done.fetch_add(1, Ordering::Relaxed) as u64 + 1;
                    if !listener.progress(n, total) {
                        stop.store(true, Ordering::Relaxed);
                    }
                });
            }
        });

        if let Some(e) = failure.into_inner() {
            return Err(e);
        }
        Ok((report.into_inner(), stop.load(Ordering::Relaxed)))
    }
}

#[uniffi::export]
impl AuroraCore {
    pub fn metadata_status(&self) -> Result<MetadataStatus> {
        let has_own = self.credentials.get(CREDENTIAL_KEY).is_ok();
        let db = self.db.lock();
        Ok(MetadataStatus {
            has_key: has_own || built_in().is_some(),
            key_is_built_in: !has_own && built_in().is_some(),
            movies: enrichment::coverage(&db, ItemKind::Movie)?.into(),
            series: enrichment::coverage(&db, ItemKind::Series)?.into(),
        })
    }

    /// Save the viewer's own TMDB key, or clear it with `None` (or a blank one), which
    /// falls back to the build's key.
    pub fn set_metadata_key(&self, key: Option<String>) -> Result<()> {
        match key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
            Some(key) => self.credentials.set(CREDENTIAL_KEY, &key)?,
            // Clearing a key that was never set is success, not an error.
            None => {
                let _ = self.credentials.delete(CREDENTIAL_KEY);
            }
        }
        Ok(())
    }

    /// Fetch artwork, cast and ratings for everything still bare. Long-running: call it
    /// off the main thread. The app starts it after every import, as Windows does, and
    /// Settings has a button for it.
    pub fn fetch_metadata(&self, listener: Arc<dyn MetadataListener>) -> Result<MetadataReport> {
        let key = self.tmdb_key().ok_or_else(|| {
            AuroraError::other(
                "No metadata key is set. Add one in Settings to fetch artwork and cast.",
            )
        })?;
        let client = TmdbClient::new(Arc::clone(&self.http), key);
        self.enrich_with(&client, listener.as_ref())
    }

    /// Billed cast first, then crew.
    pub fn credits(&self, kind: ListKind, id: i64) -> Result<Vec<Credit>> {
        let db = self.db.lock();
        Ok(enrichment::credits_for(&db, item_kind(kind), id)?
            .into_iter()
            .map(|c| Credit {
                person_id: c.person_id,
                name: c.name,
                photo_url: c
                    .profile_path
                    .filter(|p| !p.is_empty())
                    .map(|p| format!("{DEFAULT_IMAGE_BASE_URL}/w185{p}")),
                role: c.role,
                is_cast: c.is_cast,
            })
            .collect())
    }

    /// Forget a title's match so the next pass looks again: the way out of a confident
    /// wrong match.
    pub fn rematch(&self, kind: ListKind, id: i64) -> Result<()> {
        let db = self.db.lock();
        enrichment::forget(&db, item_kind(kind), id)?;
        enrichment::prune_people(&db)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::core;
    use aurora_core::neterr::NetFailure;
    use aurora_core::tmdb::{Candidate, Credit as TmdbCredit, Metadata, Person};
    use aurora_ingest::tmdb::{ImageSize, Kind};

    /// Knows one film, "Heat" (1995), with one actor.
    struct FakeTmdb;

    impl MetadataClient for FakeTmdb {
        fn search(&self, _kind: Kind, title: &str, _year: Option<i32>) -> std::result::Result<Vec<Candidate>, NetFailure> {
            Ok(if title.eq_ignore_ascii_case("heat") {
                vec![Candidate::new(949, "Heat", Some(1995))]
            } else {
                Vec::new()
            })
        }

        fn details(&self, _kind: Kind, id: i64) -> std::result::Result<Metadata, NetFailure> {
            Ok(Metadata {
                tmdb_id: id,
                title: Some("Heat".into()),
                year: Some(1995),
                poster_path: Some("/heat.jpg".into()),
                genres: vec!["Crime".into(), "Thriller".into()],
                rating: Some(7.9),
                credits: vec![TmdbCredit {
                    person: Person { tmdb_id: 1158, name: "Al Pacino".into(), profile_path: Some("/al.jpg".into()) },
                    role: Some("Vincent Hanna".into()),
                    is_cast: true,
                    order: 0,
                }],
                ..Default::default()
            })
        }

        fn image_url(&self, path: Option<&str>, _size: ImageSize) -> Option<String> {
            path.map(|p| format!("https://img.test{p}"))
        }
    }

    struct Listen(bool);
    impl MetadataListener for Listen {
        fn progress(&self, _done: u64, _total: u64) -> bool {
            self.0
        }
    }

    fn films(core: &AuroraCore, titles: &[(&str, Option<i32>)]) {
        let db = core.db.lock();
        db.execute(
            "INSERT INTO providers (id, name, kind, base_url, created_at) VALUES (1, 'P', 'm3u', 'http://p', 0)",
            [],
        )
        .unwrap();
        for (i, (title, year)) in titles.iter().enumerate() {
            db.execute(
                "INSERT INTO movies (provider_id, provider_key, title, match_key, year, url, last_seen_at)
                 VALUES (1, ?1, ?2, lower(?2), ?3, 'http://p/m.mkv', 0)",
                aurora_db::rusqlite::params![format!("m{i}"), title, year],
            )
            .unwrap();
        }
    }

    #[test]
    fn a_sweep_matches_what_it_can_and_records_the_rest() {
        let (core, _dir) = core();
        films(&core, &[("Heat", Some(1995)), ("Nothing Like It", None)]);
        let report = core.enrich_with(&FakeTmdb, &Listen(true)).unwrap();
        assert_eq!((report.matched, report.no_match, report.failed), (1, 1, 0));

        let status = core.metadata_status().unwrap();
        assert_eq!(status.movies.total, 2);
        assert_eq!(status.movies.matched, 1);
        assert_eq!(status.movies.no_match, 1);
        assert_eq!(status.movies.pending, 0);

        // The match reaches the library: genres for the genre menu, the cast for the page.
        let genres: String = core
            .db
            .lock()
            .query_row("SELECT genres FROM movies WHERE provider_key = 'm0'", [], |r| r.get(0))
            .unwrap();
        assert!(genres.contains("Crime"), "{genres}");
        let id: i64 = core
            .db
            .lock()
            .query_row("SELECT id FROM movies WHERE provider_key = 'm0'", [], |r| r.get(0))
            .unwrap();
        let cast = core.credits(ListKind::Movie, id).unwrap();
        assert_eq!(cast.len(), 1);
        assert_eq!(cast[0].name, "Al Pacino");
        assert_eq!(cast[0].photo_url.as_deref(), Some("https://image.tmdb.org/t/p/w185/al.jpg"));

        // A second sweep has nothing to do.
        let again = core.enrich_with(&FakeTmdb, &Listen(true)).unwrap();
        assert_eq!(again, MetadataReport::default());

        // Rematch puts it back in the queue and forgets its cast.
        core.rematch(ListKind::Movie, id).unwrap();
        assert_eq!(core.metadata_status().unwrap().movies.pending, 1);
        assert!(core.credits(ListKind::Movie, id).unwrap().is_empty());
    }

    #[test]
    fn a_listener_can_stop_a_sweep() {
        let (core, _dir) = core();
        films(&core, &[("Heat", Some(1995))]);
        let report = core.enrich_with(&FakeTmdb, &Listen(false)).unwrap();
        assert_eq!(report, MetadataReport::default());
        assert_eq!(core.metadata_status().unwrap().movies.pending, 1);
    }

    #[test]
    fn the_viewers_key_is_saved_used_and_cleared() {
        let (core, _dir) = core();
        core.set_metadata_key(Some("  mine  ".into())).unwrap();
        assert_eq!(core.tmdb_key().as_deref(), Some("mine"));
        let status = core.metadata_status().unwrap();
        assert!(status.has_key);
        assert!(!status.key_is_built_in);

        core.set_metadata_key(None).unwrap();
        // Back to whatever this build was compiled with, which may be nothing.
        assert_eq!(core.tmdb_key().as_deref(), built_in());
        assert_eq!(core.metadata_status().unwrap().key_is_built_in, built_in().is_some());
        // Clearing twice is still fine.
        core.set_metadata_key(Some("   ".into())).unwrap();
    }

    #[test]
    fn without_any_key_nothing_is_fetched() {
        if built_in().is_some() {
            return;
        }
        let (core, _dir) = core();
        let err = core.fetch_metadata(Arc::new(Listen(true))).unwrap_err();
        assert!(err.to_string().contains("No metadata key"), "{err}");
    }
}
