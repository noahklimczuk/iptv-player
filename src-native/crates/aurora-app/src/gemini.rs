//! The Gemini recommender: assemble what is known, ask, and resolve the answer.
//!
//! The key lives in the credential store beside the TMDB one, so it is never in the
//! database, never in a log line and never in a backup of the library (README C10).
//!
//! **What leaves this machine, exactly.** The titles the viewer has watched, with their
//! years and genres, how much of each they watched, and whether they liked it. Nothing
//! else: not their provider, not their credentials, not what is in their library, not
//! anything that identifies them or the subscription. The request is made only when they
//! have set a key, which is the opt-in — there is no key by default and no built-in one
//! unless a build supplies it.
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

/// The key to use, preferring the viewer's own.
pub(crate) fn resolve_key(services: &Services) -> Option<String> {
    services
        .credentials
        .get(CREDENTIAL_KEY)
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .or_else(|| built_in().map(str::to_string))
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
                .map_err(|e| AppError::Other(format!("{} {}", e.message, e.cause)))?;
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
    })
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetKeyArgs {
    /// Empty clears it.
    pub key: String,
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
