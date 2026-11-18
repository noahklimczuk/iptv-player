//! Firebase over REST: accounts, and Cloud Firestore as a [`SyncTransport`]
//! (`docs/APPLE_PORT_AND_SYNC.md` §3.1, §3.2, §3.6).
//!
//! Plain HTTPS from Rust, the same on every platform, so Apple devices never depend on
//! the Firebase Apple SDK. Everything goes through the small [`Http`] trait: production
//! uses [`ReqwestHttp`], and the tests use [`fake::FakeFirebase`], which answers the same
//! requests from memory so the transport can be exercised end to end with no project.
//!
//! **Accounts** are Firebase Authentication email + password (Identity Toolkit REST),
//! one per household. A sign-in returns a one-hour ID token and a refresh token; the
//! transport refreshes the ID token itself (Secure Token API) shortly before it expires,
//! or once on a 401. The refresh token is what a device keeps, in the platform
//! credential store and never in SQLite (§3.2); [`FirestoreTransport::session`] hands back
//! the current one so the host can save it if it rotated.
//!
//! **Records** live at `users/{uid}/records/{docId}_{deviceId}` -- one copy per writer
//! (see `transport`) -- with four fields: `rid` (the codec's document id), `writer`,
//! `payload` (bytes) and `serverTime` (set by the server, `REQUEST_TIME`). A pull is a
//! `runQuery` ordered by `(serverTime, __name__)` starting after the cursor. Commit
//! timestamps only move forward relative to any read that has completed, so a write not
//! yet visible to one pull has a later `serverTime` than its cursor and is seen by the
//! next.
//!
//! **The vault** -- the wrapped key (§3.5) -- is `users/{uid}/meta/vault`, created with an
//! `exists: false` precondition so two devices setting up sync at once cannot each mint
//! a key: the second is told one exists and unwraps that.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{Result, SyncError};
use crate::transport::{Envelope, Page, SyncTransport};
use crate::vault::WrappedKey;

pub mod fake;

/// Refresh the ID token this long before Firebase says it expires.
const REFRESH_MARGIN_SECS: i64 = 300;
/// Firestore's per-commit write limit.
const COMMIT_LIMIT: usize = 500;

/// Which Firebase project. Both values are public by design: the API key identifies the
/// project, and the security rules and per-user auth are what protect the data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirebaseConfig {
    pub api_key: String,
    pub project_id: String,
}

/// A signed-in household account on this device.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub uid: String,
    pub id_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: i64,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("uid", &self.uid)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------------------
// HTTP

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

#[derive(Debug, Clone)]
pub enum Body {
    Json(Value),
    Form(Vec<(String, String)>),
}

#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    /// The body parsed as JSON, or `Null` when it was not JSON.
    pub body: Value,
}

pub trait Http: Send + Sync {
    /// Send one request. `Err` only for a request that never got an HTTP answer (no
    /// network, DNS, TLS, timeout); every status code is an `Ok(Reply)`.
    fn send(
        &self,
        method: Method,
        url: &str,
        bearer: Option<&str>,
        body: Option<&Body>,
    ) -> Result<Reply>;
}

/// The production [`Http`]: blocking reqwest with rustls. Cheap to clone: the client is
/// reference-counted.
#[derive(Clone)]
pub struct ReqwestHttp {
    client: reqwest::blocking::Client,
}

impl ReqwestHttp {
    pub fn new() -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .https_only(true)
            .build()
            .map_err(|e| SyncError::Offline(e.to_string()))?;
        Ok(Self { client })
    }
}

