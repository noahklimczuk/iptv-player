//! Sync on Windows (`docs/APPLE_PORT_AND_SYNC.md` §3): the Sync panel's commands, and the
//! loop that keeps this PC in step with the household's other devices.
//!
//! Everything with a decision in it lives in `aurora_sync::household` and is tested there
//! against an in-memory Firebase. This module only connects it to the app:
//!
//! - **Its own connection.** A sync round waits on the network between its short
//!   transactions. On the app's writer it would hold `services.db` for the length of every
//!   request -- up to the 20-second timeout, offline -- and every favourite, every progress
//!   save and the DVR would wait behind it. So sync has a second writer on the same file
//!   (`aurora_db::open_secondary`) behind a lock of its own, and the panel's status is
//!   read on one of the read connections, so it never waits behind a round in flight.
//! - **Secrets in Credential Manager.** The refresh token and the vault key go through
//!   the same `CredentialStore` provider passwords do, named under this library's
//!   credential namespace so an installed copy and a portable one never share them
//!   (and `AURORA_CREDENTIAL_SERVICE` keeps a test run out of the real vault).
//! - **Which Firebase project** is compiled in, like the Gemini key: `option_env!` of
//!   `AURORA_FIREBASE_API_KEY` and `AURORA_FIREBASE_PROJECT_ID`. A build without them
//!   offers no sync, and the panel says so. Both values are public by design -- the
//!   security rules and per-account sign-in are what protect a household's data, and its
//!   contents are end-to-end encrypted besides.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use aurora_db::rusqlite::Connection;
use aurora_ingest::credentials::{credential_ref, CredentialStore};
use aurora_sync::firebase::{FirebaseConfig, ReqwestHttp};
use aurora_sync::household::{
    Account, Household, Joined, Pacer, Problem, Secrets, Status, Unlock, Waiting,
};
use aurora_sync::secrets::SharedSecrets;
use aurora_sync::{Report, SyncError};
use parking_lot::Mutex;
use tauri::State;

use crate::error::{AppError, Result};
use crate::services::Services;

pub const BUILT_IN_API_KEY: Option<&str> = option_env!("AURORA_FIREBASE_API_KEY");
pub const BUILT_IN_PROJECT: Option<&str> = option_env!("AURORA_FIREBASE_PROJECT_ID");

/// How often the loop asks the pacer whether a round is due. The pacer decides; this is
/// only how quickly a change made here can be noticed.
pub const TICK: Duration = Duration::from_secs(3);
/// The first round waits this long after launch: the first seconds belong to getting a
/// picture on screen, and nothing about sync is that urgent.
pub const START_DELAY: Duration = Duration::from_secs(8);
/// The longest opening a film waits for its position from the other devices. One small
/// query is a few hundred milliseconds; past this the film opens on what this PC knows,
/// and the fetch still lands for the next look.
pub const RESUME_BUDGET: Duration = Duration::from_millis(1500);
/// Opening a film asks for its position, and pressing Play a moment later asks again.
/// The second is not worth a read.
const RESUME_FRESH_SECS: i64 = 20;

fn config() -> Option<FirebaseConfig> {
    let api_key = BUILT_IN_API_KEY.map(str::trim).filter(|k| !k.is_empty())?;
    let project_id = BUILT_IN_PROJECT.map(str::trim).filter(|p| !p.is_empty())?;
    Some(FirebaseConfig {
        api_key: api_key.into(),
        project_id: project_id.into(),
    })
}

/// The household's secrets, in the OS credential store.
/// The sign-ins and keys sync may share (§3.4, D34), where Windows keeps them: each
/// provider's password under its `credential_ref`, and the TMDB and Gemini keys under
/// their own names. Sync sends them only while the household has sharing on.
struct WindowsSecrets {
    store: Arc<dyn CredentialStore>,
    namespace: String,
}

const SHARED_TMDB: &str = "tmdb";
const SHARED_GEMINI: &str = "gemini";
const SHARED_PROVIDER: &str = "provider:";

