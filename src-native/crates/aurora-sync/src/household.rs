//! What a host app drives: this device's sync account, where its secrets live, and when
//! to sync (`docs/APPLE_PORT_AND_SYNC.md` §3.2, §3.5, §3.6, §3.8).
//!
//! [`Household`] turns the pieces below it -- Firebase accounts, the vault, the engine --
//! into the handful of things the Sync panel offers: create an account, sign in, sync,
//! show the recovery code, reset, sign out. [`Pacer`] decides when the background loop
//! should sync. Both are plain Rust so the whole flow is tested here, against
//! [`crate::firebase::fake::FakeFirebase`], with no window and no project.
//!
//! **Where things are kept.** The refresh token and the vault key are secrets: they go to
//! the platform credential store through [`Secrets`], never into SQLite (§3.2). What is
//! not secret -- the account's email, this device's id and name, the wrapped vault key,
//! when the last sync finished and what went wrong -- is in `sync_state`, so the panel
//! can read it on a read-only connection without waiting behind a sync in flight.
//! Signing out runs [`db::disable`], which clears all of it except the clock.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use aurora_db::repo::sync as db;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::codec::Codec;
use crate::engine::{self, Engine, Report};
use crate::error::{Result, SyncError};
use crate::firebase::{self, FirebaseConfig, FirestoreTransport, Http, Session, Used};
use crate::secrets::{self, SharedSecrets};
use crate::transport::SyncTransport;
use crate::vault::{KdfParams, VaultCodec, VaultKey, WrappedKey};

/// Where a host keeps this device's sync secrets. On Windows, Credential Manager under
/// a per-library name; in tests, memory.
pub trait Secrets: Send + Sync {
    fn get(&self, name: &str) -> Option<String>;
    fn set(&self, name: &str, value: &str) -> Result<()>;
    fn delete(&self, name: &str);
    /// Whether these survive a restart, so the panel can say when they will not.
    fn is_persistent(&self) -> bool;
}

/// The Firebase session, as JSON: the account and its refresh token, which is what keeps
/// the device signed in. Not the ID token -- see [`stored`].
pub const SESSION_SECRET: &str = "session";
/// The household's vault key, in hex.
pub const VAULT_SECRET: &str = "vault";

// `sync_state` keys this module owns. `db::disable` clears every one of them.
const EMAIL: &str = "account.email";
const DEVICE_NAME: &str = "device.name";
const WRAPPED: &str = "vault.wrapped";
const LAST_SYNC: &str = "last_sync_at";
const PROBLEM: &str = "problem";

/// The shortest passphrase accepted. It is the only thing between the server's copy of
/// the vault key and an offline guessing attack, Argon2id notwithstanding.
pub const MIN_PASSPHRASE: usize = 8;

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// What stopped the last sync, in terms the panel can act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Problem {
    /// No network, or the service was down. Changes wait in the outbox.
    Offline { message: String },
    /// The free plan's daily quota is spent; it resets at midnight Pacific (§3.7).
    Quota,
    /// The session was revoked or expired for good. Needs the password again.
    SignInAgain,
    /// Anything else, with the text to show.
    Failed { message: String },
}

impl Problem {
    pub fn of(e: &SyncError) -> Self {
        match e {
            SyncError::Offline(m) => Problem::Offline { message: m.clone() },
            SyncError::QuotaExhausted => Problem::Quota,
            SyncError::Unauthenticated => Problem::SignInAgain,
            other => Problem::Failed {
                message: other.to_string(),
            },
        }
    }
}

/// Everything the Sync panel shows. The default is a build with no sync at all.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Whether this build knows which Firebase project to use. Without one there is no
    /// sync to offer, and the panel says so rather than showing a form that cannot work.
    pub available: bool,
    pub signed_in: bool,
    pub email: Option<String>,
    pub device_name: Option<String>,
    pub device_id: Option<String>,
    /// Whether the first sync has finished and changes are being captured.
    pub joined: bool,
    /// Changes made here and not yet sent.
    pub pending: usize,
    /// Records received that refer to something this device does not have yet -- a
    /// favourite of a channel from a playlist not imported here. Applied once it is.
    pub parked: usize,
    /// Unix seconds.
    pub last_sync_at: Option<i64>,
    pub problem: Option<Problem>,
    /// False where there is no OS credential store, so signing in lasts one session.
    pub secrets_persistent: bool,
    /// What this device has spent of the free plan today (§3.7).
    pub usage: Usage,
    /// Past its share of the quota, so syncing less often until it resets.
    pub thrifty: bool,
    /// Records from other devices this version could not read -- written by a newer
    /// Aurora, or damaged. Skipped, never applied, and fetched again after an update
    /// (§3.8).
    pub unreadable: usize,
    /// Whether the household shares sign-ins and keys (§3.4). Off unless chosen.
    pub shares_secrets: bool,
    /// Whether this device could: whether its host gave sync a credential store.
    pub can_share_secrets: bool,
}

