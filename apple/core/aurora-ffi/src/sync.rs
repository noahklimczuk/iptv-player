//! Household sync, for the Apple apps (docs/APPLE_PORT_AND_SYNC.md §3).
//!
//! Every decision is in `aurora_sync::household`, tested there against an in-memory
//! Firebase, exactly as Windows uses it. This module is the host glue, and mirrors
//! `aurora-app/src/sync.rs` piece for piece:
//!
//! - **Its own connection** (`aurora_db::open_secondary`). A round waits on the network
//!   between its short transactions; on the core's one connection it would hold every
//!   favourite and every progress save behind it for up to the 20-second timeout.
//! - **Secrets in the Keychain**, through the same [`SecretStore`](crate::SecretStore)
//!   provider passwords use, named under this library's credential namespace as on
//!   Windows. Only the refresh token and the vault key are stored; the hour-long ID token
//!   stays in memory (`household::stored`).
//! - **Which Firebase project** is compiled in, from `AURORA_FIREBASE_API_KEY` and
//!   `AURORA_FIREBASE_PROJECT_ID` at build time. A build without them offers no sync, and
//!   [`SyncStatus::available`] says so.
//! - **Swift drives the loop.** Windows runs a thread that asks the pacer every 3 s.
//!   On Apple the app is suspended in the background anyway, so Swift calls
//!   [`AuroraCore::sync_tick`] every [`AuroraCore::sync_tick_secs`] while a scene is
//!   active; the pacer decides whether a round is due, as on Windows.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use aurora_db::rusqlite::Connection;
use aurora_ingest::credentials::CredentialStore;
use aurora_sync::firebase::{Body, FirebaseConfig, Http, Method, Reply, ReqwestHttp};
use aurora_sync::household::{
    Account, Household, Joined, Pacer, Problem, Secrets, Status, Unlock, Waiting,
};
use aurora_sync::{Report, SyncError};
use parking_lot::Mutex;

use crate::error::{AuroraError, Result};
use crate::vod::VodKind;
use crate::AuroraCore;

const BUILT_IN_API_KEY: Option<&str> = option_env!("AURORA_FIREBASE_API_KEY");
const BUILT_IN_PROJECT: Option<&str> = option_env!("AURORA_FIREBASE_PROJECT_ID");

/// How often Swift asks whether a round is due. `aurora-app/src/sync.rs` `TICK`.
pub(crate) const TICK_SECS: u64 = 3;
/// The longest opening a film waits for its position from the other devices.
/// `aurora-app/src/sync.rs` `RESUME_BUDGET`.
pub(crate) const RESUME_BUDGET_MS: u64 = 1500;
/// Opening a film asks for its position, and pressing Play a moment later asks again.
/// The second is not worth a read. `aurora-app/src/sync.rs` `RESUME_FRESH_SECS`.
pub(crate) const RESUME_FRESH_SECS: i64 = 20;

fn built_in_config() -> Option<FirebaseConfig> {
    let api_key = BUILT_IN_API_KEY.map(str::trim).filter(|k| !k.is_empty())?;
    let project_id = BUILT_IN_PROJECT.map(str::trim).filter(|p| !p.is_empty())?;
    Some(FirebaseConfig {
        api_key: api_key.into(),
        project_id: project_id.into(),
    })
}

// ---------------------------------------------------------------------------------------
// What Swift sees

/// What went wrong with sync, for the Sync screen. `household::Problem`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SyncProblem {
    /// No network, or the service was down. Changes wait and go later.
    Offline { message: String },
    /// The free plan's daily quota is spent; it resets at midnight Pacific.
    Quota,
    /// The session was revoked or expired for good. Needs the password again.
    SignInAgain,
    /// Anything else, already worded.
    Failed { message: String },
}

impl From<Problem> for SyncProblem {
    fn from(p: Problem) -> Self {
        match p {
            Problem::Offline { message } => Self::Offline { message },
            Problem::Quota => Self::Quota,
            Problem::SignInAgain => Self::SignInAgain,
            Problem::Failed { message } => Self::Failed { message },
        }
    }
}