impl SharedSecrets for WindowsSecrets {
    fn list(&self, conn: &Connection) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let providers: Vec<(String, String)> = conn
            .prepare(
                "SELECT sync_id, credential_ref FROM providers
                  WHERE sync_id IS NOT NULL AND credential_ref IS NOT NULL",
            )
            .and_then(|mut stmt| {
                stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect()
            })
            .unwrap_or_default();
        for (sync_id, key) in providers {
            if let Ok(v) = self.store.get(&key) {
                if !v.is_empty() {
                    out.push((format!("{SHARED_PROVIDER}{sync_id}"), v));
                }
            }
        }
        for (name, key) in [
            (SHARED_TMDB, aurora_ingest::tmdb::CREDENTIAL_KEY),
            (SHARED_GEMINI, crate::gemini::CREDENTIAL_KEY),
        ] {
            if let Ok(v) = self.store.get(key) {
                if !v.is_empty() {
                    out.push((name.to_string(), v));
                }
            }
        }
        out
    }

    fn store(
        &self,
        conn: &Connection,
        name: &str,
        value: Option<&str>,
    ) -> aurora_sync::Result<bool> {
        let refused = |e: String| {
            SyncError::Account(format!(
                "Windows would not keep a sign-in from another device: {e}"
            ))
        };
        let key = if let Some(sync_id) = name.strip_prefix(SHARED_PROVIDER) {
            let id: Option<i64> = conn
                .query_row(
                    "SELECT id FROM providers WHERE sync_id = ?1",
                    [sync_id],
                    |r| r.get(0),
                )
                .map(Some)
                .or_else(|e| match e {
                    aurora_db::rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    e => Err(e),
                })?;
            // The provider itself has not arrived here yet: wait for it.
            let Some(id) = id else {
                return Ok(false);
            };
            credential_ref(&self.namespace, id)
        } else {
            match name {
                SHARED_TMDB => aurora_ingest::tmdb::CREDENTIAL_KEY.to_string(),
                SHARED_GEMINI => crate::gemini::CREDENTIAL_KEY.to_string(),
                // A kind of secret from a newer Aurora: nothing here would use it.
                _ => return Ok(true),
            }
        };
        match value {
            Some(v) => self
                .store
                .set(&key, v)
                .map_err(|e| refused(e.to_string()))?,
            None => {
                let _ = self.store.delete(&key);
            }
        }
        if let Some(sync_id) = name.strip_prefix(SHARED_PROVIDER) {
            conn.execute(
                "UPDATE providers SET credential_ref = ?2 WHERE sync_id = ?1",
                aurora_db::rusqlite::params![sync_id, value.map(|_| key)],
            )?;
        }
        Ok(true)
    }
}

struct CredentialSecrets {
    store: Arc<dyn CredentialStore>,
    /// `aurora-{namespace}-sync-`, so two libraries on one Windows account never share.
    prefix: String,
}

impl CredentialSecrets {
    fn name(&self, name: &str) -> String {
        format!("{}{name}", self.prefix)
    }
}

impl Secrets for CredentialSecrets {
    fn get(&self, name: &str) -> Option<String> {
        self.store.get(&self.name(name)).ok()
    }

    fn set(&self, name: &str, value: &str) -> aurora_sync::Result<()> {
        self.store.set(&self.name(name), value).map_err(|e| {
            SyncError::Account(format!(
                "Signed in, but Windows would not store the sign-in: {e}"
            ))
        })
    }

    fn delete(&self, name: &str) {
        let _ = self.store.delete(&self.name(name));
    }

    fn is_persistent(&self) -> bool {
        self.store.is_persistent()
    }
}

/// Sync's half of the app: the household, its connection, and its pacing.
pub struct Sync {
    /// `None` only when the HTTPS client could not be built at all.
    household: Option<Household<ReqwestHttp>>,
    library: PathBuf,
    /// Opened on first use, so a library that never signs in never opens it.
    ///
    /// Held for a whole round, and by every command that changes the account, so two of
    /// them never run at once. Never taken while `services.db` is held, nor the other
    /// way round.
    conn: Mutex<Option<Connection>>,
    pacer: Mutex<Pacer>,
    /// The last film whose position was fetched, and when: (profile, kind, id, at).
    last_resume: Mutex<Option<(i64, &'static str, i64, i64)>>,
}

impl Sync {
    /// `db` is the app's writer, for this library's credential namespace -- which it may
    /// have to create, and which is the reason this takes the writer rather than a reader.
    pub fn new(library: PathBuf, db: &Connection, credentials: Arc<dyn CredentialStore>) -> Self {
        let household = match (
            aurora_ingest::credentials::namespace(db),
            ReqwestHttp::new(),
        ) {
            (Ok(ns), Ok(http)) => Some(
                Household::new(
                    http,
                    config(),
                    Arc::new(CredentialSecrets {
                        store: credentials.clone(),
                        prefix: format!("aurora-{ns}-sync-"),
                    }),
                )
                .with_shared_secrets(Arc::new(WindowsSecrets {
                    store: credentials,
                    namespace: ns,
                })),
            ),
            (Err(e), _) => {
                tracing::error!("sync is unavailable: no credential namespace: {e}");
                None
            }
            (_, Err(e)) => {
                tracing::error!("sync is unavailable: no HTTPS client: {e}");
                None
            }
        };
        Self {
            household,
            library,
            conn: Mutex::new(None),
            pacer: Mutex::new(Pacer::default()),
            last_resume: Mutex::new(None),
        }
    }