impl Http for ReqwestHttp {
    fn send(
        &self,
        method: Method,
        url: &str,
        bearer: Option<&str>,
        body: Option<&Body>,
    ) -> Result<Reply> {
        let mut req = match method {
            Method::Get => self.client.get(url),
            Method::Post => self.client.post(url),
        };
        if let Some(token) = bearer {
            req = req.bearer_auth(token);
        }
        req = match body {
            Some(Body::Json(v)) => req.json(v),
            Some(Body::Form(pairs)) => req.form(pairs),
            None => req,
        };
        // The URL carries the API key; keep it out of the error text.
        let resp = req
            .send()
            .map_err(|e| SyncError::Offline(e.without_url().to_string()))?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .map_err(|e| SyncError::Offline(e.without_url().to_string()))?;
        Ok(Reply {
            status,
            body: serde_json::from_str(&text).unwrap_or(Value::Null),
        })
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Firebase's error code from a reply body: `error.message` for Auth, `error.status`
/// for Firestore.
fn error_code(body: &Value) -> String {
    let e = &body["error"];
    e["status"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| e["message"].as_str())
        .unwrap_or_default()
        .to_string()
}

// ---------------------------------------------------------------------------------------
// Accounts

const IDENTITY: &str = "https://identitytoolkit.googleapis.com/v1/accounts";
const SECURE_TOKEN: &str = "https://securetoken.googleapis.com/v1/token";

/// What a person should be told when an account call is refused.
fn account_error(reply: &Reply) -> SyncError {
    let code = reply.body["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let head = code.split([' ', ':']).next().unwrap_or_default();
    let msg = match head {
        "EMAIL_EXISTS" => "There is already a sync account with that email. Sign in instead.",
        "INVALID_LOGIN_CREDENTIALS" | "INVALID_PASSWORD" | "EMAIL_NOT_FOUND" => {
            "That email and password do not match a sync account."
        }
        "INVALID_EMAIL" => "That is not a valid email address.",
        "WEAK_PASSWORD" => "Choose a password of at least six characters.",
        "TOO_MANY_ATTEMPTS_TRY_LATER" => "Too many attempts. Wait a few minutes and try again.",
        "USER_DISABLED" => "This sync account has been disabled.",
        "OPERATION_NOT_ALLOWED" => {
            "Email sign-in is not enabled for this Firebase project. Turn on the \
             Email/Password provider in the Firebase console."
        }
        "INVALID_REFRESH_TOKEN" | "TOKEN_EXPIRED" | "USER_NOT_FOUND" => {
            return SyncError::Unauthenticated
        }
        _ if reply.status >= 500 => return SyncError::Offline(format!("HTTP {}", reply.status)),
        _ => return SyncError::Protocol(format!("account call failed: {code}")),
    };
    SyncError::Account(msg.to_string())
}

fn parse_secs(v: &Value) -> i64 {
    v.as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| v.as_i64())
        .unwrap_or(3600)
}

fn email_password(
    http: &dyn Http,
    cfg: &FirebaseConfig,
    endpoint: &str,
    email: &str,
    password: &str,
) -> Result<Session> {
    let reply = http.send(
        Method::Post,
        &format!("{IDENTITY}:{endpoint}?key={}", cfg.api_key),
        None,
        Some(&Body::Json(json!({
            "email": email.trim(),
            "password": password,
            "returnSecureToken": true,
        }))),
    )?;
    if reply.status != 200 {
        return Err(account_error(&reply));
    }
    let b = &reply.body;
    let field = |k: &str| {
        b[k].as_str()
            .map(str::to_string)
            .ok_or_else(|| SyncError::Protocol(format!("sign-in reply has no {k}")))
    };
    Ok(Session {
        uid: field("localId")?,
        id_token: field("idToken")?,
        refresh_token: field("refreshToken")?,
        expires_at: now_secs() + parse_secs(&b["expiresIn"]),
    })
}

/// Create the household's sync account.
pub fn sign_up(
    http: &dyn Http,
    cfg: &FirebaseConfig,
    email: &str,
    password: &str,
) -> Result<Session> {
    email_password(http, cfg, "signUp", email, password)
}

/// Sign this device in to an existing account.
pub fn sign_in(
    http: &dyn Http,
    cfg: &FirebaseConfig,
    email: &str,
    password: &str,
) -> Result<Session> {
    email_password(http, cfg, "signInWithPassword", email, password)
}

/// Exchange a refresh token for a fresh ID token.
pub fn refresh(http: &dyn Http, cfg: &FirebaseConfig, refresh_token: &str) -> Result<Session> {
    let reply = http.send(
        Method::Post,
        &format!("{SECURE_TOKEN}?key={}", cfg.api_key),
        None,
        Some(&Body::Form(vec![
            ("grant_type".into(), "refresh_token".into()),
            ("refresh_token".into(), refresh_token.into()),
        ])),
    )?;
    if reply.status != 200 {
        return Err(account_error(&reply));
    }
    let b = &reply.body;
    let field = |k: &str| {
        b[k].as_str()
            .map(str::to_string)
            .ok_or_else(|| SyncError::Protocol(format!("token reply has no {k}")))
    };
    Ok(Session {
        uid: field("user_id")?,
        id_token: field("id_token")?,
        refresh_token: field("refresh_token")?,
        expires_at: now_secs() + parse_secs(&b["expires_in"]),
    })
}

// ---------------------------------------------------------------------------------------
// Firestore

/// A document id segment Firestore accepts. The vault codec's ids and device ids are
/// hex already; anything else is hex-encoded with an `x` prefix so the `_` between record
/// and writer stays unambiguous.
fn path_safe(s: &str) -> String {
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        s.to_string()
    } else {
        format!(
            "x{}",
            s.bytes().map(|b| format!("{b:02x}")).collect::<String>()
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Cursor {
    /// `serverTime` of the last document read, as Firestore wrote it.
    t: String,
    /// Its full resource name, the tie-break for documents committed together.
    n: String,
}

/// Documents read and written, as the Spark plan's daily quotas count them (§3.7): a
/// query is billed per document it returns and at least one read when it returns none,
/// a get is one read whether or not the document exists, and every `update` in a commit
/// is one write. Deletes have a quota of their own and are not counted here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Used {
    pub reads: u64,
    pub writes: u64,
}

pub struct FirestoreTransport<H: Http> {
    http: H,
    cfg: FirebaseConfig,
    session: Mutex<Session>,
    reads: AtomicU64,
    writes: AtomicU64,
}

impl<H: Http> FirestoreTransport<H> {
    pub fn new(http: H, cfg: FirebaseConfig, session: Session) -> Self {
        Self {
            http,
            cfg,
            session: Mutex::new(session),
            reads: AtomicU64::new(0),
            writes: AtomicU64::new(0),
        }
    }

    /// What this transport has spent of the day's quota so far.
    pub fn used(&self) -> Used {
        Used {
            reads: self.reads.load(Ordering::Relaxed),
            writes: self.writes.load(Ordering::Relaxed),
        }
    }

    fn spend(&self, reads: usize, writes: usize) {
        self.reads.fetch_add(reads as u64, Ordering::Relaxed);
        self.writes.fetch_add(writes as u64, Ordering::Relaxed);
    }

    /// The current session, for the host to save if the refresh token rotated.
    pub fn session(&self) -> Session {
        self.session.lock().unwrap().clone()
    }

    fn uid(&self) -> String {
        self.session.lock().unwrap().uid.clone()
    }

    fn docs_root(&self) -> String {
        format!(
            "projects/{}/databases/(default)/documents",
            self.cfg.project_id
        )
    }

    fn url(&self, path: &str) -> String {
        format!(
            "https://firestore.googleapis.com/v1/{}{path}",
            self.docs_root()
        )
    }

    fn user_path(&self) -> String {
        format!("/users/{}", self.uid())
    }

    fn token(&self, force: bool) -> Result<String> {
        let mut s = self.session.lock().unwrap();
        if force || s.expires_at - REFRESH_MARGIN_SECS <= now_secs() {
            *s = refresh(&self.http, &self.cfg, &s.refresh_token)?;
        }
        Ok(s.id_token.clone())
    }

    /// One Firestore call, refreshing the token and retrying once on a 401.
    fn call(&self, method: Method, url: &str, body: Option<Value>) -> Result<Reply> {
        let body = body.map(Body::Json);
        let mut reply = self
            .http
            .send(method, url, Some(&self.token(false)?), body.as_ref())?;
        if reply.status == 401 {
            reply = self
                .http
                .send(method, url, Some(&self.token(true)?), body.as_ref())?;
        }
        Ok(reply)
    }

    fn check(reply: Reply) -> Result<Value> {
        if reply.status == 200 {
            return Ok(reply.body);
        }
        let code = error_code(&reply.body);
        Err(match (reply.status, code.as_str()) {
            (_, "RESOURCE_EXHAUSTED") | (429, _) => SyncError::QuotaExhausted,
            (401, _) | (403, _) | (_, "UNAUTHENTICATED") | (_, "PERMISSION_DENIED") => {
                SyncError::Unauthenticated
            }
            (s, _) if s >= 500 => SyncError::Offline(format!("HTTP {s} {code}")),
            (s, _) => SyncError::Protocol(format!("Firestore answered {s} {code}")),
        })
    }

    fn commit(&self, writes: Vec<Value>) -> Result<()> {
        for chunk in writes.chunks(COMMIT_LIMIT) {
            let reply = self.call(
                Method::Post,
                &self.url(":commit"),
                Some(json!({ "writes": chunk })),
            )?;
            Self::check(reply)?;
            self.spend(
                0,
                chunk.iter().filter(|w| w.get("update").is_some()).count(),
            );
        }
        Ok(())
    }

    fn run_query(&self, query: Value) -> Result<Vec<Value>> {
        let reply = self.call(
            Method::Post,
            &self.url(&format!("{}:runQuery", self.user_path())),
            Some(json!({ "structuredQuery": query })),
        )?;
        let body = Self::check(reply)?;
        let docs: Vec<Value> = body
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|r| r.get("document").cloned())
                    .collect()
            })
            .unwrap_or_default();
        self.spend(docs.len().max(1), 0);
        Ok(docs)
    }

