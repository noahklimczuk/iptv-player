//! Provider management commands: add, validate, refresh (README §4, §13).

use aurora_core::neterr::ErrorCode;
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
    /// The same host under the other scheme, when the given one did not answer and that
    /// one does. Many panels are published on plain HTTP, which is indistinguishable from
    /// a dead host when the address says `https` — and a viewer has no way to tell those
    /// apart from a spinner that ends in a red box.
    pub suggested_url: Option<String>,
    /// The provider type that address needs, when it is not the one being checked.
    /// `"m3u"` when an Xtream panel's API is unusable but its playlist is fine.
    pub suggested_kind: Option<String>,
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

/// The same address under the other scheme, when there is one.
///
/// `https://panel.example.com` → `http://panel.example.com`, and the reverse.
fn other_scheme(url: &str) -> Option<String> {
    let url = url.trim();
    if let Some(rest) = url.strip_prefix("https://") {
        Some(format!("http://{rest}"))
    } else {
        url.strip_prefix("http://")
            .map(|rest| format!("https://{rest}"))
    }
}

/// Whether a failure is the kind where the address itself may be the problem.
///
/// A 401 means the host is there and the credentials are wrong — trying the same host
/// under a different scheme would prove nothing and waste the viewer's time. A timeout, a
/// refused connection or a name that does not resolve are the ones worth a second look.
fn worth_probing(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::Timeout | ErrorCode::Refused | ErrorCode::Dns | ErrorCode::Tls
    )
}

/// Whether the panel is refusing *us* rather than failing to be found.
///
/// A host that hangs up, rate-limits, or reports the line's connection limit has heard
/// the request and declined it. Asking again — under another scheme, for a playlist,
/// anything — cannot answer a question it has already answered, and on a panel that is
/// throttling by volume it is the one thing guaranteed to make the situation worse.
///
/// This matters more than it looks. A failed check could previously send nine requests:
/// three for the sign-in, which is retryable and retried with backoff, then three more
/// for the scheme probe and three for the playlist probe, each of which is its own
/// retrying fetch. Against a panel already refusing this address, that is a check that
/// punishes the person running it.
fn is_refusing_us(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::Dropped | ErrorCode::RateLimited | ErrorCode::ConnectionLimit
    )
}

/// Does this host answer a request that carries no credentials?
///
/// The question that separates "the panel is down" from "something is killing my logins".
/// A reply of any kind — 200, 403, 404 — means the host is up, reachable from here, and
/// talking. If it answers that and then drops every request carrying a username and
/// password, the difference is not the panel's health: it is something acting on the
/// content of the request.
///
/// Observed on a real machine: `player_api.php` with no query returned 403 in 134 ms,
/// while the same URL with credentials was cut after 8 ms, on both of the panel's
/// addresses and under eight different User-Agents. The machine had Bitdefender, 360
/// Total Security and McAfee WebAdvisor installed, all of which inspect plaintext HTTP,
/// and the same subscription worked on a phone on the same network.
fn host_answers_without_credentials(
    http: &aurora_ingest::http::HttpClient,
    base_url: &str,
) -> bool {
    let url = format!("{}/player_api.php", base_url.trim_end_matches('/'));
    match http.fetch_bytes(&url) {
        // Any HTTP reply at all, including a refusal, is the host speaking.
        Ok(_) => true,
        Err(e) => !matches!(
            e.code,
            ErrorCode::Dropped | ErrorCode::Timeout | ErrorCode::Dns | ErrorCode::Refused
        ),
    }
}

/// Borrow whichever client is available for a diagnostic request.
fn http_for_probe<'a>(
    one_shot: &'a Option<aurora_ingest::http::HttpClient>,
    shared: &'a aurora_ingest::http::HttpClient,
) -> Option<&'a aurora_ingest::http::HttpClient> {
    Some(one_shot.as_ref().unwrap_or(shared))
}