/// Everything the Sync screen shows. `household::Status`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SyncStatus {
    /// Whether this build has a Firebase project compiled in. Without one there is no
    /// sync to offer.
    pub available: bool,
    pub signed_in: bool,
    pub email: Option<String>,
    pub device_name: Option<String>,
    pub device_id: Option<String>,
    /// Whether the first sync finished and changes are being captured.
    pub joined: bool,
    /// Changes made here and not yet sent.
    pub pending: u64,
    /// Received changes waiting for something this device has not imported yet.
    pub parked: u64,
    /// Unix seconds.
    pub last_sync_at: Option<i64>,
    pub problem: Option<SyncProblem>,
    /// Today's Firestore reads and writes from this device, of 50,000 and 20,000.
    pub reads_today: u64,
    pub writes_today: u64,
    /// Past this device's share of the free quota, so syncing less often until it resets.
    pub thrifty: bool,
}

impl From<Status> for SyncStatus {
    fn from(s: Status) -> Self {
        Self {
            available: s.available,
            signed_in: s.signed_in,
            email: s.email,
            device_name: s.device_name,
            device_id: s.device_id,
            joined: s.joined,
            pending: s.pending as u64,
            parked: s.parked as u64,
            last_sync_at: s.last_sync_at,
            problem: s.problem.map(Into::into),
            reads_today: s.usage.reads,
            writes_today: s.usage.writes,
            thrifty: s.thrifty,
        }
    }
}

/// What one round did. `engine::Report`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SyncReport {
    pub pushed: u64,
    pub pulled: u64,
    pub applied: u64,
    pub parked: u64,
    /// Records this version could not read: a newer Aurora's, or damaged.
    pub undecodable: u64,
    /// Local providers or profiles that took the household's id for the same thing.
    pub adopted: u64,
}

impl From<Report> for SyncReport {
    fn from(r: Report) -> Self {
        Self {
            pushed: r.pushed as u64,
            pulled: r.pulled as u64,
            applied: r.applied as u64,
            parked: r.parked as u64,
            undecodable: r.undecodable as u64,
            adopted: r.adopted as u64,
        }
    }
}

/// What setting up or signing in did. `household::Joined`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SyncJoined {
    /// Only when this device just set the household up: the code to write down. Shown
    /// once; afterwards only to someone who enters the passphrase.
    pub recovery_code: Option<String>,
    /// The first sync, or none when it did not finish. Signed in either way.
    pub report: Option<SyncReport>,
    pub problem: Option<SyncProblem>,
}

impl From<Joined> for SyncJoined {
    fn from(j: Joined) -> Self {
        Self {
            recovery_code: j.recovery_code,
            report: j.report.map(Into::into),
            problem: j.problem.map(Into::into),
        }
    }
}

/// `aurora-app/src/sync.rs` `sync_error`: the words Windows shows.
fn sync_error(e: SyncError) -> AuroraError {
    match e {
        SyncError::Account(m) => AuroraError::other(m),
        SyncError::Offline(_) => AuroraError::Network {
            message: "Could not reach the sync service. Check the connection and try again.".into(),
        },
        SyncError::Unauthenticated => {
            AuroraError::other("The sync account needs signing in again.")
        }
        other => AuroraError::other(other.to_string()),
    }
}

// ---------------------------------------------------------------------------------------
// The host

/// Any [`Http`] behind one type, so [`AuroraCore`] -- which UniFFI cannot make generic --
/// can hold the real client and the tests the in-memory Firebase.
#[derive(Clone)]
pub(crate) struct AnyHttp(Arc<dyn Http>);

impl AnyHttp {
    #[cfg(test)]
    pub(crate) fn new(http: impl Http + 'static) -> Self {
        Self(Arc::new(http))
    }
}

impl Http for AnyHttp {
    fn send(
        &self,
        method: Method,
        url: &str,
        bearer: Option<&str>,
        body: Option<&Body>,
    ) -> aurora_sync::Result<Reply> {
        self.0.send(method, url, bearer, body)
    }
}

