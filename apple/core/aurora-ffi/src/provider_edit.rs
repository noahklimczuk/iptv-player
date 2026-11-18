//! Checking, reading back and editing a provider: Windows' `providers_detect`,
//! `providers_validate`, `providers_credentials` and `providers_update`
//! (`aurora-app/src/providers.rs`), for the add screen and the provider editor.
//!
//! **Kept in step by hand.** The probes are where the work is: a check that fails asks,
//! once and without credentials, whether the same host answers under the other scheme;
//! an Xtream panel whose API refuses is asked whether its playlist works; and a panel
//! that is refusing *us* (rate limit, connection limit, hang-up) is not asked anything
//! more. `mirrors.rs` checks those three lists of error codes, and the playlist address,
//! against `aurora-app`.
//!
//! One part differs on purpose. Windows names the antivirus products installed on the PC
//! when a login is cut locally; an iPhone or Apple TV has none of those, so the same
//! diagnosis names what does the same thing here (a VPN, a filtering DNS, a network's
//! content filter).

use aurora_core::neterr::{ErrorAction, ErrorCode, NetFailure};
use aurora_db::rusqlite::{params, OptionalExtension};
use aurora_ingest::credentials::{credential_ref, namespace};
use aurora_ingest::http::{HttpClient, HttpConfig};
use aurora_ingest::source::{looks_like_panel_root, parse_pasted_xtream};
use aurora_ingest::xtream::XtreamClient;

use crate::error::{AuroraError, Result};
use crate::{now_unix, AuroraCore};

/// What the form holds, before it is known to work (`DraftProvider`).
#[derive(Debug, Clone, uniffi::Record)]
pub struct ProviderDraft {
    pub name: String,
    /// `"xtream"` or `"m3u"`.
    pub kind: String,
    pub url: String,
    pub username: Option<String>,
    /// `None` leaves a saved password alone, `""` clears it, anything else replaces it.
    pub password: Option<String>,
}

/// The answer to "Check connection" (`ValidationResult`).
#[derive(Debug, Clone, uniffi::Record)]
pub struct ProviderCheck {
    pub ok: bool,
    pub message: String,
    pub detail: Option<String>,
    pub expires_at: Option<i64>,
    pub days_until_expiry: Option<i64>,
    pub max_connections: Option<u32>,
    pub active_connections: Option<u32>,
    pub is_trial: bool,
    /// The same host under the other scheme, or (with `suggested_kind` `"m3u"`) the
    /// panel's playlist, when what was checked did not work and that does.
    pub suggested_url: Option<String>,
    pub suggested_kind: Option<String>,
}

impl ProviderCheck {
    fn failed(e: NetFailure) -> Self {
        Self {
            ok: false,
            message: e.message,
            detail: Some(e.cause),
            expires_at: None,
            days_until_expiry: None,
            max_connections: None,
            active_connections: None,
            is_trial: false,
            suggested_url: None,
            suggested_kind: None,
        }
    }
}

/// What a pasted address turned out to be (`DetectedSource`).
#[derive(Debug, Clone, uniffi::Record)]
pub struct DetectedSource {
    pub kind: String,
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// A provider's settings, password included, for the edit form (`ProviderCredentials`).
///
/// The one call that hands a saved password back. It is the viewer's own subscription on
/// their own device, and an account they cannot re-read is one they cannot correct after
/// a typo; only the editor asks for it, and it starts hidden there.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ProviderSettings {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub url: String,
    pub username: String,
    /// Absent when the provider never had one, or the Keychain has lost it.
    pub password: Option<String>,
}

/// How long after the last keystroke an address typed into the form is asked about
/// (`DETECT_AFTER_TYPING_MS` in Windows' setup wizard). Asked on every keystroke, a
/// link typed by hand is split at the first letter of its password.
pub const DETECT_AFTER_TYPING_MS: u32 = 200;

#[uniffi::export]
pub fn detect_after_typing_ms() -> u32 {
    DETECT_AFTER_TYPING_MS
}

/// Recognise a pasted `get.php` / `player_api.php` address and split out the sign-in
/// (`providers_detect`). A bare panel root is taken as Xtream with the sign-in still to
/// type; anything else is a playlist address.
#[uniffi::export]
pub fn detect_source(text: String) -> DetectedSource {
    match parse_pasted_xtream(&text) {
        Some(p) => DetectedSource {
            kind: "xtream".into(),
            url: p.base_url,
            username: Some(p.username),
            password: Some(p.password),
        },
        None if looks_like_panel_root(&text) => DetectedSource {
            kind: "xtream".into(),
            url: text.trim().trim_end_matches('/').to_string(),
            username: None,
            password: None,
        },
        None => DetectedSource {
            kind: "m3u".into(),
            url: text.trim().to_string(),
            username: None,
            password: None,
        },
    }
}

