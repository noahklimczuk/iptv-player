//! Episode listings, fetched the first time a show is opened.
//!
//! A refresh writes the series row and stops there. Asking a panel for the episodes of
//! every show it lists would be one request each — 28,693 of them on the subscription
//! this was measured against — so the listing is deferred until somebody actually
//! wants it. The refresh has said so in a warning since F-04 was fixed; what was
//! missing was anything that then did it, so every series on every panel sat at
//! "0 seasons" permanently.

use std::sync::atomic::{AtomicUsize, Ordering};

use aurora_db::repo::library;
use aurora_db::rusqlite::OptionalExtension;
use aurora_ingest::xtream::XtreamClient;
use parking_lot::Mutex;
use serde::Serialize;

use crate::error::{AppError, Result};
use crate::services::Services;

/// Shows per sweep. Bounded like a metadata batch, so progress is visible and stopping
/// costs one batch rather than everything.
pub const SWEEP_BATCH: u32 = 200;

/// How many listings are asked for at once.
///
/// Deliberately smaller than the metadata setting, and not configurable. That one talks
/// to TMDB, which publishes what it will tolerate; this one talks to the viewer's own
/// IPTV panel, which does not, and which is the same host their video comes from. Four is
/// enough to hide the latency — a listing came back in about 0.3s — without turning a
/// background nicety into something that competes with playback.
pub const SWEEP_CONCURRENCY: usize = 4;

#[derive(Debug, Default, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SweepReport {
    /// Shows asked about.
    pub asked: usize,
    /// Shows that came back with at least one episode.
    pub listed: usize,
    /// Episodes written in total.
    pub episodes: usize,
    /// Shows the provider would not answer for. Not fatal and not retried here.
    pub failed: usize,
}

/// How far a sweep has got, for the UI.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SweepProgress {
    pub done: usize,
    pub total: usize,
}

/// Fetch the episode listings for shows that have none.
///
/// **Why this exists.** Seasons are counted from the episodes table, and an import writes
/// the show without them — so every card said "0 seasons" and the number only appeared
/// for shows somebody had opened by hand. Seven of 28,553, on the library this was
/// measured against. The panel's own `get_series` carries no season information at all, so
/// there is no cheaper source: it is one request per show or nothing.
///
/// One title that will not resolve does not stop the sweep. A provider that has stopped
/// answering altogether is a different thing, and the caller sees it in `failed`.
pub fn sweep(
    services: &Services,
    limit: u32,
    on_progress: impl Fn(SweepProgress) + Sync,
) -> Result<SweepReport> {
    let ids = {
        let db = services.db.lock();
        let filter = aurora_db::repo::filtering::LibraryFilter::load(&db)?;
        library::series_needing_episodes(&db, &filter, limit)?
    };

    let total = ids.len();
    on_progress(SweepProgress { done: 0, total });
    if ids.is_empty() {
        return Ok(SweepReport::default());
    }

    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let report = Mutex::new(SweepReport::default());

    std::thread::scope(|scope| {
        for _ in 0..SWEEP_CONCURRENCY.min(total) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(&series_id) = ids.get(i) else { return };

                let outcome = fetch_episodes(services, series_id);
                {
                    let mut report = report.lock();
                    report.asked += 1;
                    match outcome {
                        Ok(0) => {}
                        Ok(n) => {
                            report.listed += 1;
                            report.episodes += n;
                        }
                        Err(e) => {
                            report.failed += 1;
                            tracing::debug!(series = series_id, "no listing: {e}");
                        }
                    }
                }
                on_progress(SweepProgress {
                    done: done.fetch_add(1, Ordering::Relaxed) + 1,
                    total,
                });
            });
        }
    });

    let report = report.into_inner();
    tracing::info!(
        asked = report.asked,
        listed = report.listed,
        episodes = report.episodes,
        failed = report.failed,
        "episode listing sweep finished"
    );
    Ok(report)
}

/// What the database knows about a show, and who to ask about it.
struct Show {
    base_url: String,
    username: String,
    credential_ref: Option<String>,
    /// The panel's own id, out of the `series:1234` key an import writes.
    stream_id: u32,
}