/// The household's secrets, in the Keychain. `aurora-app/src/sync.rs` `CredentialSecrets`.
struct KeychainSecrets {
    store: Arc<dyn CredentialStore>,
    /// `aurora-{namespace}-sync-`, as on Windows.
    prefix: String,
}

impl Secrets for KeychainSecrets {
    fn get(&self, name: &str) -> Option<String> {
        self.store.get(&format!("{}{name}", self.prefix)).ok()
    }

    fn set(&self, name: &str, value: &str) -> aurora_sync::Result<()> {
        self.store
            .set(&format!("{}{name}", self.prefix), value)
            .map_err(|e| {
                SyncError::Account(format!(
                    "Signed in, but the Keychain would not store the sign-in: {e}"
                ))
            })
    }

    fn delete(&self, name: &str) {
        let _ = self.store.delete(&format!("{}{name}", self.prefix));
    }

    fn is_persistent(&self) -> bool {
        true
    }
}

/// Sync's half of the core. `aurora-app/src/sync.rs` `Sync`.
pub(crate) struct SyncHost {
    /// `None` only when the HTTPS client could not be built, or there is no credential
    /// namespace.
    household: Option<Household<AnyHttp>>,
    library: PathBuf,
    /// Opened on first use, so a library that never signs in never opens it. Held for a
    /// whole round and by every account call, so two never run at once. Never taken
    /// while the core's `db` is held, nor the other way round.
    conn: Mutex<Option<Connection>>,
    pacer: Mutex<Pacer>,
    /// The last film whose position was fetched, and when: (profile, kind, id, at).
    last_resume: Mutex<Option<(i64, &'static str, i64, i64)>>,
}

impl SyncHost {
    /// The production host: the built-in project over the real network.
    pub(crate) fn new(
        library: PathBuf,
        db: &Connection,
        credentials: Arc<dyn CredentialStore>,
    ) -> Self {
        let http = match ReqwestHttp::new() {
            Ok(h) => Some(AnyHttp(Arc::new(h))),
            Err(e) => {
                tracing::error!("sync is unavailable: no HTTPS client: {e}");
                None
            }
        };
        Self::with(library, db, credentials, http, built_in_config())
    }

