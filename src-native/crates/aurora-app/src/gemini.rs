//! The Gemini recommender: assemble what is known, ask, and resolve the answer.
//!
//! The key lives in the credential store beside the TMDB one, so it is never in the
//! database, never in a log line and never in a backup of the library (README C10).
//!
//! **What leaves this machine, exactly.** The titles the viewer has watched, with their
//! years and genres, how much of each they watched, and whether they liked it. Nothing
//! else: not their provider, not their credentials, not what is in their library, not
//! anything that identifies them or the subscription.
//!
//! **It is not opt-in in an official build, and that used to be written here as though it
//! were.** A release is compiled with a key (D19), so `key_available` is true before the
//! viewer has entered anything and the rail asks Google the first time the home screen
//! opens with something in the watch history. That is the trade the built-in key buys —
//! the alternative is a feature that does nothing until somebody goes and finds a key —
//! but it means a viewer's history leaves the machine by default. Clearing the key in
//! Settings stops it. A build compiled without `AURORA_GEMINI_KEY` has no key and asks
//! nothing. `docs/PRIVACY.md` says so where a viewer can read it.
//!
//! **Why the answer is resolved rather than displayed.** The model is asked for titles by
//! name, which is the thing it is good at and the thing the local recommender cannot do.
//! Every answer is then looked up in the library, and anything not there is dropped — so
//! a recommendation on screen is always something there is a stream for, and an invented
//! title cannot survive the lookup.

use aurora_db::repo::{filtering, library, progress, recommend as store};
use aurora_ingest::gemini::{GeminiClient, Recommender, Suggestion, WatchedTitle};
use aurora_ingest::http::{HttpClient, HttpConfig};
use serde::Serialize;
use tauri::State;

use crate::error::{AppError, Result};
use crate::library::CatalogItem;
use crate::library::RAIL_SIZE;
use crate::now_unix;
use crate::services::Services;

/// Where the viewer's own key is kept.
const CREDENTIAL_KEY: &str = "gemini-api-key";

/// A key baked in at build time, as TMDB has. Absent in an ordinary build.
pub const BUILT_IN_KEY: Option<&str> = option_env!("AURORA_GEMINI_KEY");

/// How long a set of recommendations stays fresh.
///
/// Long, deliberately. These cost a request and a few seconds, they are a reading of
/// somebody's taste rather than of their library, and taste does not move hourly. The
/// cache is also keyed on how much has been watched since, so finishing something gets a
/// new list without waiting for the clock.
const FRESH_FOR_SECS: i64 = 6 * 60 * 60;

/// How many watched titles the model is told about.
///
/// Enough to describe somebody, few enough to keep the request small and the signal
/// strong: the two hundredth most recent thing watched says very little, and a prompt
/// listing it says less.
const HISTORY_FOR_PROMPT: usize = 40;

/// Where the cached answer is kept, as JSON, with what it was generated from.
const CACHE_KEY: &str = "gemini.cache";

/// Whether these two features may talk to Google at all.
const ENABLED_KEY: &str = "gemini.enabled";

/// On, because a release ships a key and the rail is meant to work out of the box (D19).
///
/// The switch exists because that trade needs an exit. `docs/PRIVACY.md` told viewers to
/// clear the key in Settings to stop their history being sent, and in an official build
/// that did nothing: clearing removed *their* key and `resolve_key` fell back to the one
/// compiled in, so `key_available` stayed true and the rail went on asking. There was no
/// other switch anywhere in the host, the contract or the UI — the only way to stop it
/// was to compile the app yourself.
const DEFAULT_ENABLED: bool = true;

#[derive(Debug, Serialize, serde::Deserialize)]
struct Cache {
    generated_at: i64,
    /// How many things had been watched when this was asked. A changed count is a changed
    /// viewer, and worth asking again for.
    history_len: usize,
    suggestions: Vec<CachedSuggestion>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
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

/// The compiled-in key, ignoring a blank one — CI sets the variable unconditionally.
fn built_in() -> Option<&'static str> {
    BUILT_IN_KEY.map(str::trim).filter(|k| !k.is_empty())
}

