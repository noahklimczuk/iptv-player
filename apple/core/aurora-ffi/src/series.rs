//! Episode listings, fetched the first time a show is opened and swept in the background.
//!
//! Windows' `aurora-app/src/series.rs` and `library_episodes` (commands.rs). An import
//! writes the series rows and stops there: asking a panel for every show's episodes is
//! one request each, tens of thousands on a real subscription. So the listing is fetched
//! when somebody opens the show, asked again once it is `LISTING_FRESH_FOR` old (a running
//! show gains an episode a week), and filled in for the rest by a background sweep.
//!
//! An M3U library builds its episodes from the playlist during the import, so none of
//! this applies to it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use aurora_core::model::MediaKind;
use aurora_db::repo::filtering::{Kind, LibraryFilter};
use aurora_db::repo::library;
use aurora_db::rusqlite::OptionalExtension;
use aurora_ingest::xtream::XtreamClient;
use parking_lot::Mutex;

use crate::error::{AuroraError, Result};
use crate::vod::Episode;
use crate::{now_unix, AuroraCore};

/// Shows per sweep batch (Windows' `SWEEP_BATCH`).
pub(crate) const SWEEP_BATCH: u32 = 200;
/// How long a show that listed nothing waits before it is asked again.
pub(crate) const LISTING_RETRY_AFTER_SECS: i64 = 6 * 3600;
/// Listings asked for at once: the viewer's own panel, which is also where their video
/// comes from, so a few to hide the latency and no more.
pub(crate) const SWEEP_CONCURRENCY: usize = 4;
/// Shows asked about per sync tick for the records sync has parked (Windows'
/// `SYNC_FETCH_PER_TICK`). A tick is three seconds, so a household's whole Continue
/// Watching arrives within a minute or so, without a burst of requests against the
/// viewer's own panel.
pub(crate) const SYNC_FETCH_PER_TICK: u32 = 4;
/// How long a stored listing is trusted before opening the show asks again.
pub(crate) const LISTING_FRESH_FOR: Duration = Duration::from_secs(6 * 3600);
/// Batches one background sweep works through before giving up (Windows'
/// `SWEEP_MAX_BATCHES`). It stops sooner, the moment a batch asks about nothing.
const SWEEP_MAX_BATCHES: usize = 200;

/// When each show was last listed. In memory, as on Windows: forgetting costs one quick
/// request the next time the show is opened.
fn listed_at() -> &'static Mutex<HashMap<i64, Instant>> {
    static LISTED_AT: OnceLock<Mutex<HashMap<i64, Instant>>> = OnceLock::new();
    LISTED_AT.get_or_init(Default::default)
}

fn listing_is_fresh(series_id: i64) -> bool {
    listed_at()
        .lock()
        .get(&series_id)
        .is_some_and(|at| at.elapsed() < LISTING_FRESH_FOR)
}

/// One listing request per show at a time (Windows' `one_at_a_time`).
///
/// Opening a show can ask for its episodes from more than one place at once, and each
/// would see the same missing or stale listing and go to the provider for it: Windows'
/// log had `get_series_info` for one show three times within 3 ms. The first caller
/// fetches and the others wait for its turn to end, then find the listing fresh and use
/// what it stored.
fn one_at_a_time<T>(
    series_id: i64,
    nothing_to_do: T,
    fetch: impl FnOnce() -> Result<T>,
) -> Result<T> {
    static TURNS: OnceLock<Mutex<HashMap<i64, Arc<Mutex<()>>>>> = OnceLock::new();
    let turns = TURNS.get_or_init(Default::default);
    let turn = Arc::clone(turns.lock().entry(series_id).or_default());

    let result = {
        let _mine = turn.lock();
        // A caller that waited finds the listing the one before it stored. A failed
        // fetch stores nothing and leaves it stale, so the next caller tries again.
        if listing_is_fresh(series_id) {
            Ok(nothing_to_do)
        } else {
            fetch()
        }
    };

    // The map would otherwise keep an entry for every show a sweep ever touched.
    let mut map = turns.lock();
    if Arc::strong_count(&turn) == 2 {
        map.remove(&series_id);
    }
    result
}

/// The panel's own id out of the `series:1234` key an import writes.
fn panel_id(provider_key: &str) -> Option<u32> {
    provider_key
        .strip_prefix("series:")
        .and_then(|id| id.trim().parse().ok())
}