/// How a device proves it may open the household's vault.
#[derive(Clone)]
pub enum Unlock {
    Passphrase(String),
    /// The code shown at setup, for a device whose owner forgot the passphrase.
    RecoveryCode(String),
}

impl std::fmt::Debug for Unlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Unlock::Passphrase(_) => "Unlock::Passphrase(..)",
            Unlock::RecoveryCode(_) => "Unlock::RecoveryCode(..)",
        })
    }
}

/// What signing in or creating an account did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Joined {
    /// Set only when this device created the vault: the code to write down. It is shown
    /// once here and afterwards only to someone who knows the passphrase.
    pub recovery_code: Option<String>,
    /// The first sync. `None` when it did not finish -- the device is signed in either
    /// way, and the background loop carries on from where it stopped.
    pub report: Option<Report>,
    pub problem: Option<Problem>,
}

/// Credentials for an account call. Never logged.
pub struct Account<'a> {
    pub email: &'a str,
    pub password: &'a str,
    pub device_name: &'a str,
}

type Live<H> = Engine<FirestoreTransport<H>, VaultCodec>;

pub struct Household<H: Http + Clone> {
    http: H,
    cfg: Option<FirebaseConfig>,
    secrets: Arc<dyn Secrets>,
    kdf: KdfParams,
    /// The session as last used, ID token and all, so each round does not mint a new
    /// one. Only its long-lived half is in `secrets`.
    live: Mutex<Option<Session>>,
    /// The host's provider passwords and API keys, when it can share them (§3.4).
    shared: Option<Arc<dyn SharedSecrets>>,
}

/// What goes in the credential store: the session without its ID token.
///
/// The ID token is a JWT of about a kilobyte that lasts an hour, and Windows Credential
/// Manager refuses a secret longer than 2,560 bytes of UTF-16 -- 1,280 characters. The
/// whole session was past that, so on Windows setting sync up signed in and then failed
/// to save the sign-in. The refresh token mints a new ID token on first use, which costs
/// one token request after a restart and no Firestore reads.
fn stored(session: &Session) -> Session {
    Session {
        id_token: String::new(),
        expires_at: 0,
        ..session.clone()
    }
}

impl<H: Http + Clone> Household<H> {
    /// `cfg` is `None` in a build with no Firebase project; every call then refuses.
    pub fn new(http: H, cfg: Option<FirebaseConfig>, secrets: Arc<dyn Secrets>) -> Self {
        Self {
            http,
            cfg,
            secrets,
            kdf: KdfParams::default(),
            live: Mutex::new(None),
            shared: None,
        }
    }

    /// Let this household sync the host's sign-ins and keys, once somebody turns that on
    /// ([`Household::set_share_secrets`]). A host that never calls this cannot share them,
    /// and skips them when they arrive until it can (§3.8).
    pub fn with_shared_secrets(mut self, shared: Arc<dyn SharedSecrets>) -> Self {
        self.shared = Some(shared);
        self
    }

    /// Turn sharing sign-ins and keys on or off for the household. The choice is a
    /// setting that syncs, so it is the household's, not just this device's.
    ///
    /// Off stops the sending. What was sent stays in the household's encrypted records
    /// until sync is reset, because removing it would also remove it from every device
    /// that had received it -- which is deleting somebody's passwords, not unsharing them.
    pub fn set_share_secrets(&self, conn: &Connection, on: bool) -> Result<()> {
        if on && self.shared.is_none() {
            return Err(SyncError::Account(
                "This device cannot keep sign-ins from other devices.".into(),
            ));
        }
        secrets::set_sharing(conn, on)
    }

    /// A cheaper key derivation, for tests.
    pub fn with_kdf(mut self, kdf: KdfParams) -> Self {
        self.kdf = kdf;
        self
    }

    pub fn is_available(&self) -> bool {
        self.cfg.is_some()
    }

    fn cfg(&self) -> Result<&FirebaseConfig> {
        self.cfg.as_ref().ok_or_else(|| {
            SyncError::Account("Sync is not available in this build of Aurora.".into())
        })
    }

