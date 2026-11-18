//! The account flow a host drives -- create, sign in on a second device, sync, recovery
//! code, reset, sign out -- end to end against the in-memory Firebase.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use aurora_db::repo::{lists, progress, sync as db};
use aurora_db::rusqlite::Connection;
use aurora_sync::firebase::fake::FakeFirebase;
use aurora_sync::firebase::FirebaseConfig;
use aurora_sync::household::{
    Account, Household, Problem, Secrets, Unlock, SESSION_SECRET, VAULT_SECRET,
};
use aurora_sync::vault::KdfParams;
use aurora_sync::{Result, SyncError};

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

struct Device {
    conn: Connection,
    secrets: Arc<MemorySecrets>,
    household: Household<FakeFirebase>,
}

fn device(fb: &FakeFirebase, offset: i64) -> Device {
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
    let secrets = Arc::new(MemorySecrets::default());
    let household = Household::new(
        fb.clone(),
        Some(FirebaseConfig {
            api_key: "k".into(),
            project_id: "aurora-test".into(),
        }),
        secrets.clone(),
    )
    .with_kdf(KdfParams { m: 64, t: 1, p: 1 });
    Device {
        conn,
        secrets,
        household,
    }
}

const PASS: &str = "correct horse";

fn account<'a>(name: &'a str) -> Account<'a> {
    Account {
        email: "home@example.com",
        password: "hunter22",
        device_name: name,
    }
}

fn favourites(conn: &Connection, item: i64) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM favorites WHERE item_id = ?1",
        [item],
        |r| r.get(0),
    )
    .unwrap()
}

fn add_film(conn: &Connection, offset: i64) -> i64 {
    let id = 20 + offset;
    conn.execute(
        "INSERT INTO movies (id,provider_id,provider_key,title,match_key,url,last_seen_at)
              VALUES (?1,?2,'9001','Heat','heat','http://x/heat.mkv',0)",
        [id, 1 + offset],
    )
    .unwrap();
    id
}

fn position(conn: &Connection, film: i64) -> Option<i64> {
    progress::get(conn, 1, progress::ItemKind::Movie, film)
        .unwrap()
        .map(|p| p.position_secs)
}

#[test]
fn opening_a_film_fetches_where_another_device_stopped_without_a_round() {
    let fb = FakeFirebase::new();
    let mut pc = device(&fb, 0);
    let mut tv = device(&fb, 100);
    let pc_film = add_film(&pc.conn, 0);
    let tv_film = add_film(&tv.conn, 100);
    pc.household
        .create_account(&mut pc.conn, &account("PC"), PASS)
        .unwrap();
    tv.household
        .sign_in(
            &mut tv.conn,
            &account("TV"),
            Unlock::Passphrase(PASS.into()),
        )
        .unwrap();

    // Watched to 40 minutes on the TV, which pushed it.
    progress::save(
        &tv.conn,
        1,
        progress::ItemKind::Movie,
        tv_film,
        2_400,
        6_000,
        1,
    )
    .unwrap();
    tv.household.sync(&mut tv.conn).unwrap();
    assert_eq!(
        position(&pc.conn, pc_film),
        None,
        "no round has run on the PC"
    );

    let before = fb.requests().len();
    assert!(pc
        .household
        .refresh_progress(&mut pc.conn, 1, "movie", pc_film)
        .unwrap());
    assert_eq!(position(&pc.conn, pc_film), Some(2_400));
    assert_eq!(
        fb.requests().len() - before,
        1,
        "one query for one record, not a pull: {:?}",
        &fb.requests()[before..]
    );

    // Nothing newer: asking again changes nothing, and a film with no key asks nothing.
    let before = fb.requests().len();
    assert!(!pc
        .household
        .refresh_progress(&mut pc.conn, 1, "movie", 999)
        .unwrap());
    assert_eq!(fb.requests().len(), before);
}

