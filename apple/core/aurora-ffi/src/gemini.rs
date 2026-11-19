//! The Gemini recommender: Windows' `aurora-app/src/gemini.rs`, on the shared
//! `aurora_ingest::gemini` client.
//!
//! **What leaves the device, exactly.** The titles the viewer has watched, with their
//! years and genres, how much of each they watched, and whether they liked it. Nothing
//! else: not their provider, not their credentials, not what is in their library.
//!
//! As on Windows, a build compiled with `AURORA_GEMINI_KEY` has a key before the viewer
//! has entered anything, so the rail asks Google the first time Home opens with
//! something in the watch history. The switch in Settings is what stops it; removing a
//! key the viewer entered falls back to the built-in one. Both rules are Windows' and
//! are tested below as Windows tests them.
//!
//! Every suggestion is looked up in the library and anything not there is dropped, so a
//! card on the rail is always something there is a stream for.
//!
//! The one difference in shape: Windows hands the web screen its reasons as a map keyed
//! `movie:12`. Swift gets each card with its reason attached, as the Home rails do.

use std::collections::HashSet;

use aurora_db::repo::{filtering, library, progress, recommend as store, settings};
use aurora_db::rusqlite::Connection;
use aurora_ingest::gemini::{GeminiClient, Recommender, Suggestion, WatchedTitle};
use aurora_ingest::http::{HttpClient, HttpConfig};
use serde::{Deserialize, Serialize};

use crate::error::{AuroraError, Result};
use crate::home::{CatalogItem, RailItem};
use crate::{now_unix, AuroraCore};

/// Where the viewer's own key is kept (the Keychain, through the credential store).
pub(crate) const CREDENTIAL_KEY: &str = "gemini-api-key";

/// A key baked in at build time, as the TMDB one is. Absent in an ordinary build.
const BUILT_IN_KEY: Option<&str> = option_env!("AURORA_GEMINI_KEY");

/// How long a set of recommendations stays fresh. Also keyed on what has been watched,
/// so finishing something gets a new list without waiting for the clock.
pub(crate) const FRESH_FOR_SECS: i64 = 6 * 60 * 60;

/// How many watched titles the model is told about.
pub(crate) const HISTORY_FOR_PROMPT: usize = 40;

/// Where the cached answer is kept, as JSON: the same row Windows uses.
pub(crate) const CACHE_KEY: &str = "gemini.cache";

/// Whether these features may talk to Google at all.
pub(crate) const ENABLED_KEY: &str = "gemini.enabled";

/// On, because a release ships a key and the rail is meant to work out of the box.
pub(crate) const DEFAULT_ENABLED: bool = true;

/// Cards on the rail (Windows' `library::RAIL_SIZE`).
pub(crate) const RAIL_SIZE: usize = 24;

#[derive(Debug, Serialize, Deserialize)]
struct Cache {
    generated_at: i64,
    /// What had been watched when this was asked (`taste_fingerprint`).
    taste: String,
    suggestions: Vec<CachedSuggestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedSuggestion {
    title: String,
    year: Option<i32>,
    kind: Option<String>,
    reason: String,
}

impl From<Suggestion> for CachedSuggestion {
    fn from(s: Suggestion) -> Self {
        Self {
            title: s.title,
            year: s.year,
            kind: s.kind,
            reason: s.reason,
        }
    }
}

/// What Settings → Gemini shows. Never the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct GeminiStatus {
    pub has_key: bool,
    pub key_is_built_in: bool,
    /// Whether there is any history to recommend from at all.
    pub has_history: bool,
    /// Off means nothing is sent, whatever key is available.
    pub enabled: bool,
}

/// The rail, and what Settings says about the last ask.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AiPicks {
    /// In the model's order, each with its own sentence about why.
    pub items: Vec<RailItem>,
    /// When the underlying suggestions were generated.
    pub generated_at: i64,
    /// How many the model offered that this library does not carry.
    pub not_in_library: u32,
}