    /// Whether this device holds what it needs to sync.
    fn has_secrets(&self) -> bool {
        self.secrets.get(SESSION_SECRET).is_some() && self.secrets.get(VAULT_SECRET).is_some()
    }

    /// The panel's view. Reads only, so it can run on a read-only connection.
    pub fn status(&self, conn: &Connection) -> Result<Status> {
        let email = db::get_state(conn, EMAIL)?;
        let usage = Usage::today(conn, now_secs())?;
        let parked: i64 = conn.query_row("SELECT COUNT(*) FROM sync_pending", [], |r| r.get(0))?;
        Ok(Status {
            available: self.is_available(),
            signed_in: email.is_some() && self.has_secrets(),
            email,
            device_name: db::get_state(conn, DEVICE_NAME)?,
            device_id: db::get_state(conn, engine::DEVICE)?,
            joined: db::is_enabled(conn)?,
            pending: db::outbox_len(conn)?,
            parked: usize::try_from(parked).unwrap_or(0),
            last_sync_at: db::get_state(conn, LAST_SYNC)?.and_then(|v| v.parse().ok()),
            problem: db::get_state(conn, PROBLEM)?.and_then(|v| serde_json::from_str(&v).ok()),
            secrets_persistent: self.secrets.is_persistent(),
            usage,
            thrifty: usage.is_thrifty(),
            unreadable: db::unreadable_len(conn)?,
            shares_secrets: secrets::sharing(conn)?,
            can_share_secrets: self.shared.is_some(),
        })
    }

    /// Create the household's account and its vault, then send this device's library.
    pub fn create_account(
        &self,
        conn: &mut Connection,
        account: &Account<'_>,
        passphrase: &str,
    ) -> Result<Joined> {
        let cfg = self.cfg()?;
        self.check_can_sign_in(conn, account.email)?;
        check_passphrase(passphrase)?;
        let session = firebase::sign_up(&self.http, cfg, account.email, account.password)?;
        // Best effort: an address nobody has proved is the one thing that makes a lost
        // password unrecoverable, but failing to send the email is no reason to fail
        // setting sync up. The panel offers to send it again.
        if let Err(e) = firebase::send_verification(&self.http, cfg, &session.id_token) {
            tracing::info!("could not send the verification email: {e}");
        }
        self.join(
            conn,
            session,
            account,
            Unlock::Passphrase(passphrase.to_string()),
        )
    }

    /// Sign this device in to an existing account.
    ///
    /// An account whose setup never finished -- created on another device that lost its
    /// connection before the vault was stored -- has no vault yet; with a passphrase this
    /// device makes it, exactly as creating the account would have.
    pub fn sign_in(
        &self,
        conn: &mut Connection,
        account: &Account<'_>,
        unlock: Unlock,
    ) -> Result<Joined> {
        let cfg = self.cfg()?;
        self.check_can_sign_in(conn, account.email)?;
        let session = firebase::sign_in(&self.http, cfg, account.email, account.password)?;
        self.join(conn, session, account, unlock)
    }

    /// Signing in again to the account already here is how "Sign in again" works, and
    /// keeps everything. Another account needs a sign-out first, so that two
    /// households' records are never mixed through this device's shadows.
    fn check_can_sign_in(&self, conn: &Connection, email: &str) -> Result<()> {
        match db::get_state(conn, EMAIL)? {
            Some(current) if !current.eq_ignore_ascii_case(email.trim()) => {
                Err(SyncError::Account(format!(
                    "This device is signed in to sync as {current}. Sign out first to use \
                     another account."
                )))
            }
            _ => Ok(()),
        }
    }