#[test]
fn every_document_read_and_written_is_counted_and_a_sign_out_does_not_reset_it() {
    let fb = FakeFirebase::new();
    let mut pc = device(&fb, 0);
    pc.household
        .create_account(&mut pc.conn, &account("PC"), PASS)
        .unwrap();
    let joined = pc.household.status(&pc.conn).unwrap().usage;
    // The vault, then this device's library: one profile, one provider, its settings.
    assert!(joined.writes >= 3, "{joined:?}");
    // The vault lookup, and the first pull, which finds nothing and is still a read.
    assert!(joined.reads >= 2, "{joined:?}");

    // Setting up reads back its own library on the next round, or on its own first pull,
    // depending on which side of the cursor the server's clock put them. Drained here so
    // what follows counts one favourite and nothing else.
    pc.household.sync(&mut pc.conn).unwrap();
    let joined = pc.household.status(&pc.conn).unwrap().usage;

    lists::toggle_favorite(&pc.conn, 1, 10, None, 1).unwrap();
    pc.household.sync(&mut pc.conn).unwrap();
    let after = pc.household.status(&pc.conn).unwrap();
    assert_eq!(after.usage.writes, joined.writes + 1, "one favourite");
    assert_eq!(
        after.usage.reads,
        joined.reads + 1,
        "one query, which returns its own echo"
    );
    assert!(!after.thrifty);

    pc.household.sign_out(&mut pc.conn).unwrap();
    assert_eq!(pc.household.status(&pc.conn).unwrap().usage, after.usage);
}

#[test]
fn a_device_that_never_signed_in_asks_the_server_nothing_on_opening_a_film() {
    let fb = FakeFirebase::new();
    let mut pc = device(&fb, 0);
    let film = add_film(&pc.conn, 0);
    assert!(!pc
        .household
        .refresh_progress(&mut pc.conn, 1, "movie", film)
        .unwrap());
    assert!(fb.requests().is_empty());
}

#[test]
fn a_build_without_a_project_offers_nothing_and_says_why() {
    let h = Household::new(
        FakeFirebase::new(),
        None,
        Arc::new(MemorySecrets::default()),
    );
    let mut conn = aurora_db::open_memory().unwrap();
    assert!(!h.status(&conn).unwrap().available);
    let err = h
        .create_account(&mut conn, &account("PC"), PASS)
        .unwrap_err();
    assert!(err.to_string().contains("not available"), "{err}");
}

#[test]
fn create_on_one_device_join_on_another_and_a_favourite_crosses() {
    let fb = FakeFirebase::new();
    let mut pc = device(&fb, 0);
    let mut tv = device(&fb, 100);

    let made = pc
        .household
        .create_account(&mut pc.conn, &account("Living room PC"), PASS)
        .unwrap();
    let code = made
        .recovery_code
        .expect("the creator is shown a recovery code");
    assert_eq!(code.split('-').count(), 11, "{code}");
    assert!(code.split('-').all(|g| g.len() == 5), "{code}");
    assert!(made.problem.is_none());

    let status = pc.household.status(&pc.conn).unwrap();
    assert!(status.signed_in && status.joined);
    assert_eq!(status.email.as_deref(), Some("home@example.com"));
    assert_eq!(status.device_name.as_deref(), Some("Living room PC"));
    assert!(status.last_sync_at.is_some());

    let joined = tv
        .household
        .sign_in(
            &mut tv.conn,
            &account("TV"),
            Unlock::Passphrase(PASS.into()),
        )
        .unwrap();
    assert_eq!(
        joined.recovery_code, None,
        "only the creator sees it unasked"
    );

    lists::toggle_favorite(&pc.conn, 1, 10, None, 1).unwrap();
    assert_eq!(pc.household.status(&pc.conn).unwrap().pending, 1);
    pc.household.sync(&mut pc.conn).unwrap();
    tv.household.sync(&mut tv.conn).unwrap();
    assert_eq!(favourites(&tv.conn, 110), 1);

    // The secrets are in the credential store, not in the database.
    assert!(pc.secrets.get(SESSION_SECRET).is_some());
    assert!(pc.secrets.get(VAULT_SECRET).is_some());
    let in_db: String = pc
        .conn
        .query_row("SELECT group_concat(value, ' ') FROM sync_state", [], |r| {
            r.get(0)
        })
        .unwrap();
    let session = pc.secrets.get(SESSION_SECRET).unwrap();
    let refresh: serde_json::Value = serde_json::from_str(&session).unwrap();
    assert!(!in_db.contains(refresh["refresh_token"].as_str().unwrap()));
    assert!(!in_db.contains(&pc.secrets.get(VAULT_SECRET).unwrap()));
}