    pub(crate) fn with(
        library: PathBuf,
        db: &Connection,
        credentials: Arc<dyn CredentialStore>,
        http: Option<AnyHttp>,
        cfg: Option<FirebaseConfig>,
    ) -> Self {
        let household = match (aurora_ingest::credentials::namespace(db), http) {
            (Ok(ns), Some(http)) => Some(Household::new(
                http,
                cfg,
                Arc::new(KeychainSecrets {
                    store: credentials,
                    prefix: format!("aurora-{ns}-sync-"),
                }),
            )),
            (Err(e), _) => {
                tracing::error!("sync is unavailable: no credential namespace: {e}");
                None
            }
            (_, None) => None,
        };
        Self {
            household,
            library,
            conn: Mutex::new(None),
            pacer: Mutex::new(Pacer::default()),
            last_resume: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_kdf(mut self, kdf: aurora_sync::vault::KdfParams) -> Self {
        self.household = self.household.map(|h| h.with_kdf(kdf));
        self
    }

    fn household(&self) -> Result<&Household<AnyHttp>> {
        self.household
            .as_ref()
            .ok_or_else(|| AuroraError::other("Sync could not start on this device."))
    }

    /// Run `f` on sync's own connection, opening it the first time.
    fn with_conn<T>(
        &self,
        f: impl FnOnce(&Household<AnyHttp>, &mut Connection) -> Result<T>,
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

    /// Ask for a round soon: an import may have brought in what parked records were
    /// waiting for.
    pub(crate) fn wake(&self) {
        self.pacer.lock().wake();
    }

    /// `aurora-app/src/sync.rs` `Sync::refresh_progress`, waiting at most
    /// [`RESUME_BUDGET_MS`]. Never an error: a position that could not be fetched is the
    /// one this device already had.
    pub(crate) fn refresh_progress(
        self: &Arc<Self>,
        enabled: bool,
        profile_id: i64,
        kind: &'static str,
        id: i64,
    ) {
        let Some(household) = &self.household else {
            return;
        };
        let now = crate::now_unix();
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
                // A round holds the connection: it is pulling everything anyway.
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
            let _ = wait.recv_timeout(Duration::from_millis(RESUME_BUDGET_MS));
        }
    }

    /// One tick: sync if signed in and the pacer says a round is due.
    /// `aurora-app/src/sync.rs` `Sync::tick`.
    pub(crate) fn tick(&self, now: i64) {
        let Some(household) = &self.household else {
            return;
        };
        if !household.is_available() {
            return;
        }
        // Swift may tick from more than one task (a scene coming back while the loop is
        // mid-round); a tick that finds a round running has nothing to add.
        let Some(mut slot) = self.conn.try_lock() else {
            return;
        };
        if slot.is_none() {
            match aurora_db::open_secondary(&self.library) {
                Ok(c) => *slot = Some(c),
                Err(e) => {
                    tracing::warn!("sync could not open its connection: {e}");
                    return;
                }
            }
        }
        let conn = slot.as_mut().expect("opened just above");
        let before = match household.status(conn) {
            Ok(s) if s.signed_in => s,
            Ok(_) => return,
            Err(e) => {
                tracing::warn!("sync could not run: {e}");
                return;
            }
        };
        let waiting = match Waiting::read(conn, now) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("sync could not run: {e}");
                return;
            }
        };
        if !self.pacer.lock().due(now, waiting) {
            return;
        }
        let outcome = household.sync(conn);
        drop(slot);
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
            (Ok(_), _) if before.problem.is_some() => tracing::info!("sync is working again"),
            (Err(e), Some(p)) if before.problem.as_ref() != Some(p) => {
                tracing::warn!("sync stopped: {e}")
            }
            _ => {}
        }
        self.pacer.lock().after(now, problem.as_ref());
    }
}

// ---------------------------------------------------------------------------------------
// The Sync screen's calls

#[uniffi::export]
impl AuroraCore {
    /// Everything the Sync screen shows. Read on the core's connection, so it never
    /// waits behind a round in flight.
    pub fn sync_status(&self) -> Result<SyncStatus> {
        let Some(household) = &self.sync.household else {
            return Ok(SyncStatus::from(Status::default()));
        };
        let db = self.db.lock();
        Ok(household.status(&db).map_err(sync_error)?.into())
    }

    /// Create the household's account, set up its encryption, and send this device's
    /// library. The passphrase never leaves the device. At least eight characters.
    pub fn sync_create_account(
        &self,
        email: String,
        password: String,
        passphrase: String,
        device_name: String,
    ) -> Result<SyncJoined> {
        // Behind Settings: refused while they are locked (F-38, `require_settings`).
        crate::parental::require_settings(&self.db.lock())?;
        let joined = self.sync.with_conn(|household, conn| {
            let account = Account {
                email: &email,
                password: &password,
                device_name: &device_name,
            };
            household
                .create_account(conn, &account, &passphrase)
                .map_err(sync_error)
        })?;
        self.sync
            .restart_pacing(crate::now_unix(), joined.problem.as_ref());
        Ok(joined.into())
    }

    /// Join the household from this device, with the passphrase or the recovery code.
    pub fn sync_sign_in(
        &self,
        email: String,
        password: String,
        passphrase: Option<String>,
        recovery_code: Option<String>,
        device_name: String,
    ) -> Result<SyncJoined> {
        // Behind Settings: refused while they are locked (F-38, `require_settings`).
        crate::parental::require_settings(&self.db.lock())?;
        let unlock = match (passphrase, recovery_code) {
            (_, Some(code)) if !code.trim().is_empty() => Unlock::RecoveryCode(code),
            (Some(p), _) => Unlock::Passphrase(p),
            _ => {
                return Err(AuroraError::other(
                    "Enter the sync passphrase, or the recovery code.",
                ))
            }
        };
        let joined = self.sync.with_conn(|household, conn| {
            let account = Account {
                email: &email,
                password: &password,
                device_name: &device_name,
            };
            household
                .sign_in(conn, &account, unlock)
                .map_err(sync_error)
        })?;
        self.sync
            .restart_pacing(crate::now_unix(), joined.problem.as_ref());
        Ok(joined.into())
    }