/// A short, stable summary of what the viewer has watched: FNV-1a over what the prompt
/// is built from, plus the untruncated history length (Windows' `taste_fingerprint`).
fn taste_fingerprint(history_len: usize, watched: &[WatchedTitle]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
    };
    // Windows hashes a 64-bit `usize`; so do the Apple targets.
    eat(&(history_len as u64).to_le_bytes());
    for w in watched {
        eat(w.title.as_bytes());
        // Rounded to a percent: the fraction moves constantly while something plays.
        eat(&((w.watched * 100.0).round() as i64).to_le_bytes());
        eat(&[u8::from(w.liked)]);
    }
    format!("{hash:016x}")
}

/// The compiled-in key, ignoring a blank one: CI sets the variable unconditionally.
fn built_in() -> Option<&'static str> {
    BUILT_IN_KEY.map(str::trim).filter(|k| !k.is_empty())
}

/// The whole key decision. Switched off means no key, even when the build ships one.
fn key_from(enabled: bool, own: Option<&str>, built_in: Option<&str>) -> Option<String> {
    if !enabled {
        return None;
    }
    own.map(str::trim)
        .filter(|k| !k.is_empty())
        .or(built_in)
        .map(str::to_string)
}

/// A watched title as the model should see it: without the provider's language tag.
fn prompt_title(stored: &str) -> String {
    aurora_core::lang::strip_language_prefix(stored).unwrap_or_else(|| stored.to_string())
}

/// What the viewer has watched, most recent first, cut to [`HISTORY_FOR_PROMPT`].
fn watched_for_prompt(db: &Connection, profile_id: i64) -> Vec<WatchedTitle> {
    store::history(db, profile_id)
        .unwrap_or_default()
        .into_iter()
        .take(HISTORY_FOR_PROMPT)
        .map(|w| WatchedTitle {
            title: prompt_title(&w.title),
            year: w.year,
            genres: w.genres,
            watched: w.fraction,
            liked: w.favourite,
        })
        .collect()
}

/// Turn the model's titles into cards, dropping whatever the library does not carry.
fn resolve(
    db: &Connection,
    suggestions: &[CachedSuggestion],
    limit: usize,
) -> Result<Vec<RailItem>> {
    let filter = filtering::LibraryFilter::load(db)?;
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    for s in suggestions {
        if items.len() >= limit {
            break;
        }
        // A model that says "series" is usually right, but not always, so a miss on one
        // is tried as the other rather than thrown away.
        let order = match s.kind.as_deref() {
            Some("series") => [filtering::Kind::Series, filtering::Kind::Movies],
            _ => [filtering::Kind::Movies, filtering::Kind::Series],
        };
        for kind in order {
            let Some(id) = library::find_by_title(db, kind, &s.title, s.year, &filter)? else {
                continue;
            };
            let item = match kind {
                filtering::Kind::Movies => {
                    library::movie(db, id)?.map(|m| CatalogItem::Movie { movie: m.into() })
                }
                _ => library::series(db, id)?.map(|s| CatalogItem::Series { series: s.into() }),
            };
            let Some(item) = item else { continue };
            let key = match &item {
                CatalogItem::Movie { movie } => format!("movie:{}", movie.id),
                CatalogItem::Series { series } => format!("series:{}", series.id),
            };
            // Two suggestions can resolve to one row: a film and its remake both
            // matching when neither carried a year.
            if !seen.insert(key) {
                break;
            }
            items.push(RailItem {
                item,
                reason: Some(s.reason.clone()).filter(|r| !r.trim().is_empty()),
                progress: None,
            });
            break;
        }
    }
    Ok(items)
}

impl AuroraCore {
    fn gemini_enabled(&self) -> bool {
        settings::get_or(&self.db.lock(), ENABLED_KEY, DEFAULT_ENABLED).unwrap_or(DEFAULT_ENABLED)
    }