    fn join(
        &self,
        conn: &mut Connection,
        session: Session,
        account: &Account<'_>,
        unlock: Unlock,
    ) -> Result<Joined> {
        let cfg = self.cfg()?.clone();
        let transport = FirestoreTransport::new(self.http.clone(), cfg, session);

        let (key, wrapped, recovery_code) = match (transport.get_vault()?, unlock) {
            (Some(w), Unlock::Passphrase(p)) => (w.unwrap_key(&p)?, w, None),
            (Some(w), Unlock::RecoveryCode(c)) => {
                let key = VaultKey::from_recovery_code(&c)?;
                check_key(&transport, &key)?;
                (key, w, None)
            }
            (None, Unlock::Passphrase(p)) => {
                check_passphrase(&p)?;
                let key = VaultKey::generate()?;
                let w = WrappedKey::wrap_with(&key, &p, self.kdf)?;
                if transport.create_vault(&w)? {
                    let code = key.recovery_code();
                    (key, w, Some(code))
                } else {
                    // Another device set the household up in the same moment. Use its key.
                    let theirs = transport.get_vault()?.ok_or_else(|| {
                        SyncError::Protocol("the vault was taken and then not found".into())
                    })?;
                    (theirs.unwrap_key(&p)?, theirs, None)
                }
            }
            (None, Unlock::RecoveryCode(_)) => {
                return Err(SyncError::Account(
                    "This account has not finished setting sync up, so it has no recovery \
                     code yet. Sign in with a passphrase to finish it."
                        .into(),
                ))
            }
        };

        // Kept across "sign in again"; a device that signed out has none and gets a new
        // one, because its old records on the server are another writer's now.
        let device = match db::get_state(conn, engine::DEVICE)? {
            Some(d) => d,
            None => new_device_id()?,
        };

        self.keep_session(transport.session())?;
        self.secrets.set(VAULT_SECRET, &key.to_hex())?;
        db::set_state(conn, EMAIL, account.email.trim())?;
        db::set_state(conn, DEVICE_NAME, device_name(account.device_name).as_str())?;
        db::set_state(conn, WRAPPED, &serde_json::to_string(&wrapped)?)?;
        db::set_state(conn, engine::DEVICE, &device)?;

        let engine =
            Engine::new(transport, VaultCodec::new(&key), device).with_secrets(self.shared.clone());
        let outcome = engine.join(conn);
        self.record(conn, &engine, &outcome)?;
        Ok(Joined {
            recovery_code,
            problem: outcome.as_ref().err().map(Problem::of),
            report: outcome.ok(),
        })
    }

    /// One round: push, pull, retry what was parked. Finishes a first sync that was cut
    /// short, too.
    pub fn sync(&self, conn: &mut Connection) -> Result<Report> {
        let engine = self.engine(conn)?;
        let outcome = engine.join(conn);
        self.record(conn, &engine, &outcome)?;
        outcome
    }

    /// Fetch one record now, ahead of the next round. Returns whether it changed anything.
    pub fn refresh_one(&self, conn: &mut Connection, key: &str) -> Result<bool> {
        let engine = self.engine(conn)?;
        let outcome = engine.refresh_one(conn, key);
        self.settle(conn, &engine)?;
        outcome
    }

    /// What opening a film or an episode does before offering to resume (§3.6): fetch
    /// this profile's position in it from the other devices, so stopping on the TV and
    /// opening the same film here a minute later resumes where the TV stopped rather than
    /// where this PC last knew.
    ///
    /// Nothing is asked of the server when sync is off or the item has no sync key.
    pub fn refresh_progress(
        &self,
        conn: &mut Connection,
        profile_id: i64,
        item_kind: &str,
        item_id: i64,
    ) -> Result<bool> {
        if !self.is_available() || !db::is_enabled(conn)? {
            return Ok(false);
        }
        match db::progress_key(conn, profile_id, item_kind, item_id)? {
            Some(key) => self.refresh_one(conn, &key),
            None => Ok(false),
        }
    }

    /// After any use of the server: save the session if the transport refreshed it, and
    /// add what it spent to today's count. Each engine is built for one operation, so its
    /// transport's count is exactly that operation's.
    fn settle(&self, conn: &Connection, engine: &Live<H>) -> Result<()> {
        Usage::add(conn, now_secs(), engine.transport().used())?;
        self.keep_session(engine.transport().session())
    }

    /// Remember `session` for the next engine, and store its long-lived half if that
    /// changed -- the refresh token rotates now and then, the ID token every hour.
    fn keep_session(&self, session: Session) -> Result<()> {
        let keep = serde_json::to_string(&stored(&session))?;
        if self.secrets.get(SESSION_SECRET).as_deref() != Some(keep.as_str()) {
            self.secrets.set(SESSION_SECRET, &keep)?;
        }
        *self.live.lock().unwrap_or_else(|p| p.into_inner()) = Some(session);
        Ok(())
    }

    /// Throw the server's copy away and upload this device's in its place (§3.5).
    ///
    /// The local side goes first: once shadows and the cursor are gone, the next round
    /// sends everything and reads everything, so a reset that fails half way is finished
    /// by any later sync rather than left inconsistent.
    pub fn reset(&self, conn: &mut Connection) -> Result<Report> {
        let engine = self.engine(conn)?;
        {
            let tx = conn.transaction()?;
            tx.execute("DELETE FROM sync_shadow", [])?;
            tx.execute("DELETE FROM sync_pending", [])?;
            db::clear_state(&tx, engine::CURSOR)?;
            tx.commit()?;
        }
        if db::is_enabled(conn)? {
            db::enable(conn)?;
        }
        let outcome = engine
            .transport()
            .delete_records()
            .and_then(|_| engine.join(conn));
        self.record(conn, &engine, &outcome)?;
        outcome
    }