    /// A round now, whatever the pacer thinks: "Sync now".
    pub fn sync_now(&self) -> Result<SyncReport> {
        let outcome = self
            .sync
            .with_conn(|household, conn| Ok(household.sync(conn)))?;
        let problem = outcome.as_ref().err().map(Problem::of);
        self.sync
            .restart_pacing(crate::now_unix(), problem.as_ref());
        Ok(outcome.map_err(sync_error)?.into())
    }

    /// Delete everything the household has on the server and upload this device's
    /// library in its place. Other devices keep their own libraries.
    pub fn sync_reset(&self) -> Result<SyncReport> {
        // Behind Settings: refused while they are locked (F-38, `require_settings`).
        crate::parental::require_settings(&self.db.lock())?;
        let outcome = self
            .sync
            .with_conn(|household, conn| Ok(household.reset(conn)))?;
        let problem = outcome.as_ref().err().map(Problem::of);
        self.sync
            .restart_pacing(crate::now_unix(), problem.as_ref());
        Ok(outcome.map_err(sync_error)?.into())
    }

    /// Stop syncing here. The library is untouched; only what sync knew is forgotten.
    pub fn sync_sign_out(&self) -> Result<()> {
        // Behind Settings: refused while they are locked (F-38, `require_settings`).
        crate::parental::require_settings(&self.db.lock())?;
        self.sync
            .with_conn(|household, conn| household.sign_out(conn).map_err(sync_error))?;
        *self.sync.pacer.lock() = Pacer::default();
        Ok(())
    }

    /// The recovery code again, for someone who knows the passphrase. Works offline.
    pub fn sync_recovery_code(&self, passphrase: String) -> Result<String> {
        // Behind Settings: refused while they are locked (F-38, `require_settings`).
        crate::parental::require_settings(&self.db.lock())?;
        let household = self.sync.household()?;
        let db = self.db.lock();
        household
            .recovery_code(&db, &passphrase)
            .map_err(sync_error)
    }

    /// Sync if a round is due. Swift calls this every [`Self::sync_tick_secs`] while a
    /// scene is active, off the main actor; it returns at once when nothing is due.
    pub fn sync_tick(&self) {
        self.sync.tick(crate::now_unix());
    }

    /// How often to call [`Self::sync_tick`], in seconds.
    pub fn sync_tick_secs(&self) -> u64 {
        TICK_SECS
    }
}

