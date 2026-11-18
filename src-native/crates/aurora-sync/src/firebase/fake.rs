//! A Firebase that lives in memory, for tests.
//!
//! It answers the requests [`super::FirestoreTransport`] and the account functions make
//! -- `accounts:signUp`, `accounts:signInWithPassword`, the Secure Token refresh,
//! `documents:commit`, `:runQuery` and a document GET -- closely enough that the whole
//! transport, cursor paging and token refresh included, runs end to end with no project.
//!
//! It also enforces what `firebase/firestore.rules` enforces, so a request for another
//! account's documents is refused here as it would be there: a token may only touch
//! `users/{its own uid}/…`.
//!
//! What it does not try to be: an emulator. Timestamps are a counter formatted as
//! RFC 3339, so they sort as strings; only the query shapes the transport sends are
//! understood.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::{Body, Http, Method, Reply};
use crate::error::{Result, SyncError};

#[derive(Debug, Default)]
struct State {
    /// email -> (password, uid)
    accounts: HashMap<String, (String, String)>,
    /// id token -> uid
    tokens: HashMap<String, String>,
    /// refresh token -> uid
    refresh: HashMap<String, String>,
    /// full document name -> fields
    docs: BTreeMap<String, Value>,
    clock: u64,
    serial: u64,
    offline: bool,
    /// Answer every Firestore call with this, to simulate quota and outages.
    firestore_status: Option<(u16, &'static str)>,
    /// Requests seen, as `METHOD url`, for assertions about what was sent.
    log: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FakeFirebase {
    state: Arc<Mutex<State>>,
}

fn reply(status: u16, body: Value) -> Result<Reply> {
    Ok(Reply { status, body })
}

fn auth_error(message: &str) -> Result<Reply> {
    reply(400, json!({ "error": { "code": 400, "message": message } }))
}

fn fs_error(status: u16, code: &str) -> Result<Reply> {
    reply(
        status,
        json!({ "error": { "code": status, "status": code } }),
    )
}

impl FakeFirebase {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_offline(&self, offline: bool) {
        self.state.lock().unwrap().offline = offline;
    }

    /// Make every Firestore call fail with this status and code until cleared.
    pub fn fail_firestore(&self, failure: Option<(u16, &'static str)>) {
        self.state.lock().unwrap().firestore_status = failure;
    }

    /// Invalidate every ID token, as an hour passing would. Refresh tokens still work.
    pub fn expire_id_tokens(&self) {
        self.state.lock().unwrap().tokens.clear();
    }

    /// Revoke every refresh token too: the account was signed out everywhere.
    pub fn revoke_all(&self) {
        let mut s = self.state.lock().unwrap();
        s.tokens.clear();
        s.refresh.clear();
    }

    /// Full names of every stored document.
    pub fn document_names(&self) -> Vec<String> {
        self.state.lock().unwrap().docs.keys().cloned().collect()
    }

    /// Every stored document's fields.
    pub fn documents(&self) -> Vec<Value> {
        self.state.lock().unwrap().docs.values().cloned().collect()
    }

    pub fn requests(&self) -> Vec<String> {
        self.state.lock().unwrap().log.clone()
    }

    fn issue(s: &mut State, uid: &str) -> Value {
        s.serial += 1;
        // As long as the real ones: an ID token is a signed JWT of about a kilobyte and a
        // refresh token a few hundred characters. Short stand-ins hid that the whole
        // session is too long for Windows Credential Manager.
        let id = format!("id-{}-{}-{}", uid, s.serial, "j".repeat(1_000));
        let rt = format!("rt-{}-{}-{}", uid, s.serial, "r".repeat(250));
        s.tokens.insert(id.clone(), uid.to_string());
        s.refresh.insert(rt.clone(), uid.to_string());
        json!({ "id": id, "rt": rt })
    }

    fn accounts(s: &mut State, url: &str, body: &Value) -> Result<Reply> {
        let email = body["email"].as_str().unwrap_or_default().to_lowercase();
        let password = body["password"].as_str().unwrap_or_default().to_string();
        if !email.contains('@') {
            return auth_error("INVALID_EMAIL");
        }
        let uid = if url.contains(":signUp") {
            if password.len() < 6 {
                return auth_error("WEAK_PASSWORD : Password should be at least 6 characters");
            }
            if s.accounts.contains_key(&email) {
                return auth_error("EMAIL_EXISTS");
            }
            s.serial += 1;
            let uid = format!("uid{}", s.serial);
            s.accounts.insert(email, (password, uid.clone()));
            uid
        } else {
            match s.accounts.get(&email) {
                Some((pw, uid)) if *pw == password => uid.clone(),
                _ => return auth_error("INVALID_LOGIN_CREDENTIALS"),
            }
        };
        let t = Self::issue(s, &uid);
        reply(
            200,
            json!({ "localId": uid, "idToken": t["id"], "refreshToken": t["rt"], "expiresIn": "3600" }),
        )
    }

    fn token(s: &mut State, form: &[(String, String)]) -> Result<Reply> {
        let rt = form
            .iter()
            .find(|(k, _)| k == "refresh_token")
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        let Some(uid) = s.refresh.get(&rt).cloned() else {
            return auth_error("INVALID_REFRESH_TOKEN");
        };
        let t = Self::issue(s, &uid);
        reply(
            200,
            json!({ "user_id": uid, "id_token": t["id"], "refresh_token": t["rt"], "expires_in": "3600" }),
        )
    }

    /// The `users/{uid}` a document name or parent path is under, if any.
    fn owner(path: &str) -> Option<&str> {
        path.split("/documents/users/")
            .nth(1)?
            .split(['/', ':'])
            .next()
    }

    fn timestamp(clock: u64) -> String {
        format!("2026-11-17T00:00:00.{clock:09}Z")
    }

    fn commit(s: &mut State, uid: &str, body: &Value) -> Result<Reply> {
        let writes = body["writes"].as_array().cloned().unwrap_or_default();
        // Check every write before applying any: a commit is atomic.
        for w in &writes {
            let name = w["update"]["name"]
                .as_str()
                .or_else(|| w["delete"].as_str())
                .unwrap_or_default();
            if Self::owner(name) != Some(uid) {
                return fs_error(403, "PERMISSION_DENIED");
            }
            if w["currentDocument"]["exists"] == json!(false) && s.docs.contains_key(name) {
                return fs_error(400, "FAILED_PRECONDITION");
            }
        }
        s.clock += 1;
        let now = Self::timestamp(s.clock);
        for w in writes {
            if let Some(name) = w["delete"].as_str() {
                s.docs.remove(name);
                continue;
            }
            let name = w["update"]["name"].as_str().unwrap_or_default().to_string();
            let mut fields = w["update"]["fields"].clone();
            for t in w["updateTransforms"].as_array().into_iter().flatten() {
                if t["setToServerValue"] == "REQUEST_TIME" {
                    let path = t["fieldPath"].as_str().unwrap_or_default();
                    fields[path] = json!({ "timestampValue": now });
                }
            }
            s.docs.insert(name, fields);
        }
        reply(200, json!({ "commitTime": now }))
    }

    fn run_query(s: &State, parent: &str, body: &Value) -> Result<Reply> {
        let q = &body["structuredQuery"];
        let collection = q["from"][0]["collectionId"].as_str().unwrap_or_default();
        let prefix = format!("{parent}/{collection}/");
        let filter = &q["where"]["fieldFilter"];
        let after = q["startAt"]["values"].as_array().map(|v| {
            (
                v[0]["timestampValue"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                v[1]["referenceValue"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            )
        });
        let mut hits: Vec<(String, String, &Value)> = s
            .docs
            .iter()
            .filter(|(name, _)| {
                name.strip_prefix(&prefix)
                    .is_some_and(|rest| !rest.contains('/'))
            })
            .filter(|(_, f)| {
                filter.is_null()
                    || f[filter["field"]["fieldPath"].as_str().unwrap_or_default()]
                        == filter["value"]
            })
            .map(|(name, f)| {
                let t = f["serverTime"]["timestampValue"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                (t, name.clone(), f)
            })
            .filter(|(t, name, _)| match &after {
                Some((at, an)) => (t, name) > (at, an),
                None => true,
            })
            .collect();
        hits.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        if let Some(limit) = q["limit"].as_u64() {
            hits.truncate(limit as usize);
        }
        let read_time = Self::timestamp(s.clock);
        if hits.is_empty() {
            return reply(200, json!([{ "readTime": read_time }]));
        }
        reply(
            200,
            Value::Array(
                hits.into_iter()
                    .map(|(_, name, f)| {
                        json!({ "document": { "name": name, "fields": f }, "readTime": read_time })
                    })
                    .collect(),
            ),
        )
    }
}

impl Http for FakeFirebase {
    fn send(
        &self,
        method: Method,
        url: &str,
        bearer: Option<&str>,
        body: Option<&Body>,
    ) -> Result<Reply> {
        let mut s = self.state.lock().unwrap();
        s.log.push(format!("{method:?} {url}"));
        if s.offline {
            return Err(SyncError::Offline("simulated".into()));
        }
        let json_body = match body {
            Some(Body::Json(v)) => v.clone(),
            _ => Value::Null,
        };

        if url.contains("identitytoolkit.googleapis.com") {
            return Self::accounts(&mut s, url, &json_body);
        }
        if url.contains("securetoken.googleapis.com") {
            let form = match body {
                Some(Body::Form(f)) => f.clone(),
                _ => Vec::new(),
            };
            return Self::token(&mut s, &form);
        }

        // Firestore.
        let Some(uid) = bearer.and_then(|t| s.tokens.get(t)).cloned() else {
            return fs_error(401, "UNAUTHENTICATED");
        };
        if let Some((status, code)) = s.firestore_status {
            return fs_error(status, code);
        }
        let path = url
            .split("https://firestore.googleapis.com/v1/")
            .nth(1)
            .unwrap_or_default();
        if path.ends_with("/documents:commit") {
            return Self::commit(&mut s, &uid, &json_body);
        }
        if let Some(parent) = path.strip_suffix(":runQuery") {
            if Self::owner(parent) != Some(uid.as_str()) {
                return fs_error(403, "PERMISSION_DENIED");
            }
            return Self::run_query(&s, parent, &json_body);
        }
        if method == Method::Get {
            if Self::owner(path) != Some(uid.as_str()) {
                return fs_error(403, "PERMISSION_DENIED");
            }
            return match s.docs.get(path) {
                Some(f) => reply(200, json!({ "name": path, "fields": f })),
                None => fs_error(404, "NOT_FOUND"),
            };
        }
        fs_error(400, "INVALID_ARGUMENT")
    }
}
