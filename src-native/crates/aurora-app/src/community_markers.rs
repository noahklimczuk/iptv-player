//! TheIntroDB markers for the episode being played, asked once and kept for the session.
//!
//! The playback-aids command runs several times as an episode loads (before and after
//! the duration is known, again after a seek is taken as evidence), so the answer is
//! cached rather than asked for each time. The body is kept rather than the markers,
//! because an open-ended credits segment can only be placed once the runtime is known,
//! and that arrives after the first call.
//!
//! Never written to the library: a schema change for a cache that refills itself in one
//! request would be the wrong trade. A failure is remembered for ten minutes so that a
//! service that is down does not cost every call its timeout.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use aurora_core::markers::SkipMarker;
use aurora_db::rusqlite::{params, Connection, OptionalExtension};
use aurora_ingest::http::HttpClient;
use aurora_ingest::introdb;
use parking_lot::Mutex;

const RETRY_FAILED_AFTER: Duration = Duration::from_secs(600);

/// Which episode TheIntroDB would call this: the show's TMDB id, season and episode.
pub type Key = (i64, u16, u16);

enum Entry {
    /// The service's answer, or `None` for "it has nothing for this episode".
    Answered(Option<String>),
    Failed(Instant),
}

fn cache() -> &'static Mutex<HashMap<Key, Entry>> {
    static CACHE: OnceLock<Mutex<HashMap<Key, Entry>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn client() -> Option<&'static HttpClient> {
    static CLIENT: OnceLock<Option<HttpClient>> = OnceLock::new();
    CLIENT
        .get_or_init(|| match introdb::client() {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::warn!("TheIntroDB client would not build: {}", e.message);
                None
            }
        })
        .as_ref()
}

/// The key for an episode, when its show has been matched to TMDB.
pub fn key_for(conn: &Connection, episode_id: i64) -> aurora_db::Result<Option<Key>> {
    Ok(conn
        .query_row(
            "SELECT s.tmdb_id, e.season, e.episode
               FROM episodes e JOIN series s ON s.id = e.series_id
              WHERE e.id = ?1 AND s.tmdb_id IS NOT NULL",
            params![episode_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?)
}

/// The markers for this episode. Goes to the network at most once per episode per
/// session (or per ten minutes after a failure), so it must be called without the
/// database lock held.
pub fn markers(key: Key, duration_secs: Option<f64>) -> Vec<SkipMarker> {
    let cached = match cache().lock().get(&key) {
        Some(Entry::Answered(body)) => Some(body.clone()),
        Some(Entry::Failed(at)) if at.elapsed() < RETRY_FAILED_AFTER => return Vec::new(),
        _ => None,
    };
    let body = match cached {
        Some(body) => body,
        None => {
            let Some(client) = client() else {
                return Vec::new();
            };
            let (tmdb_id, season, episode) = key;
            match client.fetch_string(&introdb::url(tmdb_id, season, episode)) {
                Ok(body) => {
                    cache()
                        .lock()
                        .insert(key, Entry::Answered(Some(body.clone())));
                    Some(body)
                }
                Err(e) if e.code == aurora_core::neterr::ErrorCode::NotFound => {
                    cache().lock().insert(key, Entry::Answered(None));
                    None
                }
                Err(e) => {
                    tracing::info!(?key, "TheIntroDB did not answer: {}", e.message);
                    cache().lock().insert(key, Entry::Failed(Instant::now()));
                    None
                }
            }
        }
    };
    body.map(|b| introdb::parse(&b, duration_secs))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::markers::{MarkerKind, MarkerSource};

    /// Against the real service: `cargo test -p aurora-app community -- --ignored`.
    ///
    /// Ignored by default because CI must not depend on a third party being up. MobLand
    /// S1E9 is the reference: its file's own chapters put the intro at 7:19-8:14.
    #[test]
    #[ignore]
    fn the_real_service_places_mobland_s1e9() {
        let m = markers((247_718, 1, 9), Some(2858.0));
        let intro = m
            .iter()
            .find(|x| x.kind == MarkerKind::Intro)
            .expect("an intro");
        assert_eq!(intro.source, MarkerSource::Community);
        assert!((intro.start_secs - 439.0).abs() < 10.0, "{intro:?}");
        assert!(m.iter().any(|x| x.kind == MarkerKind::Recap));
        // Cached: a second call answers without the network, identically.
        assert_eq!(markers((247_718, 1, 9), Some(2858.0)), m);
        // Nothing submitted is an empty answer, not an error.
        assert!(markers((322_430, 1, 12), Some(1514.0)).is_empty());
    }
}
