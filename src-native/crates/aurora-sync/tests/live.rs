//! Sync against the real Firebase project, end to end. Never run by CI.
//!
//! ```text
//! set -a; . /c/aurora-dev/panel.env; set +a
//! cargo test -p aurora-sync --test live -- --ignored --nocapture
//! ```
//!
//! Everything it makes is thrown away: in-memory libraries, a household account with a
//! random `@example.com` address that is deleted at the end with its documents, and a
//! second account used only to check that the published security rules keep it out. It
//! touches no profile and no credential store.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use aurora_db::repo::lists;
use aurora_db::rusqlite::Connection;
use aurora_sync::firebase::{
    self, Body, FirebaseConfig, FirestoreTransport, Http, Method, ReqwestHttp, Session,
};
use aurora_sync::household::{Account, Household, Secrets, Unlock, SESSION_SECRET};
use aurora_sync::transport::SyncTransport;
use aurora_sync::Result;
use serde_json::json;

#[derive(Default)]
struct MemorySecrets(Mutex<HashMap<String, String>>);

impl Secrets for MemorySecrets {
    fn get(&self, name: &str) -> Option<String> {
        self.0.lock().unwrap().get(name).cloned()
    }
    fn set(&self, name: &str, value: &str) -> Result<()> {
        self.0.lock().unwrap().insert(name.into(), value.into());
        Ok(())
    }
    fn delete(&self, name: &str) {
        self.0.lock().unwrap().remove(name);
    }
    fn is_persistent(&self) -> bool {
        false
    }
}

fn config() -> Option<FirebaseConfig> {
    Some(FirebaseConfig {
        api_key: std::env::var("AURORA_FIREBASE_API_KEY").ok()?,
        project_id: std::env::var("AURORA_FIREBASE_PROJECT_ID").ok()?,
    })
}

fn library(offset: i64) -> Connection {
    let conn = aurora_db::open_memory().unwrap();
    conn.execute_batch(&format!(
        "INSERT INTO providers (id,name,kind,base_url,username,created_at)
              VALUES ({p},'Line','xtream','http://panel.example','viewer',0);
         INSERT INTO channels (id,provider_id,provider_key,name,match_key,last_seen_at)
              VALUES ({c},{p},'501','BBC One','bbcone',0);",
        p = 1 + offset,
        c = 10 + offset
    ))
    .unwrap();
    conn
}

fn random_hex() -> String {
    let mut b = [0u8; 6];
    getrandom::fill(&mut b).unwrap();
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Delete an account's documents and then the account itself.
fn destroy(http: &ReqwestHttp, cfg: &FirebaseConfig, session: Session) {
    let uid = session.uid.clone();
    let id_token = session.id_token.clone();
    let t = FirestoreTransport::new(http.clone(), cfg.clone(), session);
    let records = t.delete_records().unwrap_or(0);
    let vault = format!(
        "projects/{}/databases/(default)/documents/users/{uid}/meta/vault",
        cfg.project_id
    );
    let _ = http.send(
        Method::Post,
        &format!(
            "https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents:commit",
            cfg.project_id
        ),
        Some(&id_token),
        Some(&Body::Json(json!({ "writes": [{ "delete": vault }] }))),
    );
    let gone = http.send(
        Method::Post,
        &format!(
            "https://identitytoolkit.googleapis.com/v1/accounts:delete?key={}",
            cfg.api_key
        ),
        None,
        Some(&Body::Json(json!({ "idToken": id_token }))),
    );
    println!(
        "cleaned up {uid}: {records} records, account deleted: {}",
        gone.map(|r| r.status == 200).unwrap_or(false)
    );
}

#[test]
#[ignore = "talks to the real Firebase project; run by hand with panel.env"]
fn two_devices_share_a_favourite_through_the_real_project() {
    let Some(cfg) = config() else {
        panic!("set AURORA_FIREBASE_API_KEY and AURORA_FIREBASE_PROJECT_ID (panel.env)");
    };
    let http = ReqwestHttp::new().unwrap();
    let email = format!("aurora-live-{}@example.com", random_hex());
    let password = format!("pw-{}", random_hex());
    let passphrase = "live test passphrase";
    let account = |name: &'static str| Account {
        email: Box::leak(email.clone().into_boxed_str()),
        password: Box::leak(password.clone().into_boxed_str()),
        device_name: name,
    };

    let pc_secrets = Arc::new(MemorySecrets::default());
    let pc = Household::new(http.clone(), Some(cfg.clone()), pc_secrets.clone());
    let tv = Household::new(
        http.clone(),
        Some(cfg.clone()),
        Arc::new(MemorySecrets::default()),
    );
    let mut pc_db = library(0);
    let mut tv_db = library(100);

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let made = pc
            .create_account(&mut pc_db, &account("PC"), passphrase)
            .expect("create the account");
        assert!(made.recovery_code.is_some());
        assert!(made.problem.is_none(), "{:?}", made.problem);
        println!("created; first push sent {:?}", made.report);

        tv.sign_in(
            &mut tv_db,
            &account("TV"),
            Unlock::Passphrase(passphrase.into()),
        )
        .expect("join from the second device");

        lists::toggle_favorite(&pc_db, 1, 10, None, 1).unwrap();
        let sent = pc.sync(&mut pc_db).expect("push the favourite");
        let got = tv.sync(&mut tv_db).expect("pull the favourite");
        println!("pc sent {sent:?}; tv got {got:?}");
        let n: i64 = tv_db
            .query_row(
                "SELECT COUNT(*) FROM favorites WHERE item_id = 110",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "the favourite reached the second device");

        // What the server holds is ciphertext.
        let session: Session =
            serde_json::from_str(&pc_secrets.get(SESSION_SECRET).unwrap()).unwrap();
        let t = FirestoreTransport::new(http.clone(), cfg.clone(), session.clone());
        let page = t.pull(None, 500).unwrap();
        assert!(!page.records.is_empty());
        for env in &page.records {
            let text = String::from_utf8_lossy(&env.payload);
            for leak in ["BBC", "panel.example", "viewer", "Favorites"] {
                assert!(!text.contains(leak), "{leak} reached the server");
            }
        }

        // The published rules keep another account out of this household.
        let stranger = firebase::sign_up(
            &http,
            &cfg,
            &format!("aurora-live-{}@example.com", random_hex()),
            &password,
        )
        .unwrap();
        let theirs = format!(
            "https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents/users/{}/meta/vault",
            cfg.project_id, session.uid
        );
        let refused = http
            .send(Method::Get, &theirs, Some(&stranger.id_token), None)
            .unwrap();
        println!(
            "another account reading this household's vault: {}",
            refused.status
        );
        destroy(&http, &cfg, stranger);
        assert_eq!(refused.status, 403, "the rules let another account in");
    }));

    if let Some(s) = pc_secrets.get(SESSION_SECRET) {
        let session: Session = serde_json::from_str(&s).unwrap();
        destroy(&http, &cfg, session);
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