    /// The key to use, preferring the viewer's own; none when switched off.
    pub(crate) fn gemini_key(&self) -> Option<String> {
        let own = self.credentials.get(CREDENTIAL_KEY).ok();
        key_from(self.gemini_enabled(), own.as_deref(), built_in())
    }

    fn drop_gemini_cache(&self) {
        forget_cached(&self.db.lock());
    }

    /// The rail, from the cache when it is fresh and the viewer has not changed, and
    /// otherwise from `ask`. Separate from the export so tests can stand in for Google.
    pub(crate) fn ai_picks_with(
        &self,
        profile_id: i64,
        refresh: bool,
        ask: &dyn Recommender,
    ) -> Result<AiPicks> {
        let (watched, history_len, cached) = {
            let db = self.db.lock();
            let watched = watched_for_prompt(&db, profile_id);
            // The real length, not the prompt's: the truncation is what made Windows'
            // old key stop changing after forty titles.
            let history_len = store::history(&db, profile_id)
                .map(|h| h.len())
                .unwrap_or(watched.len());
            let cached: Option<Cache> = settings::get(&db, CACHE_KEY)
                .ok()
                .flatten()
                .and_then(|raw: String| serde_json::from_str(&raw).ok());
            (watched, history_len, cached)
        };

        let now = now_unix();
        let taste = taste_fingerprint(history_len, &watched);
        let usable = cached.filter(|c| {
            !refresh
                && now - c.generated_at < FRESH_FOR_SECS
                && c.taste == taste
                && !c.suggestions.is_empty()
        });

        let suggestions = match usable {
            Some(cache) => cache.suggestions,
            None => {
                if watched.is_empty() {
                    return Err(AuroraError::other(
                        "Watch something first — these recommendations are built from what \
                         you have watched.",
                    ));
                }
                let avoid: Vec<String> = watched.iter().map(|w| w.title.clone()).collect();
                let fresh: Vec<CachedSuggestion> = ask
                    .suggest(&watched, &avoid)
                    .map_err(|e| AuroraError::Network {
                        message: format!("{} \u{2014} {}", e.message, e.cause),
                    })?
                    .into_iter()
                    .map(Into::into)
                    .collect();
                let cache = Cache {
                    generated_at: now,
                    taste,
                    suggestions: fresh.clone(),
                };
                if let Ok(raw) = serde_json::to_string(&cache) {
                    let _ = settings::set(&self.db.lock(), CACHE_KEY, &raw);
                }
                fresh
            }
        };

        let items = resolve(&self.db.lock(), &suggestions, RAIL_SIZE)?;
        Ok(AiPicks {
            not_in_library: suggestions.len().saturating_sub(items.len()) as u32,
            generated_at: now,
            items,
        })
    }
}

#[uniffi::export]
impl AuroraCore {
    /// The "Because of what you watch" rail. Cached, because a model call is seconds and
    /// money and taste is not hourly. Long-running when it asks: call it off the main
    /// thread. Home shows nothing when this fails; Settings shows why.
    pub fn ai_picks(&self, profile_id: i64, refresh: bool) -> Result<AiPicks> {
        let key = self.gemini_key().ok_or_else(|| {
            AuroraError::other(
                "No Gemini key is set. Add one in Settings to get recommendations based on \
                 what you have watched.",
            )
        })?;
        // Its own budget: a generation takes seconds, and retrying a model three times
        // over a bad minute costs money for a rail nobody is waiting on.
        let http = HttpClient::new(HttpConfig {
            max_attempts: 1,
            connect_timeout: std::time::Duration::from_secs(10),
            read_timeout: std::time::Duration::from_secs(60),
            ..Default::default()
        })?;
        self.ai_picks_with(profile_id, refresh, &GeminiClient::new(&http, &key))
    }