    /// Fetch this profile's position in a film or episode from the other devices before
    /// it is shown or resumed (§3.6), waiting at most [`RESUME_BUDGET`].
    ///
    /// `enabled` is whether sync is on, read by the caller on its reader connection, so a
    /// library that never signed in costs nothing here and never opens sync's connection.
    /// Skipped while the loop is waiting out a failure, and while a round holds sync's
    /// connection -- that round is pulling everything anyway. Never an error: a position
    /// that could not be fetched is the one this PC already had.
    pub fn refresh_progress(
        self: &Arc<Self>,
        enabled: bool,
        profile_id: i64,
        kind: &'static str,
        id: i64,
    ) {
        let Some(household) = &self.household else {
            return;
        };
        let now = now_unix();
        if !enabled || !household.is_available() || self.pacer.lock().is_holding(now) {
            return;
        }
        {
            let mut last = self.last_resume.lock();
            if let Some((p, k, i, at)) = *last {
                if (p, k, i) == (profile_id, kind, id) && now - at < RESUME_FRESH_SECS {
                    return;
                }
            }
            *last = Some((profile_id, kind, id, now));
        }
        let (done, wait) = std::sync::mpsc::channel();
        let sync = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("aurora-sync-resume".into())
            .spawn(move || {
                let Some(mut slot) = sync.conn.try_lock() else {
                    return;
                };
                if slot.is_none() {
                    match aurora_db::open_secondary(&sync.library) {
                        Ok(c) => *slot = Some(c),
                        Err(e) => {
                            tracing::debug!("sync could not open its connection: {e}");
                            return;
                        }
                    }
                }
                let conn = slot.as_mut().expect("opened just above");
                if let Some(household) = &sync.household {
                    match household.refresh_progress(conn, profile_id, kind, id) {
                        Ok(true) => {
                            tracing::info!(kind, id, "took a newer position from another device")
                        }
                        Ok(false) => {}
                        Err(e) => tracing::debug!("could not fetch a position: {e}"),
                    }
                }
                let _ = done.send(());
            });
        if spawned.is_ok() {
            let _ = wait.recv_timeout(RESUME_BUDGET);
        }
    }

    /// Ask for a round soon: an import may have brought in what parked records were
    /// waiting for. Ignored while a failure is being waited out.
    pub fn wake(&self) {
        self.pacer.lock().wake();
    }

    fn household(&self) -> Result<&Household<ReqwestHttp>> {
        self.household
            .as_ref()
            .ok_or_else(|| AppError::Other("Sync could not start on this computer.".into()))
    }

    /// Run `f` on sync's own connection, opening it the first time.
    fn with_conn<T>(
        &self,
        f: impl FnOnce(&Household<ReqwestHttp>, &mut Connection) -> Result<T>,
    ) -> Result<T> {
        let household = self.household()?;
        let mut slot = self.conn.lock();
        if slot.is_none() {
            *slot = Some(aurora_db::open_secondary(&self.library)?);
        }
        let conn = slot.as_mut().expect("opened just above");
        f(household, conn)
    }

    /// Start pacing afresh after the account changed, with the round it just ran.
    fn restart_pacing(&self, now: i64, problem: Option<&Problem>) {
        let mut pacer = Pacer::default();
        pacer.after(now, problem);
        *self.pacer.lock() = pacer;
    }