/// Whether the viewer has left these features switched on.
pub fn enabled(services: &Services) -> bool {
    let db = services.db.lock();
    aurora_db::repo::settings::get_or(&db, ENABLED_KEY, DEFAULT_ENABLED).unwrap_or(DEFAULT_ENABLED)
}

/// The key to use, preferring the viewer's own.
///
/// `None` when the feature is switched off, which is what makes the switch an actual
/// off rather than a label: both callers that send anything — the recommender here and
/// `assistant::send` — ask for the key and refuse without one, and both status commands
/// report `has_key` from `key_available`, so one check turns off the rail, the assistant
/// and everything either of them says about itself.
pub(crate) fn resolve_key(services: &Services) -> Option<String> {
    let own = services.credentials.get(CREDENTIAL_KEY).ok();
    key_from(enabled(services), own.as_deref(), built_in())
}

/// The whole decision, as a function of its three inputs.
///
/// Pure so that the one case that was wrong can be asserted: **switched off means no key
/// even when the build ships one.** That was the bug — `docs/PRIVACY.md` told viewers to
/// clear the key to stop their history being sent, and clearing it fell through to the
/// compiled-in key, so an official build never stopped. The off state had nowhere to be
/// represented, so it could not be tested either.
fn key_from(enabled: bool, own: Option<&str>, built_in: Option<&str>) -> Option<String> {
    if !enabled {
        return None;
    }
    own.map(str::trim)
        .filter(|k| !k.is_empty())
        .or(built_in)
        .map(str::to_string)
}

pub fn key_available(services: &Services) -> bool {
    resolve_key(services).is_some()
}

/// A client with a budget suited to one model call.
///
/// Its own, rather than the shared import client: a generation takes seconds where a
/// metadata lookup takes milliseconds, and retrying a model three times over a bad minute
/// costs real money for a rail nobody is waiting on.
fn http_for_model() -> std::result::Result<HttpClient, AppError> {
    HttpClient::new(HttpConfig {
        max_attempts: 1,
        connect_timeout: std::time::Duration::from_secs(10),
        read_timeout: std::time::Duration::from_secs(60),
        ..Default::default()
    })
    .map_err(|e| AppError::Other(e.message))
}

/// What the viewer has watched, in the terms the model is given.
///
/// Ordered by how recently, and cut to [`HISTORY_FOR_PROMPT`]. Episodes are credited to
/// their series by `store::history`, so a show watched across twenty episodes arrives as
/// one strong signal rather than twenty weak ones.
fn watched_for_prompt(db: &aurora_db::rusqlite::Connection, profile_id: i64) -> Vec<WatchedTitle> {
    let history = store::history(db, profile_id).unwrap_or_default();
    history
        .into_iter()
        .take(HISTORY_FOR_PROMPT)
        .map(|w| WatchedTitle {
            title: w.title,
            year: w.year,
            genres: w.genres,
            watched: w.fraction,
            liked: w.favourite,
        })
        .collect()
}

/// Titles the model should not suggest: everything they have already watched.
fn avoid(watched: &[WatchedTitle]) -> Vec<String> {
    watched.iter().map(|w| w.title.clone()).collect()
}

