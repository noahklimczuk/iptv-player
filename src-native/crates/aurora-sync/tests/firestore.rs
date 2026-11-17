//! The Firestore transport and Firebase accounts, end to end against the in-memory fake.

use aurora_db::repo::{lists, sync as db};
use aurora_db::rusqlite::Connection;
use aurora_sync::firebase::fake::FakeFirebase;
use aurora_sync::firebase::{self, FirebaseConfig, FirestoreTransport, Session};
use aurora_sync::transport::{Envelope, SyncTransport};
use aurora_sync::vault::{KdfParams, VaultCodec, VaultKey, WrappedKey};
use aurora_sync::{Engine, SyncError};

const CHEAP: KdfParams = KdfParams { m: 64, t: 1, p: 1 };

fn cfg() -> FirebaseConfig {
    FirebaseConfig {
        api_key: "test-key".into(),
        project_id: "aurora-test".into(),
    }
}

fn account(fb: &FakeFirebase, email: &str) -> Session {
    match firebase::sign_up(fb, &cfg(), email, "hunter22") {
        Ok(s) => s,
        Err(_) => firebase::sign_in(fb, &cfg(), email, "hunter22").unwrap(),
    }
}

fn transport(fb: &FakeFirebase, session: Session) -> FirestoreTransport<FakeFirebase> {
    FirestoreTransport::new(fb.clone(), cfg(), session)
}

fn env(id: &str, writer: &str) -> Envelope {
    Envelope {
        doc_id: id.into(),
        payload: format!("bytes of {id}").into_bytes(),
        writer: writer.into(),
    }
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

#[test]
fn accounts_sign_up_sign_in_and_say_what_went_wrong() {
    let fb = FakeFirebase::new();
    let s = firebase::sign_up(&fb, &cfg(), "home@example.com", "hunter22").unwrap();
    let again = firebase::sign_in(&fb, &cfg(), "home@example.com", "hunter22").unwrap();
    assert_eq!(s.uid, again.uid);

    let wrong = firebase::sign_in(&fb, &cfg(), "home@example.com", "nope").unwrap_err();
    assert!(wrong.to_string().contains("do not match"));
    let taken = firebase::sign_up(&fb, &cfg(), "home@example.com", "hunter22").unwrap_err();
    assert!(taken.to_string().contains("already a sync account"));
    let weak = firebase::sign_up(&fb, &cfg(), "new@example.com", "123").unwrap_err();
    assert!(weak.to_string().contains("six characters"));
    // The session's Debug never prints tokens.
    assert!(!format!("{s:?}").contains(&s.id_token));
}

#[test]
fn records_are_stored_one_copy_per_writer_with_a_server_time() {
    let fb = FakeFirebase::new();
    let t = transport(&fb, account(&fb, "home@example.com"));
    t.push(&[env("abc", "pc"), env("abc", "tv")]).unwrap();
    t.push(&[env("abc", "pc")]).unwrap();

    let names = fb.document_names();
    assert_eq!(names.len(), 2);
    let uid = t.session().uid;
    for n in &names {
        assert!(
            n.contains(&format!("/documents/users/{uid}/records/abc_")),
            "{n}"
        );
    }
    for doc in fb.documents() {
        assert!(doc["serverTime"]["timestampValue"].is_string());
    }
    assert_eq!(t.get("abc").unwrap().len(), 2);
}

#[test]
fn pulls_page_through_everything_and_then_only_what_is_new() {
    let fb = FakeFirebase::new();
    let t = transport(&fb, account(&fb, "home@example.com"));
    for i in 0..5 {
        t.push(&[env(&format!("r{i}"), "pc")]).unwrap();
    }
    let mut seen = Vec::new();
    let mut cursor = None;
    loop {
        let page = t.pull(cursor.as_deref(), 2).unwrap();
        seen.extend(page.records.into_iter().map(|e| e.doc_id));
        cursor = page.cursor;
        if !page.more {
            break;
        }
    }
    assert_eq!(seen, ["r0", "r1", "r2", "r3", "r4"]);

    t.push(&[env("r9", "tv")]).unwrap();
    let page = t.pull(cursor.as_deref(), 10).unwrap();
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].doc_id, "r9");
}

#[test]
fn records_written_in_one_commit_are_not_skipped_across_a_page_boundary() {
    // One commit gives all three the same serverTime; the document name breaks the tie.
    let fb = FakeFirebase::new();
    let t = transport(&fb, account(&fb, "home@example.com"));
    t.push(&[env("a", "pc"), env("b", "pc"), env("c", "pc")])
        .unwrap();
    let p1 = t.pull(None, 2).unwrap();
    let p2 = t.pull(p1.cursor.as_deref(), 2).unwrap();
    assert_eq!(p1.records.len() + p2.records.len(), 3);
}

