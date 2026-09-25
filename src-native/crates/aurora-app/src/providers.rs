//! Provider management commands: add, validate, refresh (README §4, §13).

use aurora_core::rules::{Rule, RuleSet};
use aurora_db::rusqlite::{params, OptionalExtension};
use aurora_ingest::credentials::credential_ref;
use aurora_ingest::source::{looks_like_panel_root, parse_pasted_xtream, SourceKind};
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

#[tauri::command(async)]
pub fn providers_detect(args: PastedArgs) -> DetectedSource {
    match parse_pasted_xtream(&args.text) {
        Some(p) => DetectedSource {
            kind: "xtream".into(),
            url: p.base_url,
            username: Some(p.username),
            password: Some(p.password),
        },
        // A bare host carries no credentials to find, but its shape says what it is:
        // a panel root, with the username and password written on a separate line of
        // whatever the provider sent. Defaulting to Xtream is what makes those
        // credentials enterable at all — the wizard only offers the fields for a
        // provider it believes has them.
        None if looks_like_panel_root(&args.text) => DetectedSource {
            kind: "xtream".into(),
            url: args.text.trim().trim_end_matches('/').to_string(),
            username: None,
            password: None,
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
#[tauri::command(async)]
pub fn providers_validate(
    services: State<'_, Services>,
    args: ValidateArgs,
) -> Result<ValidationResult> {
    let draft = args.draft;
    match draft.kind.as_str() {
        "xtream" => {
            let username = draft.username.unwrap_or_default();
            let password = draft.password.unwrap_or_default();
            // Saying so beats sending it. A panel asked to sign in with a blank field
            // answers HTTP 200 and an empty body, which comes back through the error
            // taxonomy as the provider having sent something unreadable — the provider
            // blamed for a field this form left empty.
            if let Err(e) = aurora_ingest::sync::missing_sign_in(&username, &password) {
                return Ok(ValidationResult {
                    ok: false,
                    message: e.message,
                    detail: Some(e.cause),
                    expires_at: None,
                    days_until_expiry: None,
                    max_connections: None,
                    active_connections: None,
                    is_trial: false,
                    credentials_detected: false,
                });
            }
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
#[tauri::command(async)]
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

/* ── Editing an existing provider ─────────────────────────────────────────── */

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderIdArgs {
    pub provider_id: i64,
}

/// A provider's settings, password included, for the edit form.
///
/// This is the one command that hands a stored secret back to the UI, and it is
/// deliberate: it is the viewer's own subscription password, on their own machine, and
/// an account they cannot re-read is one they cannot correct after a typo or a provider
/// rotation. README C10 says a credential must never appear in a log, an export or an
/// error — none of which is this. It is never fetched to render a list; only the edit
/// form asks for it, and only when opened.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCredentials {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub url: String,
    pub username: String,
    /// Absent when the provider never had one, or when the store has lost it — an M3U
    /// URL carries no password, and a credential store can be cleared underneath us.
    pub password: Option<String>,
    /// False when the store cannot keep secrets across a restart, so the form can warn
    /// before someone retypes a password that will not survive.
    pub password_is_persistent: bool,
}

#[tauri::command(async)]
pub fn providers_credentials(
    services: State<'_, Services>,
    args: ProviderIdArgs,
) -> Result<ProviderCredentials> {
    let (name, kind, url, username, credential): (String, String, String, String, Option<String>) = {
        let db = services.db.lock();
        db.query_row(
            "SELECT name, kind, base_url, COALESCE(username, ''), credential_ref
             FROM providers WHERE id = ?1",
            params![args.provider_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()
        .map_err(aurora_db::DbError::from)?
        .ok_or_else(|| AppError::Other(format!("no provider {}", args.provider_id)))?
    };

    Ok(ProviderCredentials {
        id: args.provider_id,
        name,
        kind,
        url,
        username,
        // A missing secret is not an error: the provider may simply not have one.
        password: credential.and_then(|key| services.credentials.get(&key).ok()),
        password_is_persistent: services.credentials.is_persistent(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateArgs {
    pub provider_id: i64,
    pub draft: DraftProvider,
}

/// Save edits to an existing provider.
///
/// The password follows the same convention as everywhere else in this app: `None`
/// leaves the stored one alone, `Some("")` clears it, and anything else replaces it.
/// Without that distinction, an edit form that shows a masked placeholder would wipe
/// the password every time someone changed only the name.
#[tauri::command(async)]
pub fn providers_update(services: State<'_, Services>, args: UpdateArgs) -> Result<bool> {
    let id = args.provider_id;
    let draft = args.draft;
    let db = services.db.lock();

    let changed = db
        .execute(
            "UPDATE providers SET name = ?2, kind = ?3, base_url = ?4, username = ?5
             WHERE id = ?1",
            params![id, draft.name, draft.kind, draft.url, draft.username],
        )
        .map_err(aurora_db::DbError::from)?;
    if changed == 0 {
        return Err(AppError::Other(format!("no provider {id}")));
    }

    let key = credential_ref(id);
    match draft.password {
        None => {}
        Some(p) if p.is_empty() => {
            let _ = services.credentials.delete(&key);
            db.execute(
                "UPDATE providers SET credential_ref = NULL WHERE id = ?1",
                params![id],
            )
            .map_err(aurora_db::DbError::from)?;
        }
        Some(password) => {
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
    }
    Ok(true)
}

/// Remove a provider, its credential, and everything it imported.
///
/// The library rows go with it through `ON DELETE CASCADE`, which is what someone
/// removing a provider means — leaving forty thousand orphaned channels behind would
/// be the surprising outcome. The credential is deleted explicitly, because the
/// credential store is not in the database and nothing cascades into Windows
/// Credential Manager; skipping it would leave the password on the machine after the
/// account it belongs to is gone.
#[tauri::command(async)]
pub fn providers_delete(services: State<'_, Services>, args: ProviderIdArgs) -> Result<bool> {
    let id = args.provider_id;
    let db = services.db.lock();

    let credential: Option<String> = db
        .query_row(
            "SELECT credential_ref FROM providers WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()
        .map_err(aurora_db::DbError::from)?
        .flatten();

    let removed = db
        .execute("DELETE FROM providers WHERE id = ?1", params![id])
        .map_err(aurora_db::DbError::from)?;
    if removed == 0 {
        return Ok(false);
    }

    if let Some(key) = credential {
        // A store that has already forgotten it is fine; the point is that nothing is
        // left behind, not that a delete succeeded.
        let _ = services.credentials.delete(&key);
    }
    Ok(true)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshArgs {
    pub provider_id: i64,
}

/// The password behind a provider's `credential_ref`, if it has one.
///
/// Not `.ok()`, which is what this was. A provider with no reference has no password by
/// design — an M3U URL carries its own — but one that *has* a reference the store will
/// not give back is a different thing entirely, and swallowing that difference sent the
/// import off to sign in with an empty password. The panel then answers with an empty
/// body, and the viewer is told their provider sent something Aurora could not read.
fn stored_password(
    store: &dyn aurora_ingest::credentials::CredentialStore,
    credential_ref_value: Option<String>,
) -> Result<Option<String>> {
    match credential_ref_value {
        None => Ok(None),
        Some(key) => store.get(&key).map(Some).map_err(|e| {
            AppError::Other(format!(
                "Aurora could not read this provider's saved password: {e}. Edit the \
                 provider in Settings and enter it again."
            ))
        }),
    }
}

/// Run a full refresh, emitting `ingest.progress` as it goes.
#[tauri::command(async)]
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
    options.password = stored_password(services.credentials.as_ref(), credential_ref_value)?;

    let rules = load_rules(&services)?;

    // The download happens with no lock held. It used to run inside one, which froze
    // every other command for the length of a playlist and a guide — and because the
    // DVR scheduler takes the same lock every ten seconds to ask whether a recording is
    // due, a recording falling inside a long refresh simply did not start.
    //
    // `sync::fetch` is handed no database, so it cannot reintroduce that by accident.
    let emit = |p: Progress| {
        // Best-effort: a dropped progress event must never fail an import.
        let _ = app.emit("ingest.progress", &p);
    };
    let fetched = sync::fetch(&services.http, &options, &rules, emit)
        .map_err(|e| AppError::Other(format!("{}: {}", e.message, e.cause)))?;

    // Only the writes hold it, and they are local and bounded.
    let mut db = services.db.lock();
    let report = sync::apply(&mut db, fetched, &options, emit)
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

    use aurora_ingest::credentials::{credential_ref, CredentialStore, MemoryStore};

    #[test]
    fn a_provider_with_no_stored_credential_simply_has_none() {
        // An M3U provider: the URL carries whatever it needs. Not an error.
        let store = MemoryStore::default();
        assert_eq!(stored_password(&store, None).unwrap(), None);
    }

    #[test]
    fn a_stored_credential_comes_back_as_itself() {
        let store = MemoryStore::default();
        store.set(&credential_ref(7), "hunter2").unwrap();
        assert_eq!(
            stored_password(&store, Some(credential_ref(7)))
                .unwrap()
                .as_deref(),
            Some("hunter2")
        );
    }

    #[test]
    fn a_credential_the_store_will_not_give_back_is_an_error_not_an_empty_password() {
        // The case this exists for: the row says there is a password, and Credential
        // Manager disagrees. Importing anyway signs in with nothing and the panel gets
        // blamed for the empty answer.
        let store = MemoryStore::default();
        let err = stored_password(&store, Some(credential_ref(7)))
            .unwrap_err()
            .to_string();
        assert!(err.contains("could not read"), "{err}");
        assert!(err.contains("Settings"), "it has to say what to do: {err}");
    }

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
    fn a_bare_panel_host_offers_the_credential_fields() {
        // The shape on a provider's credentials card: a host on one line, the username
        // and password on the next two. There is nothing in the URL to parse, so the
        // only thing that makes those fields appear is recognising the shape.
        let got = providers_detect(PastedArgs {
            text: "http://panel.example.com".into(),
        });
        assert_eq!(got.kind, "xtream");
        assert_eq!(got.url, "http://panel.example.com");
        assert_eq!(
            got.username, None,
            "there is nothing to fill in, only to offer"
        );
        assert_eq!(got.password, None);
    }

    #[test]
    fn a_trailing_slash_is_not_carried_into_the_base_url() {
        let got = providers_detect(PastedArgs {
            text: "http://panel.example.com/".into(),
        });
        assert_eq!(got.kind, "xtream");
        assert_eq!(got.url, "http://panel.example.com");
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