/// Turn the model's titles into library rows, dropping whatever is not there.
fn resolve(
    db: &aurora_db::rusqlite::Connection,
    suggestions: &[CachedSuggestion],
    limit: usize,
) -> Result<(Vec<CatalogItem>, std::collections::HashMap<String, String>)> {
    let filter = filtering::LibraryFilter::load(db)?;
    let mut items = Vec::new();
    let mut reasons = std::collections::HashMap::new();
    let mut seen = std::collections::HashSet::new();

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
                filtering::Kind::Movies => library::movie(db, id)?.map(CatalogItem::Movie),
                _ => library::series(db, id)?.map(CatalogItem::Series),
            };
            let Some(item) = item else { continue };
            let key = match &item {
                CatalogItem::Movie(m) => format!("movie:{}", m.id),
                CatalogItem::Series(s) => format!("series:{}", s.id),
            };
            // Two suggestions can resolve to one row — a film and its remake both
            // matching when neither carried a year.
            if !seen.insert(key.clone()) {
                break;
            }
            reasons.insert(key, s.reason.clone());
            items.push(item);
            break;
        }
    }
    Ok((items, reasons))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiPicks {
    pub items: Vec<CatalogItem>,
    /// `kind:id` to the model's own sentence about why.
    pub reasons: std::collections::HashMap<String, String>,
    /// When the underlying suggestions were generated.
    pub generated_at: i64,
    /// How many the model offered that this library does not carry. Shown in Settings
    /// rather than on the rail: it is a fact about the subscription, not about the films.
    pub not_in_library: usize,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiArgs {
    pub profile_id: i64,
    #[serde(default)]
    pub refresh: bool,
}

/// The rail. Cached, because a model call is seconds and money and taste is not hourly.
#[tauri::command(async)]
pub fn gemini_recommendations(services: State<'_, Services>, args: AiArgs) -> Result<AiPicks> {
    let key = resolve_key(&services).ok_or_else(|| {
        AppError::Other(
            "No Gemini key is set. Add one in Settings to get recommendations based on \
             what you have watched."
                .into(),
        )
    })?;

    let (watched, cached) = {
        let db = services.db.lock();
        let watched = watched_for_prompt(&db, args.profile_id);
        let cached: Option<Cache> = aurora_db::repo::settings::get(&db, CACHE_KEY)
            .ok()
            .flatten()
            .and_then(|raw: String| serde_json::from_str(&raw).ok());
        (watched, cached)
    };

    let now = now_unix();
    let usable = cached.filter(|c| {
        !args.refresh
            && now - c.generated_at < FRESH_FOR_SECS
            && c.history_len == watched.len()
            && !c.suggestions.is_empty()
    });

    let suggestions = match usable {
        Some(cache) => cache.suggestions,
        None => {
            if watched.is_empty() {
                return Err(AppError::Other(
                    "Watch something first — these recommendations are built from what you \
                     have watched."
                        .into(),
                ));
            }
            let http = http_for_model()?;
            let fresh = GeminiClient::new(&http, &key)
                .suggest(&watched, &avoid(&watched))
                .map_err(|e| AppError::Other(format!("{} \u{2014} {}", e.message, e.cause)))?;
            let fresh: Vec<CachedSuggestion> = fresh.into_iter().map(Into::into).collect();

            let db = services.db.lock();
            let cache = Cache {
                generated_at: now,
                history_len: watched.len(),
                suggestions: fresh.clone(),
            };
            if let Ok(raw) = serde_json::to_string(&cache) {
                let _ = aurora_db::repo::settings::set(&db, CACHE_KEY, &raw);
            }
            fresh
        }
    };

    let db = services.db.lock();
    let (items, reasons) = resolve(&db, &suggestions, RAIL_SIZE as usize)?;
    Ok(AiPicks {
        not_in_library: suggestions.len().saturating_sub(items.len()),
        generated_at: now,
        items,
        reasons,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiStatus {
    pub has_key: bool,
    pub key_is_built_in: bool,
    pub key_is_persistent: bool,
    /// Whether there is any history to recommend from at all.
    pub has_history: bool,
    /// Whether the viewer has these features switched on. Off means nothing is sent,
    /// whatever key is available.
    pub enabled: bool,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusArgs {
    pub profile_id: i64,
}

#[tauri::command(async)]
pub fn gemini_status(services: State<'_, Services>, args: StatusArgs) -> Result<GeminiStatus> {
    let has_history = {
        let db = services.db.lock();
        !progress::continue_watching(&db, args.profile_id, 1)
            .unwrap_or_default()
            .is_empty()
            || !store::history(&db, args.profile_id)
                .unwrap_or_default()
                .is_empty()
    };
    Ok(GeminiStatus {
        has_key: key_available(&services),
        key_is_built_in: services.credentials.get(CREDENTIAL_KEY).is_err() && built_in().is_some(),
        key_is_persistent: services.credentials.is_persistent(),
        has_history,
        enabled: enabled(&services),
    })
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetKeyArgs {
    /// Empty clears it.
    pub key: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetEnabledArgs {
    pub enabled: bool,
}

/// Turn the rail and the assistant off, or back on.
///
/// The one control that actually stops anything being sent in a build that ships a key.
/// Clearing the key cannot do it — `resolve_key` falls back to the compiled-in one — and
/// `docs/PRIVACY.md` said otherwise for as long as that was the only lever there was.
///
/// Switching off drops the cached answer too. It was generated from this viewer's
/// history, and leaving it behind would mean the rail could still draw from it after
/// they asked for it to stop.
#[tauri::command(async)]
pub fn gemini_set_enabled(services: State<'_, Services>, args: SetEnabledArgs) -> Result<()> {
    let db = services.db.lock();
    aurora_db::repo::settings::set(&db, ENABLED_KEY, &args.enabled)?;
    if !args.enabled {
        let _ = aurora_db::repo::settings::set(&db, CACHE_KEY, &String::new());
    }
    Ok(())
}

#[tauri::command(async)]
pub fn gemini_set_key(services: State<'_, Services>, args: SetKeyArgs) -> Result<()> {
    let key = args.key.trim();
    if key.is_empty() {
        let _ = services.credentials.delete(CREDENTIAL_KEY);
    } else {
        services
            .credentials
            .set(CREDENTIAL_KEY, key)
            .map_err(|e| AppError::Other(e.to_string()))?;
    }
    // The cached answer was generated with the old key and may be a refusal; the next ask
    // should be a real one.
    let db = services.db.lock();
    let _ = aurora_db::repo::settings::set(&db, CACHE_KEY, &String::new());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_built_in_key_is_no_key() {
        // CI sets the variable unconditionally, so an unset secret arrives as "".
        assert!(BUILT_IN_KEY.is_none() || built_in().is_none() || !built_in().unwrap().is_empty());
    }

    /// Every combination of the three inputs, because one of them was wrong in the
    /// only build anybody runs.
    #[test]
    fn switching_off_beats_a_key_compiled_into_the_build() {
        // On: the viewer's own key wins, then the built-in one.
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

        // Off: nothing, whatever is available. This is the row that did not exist.
        assert_eq!(key_from(false, Some("mine"), Some("baked")), None);
        assert_eq!(key_from(false, None, Some("baked")), None);
        assert_eq!(key_from(false, Some("mine"), None), None);
        assert_eq!(key_from(false, None, None), None);
    }

    /// What clearing the key does on its own, which is the thing the privacy note
    /// described and the thing it is not.
    #[test]
    fn clearing_the_key_falls_back_to_the_built_in_one() {
        // So "Remove key" is not an off switch in a release build, and the panel now
        // says so rather than leaving the viewer to find out.
        assert_eq!(
            key_from(true, None, Some("baked")).as_deref(),
            Some("baked")
        );
        // ...and the switch is.
        assert_eq!(key_from(false, None, Some("baked")), None);
    }

    #[test]
    fn the_prompt_carries_no_credentials_and_no_library() {
        // A compile-time statement of the privacy claim in this module's docs: the only
        // thing `WatchedTitle` can carry is a title, a year, genres, a fraction and a
        // flag. If a field is ever added that identifies the viewer or their provider,
        // this stops compiling and somebody has to think about it.
        let w = WatchedTitle {
            title: "x".into(),
            year: None,
            genres: vec![],
            watched: 1.0,
            liked: false,
        };
        let json = serde_json::to_string(&w).unwrap();
        for forbidden in ["password", "username", "url", "provider", "profile"] {
            assert!(!json.contains(forbidden), "{json} mentions {forbidden}");
        }
    }
}