    /// The recovery code, for someone who can prove they know the passphrase. Checked
    /// against the wrapped key kept from sign-in, so it works offline.
    pub fn recovery_code(&self, conn: &Connection, passphrase: &str) -> Result<String> {
        let wrapped: WrappedKey = db::get_state(conn, WRAPPED)?
            .map(|w| serde_json::from_str(&w))
            .transpose()?
            .ok_or_else(|| SyncError::Account("This device is not signed in to sync.".into()))?;
        Ok(wrapped.unwrap_key(passphrase)?.recovery_code())
    }

    /// Email a password-reset link to `email`. Needs no sign-in -- it is what someone
    /// who has forgotten the password presses -- and says the same whether or not there
    /// is an account with that address.
    pub fn send_password_reset(&self, email: &str) -> Result<()> {
        let email = email.trim();
        if !email.contains('@') {
            return Err(SyncError::Account(
                "Enter the email address the account was made with.".into(),
            ));
        }
        firebase::send_password_reset(&self.http, self.cfg()?, email)
    }

    /// Whether the signed-in account's address has been verified. One account lookup;
    /// no Firestore reads.
    pub fn email_verified(&self, conn: &Connection) -> Result<bool> {
        let engine = self.engine(conn)?;
        let token = engine.transport().fresh_id_token()?;
        self.keep(&engine);
        firebase::email_verified(&self.http, self.cfg()?, &token)
    }

    /// Send the verification email again.
    pub fn send_verification(&self, conn: &Connection) -> Result<()> {
        let engine = self.engine(conn)?;
        let token = engine.transport().fresh_id_token()?;
        self.keep(&engine);
        firebase::send_verification(&self.http, self.cfg()?, &token)
    }

    /// Delete the account: every record and the vault on the server, then the user
    /// itself, then sign this device out. The library on this device is untouched.
    ///
    /// Asks for the account password, for two reasons: Firebase only deletes a user
    /// with a token from a recent sign-in, which the stored session is not; and a
    /// password is the right price for something this final. Other devices find their
    /// session revoked on their next round and say so.
    pub fn delete_account(&self, conn: &mut Connection, password: &str) -> Result<()> {
        let cfg = self.cfg()?.clone();
        let email = db::get_state(conn, EMAIL)?
            .ok_or_else(|| SyncError::Account("This device is not signed in to sync.".into()))?;
        let session = firebase::sign_in(&self.http, &cfg, &email, password)?;
        let token = session.id_token.clone();
        let server = FirestoreTransport::new(self.http.clone(), cfg.clone(), session);
        server.delete_records()?;
        server.delete_vault()?;
        firebase::delete_user(&self.http, &cfg, &token)?;
        self.sign_out(conn)
    }

    /// Keep the session an engine may have refreshed, so the next call reuses it.
    fn keep(&self, engine: &Live<H>) {
        *self.live.lock().unwrap_or_else(|p| p.into_inner()) = Some(engine.transport().session());
    }

    /// Stop syncing on this device. Its library is untouched; only what sync knew goes.
    pub fn sign_out(&self, conn: &mut Connection) -> Result<()> {
        *self.live.lock().unwrap_or_else(|p| p.into_inner()) = None;
        self.secrets.delete(SESSION_SECRET);
        self.secrets.delete(VAULT_SECRET);
        db::disable(conn)?;
        Ok(())
    }

    fn engine(&self, conn: &Connection) -> Result<Live<H>> {
        let not_signed_in = || SyncError::Account("This device is not signed in to sync.".into());
        let cfg = self.cfg()?.clone();
        let saved: Session = self
            .secrets
            .get(SESSION_SECRET)
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .ok_or_else(not_signed_in)?;
        // The one in memory, unless the store now says otherwise: signed in again
        // elsewhere in this process, or a restart, which leaves nothing in memory.
        let live = self.live.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let session = match live {
            Some(l) if stored(&l) == stored(&saved) => l,
            _ => saved,
        };
        let key = self
            .secrets
            .get(VAULT_SECRET)
            .map(|k| VaultKey::from_hex(&k))
            .transpose()?
            .ok_or_else(not_signed_in)?;
        let device = db::get_state(conn, engine::DEVICE)?.ok_or_else(not_signed_in)?;
        Ok(Engine::new(
            FirestoreTransport::new(self.http.clone(), cfg, session),
            VaultCodec::new(&key),
            device,
        )
        .with_secrets(self.shared.clone()))
    }