    pub fn gemini_status(&self, profile_id: i64) -> Result<GeminiStatus> {
        let has_own = self.credentials.get(CREDENTIAL_KEY).is_ok();
        let has_history = {
            let db = self.db.lock();
            !progress::continue_watching(&db, profile_id, 1)
                .unwrap_or_default()
                .is_empty()
                || !store::history(&db, profile_id)
                    .unwrap_or_default()
                    .is_empty()
        };
        Ok(GeminiStatus {
            has_key: self.gemini_key().is_some(),
            key_is_built_in: !has_own && built_in().is_some(),
            has_history,
            enabled: self.gemini_enabled(),
        })
    }

    /// Turn the rail and the assistant off, or back on. Switching off drops the cached
    /// answer too: it was generated from this viewer's history.
    pub fn set_gemini_enabled(&self, enabled: bool) -> Result<()> {
        // Behind Settings: refused while they are locked (F-38, `require_settings`).
        crate::parental::require_settings(&self.db.lock())?;
        settings::set(&self.db.lock(), ENABLED_KEY, &enabled)?;
        if !enabled {
            self.drop_gemini_cache();
        }
        Ok(())
    }

    /// Save the viewer's own key, or clear it with a blank one (which falls back to the
    /// build's). The cached answer goes either way: it may be a refusal from the old key.
    pub fn set_gemini_key(&self, key: String) -> Result<()> {
        // Behind Settings: refused while they are locked (F-38, `require_settings`).
        crate::parental::require_settings(&self.db.lock())?;
        let key = key.trim();
        if key.is_empty() {
            let _ = self.credentials.delete(CREDENTIAL_KEY);
        } else {
            self.credentials.set(CREDENTIAL_KEY, key)?;
        }
        self.drop_gemini_cache();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::core;
    use crate::vod::VodKind;
    use aurora_core::neterr::{ErrorCode, NetFailure};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn watched(titles: &[(&str, f32, bool)]) -> Vec<WatchedTitle> {
        titles
            .iter()
            .map(|(t, frac, liked)| WatchedTitle {
                title: (*t).to_string(),
                year: None,
                genres: Vec::new(),
                watched: *frac,
                liked: *liked,
            })
            .collect()
    }

    /// Stands in for Google: answers with `titles` and counts the asks.
    struct Model {
        titles: Vec<(&'static str, &'static str)>,
        asked: AtomicUsize,
        fail: bool,
    }

    impl Model {
        fn new(titles: &[(&'static str, &'static str)]) -> Self {
            Self {
                titles: titles.to_vec(),
                asked: AtomicUsize::new(0),
                fail: false,
            }
        }
    }

    impl Recommender for Model {
        fn suggest(
            &self,
            watched: &[WatchedTitle],
            _avoid: &[String],
        ) -> std::result::Result<Vec<Suggestion>, NetFailure> {
            self.asked.fetch_add(1, Ordering::SeqCst);
            // Only what the privacy note promises is in the prompt.
            let json = serde_json::to_string(watched).unwrap();
            for forbidden in ["password", "username", "http", "provider", "profile"] {
                assert!(!json.contains(forbidden), "{json} mentions {forbidden}");
            }
            if self.fail {
                return Err(NetFailure {
                    code: ErrorCode::Unauthorized,
                    message: "Gemini refused the key".into(),
                    cause: "It may have been revoked.".into(),
                    actions: vec![],
                    retryable: false,
                });
            }
            Ok(self
                .titles
                .iter()
                .map(|(t, k)| Suggestion {
                    title: (*t).to_string(),
                    year: None,
                    reason: format!("Because you liked {}", watched[0].title),
                    kind: Some((*k).to_string()),
                })
                .collect())
        }
    }

    fn library(core: &AuroraCore) {
        let db = core.db.lock();
        db.execute(
            "INSERT INTO providers (id, name, kind, base_url, created_at) VALUES (1, 'P', 'm3u', 'http://p', 0)",
            [],
        )
        .unwrap();
        for (i, title) in ["EN Heat", "Alien", "Ronin", "Thief"].iter().enumerate() {
            db.execute(
                "INSERT INTO movies (provider_id, provider_key, title, match_key, group_title,
                                     url, added_at, last_seen_at)
                 VALUES (1, ?1, ?2, ?3, 'Films', 'http://p/m.mp4', 0, 0)",
                aurora_db::rusqlite::params![
                    format!("k{i}"),
                    title,
                    aurora_core::title::match_key(title)
                ],
            )
            .unwrap();
        }
    }