/// What a background sweep did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, uniffi::Record)]
pub struct ListingReport {
    /// Shows asked about.
    pub asked: u64,
    /// Shows that came back with at least one episode.
    pub listed: u64,
    /// Episodes written in all.
    pub episodes: u64,
    /// Shows the provider would not answer for; asked again after a cooldown.
    pub failed: u64,
}

/// Told how a sweep is going. Returning false stops it after the shows in flight.
#[uniffi::export(with_foreign)]
pub trait ListingListener: Send + Sync {
    fn progress(&self, done: u64, total: u64) -> bool;
}

/// Who to ask about a show.
struct Show {
    base_url: String,
    username: String,
    credential_ref: Option<String>,
    stream_id: u32,
}

impl AuroraCore {
    /// The provider behind a show, or `None` when it is not one an Xtream panel can be
    /// asked about.
    fn show(&self, series_id: i64) -> Result<Option<Show>> {
        let db = self.db.lock();
        let row = db
            .query_row(
                "SELECT p.kind, p.base_url, COALESCE(p.username, ''), p.credential_ref, s.provider_key
                 FROM series s JOIN providers p ON p.id = s.provider_id
                 WHERE s.id = ?1",
                [series_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((kind, base_url, username, credential_ref, key)) = row else {
            return Err(AuroraError::not_found(
                "This series is no longer in your library.",
            ));
        };
        if kind != "xtream" {
            return Ok(None);
        }
        Ok(panel_id(&key).map(|stream_id| Show {
            base_url,
            username,
            credential_ref,
            stream_id,
        }))
    }

    /// Ask the panel for one show's episodes and store them. Returns how many were
    /// written; none is a success, since a show really can have nothing listed yet.
    pub(crate) fn fetch_episodes(&self, series_id: i64) -> Result<usize> {
        one_at_a_time(series_id, 0, || self.fetch_now(series_id))
    }

    fn fetch_now(&self, series_id: i64) -> Result<usize> {
        let Some(show) = self.show(series_id)? else {
            return Ok(0);
        };
        let password = match &show.credential_ref {
            None => String::new(),
            Some(key) => self.credentials.get(key).map_err(|e| {
                AuroraError::credentials(format!(
                    "Aurora could not read this provider's saved password: {e}. Edit the \
                     provider in Settings and enter it again."
                ))
            })?,
        };
        let client = XtreamClient::new(&self.http, &show.base_url, &show.username, &password);
        let info = client.series_info(show.stream_id)?;
        // The panel answered, so what is stored is now its current listing, even empty.
        listed_at().lock().insert(series_id, Instant::now());

        let episodes: Vec<library::NewEpisode> = info
            .episodes
            .iter()
            .filter_map(|ep| {
                // No id means no URL, no number means nowhere in a season: neither can
                // be played or listed, so neither is stored.
                let id = ep.id.as_deref()?.trim().parse::<u32>().ok()?;
                let number = ep.episode_num?;
                Some(library::NewEpisode {
                    season: ep.season.unwrap_or(1).min(u32::from(u16::MAX)) as u16,
                    episode: number.min(u32::from(u16::MAX)) as u16,
                    title: ep.title.clone().filter(|t| !t.trim().is_empty()),
                    url: client.stream_url(
                        MediaKind::Episode,
                        id,
                        ep.container_extension.as_deref(),
                    ),
                    still: ep.info.movie_image.clone().filter(|s| !s.trim().is_empty()),
                    overview: ep.info.plot.clone().filter(|s| !s.trim().is_empty()),
                    // Nearest minute: a 3,563-second episode is 59, not 0.
                    runtime_mins: ep
                        .info
                        .duration_secs
                        .filter(|s| *s > 0)
                        .map(|secs| ((secs + 30) / 60).max(1)),
                })
            })
            .collect();
        if episodes.is_empty() {
            return Ok(0);
        }
        let mut db = self.db.lock();
        Ok(library::upsert_episodes(
            &mut db,
            series_id,
            &episodes,
            now_unix(),
        )?)
    }

    /// Remember how asking the panel about a show went, so a show that produced nothing
    /// waits [`LISTING_RETRY_AFTER_SECS`] before it is asked again (Windows'
    /// `note_attempt`).
    fn note_attempt(&self, series_id: i64, outcome: &Result<usize>, now: i64) {
        let attempt = match outcome {
            Ok(0) => Some(library::ListingAttempt::Empty),
            Ok(_) => None,
            Err(_) => Some(library::ListingAttempt::Failed),
        };
        let db = self.db.lock();
        let wrote = match attempt {
            Some(attempt) => library::record_listing_attempt(&db, series_id, attempt, now),
            None => library::clear_listing_attempt(&db, series_id),
        };
        // Not fatal: the cost of losing one of these is one extra request later.
        if let Err(e) = wrote {
            tracing::warn!(series = series_id, "could not record the attempt: {e}");
        }
    }

    /// Fetch the episode listings that parked sync records are waiting for, so they can
    /// apply: Windows' `fetch_for_sync`, run after each sync tick
    /// ([`library::series_waited_on_by_sync`]).
    ///
    /// Progress on a show from another device names one of its episodes, and a panel's
    /// episodes are stored only once the show is opened. Without this, a show half-watched
    /// on the PC reached this device's Continue Watching only if somebody opened it here
    /// first -- which they would not, because it was not on Continue Watching. With
    /// nothing waiting it is one query on two tables that are empty on most libraries.
    pub(crate) fn fetch_for_sync(&self) {
        let now = now_unix();
        let ids = {
            let db = self.db.lock();
            library::series_waited_on_by_sync(
                &db,
                SYNC_FETCH_PER_TICK,
                now - LISTING_RETRY_AFTER_SECS,
            )
        };
        let ids = match ids {
            Ok(ids) => ids,
            Err(e) => {
                tracing::warn!("could not read what sync is waiting for: {e}");
                return;
            }
        };
        let mut stored = 0;
        for series_id in ids {
            // Listed recently and the episode still is not there: the panel does not
            // list it (yet), and asking again now would get the same answer.
            if listing_is_fresh(series_id) {
                continue;
            }
            let outcome = self.fetch_episodes(series_id);
            self.note_attempt(series_id, &outcome, now);
            match outcome {
                Ok(n) => stored += n,
                Err(e) => tracing::debug!(series = series_id, "no listing for sync: {e}"),
            }
        }
        if stored > 0 {
            tracing::info!(
                episodes = stored,
                "stored the episodes synced progress names"
            );
            self.sync.wake();
        }
    }

    /// One batch of the background sweep: shows with no listing that are not in their
    /// retry cooldown, several at once. Each attempt is recorded so a show that will not
    /// list leaves the queue rather than being asked again by the next batch.
    fn sweep_batch(&self, listener: &dyn ListingListener) -> Result<(ListingReport, bool)> {
        let now = now_unix();
        let ids = {
            let db = self.db.lock();
            let filter = LibraryFilter::load(&db)?;
            library::series_needing_episodes(
                &db,
                &filter,
                SWEEP_BATCH,
                now - LISTING_RETRY_AFTER_SECS,
            )?
        };
        let total = ids.len() as u64;
        if ids.is_empty() {
            return Ok((ListingReport::default(), false));
        }
        let stop = AtomicBool::new(!listener.progress(0, total));
        let next = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        let report = Mutex::new(ListingReport::default());

        std::thread::scope(|scope| {
            for _ in 0..SWEEP_CONCURRENCY.min(ids.len()) {
                scope.spawn(|| loop {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let Some(&series_id) = ids.get(next.fetch_add(1, Ordering::Relaxed)) else {
                        return;
                    };
                    let outcome = self.fetch_episodes(series_id);
                    self.note_attempt(series_id, &outcome, now);
                    {
                        let mut report = report.lock();
                        report.asked += 1;
                        match outcome {
                            Ok(0) => {}
                            Ok(n) => {
                                report.listed += 1;
                                report.episodes += n as u64;
                            }
                            Err(e) => {
                                report.failed += 1;
                                tracing::debug!(series = series_id, "no listing: {e}");
                            }
                        }
                    }
                    let n = done.fetch_add(1, Ordering::Relaxed) as u64 + 1;
                    if !listener.progress(n, total) {
                        stop.store(true, Ordering::Relaxed);
                    }
                });
            }
        });
        Ok((report.into_inner(), stop.load(Ordering::Relaxed)))
    }
}

#[uniffi::export]
impl AuroraCore {
    /// A show's episodes, one season or all, fetched from the panel first if none are
    /// stored and refreshed if the stored listing is old. With "Collapse duplicates" on,
    /// one show in the grid stands for every copy of it, and this merges their episodes:
    /// a season only the hidden copy has is still reachable.
    pub fn episodes(&self, series_id: i64, season: Option<u16>) -> Result<Vec<Episode>> {
        let (group, missing, stale) = {
            let db = self.db.lock();
            let filter = LibraryFilter::load(&db)?;
            let group = filter.group_of(&db, Kind::Series, series_id)?;
            let mut missing = Vec::new();
            let mut stale = Vec::new();
            for id in &group {
                // A question about the show, not the season on screen: a show whose
                // episodes are all in season 2 has nothing in season 1 either way.
                if library::episodes_for(&db, *id, None)?.is_empty() {
                    missing.push(*id);
                } else if !listing_is_fresh(*id) {
                    stale.push(*id);
                }
            }
            (group, missing, stale)
        };
        // The lock is dropped above: these go to the network.
        for id in &missing {
            if let Err(e) = self.fetch_episodes(*id) {
                // Fatal only for the show actually asked for; a copy that will not list
                // means showing less, not nothing.
                if *id == series_id {
                    return Err(e);
                }
                tracing::warn!(
                    series = id,
                    "a duplicate copy would not list its episodes ({e})"
                );
            }
        }
        // Brought up to date, but a panel that will not answer now leaves the stored
        // episodes on screen rather than replacing them with an error.
        for id in &stale {
            if let Err(e) = self.fetch_episodes(*id) {
                tracing::warn!(
                    series = id,
                    "could not refresh episodes ({e}); showing those stored"
                );
            }
        }

        let db = self.db.lock();
        let mut merged: Vec<library::EpisodeRow> = Vec::new();
        for id in &group {
            for episode in library::episodes_for(&db, *id, season)? {
                // Keyed on which episode it is: the group is best copy first, so the first
                // seen is the one to keep.
                if !merged
                    .iter()
                    .any(|kept| kept.season == episode.season && kept.episode == episode.episode)
                {
                    merged.push(episode);
                }
            }
        }
        merged.sort_by_key(|e| (e.season, e.episode));
        Ok(merged.into_iter().map(Episode::from).collect())
    }

    /// Fill in the episode listings an import left out, in the background. Long-running:
    /// call it off the main thread. Ends as soon as a batch has nothing to ask about.
    pub fn list_episodes(&self, listener: Arc<dyn ListingListener>) -> Result<ListingReport> {
        let mut total = ListingReport::default();
        for _ in 0..SWEEP_MAX_BATCHES {
            let (batch, stopped) = self.sweep_batch(listener.as_ref())?;
            total.asked += batch.asked;
            total.listed += batch.listed;
            total.episodes += batch.episodes;
            total.failed += batch.failed;
            if stopped || batch.asked == 0 {
                break;
            }
        }
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three callers asking for the same show at once make one request, and the two
    /// that waited are answered from what it stored.
    #[test]
    fn callers_at_once_share_one_listing_request() {
        let show = -4242;
        let asked = AtomicUsize::new(0);
        let start = std::sync::Barrier::new(3);
        std::thread::scope(|s| {
            for _ in 0..3 {
                s.spawn(|| {
                    start.wait();
                    one_at_a_time(show, 0, || {
                        asked.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(50));
                        listed_at().lock().insert(show, Instant::now());
                        Ok(138)
                    })
                    .unwrap();
                });
            }
        });
        assert_eq!(asked.load(Ordering::SeqCst), 1);
    }

    /// A request that failed stored nothing, so the next caller asks again rather than
    /// being told the listing is fresh.
    #[test]
    fn a_failed_listing_request_is_tried_again_by_the_next_caller() {
        let show = -4243;
        let asked = AtomicUsize::new(0);
        let attempt = || {
            one_at_a_time(show, 0, || {
                asked.fetch_add(1, Ordering::SeqCst);
                Err(AuroraError::not_found("the panel would not answer"))
            })
        };
        assert!(attempt().is_err());
        assert!(attempt().is_err());
        assert_eq!(asked.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_provider_key_yields_the_panels_own_id() {
        assert_eq!(panel_id("series:1234"), Some(1234));
        assert_eq!(panel_id("series: 77 "), Some(77));
        assert_eq!(panel_id("movie:1234"), None);
        assert_eq!(panel_id("series:abc"), None);
    }
}