#[test]
fn a_wrong_passphrase_is_called_that_and_signs_nothing_in() {
    let fb = FakeFirebase::new();
    let mut pc = device(&fb, 0);
    let mut tv = device(&fb, 100);
    pc.household
        .create_account(&mut pc.conn, &account("PC"), PASS)
        .unwrap();
    let err = tv
        .household
        .sign_in(
            &mut tv.conn,
            &account("TV"),
            Unlock::Passphrase("nope nope".into()),
        )
        .unwrap_err();
    assert!(matches!(err, SyncError::Account(_)));
    assert!(
        err.to_string().contains("passphrase does not match"),
        "{err}"
    );
    assert!(!tv.household.status(&tv.conn).unwrap().signed_in);
}

#[test]
fn a_short_passphrase_is_refused_before_any_account_is_made() {
    let fb = FakeFirebase::new();
    let mut pc = device(&fb, 0);
    let err = pc
        .household
        .create_account(&mut pc.conn, &account("PC"), "short")
        .unwrap_err();
    assert!(err.to_string().contains("at least 8"), "{err}");
    assert!(fb.requests().is_empty());
}

#[test]
fn the_recovery_code_opens_the_vault_and_a_wrong_one_is_caught() {
    let fb = FakeFirebase::new();
    let mut pc = device(&fb, 0);
    let code = pc
        .household
        .create_account(&mut pc.conn, &account("PC"), PASS)
        .unwrap()
        .recovery_code
        .unwrap();
    // Shown again only to someone with the passphrase.
    assert_eq!(pc.household.recovery_code(&pc.conn, PASS).unwrap(), code);
    assert!(pc.household.recovery_code(&pc.conn, "wrong pass").is_err());

    // A valid code for some other vault: well formed, so only a record can tell.
    let other = aurora_sync::vault::VaultKey::generate()
        .unwrap()
        .recovery_code();
    let mut tv = device(&fb, 100);
    let err = tv
        .household
        .sign_in(&mut tv.conn, &account("TV"), Unlock::RecoveryCode(other))
        .unwrap_err();
    assert!(err.to_string().contains("does not unlock"), "{err}");

    tv.household
        .sign_in(&mut tv.conn, &account("TV"), Unlock::RecoveryCode(code))
        .unwrap();
    assert!(tv.household.status(&tv.conn).unwrap().signed_in);
}

#[test]
fn an_account_whose_setup_was_cut_short_is_finished_by_the_next_sign_in() {
    let fb = FakeFirebase::new();
    let cfg = FirebaseConfig {
        api_key: "k".into(),
        project_id: "aurora-test".into(),
    };
    // The account exists; the device that made it never stored a vault.
    aurora_sync::firebase::sign_up(&fb, &cfg, "home@example.com", "hunter22").unwrap();
    let mut pc = device(&fb, 0);
    let joined = pc
        .household
        .sign_in(
            &mut pc.conn,
            &account("PC"),
            Unlock::Passphrase(PASS.into()),
        )
        .unwrap();
    assert!(joined.recovery_code.is_some());
}

#[test]
fn going_offline_is_reported_and_the_next_round_catches_up() {
    let fb = FakeFirebase::new();
    let mut pc = device(&fb, 0);
    pc.household
        .create_account(&mut pc.conn, &account("PC"), PASS)
        .unwrap();
    let mut tv = device(&fb, 100);
    tv.household
        .sign_in(
            &mut tv.conn,
            &account("TV"),
            Unlock::Passphrase(PASS.into()),
        )
        .unwrap();

    fb.set_offline(true);
    let err = tv.household.sync(&mut tv.conn).unwrap_err();
    assert!(matches!(err, SyncError::Offline(_)));
    assert!(matches!(
        tv.household.status(&tv.conn).unwrap().problem,
        Some(Problem::Offline { .. })
    ));
    fb.set_offline(false);
    tv.household.sync(&mut tv.conn).unwrap();
    assert_eq!(tv.household.status(&tv.conn).unwrap().problem, None);
}