    fn envelope(doc: &Value) -> Option<Envelope> {
        let f = &doc["fields"];
        Some(Envelope {
            doc_id: f["rid"]["stringValue"].as_str()?.to_string(),
            writer: f["writer"]["stringValue"].as_str()?.to_string(),
            payload: B64.decode(f["payload"]["bytesValue"].as_str()?).ok()?,
        })
    }

    /// The wrapped vault key, if this household has set sync up.
    pub fn get_vault(&self) -> Result<Option<WrappedKey>> {
        let reply = self.call(
            Method::Get,
            &self.url(&format!("{}/meta/vault", self.user_path())),
            None,
        )?;
        if reply.status == 404 {
            self.spend(1, 0);
            return Ok(None);
        }
        let doc = Self::check(reply)?;
        self.spend(1, 0);
        let raw = doc["fields"]["wrapped"]["stringValue"]
            .as_str()
            .ok_or_else(|| SyncError::Protocol("vault document has no key".into()))?;
        Ok(Some(serde_json::from_str(raw)?))
    }

    /// Store the wrapped vault key, unless one exists. Returns false when another device
    /// got there first; read that one with [`Self::get_vault`] instead.
    pub fn create_vault(&self, wrapped: &WrappedKey) -> Result<bool> {
        let name = format!("{}{}/meta/vault", self.docs_root(), self.user_path());
        let reply = self.call(
            Method::Post,
            &self.url(":commit"),
            Some(json!({ "writes": [{
                "update": {
                    "name": name,
                    "fields": { "wrapped": { "stringValue": serde_json::to_string(wrapped)? } },
                },
                "currentDocument": { "exists": false },
            }]})),
        )?;
        match (reply.status, error_code(&reply.body).as_str()) {
            (200, _) => {
                self.spend(0, 1);
                Ok(true)
            }
            (_, "FAILED_PRECONDITION") | (_, "ALREADY_EXISTS") => Ok(false),
            _ => Self::check(reply).map(|_| false),
        }
    }