    /// Keep a rotated session, and note how the round went for the panel.
    fn record(&self, conn: &Connection, engine: &Live<H>, outcome: &Result<Report>) -> Result<()> {
        self.settle(conn, engine)?;
        match outcome {
            Ok(_) => {
                db::set_state(conn, LAST_SYNC, &now_secs().to_string())?;
                db::clear_state(conn, PROBLEM)?;
            }
            Err(e) => {
                db::set_state(conn, PROBLEM, &serde_json::to_string(&Problem::of(e))?)?;
            }
        }
        Ok(())
    }
}

fn check_passphrase(passphrase: &str) -> Result<()> {
    if passphrase.chars().count() < MIN_PASSPHRASE {
        return Err(SyncError::Account(format!(
            "The sync passphrase needs at least {MIN_PASSPHRASE} characters."
        )));
    }
    Ok(())
}

/// A recovery code cannot be checked against the vault without the passphrase, so check
/// it against a record instead: if the household has any, the right key decodes it.
///
/// Without this a mistyped-but-valid code would be accepted, and every record pulled
/// afterwards would be skipped as undecodable while the cursor moved past it.
fn check_key<T: SyncTransport>(transport: &T, key: &VaultKey) -> Result<()> {
    let page = transport.pull(None, 1)?;
    if let Some(env) = page.records.first() {
        if VaultCodec::new(key)
            .decode(&env.doc_id, &env.payload)
            .is_err()
        {
            return Err(SyncError::Account(
                "That recovery code does not unlock this household's sync.".into(),
            ));
        }
    }
    Ok(())
}

fn new_device_id() -> Result<String> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|e| SyncError::Protocol(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn device_name(given: &str) -> String {
    let name: String = given.trim().chars().take(60).collect();
    if name.is_empty() {
        "This device".into()
    } else {
        name
    }
}

// ---------------------------------------------------------------------------------------
// When to sync

/// Wait this long after a change before sending it, so a burst of edits is one round.
pub const DEBOUNCE_SECS: i64 = 3;
/// Watch progress changes every few seconds during playback; send it at most this often
/// (§3.7: about one write a minute while playing).
pub const PROGRESS_SECS: i64 = 60;
/// With nothing to send, look for other devices' changes this often (§3.6).
pub const INTERVAL_SECS: i64 = 120;
const BACKOFF_FIRST_SECS: i64 = 30;
const BACKOFF_MAX_SECS: i64 = 600;

/// What is waiting in the outbox, as far as pacing cares.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Waiting {
    pub changes: usize,
    /// Every waiting change is watch progress, which can wait for the next minute.
    pub only_progress: bool,
    /// This device is past its share of today's quota ([`Usage::is_thrifty`]).
    pub thrifty: bool,
}

impl Waiting {
    pub fn read(conn: &Connection, now: i64) -> Result<Self> {
        let (changes, only_progress): (i64, i64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(MIN(kind = 'progress'), 1) FROM sync_outbox",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(Self {
            changes: usize::try_from(changes).unwrap_or(0),
            only_progress: only_progress != 0,
            thrifty: Usage::today(conn, now)?.is_thrifty(),
        })
    }
}

/// The Spark plan's daily quotas (§3.7). Checked against Firebase's pricing page in
/// November 2026.
pub const QUOTA_READS: u64 = 50_000;
pub const QUOTA_WRITES: u64 = 20_000;
/// The devices a household is planned for: a PC, a TV and a phone. A device can count
/// only what it spent itself, so each takes an equal share of the 70% of the quota at
/// which §3.7 says to slow down.
const PLANNED_DEVICES: u64 = 3;
/// With a thrifty device, progress and the look for others' changes wait this long.
/// Any other edit -- a favourite, a setting -- still goes after [`DEBOUNCE_SECS`].
pub const THRIFTY_SECS: i64 = 600;

/// What this device has spent of the free plan's quota today. "Today" is Firestore's
/// day, which ends at [`next_quota_reset`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Firestore days since the epoch, so a count from yesterday is not read as today's.
    pub day: i64,
    pub reads: u64,
    pub writes: u64,
}