/// The same address under the other scheme (`other_scheme`).
fn other_scheme(url: &str) -> Option<String> {
    let url = url.trim();
    if let Some(rest) = url.strip_prefix("https://") {
        Some(format!("http://{rest}"))
    } else {
        url.strip_prefix("http://").map(|rest| format!("https://{rest}"))
    }
}

/// Failures where the address itself may be wrong (`worth_probing`).
fn worth_probing(code: ErrorCode) -> bool {
    matches!(code, ErrorCode::Timeout | ErrorCode::Refused | ErrorCode::Dns | ErrorCode::Tls)
}

/// The panel heard and declined (`is_refusing_us`): asking again only makes it worse.
fn is_refusing_us(code: ErrorCode) -> bool {
    matches!(code, ErrorCode::Dropped | ErrorCode::RateLimited | ErrorCode::ConnectionLimit)
}

/// Does the host answer a request with no sign-in in it
/// (`host_answers_without_credentials`)?
fn host_answers_without_credentials(http: &HttpClient, base_url: &str) -> bool {
    let url = format!("{}/player_api.php", base_url.trim_end_matches('/'));
    match http.fetch_bytes(&url) {
        Ok(_) => true,
        Err(e) => !matches!(
            e.code,
            ErrorCode::Dropped | ErrorCode::Timeout | ErrorCode::Dns | ErrorCode::Refused
        ),
    }
}

/// The host is up and only signed-in requests die (`locally_blocked`), in the words
/// that fit an iPhone or Apple TV.
fn locally_blocked(url: &str, took: std::time::Duration) -> NetFailure {
    let host = url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or(url);
    NetFailure {
        code: ErrorCode::Dropped,
        message: "Something on this network is blocking the sign-in".into(),
        cause: format!(
            "{host} answers normally when asked without a username and password, and \
             drops the connection the moment one carries both ({} ms). That is not your \
             provider being down. A VPN, a filtering DNS or a network's content filter \
             commonly blocks sign-ins sent over plain http:// — try with it switched off, \
             or on another network.",
            took.as_millis()
        ),
        actions: vec![ErrorAction::Retry, ErrorAction::OpenSettings],
        retryable: true,
    }
}

/// One attempt and a short wait, for a question asked while someone watches a spinner
/// (`probe_client`).
fn probe_client() -> Option<HttpClient> {
    HttpClient::new(HttpConfig {
        max_attempts: 1,
        connect_timeout: std::time::Duration::from_secs(8),
        read_timeout: std::time::Duration::from_secs(20),
        ..Default::default()
    })
    .ok()
}

/// Does the host answer under the other scheme? Asked without credentials
/// (`probe_other_scheme`).
fn probe_other_scheme(http: &HttpClient, url: &str) -> Option<String> {
    let alternative = other_scheme(url)?;
    let probe = format!("{}/player_api.php", alternative.trim_end_matches('/'));
    match http.fetch_bytes(&probe) {
        Ok(_) => Some(alternative),
        Err(e) if !worth_probing(e.code) => Some(alternative),
        Err(_) => None,
    }
}

/// The playlist an Xtream panel serves the same line at (`m3u_url`).
fn m3u_url(base_url: &str, username: &str, password: &str) -> String {
    format!(
        "{}/get.php?username={}&password={}&type=m3u_plus&output=ts",
        base_url.trim_end_matches('/'),
        urlencoding(username),
        urlencoding(password),
    )
}