/// The security products Windows knows about, by name.
///
/// Named rather than guessed at. "Security software such as Bitdefender, ESET or others"
/// asks somebody to go and find out what is on their own machine; "Bitdefender Antivirus
/// and 360 Total Security are running on this computer" tells them where to go. Windows
/// keeps the list in the Security Center, which is where every one of them registers.
///
/// Best effort, and the sentence reads fine without it. This only runs on the one failure
/// already diagnosed as local, which is rare and always something a person just asked for.
#[cfg(windows)]
fn installed_security_products() -> Vec<String> {
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct \
             -ErrorAction SilentlyContinue | Select-Object -ExpandProperty displayName",
        ])
        .output();
    let Ok(out) = out else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        // Windows Defender is on every machine and is not the one doing this: it does not
        // inspect plaintext HTTP for credential submission. Naming it would send people
        // to the wrong settings page.
        //
        // Matched by its full name, not by the word "defender" — which is also inside
        // *Bit*defender, the product most likely to be the actual cause. Filtering on the
        // substring hid the one name worth printing.
        .filter(|l| {
            let lower = l.to_ascii_lowercase();
            !lower.contains("windows defender") && !lower.contains("microsoft defender")
        })
        .map(str::to_string)
        .collect()
}

#[cfg(not(windows))]
fn installed_security_products() -> Vec<String> {
    Vec::new()
}

/// "A and B", "A, B and C" — a list a person reads rather than parses.
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The failure to report when the host is plainly up and only *our* logins are dying.
fn locally_blocked(url: &str, took: std::time::Duration) -> aurora_core::neterr::NetFailure {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url);
    let products = installed_security_products();
    let culprits = if products.is_empty() {
        "Security software that inspects web traffic (Bitdefender, 360 Total Security, \
         McAfee WebAdvisor, ESET and others) commonly blocks"
            .to_string()
    } else {
        format!(
            "{} {} running on this computer, and software like it blocks",
            and_list(&products),
            if products.len() == 1 { "is" } else { "are" }
        )
    };
    aurora_core::neterr::NetFailure {
        code: ErrorCode::Dropped,
        message: "Something on this computer is blocking the sign-in".into(),
        cause: format!(
            "{host} answers normally when asked without a username and password, and \
             drops the connection the moment one carries both ({} ms). That is not your \
             provider being down. {culprits} sign-ins sent over plain http:// to protect \
             you from putting a password on an unencrypted connection — allow {host} in \
             its web protection and try again. The same subscription working on a phone \
             on this network confirms it.",
            took.as_millis()
        ),
        actions: vec![
            aurora_core::neterr::ErrorAction::Retry,
            aurora_core::neterr::ErrorAction::OpenSettings,
        ],
        retryable: true,
    }
}

/// A client for a single probe: one attempt, and a short wait for it.
///
/// The shared client retries three times with backoff, which is right for an import that
/// must survive a provider's bad minute and wrong for a question being asked on the
/// viewer's behalf while they watch a spinner — and wrong three times over when the
/// answer is already known to be "no".
fn probe_client() -> Option<aurora_ingest::http::HttpClient> {
    aurora_ingest::http::HttpClient::new(aurora_ingest::http::HttpConfig {
        max_attempts: 1,
        connect_timeout: std::time::Duration::from_secs(8),
        read_timeout: std::time::Duration::from_secs(20),
        ..Default::default()
    })
    .ok()
}

/// Does this host answer under the other scheme?
///
/// Asked **without credentials**, deliberately. The thing being established is whether
/// anything is listening and speaking HTTP there, which an unauthenticated request answers
/// just as well — and the alternative is usually `http://`, where sending a username and
/// password to find out would put them on the wire in clear text to a host that has not
/// yet been shown to be the right one.
///
/// Any HTTP answer counts, including a refusal: a panel that replies "401" or "forbidden"
/// to an empty sign-in is a panel that is *there*, which is the whole question.
fn probe_other_scheme(http: &aurora_ingest::http::HttpClient, url: &str) -> Option<String> {
    let alternative = other_scheme(url)?;
    let probe = format!("{}/player_api.php", alternative.trim_end_matches('/'));
    match http.fetch_bytes(&probe) {
        Ok(_) => Some(alternative),
        // A reply that is an HTTP error is still a reply. Only the transport failures
        // mean nothing is there.
        Err(e) if !worth_probing(e.code) => Some(alternative),
        Err(_) => None,
    }
}