    /// Delete every record on the server, keeping the vault. What **Reset sync** does
    /// before re-uploading from the device the viewer chose (§3.5).
    pub fn delete_records(&self) -> Result<usize> {
        let mut deleted = 0;
        loop {
            let docs = self.run_query(json!({
                "from": [{ "collectionId": "records" }],
                "select": { "fields": [{ "fieldPath": "__name__" }] },
                "limit": COMMIT_LIMIT,
            }))?;
            if docs.is_empty() {
                return Ok(deleted);
            }
            let writes: Vec<Value> = docs
                .iter()
                .filter_map(|d| d["name"].as_str())
                .map(|n| json!({ "delete": n }))
                .collect();
            deleted += writes.len();
            self.commit(writes)?;
        }
    }
}

impl<H: Http> SyncTransport for FirestoreTransport<H> {
    fn push(&self, batch: &[Envelope]) -> Result<()> {
        let root = format!("{}{}/records", self.docs_root(), self.user_path());
        let writes = batch
            .iter()
            .map(|e| {
                json!({
                    "update": {
                        "name": format!("{root}/{}_{}", path_safe(&e.doc_id), path_safe(&e.writer)),
                        "fields": {
                            "rid":     { "stringValue": e.doc_id },
                            "writer":  { "stringValue": e.writer },
                            "payload": { "bytesValue": B64.encode(&e.payload) },
                        },
                    },
                    "updateTransforms": [
                        { "fieldPath": "serverTime", "setToServerValue": "REQUEST_TIME" }
                    ],
                })
            })
            .collect();
        self.commit(writes)
    }

    fn pull(&self, cursor: Option<&str>, limit: usize) -> Result<Page> {
        let mut query = json!({
            "from": [{ "collectionId": "records" }],
            "orderBy": [
                { "field": { "fieldPath": "serverTime" }, "direction": "ASCENDING" },
                { "field": { "fieldPath": "__name__" },   "direction": "ASCENDING" },
            ],
            // One more than asked, to know whether there is another page.
            "limit": limit + 1,
        });
        if let Some(c) = cursor.and_then(|c| serde_json::from_str::<Cursor>(c).ok()) {
            query["startAt"] = json!({
                "values": [ { "timestampValue": c.t }, { "referenceValue": c.n } ],
                "before": false,
            });
        }
        let mut docs = self.run_query(query)?;
        let more = docs.len() > limit;
        docs.truncate(limit);
        let next = match docs.last() {
            Some(d) => Some(serde_json::to_string(&Cursor {
                t: d["fields"]["serverTime"]["timestampValue"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                n: d["name"].as_str().unwrap_or_default().to_string(),
            })?),
            None => cursor.map(str::to_string),
        };
        Ok(Page {
            records: docs.iter().filter_map(Self::envelope).collect(),
            cursor: next,
            more,
        })
    }

    fn get(&self, doc_id: &str) -> Result<Vec<Envelope>> {
        let docs = self.run_query(json!({
            "from": [{ "collectionId": "records" }],
            "where": { "fieldFilter": {
                "field": { "fieldPath": "rid" },
                "op": "EQUAL",
                "value": { "stringValue": doc_id },
            }},
        }))?;
        Ok(docs.iter().filter_map(Self::envelope).collect())
    }
}