fn urlencoding(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Does the panel's playlist work although its API does not (`probe_m3u`)?
fn probe_m3u(http: &HttpClient, base_url: &str, username: &str, password: &str) -> Option<String> {
    let url = m3u_url(base_url, username, password);
    let parsed = aurora_ingest::playlist::fetch(http, &url).ok()?;
    (!parsed.result.entries.is_empty()).then_some(url)
}

#[uniffi::export]
impl AuroraCore {
    /// Check what is in the form without saving it (`providers_validate`).
    pub fn check_provider(&self, draft: ProviderDraft) -> ProviderCheck {
        if draft.kind == "xtream" {
            self.check_xtream(&draft)
        } else {
            self.check_playlist(&draft.url)
        }
    }

    /// A provider's saved settings and password, for the editor
    /// (`providers_credentials`).
    pub fn provider_settings(&self, provider_id: i64) -> Result<ProviderSettings> {
        let (name, kind, url, username, credential): (String, String, String, String, Option<String>) = {
            let db = self.db.lock();
            db.query_row(
                "SELECT name, kind, base_url, COALESCE(username, ''), credential_ref
                 FROM providers WHERE id = ?1",
                params![provider_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?
            .ok_or_else(|| AuroraError::other("That provider has been removed."))?
        };
        Ok(ProviderSettings {
            id: provider_id,
            name,
            kind,
            url,
            username,
            password: credential.and_then(|key| self.credentials.get(&key).ok()),
        })
    }

    /// Save edits to a provider (`providers_update`). The password follows the app's
    /// rule: `None` keeps the saved one, `""` clears it, anything else replaces it.
    /// Imports nothing: refresh afterwards to see what the new settings bring in.
    pub fn update_provider(&self, provider_id: i64, draft: ProviderDraft) -> Result<()> {
        let url = draft.url.trim();
        if url.is_empty() {
            return Err(AuroraError::other("Enter the provider's address."));
        }
        let name = if draft.name.trim().is_empty() { url } else { draft.name.trim() };
        let username = draft.username.as_deref().map(str::trim);
        let db = self.db.lock();
        let changed = db.execute(
            "UPDATE providers SET name = ?2, kind = ?3, base_url = ?4, username = ?5
             WHERE id = ?1",
            params![provider_id, name, draft.kind, url, username],
        )?;
        if changed == 0 {
            return Err(AuroraError::other("That provider has been removed."));
        }

        // Where the secret is filed now; a provider saved before namespacing used the
        // old name, which is tidied away below once nothing points at it.
        let previous: Option<String> = db.query_row(
            "SELECT credential_ref FROM providers WHERE id = ?1",
            params![provider_id],
            |r| r.get(0),
        )?;
        let key = credential_ref(&namespace(&db)?, provider_id);
        match draft.password {
            None => {}
            Some(p) if p.is_empty() => {
                let _ = self.credentials.delete(&key);
                if let Some(old) = previous.as_deref().filter(|old| *old != key) {
                    let _ = self.credentials.delete(old);
                }
                db.execute(
                    "UPDATE providers SET credential_ref = NULL WHERE id = ?1",
                    params![provider_id],
                )?;
            }
            Some(password) => {
                self.credentials.set(&key, &password)?;
                db.execute(
                    "UPDATE providers SET credential_ref = ?2 WHERE id = ?1",
                    params![provider_id, key],
                )?;
                if let Some(old) = previous.as_deref().filter(|old| *old != key) {
                    let _ = self.credentials.delete(old);
                }
            }
        }
        Ok(())
    }
}

impl AuroraCore {
    fn check_xtream(&self, draft: &ProviderDraft) -> ProviderCheck {
        let username = draft.username.clone().unwrap_or_default();
        let password = draft.password.clone().unwrap_or_default();
        // Say a field is empty rather than send it: a panel asked to sign in blank
        // answers with an empty page, which would read as the provider's fault.
        if let Err(e) = aurora_ingest::sync::missing_sign_in(&username, &password) {
            return ProviderCheck::failed(e);
        }
        let one_shot = probe_client();
        let http = one_shot.as_ref().unwrap_or(&self.http);
        let client = XtreamClient::new(http, &draft.url, &username, &password);
        let started = std::time::Instant::now();
        let mut outcome = client.authenticate(now_unix());
        let took = started.elapsed();

        // Cut faster than a round trip, by something that lets unsigned requests through.
        if let Err(e) = &outcome {
            if e.code == ErrorCode::Dropped && host_answers_without_credentials(http, &draft.url) {
                outcome = Err(locally_blocked(&draft.url, took));
            }
        }

        let mut suggested_kind = None;
        let suggested_url = match (&outcome, one_shot.as_ref()) {
            (Err(e), _) if is_refusing_us(e.code) => None,
            (Err(e), Some(http)) if worth_probing(e.code) => probe_other_scheme(http, &draft.url),
            (Err(_), Some(http)) => {
                let found = probe_m3u(http, &draft.url, &username, &password);
                if found.is_some() {
                    suggested_kind = Some("m3u".to_string());
                }
                found
            }
            _ => None,
        };
        match outcome {
            Ok(status) => ProviderCheck {
                ok: true,
                message: "Connected".into(),
                detail: None,
                expires_at: status.expires_at,
                days_until_expiry: status.days_until_expiry,
                max_connections: status.max_connections,
                active_connections: status.active_connections,
                is_trial: status.is_trial,
                suggested_url: None,
                suggested_kind: None,
            },
            Err(e) => {
                tracing::warn!(code = ?e.code, "provider check failed: {}", e.message);
                ProviderCheck { suggested_url, suggested_kind, ..ProviderCheck::failed(e) }
            }
        }
    }

    /// A plain playlist works if it parses into something.
    fn check_playlist(&self, url: &str) -> ProviderCheck {
        match aurora_ingest::playlist::fetch(&self.http, url.trim()) {
            Ok(parsed) => {
                let n = parsed.result.entries.len();
                ProviderCheck {
                    ok: n > 0,
                    message: if n > 0 { format!("Found {n} entries") } else { "That playlist is empty".into() },
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
                    suggested_url: None,
                    suggested_kind: None,
                }
            }
            Err(e) => ProviderCheck::failed(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(kind: &str, url: &str, username: Option<&str>, password: Option<&str>) -> ProviderDraft {
        ProviderDraft {
            name: "Panel".into(),
            kind: kind.into(),
            url: url.into(),
            username: username.map(Into::into),
            password: password.map(Into::into),
        }
    }

    #[test]
    fn a_pasted_playlist_address_is_split_into_a_sign_in() {
        let found = detect_source("http://panel.example:8080/get.php?username=me&password=pw&type=m3u_plus".into());
        assert_eq!(found.kind, "xtream");
        assert_eq!(found.url, "http://panel.example:8080");
        assert_eq!(found.username.as_deref(), Some("me"));
        assert_eq!(found.password.as_deref(), Some("pw"));
        assert_eq!(detect_source("https://example.com/list.m3u".into()).kind, "m3u");
    }

    #[test]
    fn the_other_scheme_and_the_playlist_address() {
        assert_eq!(other_scheme("https://a.example:80").as_deref(), Some("http://a.example:80"));
        assert_eq!(other_scheme("http://a.example").as_deref(), Some("https://a.example"));
        assert_eq!(other_scheme("a.example"), None);
        assert_eq!(
            m3u_url("http://p.example/", "a b", "p&w"),
            "http://p.example/get.php?username=a%20b&password=p%26w&type=m3u_plus&output=ts"
        );
    }

    #[test]
    fn a_blank_sign_in_is_named_without_asking_the_panel() {
        let (core, _dir) = crate::testing::core();
        let check = core.check_provider(draft("xtream", "http://127.0.0.1:9", Some("me"), Some("")));
        assert!(!check.ok);
        assert!(check.suggested_url.is_none());
    }

    #[test]
    fn editing_keeps_replaces_or_clears_the_password() {
        let (core, _dir) = crate::testing::core();
        let id = core
            .add_xtream("Old".into(), "http://old.example".into(), "me".into(), "first".into())
            .unwrap();

        // Renaming alone leaves the password where it was.
        core.update_provider(id, draft("xtream", " http://new.example ", Some(" you "), None)).unwrap();
        let s = core.provider_settings(id).unwrap();
        assert_eq!((s.name.as_str(), s.url.as_str(), s.username.as_str()), ("Panel", "http://new.example", "you"));
        assert_eq!(s.password.as_deref(), Some("first"));

        core.update_provider(id, draft("xtream", "http://new.example", Some("you"), Some("second"))).unwrap();
        assert_eq!(core.provider_settings(id).unwrap().password.as_deref(), Some("second"));

        core.update_provider(id, draft("xtream", "http://new.example", Some("you"), Some(""))).unwrap();
        assert_eq!(core.provider_settings(id).unwrap().password, None);
    }

    #[test]
    fn a_removed_provider_cannot_be_edited() {
        let (core, _dir) = crate::testing::core();
        assert!(core.provider_settings(42).is_err());
        assert!(core.update_provider(42, draft("m3u", "http://x.example/a.m3u", None, None)).is_err());
        let id = core.add_m3u("List".into(), "http://x.example/a.m3u".into()).unwrap();
        assert!(core.update_provider(id, draft("m3u", "  ", None, None)).is_err());
    }
}