#[test]
fn an_expired_token_is_refreshed_without_bothering_anyone() {
    let fb = FakeFirebase::new();
    let session = account(&fb, "home@example.com");
    let t = transport(&fb, session.clone());
    fb.expire_id_tokens();
    t.push(&[env("a", "pc")]).unwrap();
    assert_ne!(t.session().id_token, session.id_token);
    assert!(fb.requests().iter().any(|r| r.contains("securetoken")));

    // A session already past its expiry refreshes before the call, not after a 401.
    let mut stale = t.session();
    stale.expires_at = 0;
    let t2 = transport(&fb, stale);
    t2.pull(None, 10).unwrap();
}

#[test]
fn a_revoked_account_asks_to_sign_in_again() {
    let fb = FakeFirebase::new();
    let t = transport(&fb, account(&fb, "home@example.com"));
    fb.revoke_all();
    assert!(matches!(
        t.push(&[env("a", "pc")]),
        Err(SyncError::Unauthenticated)
    ));
}

#[test]
fn a_spent_quota_and_an_outage_are_reported_as_such() {
    let fb = FakeFirebase::new();
    let t = transport(&fb, account(&fb, "home@example.com"));
    fb.fail_firestore(Some((429, "RESOURCE_EXHAUSTED")));
    assert!(matches!(t.pull(None, 1), Err(SyncError::QuotaExhausted)));
    fb.fail_firestore(Some((503, "UNAVAILABLE")));
    assert!(matches!(t.pull(None, 1), Err(SyncError::Offline(_))));
    fb.fail_firestore(None);
    fb.set_offline(true);
    assert!(matches!(t.pull(None, 1), Err(SyncError::Offline(_))));
}

#[test]
fn one_household_cannot_read_anothers_records() {
    let fb = FakeFirebase::new();
    let ours = transport(&fb, account(&fb, "home@example.com"));
    let theirs = transport(&fb, account(&fb, "neighbour@example.com"));
    ours.push(&[env("a", "pc")]).unwrap();
    assert!(theirs.pull(None, 10).unwrap().records.is_empty());
}

#[test]
fn only_one_device_can_create_the_vault_and_the_other_uses_it() {
    let fb = FakeFirebase::new();
    let a = transport(&fb, account(&fb, "home@example.com"));
    let b = transport(&fb, account(&fb, "home@example.com"));
    assert_eq!(a.get_vault().unwrap(), None);

    let key_a = VaultKey::generate().unwrap();
    let key_b = VaultKey::generate().unwrap();
    let wrap = |k: &VaultKey| WrappedKey::wrap_with(k, "pass phrase", CHEAP).unwrap();
    assert!(a.create_vault(&wrap(&key_a)).unwrap());
    assert!(!b.create_vault(&wrap(&key_b)).unwrap());

    let stored = b.get_vault().unwrap().unwrap();
    assert_eq!(stored.unwrap_key("pass phrase").unwrap(), key_a);
}

#[test]
fn reset_deletes_every_record_and_keeps_the_vault() {
    let fb = FakeFirebase::new();
    let t = transport(&fb, account(&fb, "home@example.com"));
    let key = VaultKey::generate().unwrap();
    t.create_vault(&WrappedKey::wrap_with(&key, "p", CHEAP).unwrap())
        .unwrap();
    t.push(&[env("a", "pc"), env("b", "tv")]).unwrap();
    assert_eq!(t.delete_records().unwrap(), 2);
    assert!(t.pull(None, 10).unwrap().records.is_empty());
    assert!(t.get_vault().unwrap().is_some());
}

#[test]
fn two_devices_sync_a_favourite_through_encrypted_firestore() {
    let fb = FakeFirebase::new();
    let key = VaultKey::generate().unwrap();
    let mut pc = library(0);
    let mut tv = library(100);
    let e_pc = Engine::new(
        transport(&fb, account(&fb, "home@example.com")),
        VaultCodec::new(&key),
        "a1b2c3",
    );
    let e_tv = Engine::new(
        transport(&fb, account(&fb, "home@example.com")),
        VaultCodec::new(&key),
        "d4e5f6",
    );
    e_pc.join(&mut pc).unwrap();
    e_tv.join(&mut tv).unwrap();

    lists::toggle_favorite(&pc, 1, 10, None, 1).unwrap();
    e_pc.sync(&mut pc).unwrap();
    e_tv.sync(&mut tv).unwrap();
    let n: i64 = tv
        .query_row(
            "SELECT COUNT(*) FROM favorites WHERE item_id = 110",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(db::outbox_len(&tv).unwrap(), 0);

    // Nothing readable reached the server.
    for doc in fb.documents() {
        let text = doc.to_string();
        for leak in ["BBC", "501", "Favorites", "panel.example", "viewer"] {
            assert!(!text.contains(leak), "{leak} reached the server: {text}");
        }
    }
}
