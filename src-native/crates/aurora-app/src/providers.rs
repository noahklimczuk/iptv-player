//! Provider management commands: add, validate, refresh (README §4, §13).

use aurora_core::rules::{Rule, RuleSet};
use aurora_db::rusqlite::{params, OptionalExtension};
use aurora_ingest::credentials::credential_ref;
use aurora_ingest::source::{parse_pasted_xtream, SourceKind};
use aurora_ingest::sync::{self, Phase, Progress, SyncOptions, SyncReport};
use aurora_ingest::xtream::XtreamClient;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::error::{AppError, Result};
use crate::services::Services;

/// What the user typed, before it is known to be valid.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftProvider {
    pub name: String,
    /// `xtream` or `m3u`.
    pub kind: String,
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// Result of pressing "check" in the wizard, before anything is saved.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub ok: bool,
    pub message: String,
    pub detail: Option<String>,
    pub expires_at: Option<i64>,
    pub days_until_expiry: Option<i64>,
    pub max_connections: Option<u32>,
    pub active_connections: Option<u32>,
    pub is_trial: bool,
    /// True when the paste was recognised as a full Xtream URL and split up.
    pub credentials_detected: bool,
}

/// README §13: recognise a pasted `get.php` URL and fill the form from it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedSource {
    pub kind: String,
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PastedArgs {
    pub text: String,
}