/// Fetch one show's episodes from its provider and store them.
///
/// Returns the number written. Doing nothing is a success: a show really can have no
/// episodes listed yet, and that is not a failure to report to the viewer.
pub fn fetch_episodes(services: &Services, series_id: i64) -> Result<usize> {
    let show = match look_up(services, series_id)? {
        Some(s) => s,
        // An M3U library builds its episodes from the playlist during the import, so
        // there is nothing to go and ask for. Neither is there for a show whose row
        // predates provider keys.
        None => return Ok(0),
    };

    let password = match &show.credential_ref {
        None => String::new(),
        Some(key) => services.credentials.get(key).map_err(|e| {
            AppError::Other(format!(
                "Aurora could not read this provider's saved password: {e}. Edit the \
                 provider in Settings and enter it again."
            ))
        })?,
    };

    let client = XtreamClient::new(&services.http, &show.base_url, &show.username, &password);
    // Worded the same way a refresh words its failures, because to the viewer they are
    // the same event: the provider would not answer.
    let info = client
        .series_info(show.stream_id)
        .map_err(|e| AppError::Other(format!("{}: {}", e.message, e.cause)))?;

    let mut episodes = Vec::with_capacity(info.episodes.len());
    for ep in &info.episodes {
        // An episode with no id has no URL, and one with no number has nowhere to sit
        // in a season. Either way it cannot be played or listed, so it is dropped
        // rather than stored as a row that looks like an episode and is not.
        let (Some(id), Some(number)) = (ep.id.as_deref(), ep.episode_num) else {
            continue;
        };
        let Ok(stream_id) = id.trim().parse::<u32>() else {
            continue;
        };
        episodes.push(library::NewEpisode {
            season: ep.season.unwrap_or(1).min(u32::from(u16::MAX)) as u16,
            episode: number.min(u32::from(u16::MAX)) as u16,
            title: ep.title.clone().filter(|t| !t.trim().is_empty()),
            url: client.stream_url(
                aurora_core::model::MediaKind::Episode,
                stream_id,
                ep.container_extension.as_deref(),
            ),
            still: ep.info.movie_image.clone().filter(|s| !s.trim().is_empty()),
        });
    }

    if episodes.is_empty() {
        return Ok(0);
    }

    let mut db = services.db.lock();
    let written = library::upsert_episodes(&mut db, series_id, &episodes, crate::now_unix())?;
    tracing::info!(series = series_id, written, "episode listing stored");
    Ok(written)
}

/// How many batches the background sweep will work through before giving up the thread.
const SWEEP_MAX_BATCHES: usize = 200;

/// Fill in the episode listings after an import, in the background.
///
/// Fire-and-forget, like the metadata pass beside it: an import succeeded whatever the
/// panel later says about one show, and a refresh must not wait for 28,000 requests.
/// Stops as soon as a batch asks about nothing, which is how "there is no backlog left"
/// arrives.
pub fn sweep_in_background(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        use tauri::Manager;
        let services = app.state::<Services>();

        let mut total = SweepReport::default();
        for _ in 0..SWEEP_MAX_BATCHES {
            let batch = match sweep(&services, SWEEP_BATCH, |p| {
                crate::emit(&app, "series.listings", &p);
            }) {
                Ok(batch) => batch,
                // One failed batch ends the sweep rather than retrying for ever: the
                // usual cause is the provider, and it does not improve by being asked two
                // hundred more times.
                Err(e) => {
                    tracing::warn!("episode listing sweep stopped: {e}");
                    break;
                }
            };
            if batch.asked == 0 {
                break;
            }
            total.asked += batch.asked;
            total.listed += batch.listed;
            total.episodes += batch.episodes;
            total.failed += batch.failed;
        }
        crate::emit(&app, "series.listingsDone", &total);
    });
}

/// The provider behind a series, and the panel's id for it. `None` when this is not a
/// show an Xtream panel can be asked about.
fn look_up(services: &Services, series_id: i64) -> Result<Option<Show>> {
    let db = services.db.lock();
    let row = db
        .query_row(
            "SELECT p.kind, p.base_url, COALESCE(p.username, ''), p.credential_ref,
                    s.provider_key
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
        .optional()
        .map_err(aurora_db::DbError::from)?;

    let Some((kind, base_url, username, credential_ref, provider_key)) = row else {
        return Err(AppError::Other(format!("no series {series_id}")));
    };
    if kind != "xtream" {
        return Ok(None);
    }
    let Some(stream_id) = provider_key
        .strip_prefix("series:")
        .and_then(|id| id.trim().parse::<u32>().ok())
    else {
        return Ok(None);
    };

    Ok(Some(Show {
        base_url,
        username,
        credential_ref,
        stream_id,
    }))
}

#[cfg(test)]
mod tests {

    /// The key an import writes is `series:1234`; anything else is not a panel show.
    #[test]
    fn a_provider_key_yields_the_panels_own_id() {
        let read = |key: &str| {
            key.strip_prefix("series:")
                .and_then(|id| id.trim().parse::<u32>().ok())
        };
        assert_eq!(read("series:1234"), Some(1234));
        assert_eq!(read("series: 77 "), Some(77));
        assert_eq!(read("1234"), None, "an unprefixed key is not ours");
        assert_eq!(read("movie:1234"), None);
        assert_eq!(read("series:"), None);
        assert_eq!(read("series:abc"), None);
    }
}