impl Usage {
    /// Which Firestore day `now` falls in. The days run from one reset to the next.
    pub fn day_of(now: i64) -> i64 {
        (next_quota_reset(now) - 1).div_euclid(86_400)
    }

    /// Today's count, or nothing spent when the stored one is from an earlier day.
    pub fn today(conn: &Connection, now: i64) -> Result<Self> {
        let day = Self::day_of(now);
        let stored: Option<Usage> =
            db::get_state(conn, db::USAGE)?.and_then(|v| serde_json::from_str(&v).ok());
        Ok(match stored {
            Some(u) if u.day == day => u,
            _ => Usage {
                day,
                ..Usage::default()
            },
        })
    }

    /// Add what one operation spent.
    pub fn add(conn: &Connection, now: i64, used: Used) -> Result<Self> {
        let mut u = Self::today(conn, now)?;
        if used == Used::default() {
            return Ok(u);
        }
        u.reads += used.reads;
        u.writes += used.writes;
        db::set_state(conn, db::USAGE, &serde_json::to_string(&u)?)?;
        Ok(u)
    }

    /// Past this device's share of 70% of either quota.
    pub fn is_thrifty(&self) -> bool {
        self.reads * 10 * PLANNED_DEVICES >= QUOTA_READS * 7
            || self.writes * 10 * PLANNED_DEVICES >= QUOTA_WRITES * 7
    }
}

/// Decides when the background loop syncs. Pure: the loop passes the time in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pacer {
    last_attempt: Option<i64>,
    failures: u32,
    /// Nothing before this, after a failure.
    hold_until: i64,
    /// The account needs signing in again; nothing until it is.
    stopped: bool,
    /// Something asked for a round soon: launch, an import that may unpark records.
    woken: bool,
}

impl Pacer {
    /// Ask for a round at the next tick, unless a failure is being waited out.
    pub fn wake(&mut self) {
        self.woken = true;
    }

    pub fn due(&self, now: i64, waiting: Waiting) -> bool {
        if self.stopped || now < self.hold_until {
            return false;
        }
        let Some(last) = self.last_attempt else {
            return true;
        };
        // A failure's retry is due as soon as its backoff is over, not a whole interval
        // later.
        if self.woken || self.failures > 0 {
            return true;
        }
        let since = now - last;
        let (progress, interval) = if waiting.thrifty {
            (THRIFTY_SECS, THRIFTY_SECS)
        } else {
            (PROGRESS_SECS, INTERVAL_SECS)
        };
        if waiting.changes > 0 {
            since
                >= if waiting.only_progress {
                    progress
                } else {
                    DEBOUNCE_SECS
                }
        } else {
            since >= interval
        }
    }

    pub fn after(&mut self, now: i64, outcome: Option<&Problem>) {
        self.last_attempt = Some(now);
        self.woken = false;
        match outcome {
            None => {
                self.failures = 0;
                self.hold_until = 0;
            }
            Some(Problem::Quota) => self.hold_until = next_quota_reset(now),
            Some(Problem::SignInAgain) => self.stopped = true,
            Some(Problem::Offline { .. } | Problem::Failed { .. }) => {
                self.failures = self.failures.saturating_add(1);
                let wait = BACKOFF_FIRST_SECS
                    .saturating_mul(1 << (self.failures - 1).min(10))
                    .min(BACKOFF_MAX_SECS);
                self.hold_until = now + wait;
            }
        }
    }

    /// Whether the loop has given up until the account is signed in again.
    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// Whether a failure is being waited out: offline, out of quota, or signed out. Then
    /// nothing else should reach for the server either.
    pub fn is_holding(&self, now: i64) -> bool {
        self.stopped || now < self.hold_until
    }
}