    #[test]
    fn suggestions_are_kept_only_where_the_library_has_them() {
        let (core, _dir) = core();
        library(&core);
        core.save_progress(1, VodKind::Movie, 1, 5000, 6000)
            .unwrap();
        let model = Model::new(&[
            ("Ronin", "movie"),
            ("Not In This Library", "movie"),
            // Called a series by the model, found as a film all the same.
            ("Thief", "series"),
        ]);
        let picks = core.ai_picks_with(1, false, &model).unwrap();
        let titles: Vec<String> = picks
            .items
            .iter()
            .map(|i| match &i.item {
                CatalogItem::Movie { movie } => movie.title.clone(),
                CatalogItem::Series { series } => series.title.clone(),
            })
            .collect();
        assert_eq!(titles, ["Ronin", "Thief"]);
        assert_eq!(picks.not_in_library, 1);
        // The model was told "Heat", not the provider's "EN Heat".
        assert_eq!(
            picks.items[0].reason.as_deref(),
            Some("Because you liked Heat")
        );
    }

    #[test]
    fn the_answer_is_cached_until_the_viewer_changes_or_asks_again() {
        let (core, _dir) = core();
        library(&core);
        core.save_progress(1, VodKind::Movie, 1, 5000, 6000)
            .unwrap();
        let model = Model::new(&[("Ronin", "movie")]);
        core.ai_picks_with(1, false, &model).unwrap();
        core.ai_picks_with(1, false, &model).unwrap();
        assert_eq!(
            model.asked.load(Ordering::SeqCst),
            1,
            "the second was cached"
        );

        // Watching something else is a different viewer.
        core.save_progress(1, VodKind::Movie, 2, 5000, 6000)
            .unwrap();
        core.ai_picks_with(1, false, &model).unwrap();
        assert_eq!(model.asked.load(Ordering::SeqCst), 2);

        // "Get recommendations now" always asks.
        core.ai_picks_with(1, true, &model).unwrap();
        assert_eq!(model.asked.load(Ordering::SeqCst), 3);

        // Switching off drops what was cached, so nothing is drawn from it afterwards.
        core.set_gemini_enabled(false).unwrap();
        core.set_gemini_enabled(true).unwrap();
        core.ai_picks_with(1, false, &model).unwrap();
        assert_eq!(model.asked.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn nothing_watched_is_said_and_nothing_is_sent() {
        let (core, _dir) = core();
        library(&core);
        let model = Model::new(&[("Ronin", "movie")]);
        let err = core.ai_picks_with(1, false, &model).unwrap_err();
        assert!(
            err.to_string().starts_with("Watch something first"),
            "{err}"
        );
        assert_eq!(model.asked.load(Ordering::SeqCst), 0);
        assert!(!core.gemini_status(1).unwrap().has_history);
    }

    #[test]
    fn a_refusal_reads_as_windows_words_it_and_is_not_cached() {
        let (core, _dir) = core();
        library(&core);
        core.save_progress(1, VodKind::Movie, 1, 5000, 6000)
            .unwrap();
        let mut model = Model::new(&[]);
        model.fail = true;
        let err = core.ai_picks_with(1, false, &model).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Gemini refused the key \u{2014} It may have been revoked."
        );
        core.ai_picks_with(1, false, &model).unwrap_err();
        assert_eq!(model.asked.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn the_viewers_key_is_saved_used_and_cleared_and_the_switch_beats_it() {
        let (core, _dir) = core();
        core.set_gemini_key("  mine  ".into()).unwrap();
        assert_eq!(core.gemini_key().as_deref(), Some("mine"));
        let status = core.gemini_status(1).unwrap();
        assert!(status.has_key && status.enabled && !status.key_is_built_in);

        core.set_gemini_enabled(false).unwrap();
        assert_eq!(core.gemini_key(), None);
        let status = core.gemini_status(1).unwrap();
        assert!(!status.has_key && !status.enabled);
        assert!(core
            .ai_picks(1, false)
            .unwrap_err()
            .to_string()
            .starts_with("No Gemini key is set"));

        core.set_gemini_enabled(true).unwrap();
        core.set_gemini_key(String::new()).unwrap();
        assert_eq!(core.gemini_key().as_deref(), built_in());
    }

    // Windows' own tests of the pure parts, kept word for word in what they assert.

    #[test]
    fn watching_a_forty_first_thing_changes_the_cache_key() {
        let names: Vec<String> = (0..HISTORY_FOR_PROMPT)
            .map(|i| format!("Film {i}"))
            .collect();
        let prompt = watched(
            &names
                .iter()
                .map(|t| (t.as_str(), 1.0, false))
                .collect::<Vec<_>>(),
        );
        assert_ne!(
            taste_fingerprint(40, &prompt),
            taste_fingerprint(41, &prompt)
        );
    }

    #[test]
    fn finishing_something_again_changes_the_cache_key() {
        let before = watched(&[("Heat", 0.3, false), ("Alien", 1.0, false)]);
        let after = watched(&[("Heat", 1.0, false), ("Alien", 1.0, false)]);
        assert_ne!(taste_fingerprint(2, &before), taste_fingerprint(2, &after));
        let liked = watched(&[("Heat", 1.0, true), ("Alien", 1.0, false)]);
        assert_ne!(taste_fingerprint(2, &after), taste_fingerprint(2, &liked));
    }

    #[test]
    fn a_playhead_moving_does_not_invalidate_the_cache() {
        let a = watched(&[("Heat", 0.5001, false)]);
        let b = watched(&[("Heat", 0.5004, false)]);
        assert_eq!(taste_fingerprint(1, &a), taste_fingerprint(1, &b));
    }

    #[test]
    fn the_prompt_drops_the_providers_language_tag() {
        assert_eq!(prompt_title("EN Inception"), "Inception");
        assert_eq!(prompt_title("EN ★ Inception"), "Inception");
        assert_eq!(prompt_title("Inception"), "Inception");
        assert_eq!(prompt_title("JFK"), "JFK");
    }

    #[test]
    fn switching_off_beats_a_key_compiled_into_the_build() {
        assert_eq!(
            key_from(true, Some("mine"), Some("baked")).as_deref(),
            Some("mine")
        );
        assert_eq!(
            key_from(true, None, Some("baked")).as_deref(),
            Some("baked")
        );
        assert_eq!(
            key_from(true, Some("  "), Some("baked")).as_deref(),
            Some("baked")
        );
        assert_eq!(
            key_from(true, Some(" mine "), None).as_deref(),
            Some("mine")
        );
        assert_eq!(key_from(true, None, None), None);
        assert_eq!(key_from(false, Some("mine"), Some("baked")), None);
        assert_eq!(key_from(false, None, Some("baked")), None);
        assert_eq!(key_from(false, Some("mine"), None), None);
        assert_eq!(key_from(false, None, None), None);
    }
}

/// Drop the cached suggestions, so the next ask is answered afresh (Windows'
/// `gemini::forget_cached`). Settings → Privacy calls this when a viewer clears their
/// history: the cache is a reading of that history.
pub(crate) fn forget_cached(db: &aurora_db::rusqlite::Connection) {
    let _ = settings::set(db, CACHE_KEY, &String::new());
}