impl AuroraCore {
    /// Before a film's or an episode's position is shown: take a newer one from the
    /// household's other devices, waiting at most 1.5 s. `aurora-app/src/library.rs`
    /// `progress_get`.
    pub(crate) fn refresh_progress_from_household(&self, profile_id: i64, kind: VodKind, id: i64) {
        let enabled = {
            let db = self.db.lock();
            aurora_db::repo::sync::is_enabled(&db).unwrap_or(false)
        };
        let kind = match kind {
            VodKind::Movie => "movie",
            VodKind::Episode => "episode",
        };
        self.sync.refresh_progress(enabled, profile_id, kind, id);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use aurora_sync::firebase::fake::FakeFirebase;

    use super::*;
    use crate::secrets::tests::MemorySecrets;
    use crate::testing::core_with_sync;

    const PASS: &str = "correct horse battery";

    /// One device: a library with the panel's channel and film, as an import would leave
    /// it. `offset` keeps row ids apart from the other device's, because ids are local.
    struct Device {
        core: Arc<AuroraCore>,
        keychain: Arc<MemorySecrets>,
        dir: tempfile::TempDir,
        channel: i64,
        film: i64,
    }

    fn device(fb: &FakeFirebase, offset: i64) -> Device {
        let dir = tempfile::tempdir().unwrap();
        let keychain = Arc::new(MemorySecrets::default());
        let core = core_with_sync(dir.path(), fb, keychain.clone());
        let (channel, film) = (10 + offset, 20 + offset);
        core.db
            .lock()
            .execute_batch(&format!(
                "INSERT INTO providers (id,name,kind,base_url,username,created_at)
                      VALUES ({p},'Line','xtream','http://panel.example','viewer',0);
                 INSERT INTO channels (id,provider_id,provider_key,name,match_key,last_seen_at)
                      VALUES ({channel},{p},'501','BBC One','bbcone',0);
                 INSERT INTO movies (id,provider_id,provider_key,title,match_key,url,last_seen_at)
                      VALUES ({film},{p},'9001','Heat','heat','http://x/heat.mkv',0);",
                p = 1 + offset,
            ))
            .unwrap();
        Device {
            core,
            keychain,
            dir,
            channel,
            film,
        }
    }

    fn create(d: &Device) -> SyncJoined {
        d.core
            .sync_create_account(
                "home@example.com".into(),
                "hunter22".into(),
                PASS.into(),
                "Living room".into(),
            )
            .unwrap()
    }

    fn join(d: &Device) -> SyncJoined {
        d.core
            .sync_sign_in(
                "home@example.com".into(),
                "hunter22".into(),
                Some(PASS.into()),
                None,
                "iPhone".into(),
            )
            .unwrap()
    }

    fn favourites(d: &Device) -> i64 {
        d.core
            .db
            .lock()
            .query_row(
                "SELECT count(*) FROM favorites WHERE item_kind = 'channel' AND item_id = ?1",
                [d.channel],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn a_favourite_and_a_films_position_follow_the_viewer_to_another_device() {
        let fb = FakeFirebase::new();
        let tv = device(&fb, 0);
        let phone = device(&fb, 100);

        let made = create(&tv);
        assert!(
            made.recovery_code.is_some(),
            "the setting-up device shows the code"
        );
        assert!(made.problem.is_none(), "{:?}", made.problem);
        assert!(tv.core.toggle_favorite(1, tv.channel, None).unwrap());
        tv.core
            .save_progress(1, VodKind::Movie, tv.film, 1_234, 5_400)
            .unwrap();
        assert!(tv.core.sync_now().unwrap().pushed >= 2);

        let joined = join(&phone);
        assert!(
            joined.recovery_code.is_none(),
            "only the first device gets the code"
        );
        assert!(joined.problem.is_none(), "{:?}", joined.problem);
        assert_eq!(favourites(&phone), 1, "the favourite reached the phone");
        let there = phone
            .core
            .watch_progress(1, VodKind::Movie, phone.film)
            .unwrap();
        assert_eq!(there.map(|p| p.position_secs), Some(1_234));

        // Watched on to 40 minutes on the phone. Opening the film on the TV asks the
        // household for that one record before showing it -- no round needed.
        phone
            .core
            .save_progress(1, VodKind::Movie, phone.film, 2_400, 5_400)
            .unwrap();
        phone.core.sync_now().unwrap();
        let before = fb.requests().len();
        let here = tv.core.watch_progress(1, VodKind::Movie, tv.film).unwrap();
        assert_eq!(here.map(|p| p.position_secs), Some(2_400));
        assert_eq!(
            fb.requests().len() - before,
            1,
            "one query for one record: {:?}",
            &fb.requests()[before..]
        );

        let status = tv.core.sync_status().unwrap();
        assert!(status.available && status.signed_in && status.joined);
        assert_eq!(status.email.as_deref(), Some("home@example.com"));
        assert_eq!(status.pending, 0);
        assert!(status.reads_today > 0 && status.writes_today > 0);
        assert!(!status.thrifty);
        assert_ne!(
            status.device_id,
            phone.core.sync_status().unwrap().device_id
        );
    }

    #[test]
    fn the_keychain_holds_the_sign_in_but_not_the_hour_long_token_and_a_restart_keeps_it() {
        let fb = FakeFirebase::new();
        let tv = device(&fb, 0);
        let phone = device(&fb, 100);
        create(&tv);

        // Named under this library's namespace, as on Windows, so two libraries on one
        // device never share a household by accident.
        let stored = tv.keychain.dump();
        let session = stored
            .iter()
            .find(|(k, _)| k.starts_with("aurora-") && k.ends_with("-sync-session"))
            .map(|(_, v)| v.clone())
            .expect("the sign-in is in the Keychain");
        assert!(stored.iter().any(|(k, _)| k.ends_with("-sync-vault")));
        let session: serde_json::Value = serde_json::from_str(&session).unwrap();
        assert_eq!(session["id_token"], "", "the ID token stays in memory");

        // A restart: the same folder and Keychain, a new core.
        let Device {
            core,
            keychain,
            dir,
            channel,
            ..
        } = tv;
        drop(core);
        let core = core_with_sync(dir.path(), &fb, keychain);
        assert!(core.sync_status().unwrap().signed_in);
        core.toggle_favorite(1, channel, None).unwrap();
        core.sync_now().unwrap();
        join(&phone);
        assert_eq!(favourites(&phone), 1);
    }

    #[test]
    fn a_device_that_never_signed_in_asks_the_server_nothing() {
        let fb = FakeFirebase::new();
        let d = device(&fb, 0);
        d.core.sync_tick();
        d.core.watch_progress(1, VodKind::Movie, d.film).unwrap();
        assert!(fb.requests().is_empty(), "{:?}", fb.requests());
        let status = d.core.sync_status().unwrap();
        assert!(status.available && !status.signed_in);
    }

    #[test]
    fn signing_out_stops_syncing_and_keeps_the_library() {
        let fb = FakeFirebase::new();
        let d = device(&fb, 0);
        create(&d);
        d.core.toggle_favorite(1, d.channel, None).unwrap();
        d.core.sync_sign_out().unwrap();
        assert!(!d.core.sync_status().unwrap().signed_in);
        assert_eq!(favourites(&d), 1, "the library is untouched");
        assert!(d.keychain.dump().is_empty(), "the Keychain items are gone");
        let before = fb.requests().len();
        d.core.sync_tick();
        assert_eq!(fb.requests().len(), before);
    }

    #[test]
    fn a_wrong_passphrase_is_called_that() {
        let fb = FakeFirebase::new();
        let tv = device(&fb, 0);
        let phone = device(&fb, 100);
        create(&tv);
        let err = phone
            .core
            .sync_sign_in(
                "home@example.com".into(),
                "hunter22".into(),
                Some("not it at all".into()),
                None,
                "iPhone".into(),
            )
            .unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("passphrase"),
            "{err}"
        );
        assert!(!phone.core.sync_status().unwrap().signed_in);
    }

    #[test]
    fn the_recovery_code_joins_a_device_and_can_be_shown_again() {
        let fb = FakeFirebase::new();
        let tv = device(&fb, 0);
        let phone = device(&fb, 100);
        let code = create(&tv).recovery_code.unwrap();
        assert_eq!(tv.core.sync_recovery_code(PASS.into()).unwrap(), code);
        let joined = phone
            .core
            .sync_sign_in(
                "home@example.com".into(),
                "hunter22".into(),
                None,
                Some(code),
                "iPhone".into(),
            )
            .unwrap();
        assert!(joined.problem.is_none());
    }

    #[test]
    fn a_build_without_a_firebase_project_offers_no_sync() {
        let dir = tempfile::tempdir().unwrap();
        let core = AuroraCore::open_with(
            dir.path().display().to_string(),
            Arc::new(MemorySecrets::default()),
            |library, db, creds| {
                SyncHost::with(
                    library,
                    db,
                    creds,
                    Some(AnyHttp::new(FakeFirebase::new())),
                    None,
                )
            },
        )
        .unwrap();
        assert!(!core.sync_status().unwrap().available);
        assert!(core
            .sync_create_account("a@b.c".into(), "hunter22".into(), PASS.into(), "x".into())
            .is_err());
    }
}