/// When Firestore's daily quotas reset: midnight Pacific. Taken as 08:00 UTC, which is
/// midnight in winter and one in the morning in summer -- an hour late at worst, which
/// costs nothing, where an hour early would spend a round on a refusal.
pub fn next_quota_reset(now: i64) -> i64 {
    const DAY: i64 = 86_400;
    const RESET: i64 = 8 * 3_600;
    let today = now - now.rem_euclid(DAY) + RESET;
    if today > now {
        today
    } else {
        today + DAY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offline() -> Problem {
        Problem::Offline {
            message: "x".into(),
        }
    }

    #[test]
    fn the_first_tick_syncs_and_then_it_waits_for_a_reason() {
        let mut p = Pacer::default();
        assert!(p.due(1_000, Waiting::default()));
        p.after(1_000, None);
        assert!(!p.due(1_001, Waiting::default()));
        assert!(p.due(1_000 + INTERVAL_SECS, Waiting::default()));
    }

    #[test]
    fn a_favourite_goes_in_seconds_and_progress_once_a_minute() {
        let mut p = Pacer::default();
        p.after(1_000, None);
        let edit = Waiting {
            changes: 1,
            only_progress: false,
            thrifty: false,
        };
        let progress = Waiting {
            changes: 1,
            only_progress: true,
            thrifty: false,
        };
        assert!(!p.due(1_001, edit));
        assert!(p.due(1_000 + DEBOUNCE_SECS, edit));
        assert!(!p.due(1_030, progress));
        assert!(p.due(1_000 + PROGRESS_SECS, progress));
    }

    #[test]
    fn offline_backs_off_doubling_up_to_ten_minutes_and_a_success_resets_it() {
        let mut p = Pacer::default();
        let mut now = 0;
        let mut waits = Vec::new();
        for _ in 0..7 {
            p.after(now, Some(&offline()));
            let mut t = now + 1;
            while !p.due(t, Waiting::default()) {
                t += 1;
            }
            waits.push(t - now);
            now = t;
        }
        assert_eq!(waits, [30, 60, 120, 240, 480, 600, 600]);
        p.after(now, None);
        p.wake();
        assert!(p.due(now + 1, Waiting::default()));
    }

    #[test]
    fn a_wake_does_not_cut_a_backoff_short() {
        let mut p = Pacer::default();
        p.after(0, Some(&offline()));
        p.wake();
        assert!(!p.due(5, Waiting::default()));
        assert!(p.due(30, Waiting::default()));
    }

    #[test]
    fn a_spent_quota_waits_for_the_reset_and_a_revoked_account_stops() {
        // 20:00 UTC on some day.
        let now = 1_795_464_000;
        let mut p = Pacer::default();
        p.after(now, Some(&Problem::Quota));
        let reset = next_quota_reset(now);
        assert_eq!(reset - now, 12 * 3_600);
        assert!(!p.due(reset - 1, Waiting::default()));
        assert!(p.due(reset, Waiting::default()));

        p.after(now, Some(&Problem::SignInAgain));
        assert!(p.is_stopped());
        assert!(!p.due(now + 10 * 86_400, Waiting::default()));
    }

    #[test]
    fn a_thrifty_device_holds_progress_and_polling_for_ten_minutes_but_not_a_favourite() {
        let mut p = Pacer::default();
        p.after(1_000, None);
        let thrifty = |only_progress| Waiting {
            changes: 1,
            only_progress,
            thrifty: true,
        };
        assert!(p.due(1_000 + DEBOUNCE_SECS, thrifty(false)));
        assert!(!p.due(1_000 + PROGRESS_SECS, thrifty(true)));
        assert!(p.due(1_000 + THRIFTY_SECS, thrifty(true)));
        let idle = Waiting {
            thrifty: true,
            ..Waiting::default()
        };
        assert!(!p.due(1_000 + INTERVAL_SECS, idle));
        assert!(p.due(1_000 + THRIFTY_SECS, idle));
    }

    #[test]
    fn a_device_is_thrifty_past_its_third_of_seventy_percent_of_either_quota() {
        let at = |reads, writes| Usage {
            day: 0,
            reads,
            writes,
        };
        assert!(!at(11_666, 4_666).is_thrifty());
        assert!(at(11_667, 0).is_thrifty());
        assert!(at(0, 4_667).is_thrifty());
    }

    #[test]
    fn the_count_starts_again_when_firestore_s_day_does() {
        let conn = aurora_db::open_memory().unwrap();
        // 20:00 UTC; the quota resets at 08:00 UTC tomorrow.
        let evening = 1_795_464_000;
        let reset = next_quota_reset(evening);
        let spent = Used {
            reads: 5,
            writes: 2,
        };
        Usage::add(&conn, evening, spent).unwrap();
        Usage::add(&conn, reset - 1, spent).unwrap();
        let before = Usage::today(&conn, reset - 1).unwrap();
        assert_eq!((before.reads, before.writes), (10, 4));
        let after = Usage::today(&conn, reset).unwrap();
        assert_eq!((after.reads, after.writes), (0, 0));
        assert_eq!(Usage::add(&conn, reset, spent).unwrap().reads, 5);
    }

    #[test]
    fn the_quota_reset_is_always_ahead() {
        assert_eq!(next_quota_reset(0), 8 * 3_600);
        assert_eq!(next_quota_reset(8 * 3_600), 8 * 3_600 + 86_400);
    }
}
