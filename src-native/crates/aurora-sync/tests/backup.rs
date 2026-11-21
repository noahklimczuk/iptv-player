//! A backup file, written on one install and restored onto another.

use aurora_db::repo::{lists, settings, sync as db};
use aurora_db::rusqlite::Connection;
use aurora_sync::backup::{self, Backup};

const NOW: i64 = 1_800_000_000;

/// An install that has imported a line, at row ids offset by `offset`.
fn install(offset: i64) -> Connection {
    let conn = aurora_db::open_memory().unwrap();
    conn.execute_batch(&format!(
        "INSERT INTO providers (id,name,kind,base_url,username,created_at)
              VALUES ({p},'Line','xtream','http://panel.example/','viewer',0);
         INSERT INTO channels (id,provider_id,provider_key,name,match_key,last_seen_at)
              VALUES ({c1},{p},'501','BBC One','bbcone',0),
                     ({c2},{p},'502','BBC Two','bbctwo',0);",
        p = 1 + offset,
        c1 = 10 + offset,
        c2 = 11 + offset,
    ))
    .unwrap();
    conn
}

/// What a viewer had set up: two favourites, a renamed channel, a personal setting and
/// one that only ever belongs to the device.
fn set_up(conn: &Connection, offset: i64) {
    lists::toggle_favorite(conn, 1, 10 + offset, None, 1).unwrap();
    lists::toggle_favorite(conn, 1, 11 + offset, None, 1).unwrap();
    conn.execute(
        "UPDATE channels SET custom_name = 'One' WHERE id = ?1",
        [10 + offset],
    )
    .unwrap();
    settings::set(conn, "filter.english_only", &true).unwrap();
    settings::set(conn, "timeshift.folder", &"D:\\Timeshift").unwrap();
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn round_trip(from: &mut Connection) -> Backup {
    let text = backup::export(from, NOW, "2.0.0")
        .unwrap()
        .to_json()
        .unwrap();
    Backup::from_json(&text).unwrap()
}

#[test]
fn a_backup_restores_onto_a_fresh_install_once_its_library_arrives() {
    let mut old = install(0);
    set_up(&old, 0);
    let file = round_trip(&mut old);
    assert!(file.counts()["favorite"] >= 2, "{:?}", file.counts());

    // A new PC: nothing on it at all.
    let mut new = aurora_db::open_memory().unwrap();
    let done = backup::restore(&mut new, &file, NOW).unwrap();
    assert_eq!(done.providers, 1);
    assert!(
        done.waiting >= 3,
        "favourites and the rename wait for channels: {done:?}"
    );
    assert_eq!(count(&new, "SELECT COUNT(*) FROM providers"), 1);
    assert_eq!(count(&new, "SELECT COUNT(*) FROM favorites"), 0);
    // Settings need nothing imported, including the device's own.
    assert_eq!(
        settings::get::<bool>(&new, "filter.english_only").unwrap(),
        Some(true)
    );
    assert_eq!(
        settings::get::<String>(&new, "timeshift.folder")
            .unwrap()
            .as_deref(),
        Some("D:\\Timeshift")
    );

    // The restored provider is refreshed: its channels arrive, under new row ids.
    let p: i64 = count(&new, "SELECT id FROM providers");
    new.execute_batch(&format!(
        "INSERT INTO channels (id,provider_id,provider_key,name,match_key,last_seen_at)
              VALUES (70,{p},'501','BBC One','bbcone',0), (71,{p},'502','BBC Two','bbctwo',0);"
    ))
    .unwrap();
    assert!(backup::retry_parked(&mut new, NOW).unwrap() >= 3);
    assert_eq!(backup::waiting(&new).unwrap(), 0);
    assert_eq!(
        count(
            &new,
            "SELECT COUNT(*) FROM favorites WHERE item_id IN (70, 71)"
        ),
        2
    );
    assert_eq!(
        count(
            &new,
            "SELECT COUNT(*) FROM channels WHERE id = 70 AND custom_name = 'One'"
        ),
        1
    );
}

#[test]
fn a_provider_added_again_by_hand_takes_the_backups_place() {
    let mut old = install(0);
    set_up(&old, 0);
    let file = round_trip(&mut old);

    // Reinstalled, the line added again through the wizard (a new identity), then the
    // backup restored: everything lands at once, on the provider already there.
    let mut new = install(500);
    let done = backup::restore(&mut new, &file, NOW).unwrap();
    assert_eq!(done.waiting, 0, "{done:?}");
    assert_eq!(count(&new, "SELECT COUNT(*) FROM providers"), 1);
    assert_eq!(
        count(
            &new,
            "SELECT COUNT(*) FROM favorites WHERE item_id IN (510, 511)"
        ),
        2
    );
}

#[test]
fn making_a_backup_changes_nothing_sync_knows() {
    let mut conn = install(0);
    set_up(&conn, 0);
    backup::export(&mut conn, NOW, "2.0.0").unwrap();
    assert!(
        !db::is_enabled(&conn).unwrap(),
        "a backup must not switch sync on"
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_outbox"), 0);

    // With sync on and changes waiting to go, they are still waiting afterwards.
    db::enable(&mut conn).unwrap();
    conn.execute("DELETE FROM sync_outbox WHERE kind <> 'favorite'", [])
        .unwrap();
    let waiting = count(&conn, "SELECT COUNT(*) FROM sync_outbox");
    let file = backup::export(&mut conn, NOW, "2.0.0").unwrap();
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_outbox"), waiting);
    assert!(
        file.counts()["setting"] >= 2,
        "the file still has everything: {:?}",
        file.counts()
    );
}

#[test]
fn a_restore_with_sync_on_goes_to_the_household() {
    let mut old = install(0);
    set_up(&old, 0);
    let file = round_trip(&mut old);

    let mut new = install(500);
    db::enable(&mut new).unwrap();
    new.execute("DELETE FROM sync_outbox", []).unwrap();
    backup::restore(&mut new, &file, NOW).unwrap();
    assert!(
        count(
            &new,
            "SELECT COUNT(*) FROM sync_outbox WHERE kind = 'favorite'"
        ) >= 2,
        "restored favourites are captured like any local change"
    );
}

#[test]
fn no_password_is_ever_in_the_file() {
    let mut conn = install(0);
    let text = backup::export(&mut conn, NOW, "2.0.0")
        .unwrap()
        .to_json()
        .unwrap();
    assert!(!text.to_lowercase().contains("password"), "{text}");
    assert!(
        text.contains("panel.example"),
        "the provider itself is there"
    );
}

#[test]
fn anything_but_an_aurora_backup_is_refused_in_words() {
    let err = |t: &str| Backup::from_json(t).unwrap_err().to_string();
    assert!(err("not json").contains("not an Aurora backup"));
    assert!(err(r#"{"format":"something-else","version":1}"#).contains("not an Aurora backup"));
    assert!(
        err(r#"{"format":"aurora-backup","version":99,"records":[]}"#).contains("newer Aurora")
    );
    assert!(err(r#"{"format":"aurora-backup","version":1}"#).contains("damaged"));
}

#[test]
fn a_kind_from_a_newer_aurora_is_skipped_not_misapplied() {
    let mut conn = install(0);
    let mut file = backup::export(&mut conn, NOW, "2.0.0").unwrap();
    file.records.push(backup::Entry {
        kind: "hologram".into(),
        ident: serde_json::json!({ "id": 1 }),
        fields: Default::default(),
    });
    let mut new = aurora_db::open_memory().unwrap();
    assert_eq!(backup::restore(&mut new, &file, NOW).unwrap().skipped, 1);
}