#[test]
fn a_revoked_session_asks_to_sign_in_again_and_signing_in_keeps_everything() {
    let fb = FakeFirebase::new();
    let mut pc = device(&fb, 0);
    pc.household
        .create_account(&mut pc.conn, &account("PC"), PASS)
        .unwrap();
    let device_id = pc.household.status(&pc.conn).unwrap().device_id;
    fb.revoke_all();
    lists::toggle_favorite(&pc.conn, 1, 10, None, 1).unwrap();
    assert!(matches!(
        pc.household.sync(&mut pc.conn),
        Err(SyncError::Unauthenticated)
    ));
    assert_eq!(
        pc.household.status(&pc.conn).unwrap().problem,
        Some(Problem::SignInAgain)
    );

    pc.household
        .sign_in(
            &mut pc.conn,
            &account("PC"),
            Unlock::Passphrase(PASS.into()),
        )
        .unwrap();
    let status = pc.household.status(&pc.conn).unwrap();
    assert_eq!(status.device_id, device_id);
    assert_eq!(
        status.pending, 0,
        "the waiting favourite went with the sign-in"
    );
    assert_eq!(status.problem, None);
}

#[test]
fn another_account_needs_a_sign_out_first() {
    let fb = FakeFirebase::new();
    let mut pc = device(&fb, 0);
    pc.household
        .create_account(&mut pc.conn, &account("PC"), PASS)
        .unwrap();
    let other = Account {
        email: "neighbour@example.com",
        password: "hunter22",
        device_name: "PC",
    };
    let err = pc
        .household
        .create_account(&mut pc.conn, &other, PASS)
        .unwrap_err();
    assert!(err.to_string().contains("Sign out first"), "{err}");

    pc.household.sign_out(&mut pc.conn).unwrap();
    let status = pc.household.status(&pc.conn).unwrap();
    assert!(!status.signed_in && !status.joined);
    assert_eq!(status.email, None);
    assert!(pc.secrets.get(VAULT_SECRET).is_none());
    // The library itself is untouched.
    let channels: i64 = pc
        .conn
        .query_row("SELECT COUNT(*) FROM channels", [], |r| r.get(0))
        .unwrap();
    assert_eq!(channels, 1);
    pc.household
        .create_account(&mut pc.conn, &other, PASS)
        .unwrap();
}

#[test]
fn reset_replaces_the_server_copy_with_this_devices() {
    let fb = FakeFirebase::new();
    let mut pc = device(&fb, 0);
    let mut tv = device(&fb, 100);
    pc.household
        .create_account(&mut pc.conn, &account("PC"), PASS)
        .unwrap();
    tv.household
        .sign_in(
            &mut tv.conn,
            &account("TV"),
            Unlock::Passphrase(PASS.into()),
        )
        .unwrap();
    // Only the TV has this favourite.
    lists::toggle_favorite(&tv.conn, 1, 110, None, 1).unwrap();
    tv.household.sync(&mut tv.conn).unwrap();
    let before = fb.document_names().len();
    assert!(before > 0);

    let report = pc.household.reset(&mut pc.conn).unwrap();
    assert!(report.pushed > 0);
    // Every document left on the server is the PC's.
    let pc_id = pc.household.status(&pc.conn).unwrap().device_id.unwrap();
    for name in fb.document_names() {
        if name.contains("/records/") {
            assert!(name.ends_with(&format!("_{pc_id}")), "{name}");
        }
    }
    // A device joining now sees the PC's library, without the TV's favourite.
    let mut laptop = device(&fb, 200);
    laptop
        .household
        .sign_in(
            &mut laptop.conn,
            &account("Laptop"),
            Unlock::Passphrase(PASS.into()),
        )
        .unwrap();
    assert_eq!(favourites(&laptop.conn, 210), 0);
    assert_eq!(db::outbox_len(&pc.conn).unwrap(), 0);
}
