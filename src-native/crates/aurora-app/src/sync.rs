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
use aurora_ingest::credentials::CredentialStore;
use aurora_sync::firebase::{FirebaseConfig, ReqwestHttp};
use aurora_sync::household::{
    Account, Household, Joined, Pacer, Problem, Secrets, Status, Unlock, Waiting,
};
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

fn config() -> Option<FirebaseConfig> {
    let api_key = BUILT_IN_API_KEY.map(str::trim).filter(|k| !k.is_empty())?;
    let project_id = BUILT_IN_PROJECT.map(str::trim).filter(|p| !p.is_empty())?;
    Some(FirebaseConfig {
        api_key: api_key.into(),
        project_id: project_id.into(),
    })
}

/// The household's secrets, in the OS credential store.
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
}

impl Sync {
    /// `db` is the app's writer, for this library's credential namespace -- which it may
    /// have to create, and which is the reason this takes the writer rather than a reader.
    pub fn new(library: PathBuf, db: &Connection, credentials: Arc<dyn CredentialStore>) -> Self {
        let household = match (
            aurora_ingest::credentials::namespace(db),
            ReqwestHttp::new(),
        ) {
            (Ok(ns), Ok(http)) => Some(Household::new(
                http,
                config(),
                Arc::new(CredentialSecrets {
                    store: credentials,
                    prefix: format!("aurora-{ns}-sync-"),
                }),
            )),
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
                .due(now, Waiting::read(conn).map_err(sync_error)?)
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
    let outcome = services
        .sync
        .with_conn(|household, conn| Ok(household.reset(conn)))?;
    let problem = outcome.as_ref().err().map(Problem::of);
    services.sync.restart_pacing(now_unix(), problem.as_ref());
    outcome.map_err(sync_error)
}

#[tauri::command(async)]
pub fn sync_sign_out(services: State<'_, Services>) -> Result<()> {
    services
        .sync
        .with_conn(|household, conn| household.sign_out(conn).map_err(sync_error))?;
    *services.sync.pacer.lock() = Pacer::default();
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryArgs {
    pub passphrase: String,
}

/// The recovery code again, for someone who knows the passphrase. Works offline.
#[tauri::command(async)]
pub fn sync_recovery_code(services: State<'_, Services>, args: RecoveryArgs) -> Result<String> {
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