#[tauri::command]
pub fn providers_detect(args: PastedArgs) -> DetectedSource {
    match parse_pasted_xtream(&args.text) {
        Some(p) => DetectedSource {
            kind: "xtream".into(),
            url: p.base_url,
            username: Some(p.username),
            password: Some(p.password),
        },
        None => DetectedSource {
            kind: "m3u".into(),
            url: args.text.trim().to_string(),
            username: None,
            password: None,
        },
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateArgs {
    pub draft: DraftProvider,
}

/// Check credentials without saving anything, so the wizard can say yes or no live.
#[tauri::command]
pub fn providers_validate(
    services: State<'_, Services>,
    args: ValidateArgs,
) -> Result<ValidationResult> {
    let draft = args.draft;
    match draft.kind.as_str() {
        "xtream" => {
            let username = draft.username.unwrap_or_default();
            let password = draft.password.unwrap_or_default();
            let client = XtreamClient::new(&services.http, &draft.url, &username, &password);
            match client.authenticate(now_unix()) {
                Ok(status) => Ok(ValidationResult {
                    ok: true,
                    message: "Connected".into(),
                    detail: None,
                    expires_at: status.expires_at,
                    days_until_expiry: status.days_until_expiry,
                    max_connections: status.max_connections,
                    active_connections: status.active_connections,
                    is_trial: status.is_trial,
                    credentials_detected: false,
                }),
                Err(e) => Ok(ValidationResult {
                    ok: false,
                    message: e.message,
                    detail: Some(e.cause),
                    expires_at: None,
                    days_until_expiry: None,
                    max_connections: None,
                    active_connections: None,
                    is_trial: false,
                    credentials_detected: false,
                }),
            }
        }
        _ => {
            // For a plain playlist the only meaningful check is that it parses.
            match aurora_ingest::playlist::fetch(&services.http, &draft.url) {
                Ok(parsed) => {
                    let n = parsed.result.entries.len();
                    Ok(ValidationResult {
                        ok: n > 0,
                        message: if n > 0 {
                            format!("Found {n} entries")
                        } else {
                            "That playlist is empty".into()
                        },
                        detail: parsed
                            .result
                            .warnings
                            .first()
                            .map(|w| format!("line {}: {}", w.line, w.message)),
                        expires_at: None,
                        days_until_expiry: None,
                        max_connections: None,
                        active_connections: None,
                        is_trial: false,
                        credentials_detected: false,
                    })
                }
                Err(e) => Ok(ValidationResult {
                    ok: false,
                    message: e.message,
                    detail: Some(e.cause),
                    expires_at: None,
                    days_until_expiry: None,
                    max_connections: None,
                    active_connections: None,
                    is_trial: false,
                    credentials_detected: false,
                }),
            }
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedProvider {
    pub id: i64,
}

/// Persist a provider. The password goes to the OS credential store; the database
/// gets only a reference to it.
#[tauri::command]
pub fn providers_save(services: State<'_, Services>, args: ValidateArgs) -> Result<SavedProvider> {
    let draft = args.draft;
    let db = services.db.lock();

    db.execute(
        "INSERT INTO providers (name, kind, base_url, username, credential_ref, enabled, created_at)
         VALUES (?1, ?2, ?3, ?4, NULL, 1, ?5)",
        params![draft.name, draft.kind, draft.url, draft.username, now_unix()],
    )
    .map_err(aurora_db::DbError::from)?;
    let id = db.last_insert_rowid();

    if let Some(password) = draft.password.filter(|p| !p.is_empty()) {
        let key = credential_ref(id);
        services
            .credentials
            .set(&key, &password)
            .map_err(|e| AppError::Other(e.to_string()))?;
        db.execute(
            "UPDATE providers SET credential_ref = ?2 WHERE id = ?1",
            params![id, key],
        )
        .map_err(aurora_db::DbError::from)?;
    }
    Ok(SavedProvider { id })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshArgs {
    pub provider_id: i64,
}

/// Run a full refresh, emitting `ingest.progress` as it goes.
#[tauri::command]
pub fn providers_refresh(
    app: tauri::AppHandle,
    services: State<'_, Services>,
    args: RefreshArgs,
) -> Result<SyncReport> {
    let (kind, base_url, username, credential_ref_value) = {
        let db = services.db.lock();
        db.query_row(
            "SELECT kind, base_url, COALESCE(username, '') , credential_ref
             FROM providers WHERE id = ?1",
            params![args.provider_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
        .map_err(aurora_db::DbError::from)?
        .ok_or_else(|| AppError::Other(format!("no provider {}", args.provider_id)))?
    };

    let source = match kind.as_str() {
        "xtream" => SourceKind::Xtream {
            base_url,
            username: username.clone(),
        },
        _ => SourceKind::M3u { url: base_url },
    };

    let mut options = SyncOptions::new(args.provider_id, source, now_unix());
    options.password = credential_ref_value.and_then(|key| services.credentials.get(&key).ok());

    let rules = load_rules(&services)?;

    let mut db = services.db.lock();
    let report = sync::run(&mut db, &services.http, &options, &rules, |p: Progress| {
        // Best-effort: a dropped progress event must never fail an import.
        let _ = app.emit("ingest.progress", &p);
    })
    .map_err(|e| AppError::Other(format!("{}: {}", e.message, e.cause)))?;

    db.execute(
        "UPDATE providers SET last_refresh_at = ?2 WHERE id = ?1",
        params![args.provider_id, now_unix()],
    )
    .map_err(aurora_db::DbError::from)?;

    // Fresh guide data is the moment a series rule can find new episodes. Doing it here
    // rather than on a timer means the schedule is right as soon as the import is.
    match crate::dvr::expand_rules_now(&db, now_unix()) {
        Ok(added) if !added.is_empty() => {
            tracing::info!("series rules scheduled {} new recordings", added.len());
        }
        Ok(_) => {}
        // A rule that could not be expanded must not fail the import the user waited for.
        Err(e) => tracing::warn!("could not expand series rules after refresh: {e}"),
    }

    let _ = app.emit(
        "library.refreshed",
        &serde_json::json!({
            "added": report.channels + report.movies + report.episodes,
            "removed": report.channels_missing,
            "updated": 0,
        }),
    );
    let _ = Phase::Done;
    Ok(report)
}

fn load_rules(services: &Services) -> Result<RuleSet> {
    let db = services.db.lock();
    let mut stmt = db
        .prepare("SELECT definition FROM rules WHERE enabled = 1 ORDER BY sort_order")
        .map_err(aurora_db::DbError::from)?;
    let defs = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(aurora_db::DbError::from)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(aurora_db::DbError::from)?;

    let rules: Vec<Rule> = defs
        .iter()
        .filter_map(|d| serde_json::from_str(d).ok())
        .collect();
    RuleSet::compile(&rules).map_err(AppError::Core)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_a_pasted_xtream_url() {
        let got = providers_detect(PastedArgs {
            text: "http://example.com:8080/get.php?username=alice&password=hunter2".into(),
        });
        assert_eq!(got.kind, "xtream");
        assert_eq!(got.url, "http://example.com:8080");
        assert_eq!(got.username.as_deref(), Some("alice"));
    }

    #[test]
    fn a_plain_playlist_url_stays_an_m3u_source() {
        let got = providers_detect(PastedArgs {
            text: "  http://example.com/list.m3u  ".into(),
        });
        assert_eq!(got.kind, "m3u");
        assert_eq!(got.url, "http://example.com/list.m3u");
        assert!(got.username.is_none());
    }
}