/// The M3U address an Xtream panel serves the same subscription at.
///
/// `get.php` is the other half of the Xtream protocol, and on plenty of panels it is the
/// half that works: `player_api.php` can be disabled, restricted to certain clients, or
/// simply broken, while the playlist it would have described is served perfectly well.
/// Most other IPTV players use this endpoint and never touch the API at all, which is why
/// a subscription can be "working everywhere else" and fail here.
fn m3u_url(base_url: &str, username: &str, password: &str) -> String {
    format!(
        "{}/get.php?username={}&password={}&type=m3u_plus&output=ts",
        base_url.trim_end_matches('/'),
        urlencoding(username),
        urlencoding(password),
    )
}

/// Percent-encode a credential for a query string.
///
/// Small and local rather than a dependency: the characters that matter in a panel's
/// username and password are the handful below, and `aurora_ingest` already has its own
/// copy for the same reason.
fn urlencoding(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Does this panel serve a usable playlist, even though its API would not answer?
///
/// Returns the address when it does. The credentials go with it, which is no new
/// exposure: they were just sent to the same host, over the same scheme, by the API call
/// that failed.
fn probe_m3u(
    http: &aurora_ingest::http::HttpClient,
    base_url: &str,
    username: &str,
    password: &str,
) -> Option<String> {
    let url = m3u_url(base_url, username, password);
    let parsed = aurora_ingest::playlist::fetch(http, &url).ok()?;
    // Entries, not merely a 200. A panel that refuses returns an error page, and an error
    // page parses as a playlist with nothing in it.
    (!parsed.result.entries.is_empty()).then_some(url)
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
                    suggested_url: None,
                    suggested_kind: None,
                });
            }
            // One attempt, not three.
            //
            // The shared client retries with a 1s, 3s, 7s backoff, which is right for an
            // import that has to survive a provider's bad minute. For a button somebody
            // just pressed it is wrong twice: they wait eleven seconds to be told no, and
            // the panel gets three sign-in attempts per press — which is the opposite of
            // helpful when the reason for the failure is that it is already throttling
            // this address.
            //
            // It also makes the timing below mean something: three attempts and two sleeps
            // measure the backoff, not the network.
            let one_shot = probe_client();
            let http = one_shot.as_ref().unwrap_or(&services.http);
            let client = XtreamClient::new(http, &draft.url, &username, &password);
            let started = std::time::Instant::now();
            let mut outcome = client.authenticate(now_unix());
            let took = started.elapsed();

            // A refusal that arrives faster than a round trip did not come from the
            // provider.
            //
            // Observed on a real machine: every credentialed request to a panel was
            // closed in 8-24 ms, while the same panel answered un-credentialed requests
            // in 130-150 ms. Nothing across the internet can decline in a tenth of the
            // time it takes to say hello; the connection was being cut locally. The
            // machine had Bitdefender, 360 Total Security and McAfee WebAdvisor on it,
            // all of which inspect plaintext HTTP, and an IPTV panel login is exactly
            // the shape of request their filters act on. The same subscription worked on
            // a phone on the same network.
            //
            // The app had been blaming the provider for it, which sends somebody to
            // their provider's support instead of to the thing that is actually in the
            // way.
            if let Err(e) = &outcome {
                if e.code == ErrorCode::Dropped
                    && http_for_probe(&one_shot, &services.http)
                        .is_some_and(|h| host_answers_without_credentials(h, &draft.url))
                {
                    outcome = Err(locally_blocked(&draft.url, took));
                }
            }
            // Before reporting "it did not answer", find out whether it answers somewhere
            // else. A panel published on plain HTTP — which many are — is indistinguishable
            // from a dead one when the address says `https`, and the viewer has nothing to
            // tell those apart with but a spinner that ends in a red box.
            let mut suggested_kind = None;
            let probe = one_shot;
            let suggested_url = match (&outcome, probe.as_ref()) {
                // Heard and declined. Nothing further to ask, and asking is what makes a
                // throttled panel throttle harder.
                (Err(e), _) if is_refusing_us(e.code) => None,
                (Err(e), Some(http)) if worth_probing(e.code) => {
                    probe_other_scheme(http, &draft.url)
                }
                // The API said no but the host is there. Before reporting a dead
                // subscription, ask whether the playlist half of the same protocol works
                // — on plenty of panels `player_api.php` is disabled or restricted while
                // `get.php` serves the whole library, which is why a line can work in
                // every other player and fail here.
                (Err(_), Some(http)) => {
                    let found = probe_m3u(http, &draft.url, &username, &password);
                    if found.is_some() {
                        suggested_kind = Some("m3u".to_string());
                    }
                    found
                }
                _ => None,
            };
            if let Err(e) = &outcome {
                // The request is logged and its outcome was not, so a check that failed
                // left `aurora.log` saying only that something had been asked — which is
                // no help at all to the one person who needs it, reading the log after
                // the fact to find out why their provider will not connect.
                tracing::warn!(
                    code = ?e.code,
                    suggested = suggested_url.is_some(),
                    "provider check failed: {} — {}",
                    e.message,
                    e.cause
                );
            }
            match outcome {
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
                    suggested_url: None,
                    suggested_kind: None,
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
                    suggested_url,
                    suggested_kind,
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
                        suggested_url: None,
                        suggested_kind: None,
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
                    suggested_url: None,
                    suggested_kind: None,
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
    let db = services.db.lock();
    let id = save_provider(&db, services.credentials.as_ref(), args.draft, now_unix())?;
    Ok(SavedProvider { id })
}