    /// One tick of the background loop: sync if signed in and the pacer says so.
    pub fn tick(&self, now: i64) {
        let Some(household) = &self.household else {
            return;
        };
        if !household.is_available() {
            return;
        }
        let ran = self.with_conn(|household, conn| {
            let before = household.status(conn).map_err(sync_error)?;
            if !before.signed_in {
                return Ok(None);
            }
            if !self
                .pacer
                .lock()
                .due(now, Waiting::read(conn, now).map_err(sync_error)?)
            {
                return Ok(None);
            }
            let outcome = household.sync(conn);
            Ok(Some((before.problem, outcome)))
        });
        let (previous, outcome) = match ran {
            Ok(Some(round)) => round,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!("sync could not run: {e}");
                return;
            }
        };
        let problem = outcome.as_ref().err().map(Problem::of);
        // Said once when it starts, not every round it lasts (§3.8).
        match (&outcome, &problem) {
            (Ok(r), _) if r.pulled + r.pushed > 0 => tracing::info!(
                pushed = r.pushed,
                pulled = r.pulled,
                applied = r.applied,
                parked = r.parked,
                undecodable = r.undecodable,
                "synced"
            ),
            (Ok(_), _) if previous.is_some() => tracing::info!("sync is working again"),
            (Err(e), Some(p)) if previous.as_ref() != Some(p) => {
                tracing::warn!("sync stopped: {e}")
            }
            _ => {}
        }
        self.pacer.lock().after(now, problem.as_ref());
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn sync_error(e: SyncError) -> AppError {
    AppError::Other(match e {
        // Already worded for the person; the rest are prefixed with what was going on.
        SyncError::Account(m) => m,
        SyncError::Offline(_) => {
            "Could not reach the sync service. Check the connection and try again.".into()
        }
        SyncError::Unauthenticated => "The sync account needs signing in again.".into(),
        other => other.to_string(),
    })
}

// ---------------------------------------------------------------------------------------
// Commands

#[tauri::command(async)]
pub fn sync_status(services: State<'_, Services>) -> Result<Status> {
    let Some(household) = &services.sync.household else {
        return Ok(Status::default());
    };
    let db = services.read();
    household.status(&db).map_err(sync_error)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateArgs {
    pub email: String,
    pub password: String,
    pub passphrase: String,
    pub device_name: String,
}

#[tauri::command(async)]
pub fn sync_create_account(services: State<'_, Services>, args: CreateArgs) -> Result<Joined> {
    crate::profiles::require_settings(&services.db.lock())?;
    let joined = services.sync.with_conn(|household, conn| {
        let account = Account {
            email: &args.email,
            password: &args.password,
            device_name: &args.device_name,
        };
        household
            .create_account(conn, &account, &args.passphrase)
            .map_err(sync_error)
    })?;
    services
        .sync
        .restart_pacing(now_unix(), joined.problem.as_ref());
    Ok(joined)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInArgs {
    pub email: String,
    pub password: String,
    /// One of these two.
    pub passphrase: Option<String>,
    pub recovery_code: Option<String>,
    pub device_name: String,
}

#[tauri::command(async)]
pub fn sync_sign_in(services: State<'_, Services>, args: SignInArgs) -> Result<Joined> {
    crate::profiles::require_settings(&services.db.lock())?;
    let unlock = match (args.passphrase, args.recovery_code) {
        (_, Some(code)) if !code.trim().is_empty() => Unlock::RecoveryCode(code),
        (Some(p), _) => Unlock::Passphrase(p),
        _ => {
            return Err(AppError::Other(
                "Enter the sync passphrase, or the recovery code.".into(),
            ))
        }
    };
    let joined = services.sync.with_conn(|household, conn| {
        let account = Account {
            email: &args.email,
            password: &args.password,
            device_name: &args.device_name,
        };
        household
            .sign_in(conn, &account, unlock)
            .map_err(sync_error)
    })?;
    services
        .sync
        .restart_pacing(now_unix(), joined.problem.as_ref());
    Ok(joined)
}

/// A round now, whatever the pacer thinks: the panel's "Sync now".
#[tauri::command(async)]
pub fn sync_now(services: State<'_, Services>) -> Result<Report> {
    let outcome = services
        .sync
        .with_conn(|household, conn| Ok(household.sync(conn)))?;
    let problem = outcome.as_ref().err().map(Problem::of);
    services.sync.restart_pacing(now_unix(), problem.as_ref());
    outcome.map_err(sync_error)
}

/// Throw away the household's server copy and upload this PC's library in its place.
#[tauri::command(async)]
pub fn sync_reset(services: State<'_, Services>) -> Result<Report> {
    crate::profiles::require_settings(&services.db.lock())?;
    let outcome = services
        .sync
        .with_conn(|household, conn| Ok(household.reset(conn)))?;
    let problem = outcome.as_ref().err().map(Problem::of);
    services.sync.restart_pacing(now_unix(), problem.as_ref());
    outcome.map_err(sync_error)
}

#[tauri::command(async)]
pub fn sync_sign_out(services: State<'_, Services>) -> Result<()> {
    crate::profiles::require_settings(&services.db.lock())?;
    services
        .sync
        .with_conn(|household, conn| household.sign_out(conn).map_err(sync_error))?;
    *services.sync.pacer.lock() = Pacer::default();
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareSecretsArgs {
    pub enabled: bool,
}

/// Share provider passwords and API keys with the household's other devices, or stop
/// (§3.4, D34). Off unless chosen; behind the Settings lock like the rest of sync.
#[tauri::command(async)]
pub fn sync_set_share_secrets(services: State<'_, Services>, args: ShareSecretsArgs) -> Result<()> {
    crate::profiles::require_settings(&services.db.lock())?;
    services.sync.with_conn(|household, conn| {
        household
            .set_share_secrets(conn, args.enabled)
            .map_err(sync_error)
    })
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryArgs {
    pub passphrase: String,
}

/// The recovery code again, for someone who knows the passphrase. Works offline.
#[tauri::command(async)]
pub fn sync_recovery_code(services: State<'_, Services>, args: RecoveryArgs) -> Result<String> {
    crate::profiles::require_settings(&services.db.lock())?;
    // The reader: this only reads `sync_state`, and Argon2 takes a second -- no reason to
    // make a round wait for it, or it for a round.
    let household = services.sync.household()?;
    let db = services.read();
    household
        .recovery_code(&db, &args.passphrase)
        .map_err(sync_error)
}

/// What the panel suggests calling this PC.
#[tauri::command(async)]
pub fn sync_device_name() -> Result<String> {
    Ok(std::env::var("COMPUTERNAME")
        .ok()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Windows PC".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_ingest::credentials::MemoryStore;

    fn library() -> Connection {
        let conn = aurora_db::open_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO providers (id,name,kind,base_url,username,credential_ref,created_at)
                  VALUES (1,'Line','xtream','http://panel.example/','viewer','aurora-ns-provider-1',0),
                         (2,'Other','xtream','http://other.example/','viewer',NULL,0);",
        )
        .unwrap();
        conn
    }

    fn secrets(store: &Arc<MemoryStore>) -> WindowsSecrets {
        WindowsSecrets {
            store: store.clone(),
            namespace: "ns".into(),
        }
    }

    fn sync_id(conn: &Connection, id: i64) -> String {
        conn.query_row("SELECT sync_id FROM providers WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap()
    }

    #[test]
    fn it_lists_what_windows_holds_under_names_every_device_shares() {
        let conn = library();
        let store = Arc::new(MemoryStore::default());
        store.set("aurora-ns-provider-1", "line-password").unwrap();
        store
            .set(aurora_ingest::tmdb::CREDENTIAL_KEY, "tmdb-key")
            .unwrap();
        let listed = secrets(&store).list(&conn);

        let line = format!("provider:{}", sync_id(&conn, 1));
        assert!(
            listed.contains(&(line, "line-password".into())),
            "{listed:?}"
        );
        assert!(
            listed.contains(&("tmdb".into(), "tmdb-key".into())),
            "{listed:?}"
        );
        // A provider with no password, and a key nobody set, are not listed.
        assert_eq!(listed.len(), 2, "{listed:?}");
    }

    #[test]
    fn a_password_lands_on_the_provider_it_names_or_waits_for_it() {
        let conn = library();
        let store = Arc::new(MemoryStore::default());
        let shared = secrets(&store);

        // A provider this device does not have: wait.
        assert!(!shared
            .store(&conn, "provider:not-here", Some("pw"))
            .unwrap());

        // The second provider had no password; now it has one, under its own reference.
        let other = format!("provider:{}", sync_id(&conn, 2));
        assert!(shared.store(&conn, &other, Some("from-the-tv")).unwrap());
        let key: String = conn
            .query_row(
                "SELECT credential_ref FROM providers WHERE id = 2",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(key, credential_ref("ns", 2));
        assert_eq!(store.get(&key).unwrap(), "from-the-tv");

        // Removed on another device: removed here, and the reference cleared.
        assert!(shared.store(&conn, &other, None).unwrap());
        assert!(store.get(&key).is_err());
        let cleared: Option<String> = conn
            .query_row(
                "SELECT credential_ref FROM providers WHERE id = 2",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cleared, None);

        // The Gemini key goes where the Gemini panel reads it.
        assert!(shared.store(&conn, "gemini", Some("g-key")).unwrap());
        assert_eq!(store.get(crate::gemini::CREDENTIAL_KEY).unwrap(), "g-key");
    }
}