/// The order that makes saving a provider safe, separated from the Tauri state so it
/// can be tested against a store that refuses.
///
/// This used to insert the row, then write the credential, then update the row with
/// its reference — three steps with no way back. A credential store that refused in
/// the middle left a provider behind with no password, the wizard showing a failure,
/// and a refresh that would go and sign in blank. Neither half can join the other's
/// transaction (one is SQLite, the other is Windows Credential Manager), so the order
/// is the whole mechanism: write the secret first, insert the row already carrying its
/// reference, and take the secret back out again if the insert fails.
fn save_provider(
    db: &aurora_db::rusqlite::Connection,
    store: &dyn aurora_ingest::credentials::CredentialStore,
    draft: DraftProvider,
    now: i64,
) -> Result<i64> {
    let password = draft.password.filter(|p| !p.is_empty());

    // A key that does not depend on the row id, because there is no row yet. Ids are
    // never reused, so the one this names is the one the insert will take.
    let next_id: i64 = db
        .query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM providers", [], |r| {
            r.get(0)
        })
        .map_err(aurora_db::DbError::from)?;
    // Namespaced, because the row id alone is shared with every other library on this
    // account — see `credential_ref`.
    let namespace =
        aurora_ingest::credentials::namespace(db).map_err(|e| AppError::Other(e.to_string()))?;
    let key = credential_ref(&namespace, next_id);

    if let Some(password) = &password {
        store
            .set(&key, password)
            .map_err(|e| AppError::Other(e.to_string()))?;
    }

    let stored = password.as_ref().map(|_| key.as_str());
    let inserted = db.execute(
        "INSERT INTO providers (id, name, kind, base_url, username, credential_ref,
                                enabled, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7)",
        params![
            next_id,
            draft.name,
            draft.kind,
            draft.url,
            draft.username,
            stored,
            now
        ],
    );

    if let Err(e) = inserted {
        // Nothing was saved, so nothing should be left in the credential store either:
        // a stray secret under a key no provider references is exactly what README C10
        // is about.
        if password.is_some() {
            let _ = store.delete(&key);
        }
        return Err(aurora_db::DbError::from(e).into());
    }

    Ok(next_id)
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

    // What this provider's secret is filed under now, which for a provider saved
    // before namespacing is the old un-namespaced name.
    let previous: Option<String> = db
        .query_row(
            "SELECT credential_ref FROM providers WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .map_err(aurora_db::DbError::from)?;
    let namespace =
        aurora_ingest::credentials::namespace(&db).map_err(|e| AppError::Other(e.to_string()))?;
    let key = credential_ref(&namespace, id);
    match draft.password {
        None => {}
        Some(p) if p.is_empty() => {
            let _ = services.credentials.delete(&key);
            if let Some(old) = previous.as_deref().filter(|old| *old != key) {
                let _ = services.credentials.delete(old);
            }
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
            // The row now points at the namespaced key, so anything left under the old
            // one is a secret nothing references — exactly what README C10 forbids.
            if let Some(old) = previous.as_deref().filter(|old| *old != key) {
                let _ = services.credentials.delete(old);
            }
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
    refresh_provider(&app, &services, args.provider_id)
}

/// The refresh itself, reachable without a command.
///
/// Split out so the nightly sweep can run exactly what the button in Settings runs.
/// A second implementation would be a second set of bugs: this one updates
/// `last_refresh_at`, expands series rules against the new guide, emits
/// `library.refreshed`, and starts enrichment and the episode sweep -- all of which the
/// scheduled path needs too, and none of which is obvious from the outside.
pub fn refresh_provider(
    app: &tauri::AppHandle,
    services: &Services,
    provider_id: i64,
) -> Result<SyncReport> {
    let (kind, base_url, username, credential_ref_value) = {
        let db = services.db.lock();
        db.query_row(
            "SELECT kind, base_url, COALESCE(username, '') , credential_ref
             FROM providers WHERE id = ?1",
            params![provider_id],
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
        .ok_or_else(|| AppError::Other(format!("no provider {}", provider_id)))?
    };

    let source = match kind.as_str() {
        "xtream" => SourceKind::Xtream {
            base_url,
            username: username.clone(),
        },
        _ => SourceKind::M3u { url: base_url },
    };

    let mut options = SyncOptions::new(provider_id, source, now_unix());
    options.password = stored_password(services.credentials.as_ref(), credential_ref_value)?;

    let rules = load_rules(services)?;

    // The download happens with no lock held. It used to run inside one, which froze
    // every other command for the length of a playlist and a guide — and because the
    // DVR scheduler takes the same lock every ten seconds to ask whether a recording is
    // due, a recording falling inside a long refresh simply did not start.
    //
    // `sync::fetch` is handed no database, so it cannot reintroduce that by accident.
    let emit = |p: Progress| {
        // Best-effort: a dropped progress event must never fail an import.
        crate::emit(app, "ingest.progress", &p);
    };
    let fetched = sync::fetch(&services.http, &options, &rules, emit)
        .map_err(|e| AppError::Other(format!("{}: {}", e.message, e.cause)))?;

    // Only the writes hold it, and they are local and bounded.
    let mut db = services.db.lock();
    let report = sync::apply(&mut db, fetched, &options, emit)
        .map_err(|e| AppError::Other(format!("{}: {}", e.message, e.cause)))?;

    db.execute(
        "UPDATE providers SET last_refresh_at = ?2 WHERE id = ?1",
        params![provider_id, now_unix()],
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

    // The lock has to be gone before this: it starts a thread that takes the same one.
    drop(db);

    // Posters, cast and ratings for everything just imported. Enrichment was only ever
    // started by a button in Settings, so a library imported through the wizard was a
    // wall of grey rectangles until somebody found that screen — which reads as a
    // broken app rather than an optional step nobody mentioned. It runs in the
    // background and cannot fail the refresh; with no key set it does nothing at all.
    crate::metadata::enrich_in_background(app.clone());

    // And the episode listings, for the same reason. An import writes each show without
    // them, so the seasons count on every card was zero until somebody opened the show
    // by hand — seven of 28,553 on the library this was measured against. The panel's
    // series list carries no season information, so one request per show is the only
    // way there is.
    crate::series::sweep_in_background(app.clone());

    Ok(report)
}

/// How stale a provider's library may get before it is refreshed without being asked.
pub const REFRESH_AFTER_SECS: i64 = 24 * 60 * 60;

/// Providers whose library is older than `REFRESH_AFTER_SECS`, oldest first.
///
/// Age is measured from `last_refresh_at` **or `created_at`**, whichever is later, and
/// the fallback is the whole point. Only `refresh_provider` stamps `last_refresh_at` --
/// the setup wizard imports a library without ever touching it -- so reading the column
/// alone made every newly added provider "never refreshed", and the sweep re-imported it
/// two minutes after the wizard had finished. Caught by a real panel: a library of 6,698
/// channels came back as 13,396.
///
/// A provider created over a day ago that has still never completed a refresh is stale,
/// which is the case the null was there for: imported through the wizard and interrupted.
pub fn stale_providers(db: &aurora_db::rusqlite::Connection, now: i64) -> Result<Vec<i64>> {
    let cutoff = now - REFRESH_AFTER_SECS;
    let mut stmt = db
        .prepare(
            "SELECT id FROM providers
              WHERE MAX(COALESCE(last_refresh_at, 0), COALESCE(created_at, 0)) <= ?1
              ORDER BY MAX(COALESCE(last_refresh_at, 0), COALESCE(created_at, 0))",
        )
        .map_err(aurora_db::DbError::from)?;
    let ids = stmt
        .query_map(params![cutoff], |r| r.get::<_, i64>(0))
        .map_err(aurora_db::DbError::from)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(aurora_db::DbError::from)?;
    Ok(ids)
}

/// Refresh every provider that is due, one at a time. Returns how many were refreshed.
///
/// Sequential on purpose. Two refreshes at once means two playlist downloads and two
/// `sync::apply` passes competing for the single writer connection, on a machine whose
/// owner is probably watching something.
///
/// A provider that fails is logged and skipped rather than retried here: the sweep comes
/// round again, and a provider that is down stays down for longer than this loop.
pub fn refresh_stale(app: &tauri::AppHandle, services: &Services, now: i64) -> usize {
    let due = {
        let db = services.db.lock();
        match stale_providers(&db, now) {
            Ok(ids) => ids,
            Err(e) => {
                tracing::warn!("could not work out which providers are stale: {e}");
                return 0;
            }
        }
    };
    if due.is_empty() {
        return 0;
    }
    tracing::info!("{} provider(s) have a library over a day old", due.len());

    let mut done = 0;
    for id in due {
        match refresh_provider(app, services, id) {
            Ok(report) => {
                done += 1;
                tracing::info!(
                    "provider {id} refreshed on schedule: {} channels, {} films, {} episodes",
                    report.channels,
                    report.movies,
                    report.episodes
                );
            }
            Err(e) => tracing::warn!("the scheduled refresh of provider {id} failed: {e}"),
        }
    }
    done
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
mod local_cut_tests {
    use super::*;

    /// A playlist on this machine, or on a box in the cupboard, really can refuse in
    /// twenty milliseconds. Telling those people their antivirus is at fault would be a
    /// And a panel on the internet is not local, including the ones whose addresses look
    /// The message has to point at the thing that is actually in the way, and name the
    /// host so it can be allowed.
    #[test]
    fn the_message_names_the_host_and_the_likely_cause() {
        let f = locally_blocked(
            "http://64582429.max-jbnott.online/player_api.php",
            std::time::Duration::from_millis(9),
        );
        assert!(f.message.contains("this computer"), "{f:?}");
        assert!(f.cause.contains("64582429.max-jbnott.online"), "{f:?}");
        assert!(f.cause.contains("9 ms"), "{f:?}");
        // Either the products this machine actually has, or the generic list when the
        // Security Center has nothing to say.
        assert!(
            f.cause.contains("running on this computer") || f.cause.contains("Bitdefender"),
            "{f:?}"
        );
        // It names what the rule is about, which is what makes it findable in a settings
        // screen full of toggles.
        assert!(f.cause.contains("http://"), "{f:?}");
        // It must not accuse the provider, which is the whole point of the distinction.
        assert!(!f.message.contains("provider"), "{f:?}");
        // Retryable: allowing it in the filter and pressing the button again is the fix.
        assert!(f.retryable);
    }

    #[test]
    fn a_list_of_products_reads_like_a_sentence() {
        assert_eq!(and_list(&[]), "");
        assert_eq!(and_list(&["Bitdefender".into()]), "Bitdefender");
        assert_eq!(
            and_list(&["Bitdefender".into(), "360 Total Security".into()]),
            "Bitdefender and 360 Total Security"
        );
        assert_eq!(
            and_list(&["A".into(), "B".into(), "C".into()]),
            "A, B and C"
        );
    }
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
        store.set(&credential_ref("ns", 7), "hunter2").unwrap();
        assert_eq!(
            stored_password(&store, Some(credential_ref("ns", 7)))
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
        let err = stored_password(&store, Some(credential_ref("ns", 7)))
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

    use aurora_ingest::credentials::CredentialError;

    fn draft(name: &str, password: Option<&str>) -> DraftProvider {
        DraftProvider {
            name: name.into(),
            kind: "xtream".into(),
            url: "http://panel.example".into(),
            username: Some("bob".into()),
            password: password.map(str::to_string),
        }
    }

    /// A store that always refuses, standing in for a Credential Manager that is
    /// locked, full, or governed by a policy that says no.
    struct RefusingStore;

    impl CredentialStore for RefusingStore {
        fn set(&self, _: &str, _: &str) -> std::result::Result<(), CredentialError> {
            Err(CredentialError::Backend("the vault said no".into()))
        }
        fn get(&self, key: &str) -> std::result::Result<String, CredentialError> {
            Err(CredentialError::NotFound(key.into()))
        }
        fn delete(&self, _: &str) -> std::result::Result<(), CredentialError> {
            Ok(())
        }
        fn is_persistent(&self) -> bool {
            true
        }
    }

    /// Seed a provider with a given `last_refresh_at`. `None` means it never finished one.
    fn provider_refreshed_at(db: &aurora_db::rusqlite::Connection, id: i64, at: Option<i64>) {
        db.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at,last_refresh_at)
             VALUES (?1,?2,'m3u','https://example.com/p.m3u',0,?3)",
            params![id, format!("P{id}"), at],
        )
        .unwrap();
    }

    const DAY: i64 = 24 * 60 * 60;

    #[test]
    fn a_library_refreshed_today_is_not_due() {
        let db = aurora_db::open_memory().unwrap();
        let now = 10 * DAY;
        provider_refreshed_at(&db, 1, Some(now - 3600));
        assert_eq!(stale_providers(&db, now).unwrap(), Vec::<i64>::new());
    }

    #[test]
    fn a_library_a_day_old_is_due() {
        let db = aurora_db::open_memory().unwrap();
        let now = 10 * DAY;
        provider_refreshed_at(&db, 1, Some(now - REFRESH_AFTER_SECS));
        assert_eq!(stale_providers(&db, now).unwrap(), vec![1]);
    }

    /// The boundary, stated: a second under a day is not due, which is what keeps the
    /// half-hourly sweep from refreshing the same provider twice in a row.
    #[test]
    fn a_second_under_a_day_is_not_due() {
        let db = aurora_db::open_memory().unwrap();
        let now = 10 * DAY;
        provider_refreshed_at(&db, 1, Some(now - REFRESH_AFTER_SECS + 1));
        assert!(stale_providers(&db, now).unwrap().is_empty());
    }

    /// Never counts as older than a day. A provider imported through the wizard and then
    /// interrupted has no `last_refresh_at` at all, and it is exactly the one that most
    /// needs picking up.
    /// The bug this fallback exists for: the wizard imports a library and never stamps
    /// `last_refresh_at`, so reading that column alone made a provider added seconds ago
    /// due for a full re-import on the next sweep.
    #[test]
    fn a_provider_added_just_now_is_not_due() {
        let db = aurora_db::open_memory().unwrap();
        let now = 10 * DAY;
        db.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at,last_refresh_at)
             VALUES (1,'P','m3u','https://example.com/p.m3u',?1,NULL)",
            params![now - 60],
        )
        .unwrap();
        assert_eq!(stale_providers(&db, now).unwrap(), Vec::<i64>::new());
    }

    #[test]
    fn a_provider_that_never_finished_one_is_due() {
        let db = aurora_db::open_memory().unwrap();
        provider_refreshed_at(&db, 1, None);
        assert_eq!(stale_providers(&db, 10 * DAY).unwrap(), vec![1]);
    }

    /// Oldest first, so the worst library is fixed first if the sweep is interrupted.
    #[test]
    fn the_stalest_library_is_refreshed_first() {
        let db = aurora_db::open_memory().unwrap();
        let now = 10 * DAY;
        provider_refreshed_at(&db, 1, Some(now - 2 * DAY));
        provider_refreshed_at(&db, 2, None);
        provider_refreshed_at(&db, 3, Some(now - 5 * DAY));
        provider_refreshed_at(&db, 4, Some(now - 60));
        // 2 never did, then 3 at five days, then 1 at two. 4 is not due at all.
        assert_eq!(stale_providers(&db, now).unwrap(), vec![2, 3, 1]);
    }

    fn provider_count(db: &aurora_db::rusqlite::Connection) -> i64 {
        db.query_row("SELECT count(*) FROM providers", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn a_saved_provider_keeps_its_password_and_its_reference() {
        let db = aurora_db::open_memory().unwrap();
        let store = MemoryStore::default();

        let id = save_provider(&db, &store, draft("Panel", Some("hunter2")), 100).unwrap();

        let stored: Option<String> = db
            .query_row(
                "SELECT credential_ref FROM providers WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        let ns = aurora_ingest::credentials::namespace(&db).unwrap();
        assert_eq!(stored.as_deref(), Some(credential_ref(&ns, id).as_str()));
        assert_eq!(store.get(&credential_ref(&ns, id)).unwrap(), "hunter2");
    }

    /// The bug: the row went in first, so a credential store that refused left a
    /// provider behind with no password — the wizard reporting a failure, the list
    /// showing the provider, and a refresh that would sign in blank.
    #[test]
    fn a_failed_credential_write_leaves_no_provider_behind() {
        let db = aurora_db::open_memory().unwrap();

        let err = save_provider(&db, &RefusingStore, draft("Panel", Some("hunter2")), 100)
            .unwrap_err()
            .to_string();

        assert!(err.contains("the vault said no"), "{err}");
        assert_eq!(
            provider_count(&db),
            0,
            "a provider was saved that has no password"
        );
    }

    /// And the other way round: a refused insert must not leave a secret behind under
    /// a key nothing references.
    #[test]
    fn a_failed_insert_takes_the_secret_back_out() {
        let db = aurora_db::open_memory().unwrap();
        let store = MemoryStore::default();
        // `kind` is constrained by the schema, so this insert cannot succeed.
        let mut bad = draft("Panel", Some("hunter2"));
        bad.kind = "not-a-kind".into();

        assert!(save_provider(&db, &store, bad, 100).is_err());
        assert_eq!(provider_count(&db), 0);
        let ns = aurora_ingest::credentials::namespace(&db).unwrap();
        assert!(
            store.get(&credential_ref(&ns, 1)).is_err(),
            "a secret was left under a key no provider references"
        );
    }

    #[test]
    fn a_provider_with_no_password_stores_no_reference() {
        let db = aurora_db::open_memory().unwrap();
        let store = MemoryStore::default();

        // An M3U URL carries its own credentials; there is nothing to keep.
        let id = save_provider(&db, &store, draft("Playlist", None), 100).unwrap();
        let stored: Option<String> = db
            .query_row(
                "SELECT credential_ref FROM providers WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, None);
        let ns = aurora_ingest::credentials::namespace(&db).unwrap();
        assert!(store.get(&credential_ref(&ns, id)).is_err());
    }

    #[test]
    fn two_providers_get_different_keys() {
        let db = aurora_db::open_memory().unwrap();
        let store = MemoryStore::default();

        let a = save_provider(&db, &store, draft("One", Some("first")), 100).unwrap();
        let b = save_provider(&db, &store, draft("Two", Some("second")), 100).unwrap();

        assert_ne!(a, b);
        let ns = aurora_ingest::credentials::namespace(&db).unwrap();
        assert_eq!(store.get(&credential_ref(&ns, a)).unwrap(), "first");
        assert_eq!(store.get(&credential_ref(&ns, b)).unwrap(), "second");
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
