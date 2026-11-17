//! Several devices, one household, one server: the sync rules end to end.
//!
//! Each simulated device is its own in-memory database, seeded the way a real device
//! would be -- the same provider line imported independently, so with its own provider
//! UUID and its own row ids. That is the situation the sync keys exist for, and joining
//! is what reconciles them.

use aurora_db::repo::{lists, progress, settings, sync as db};
use aurora_db::rusqlite::Connection;
use aurora_sync::codec::PlainCodec;
use aurora_sync::transport::MemoryTransport;
use aurora_sync::Engine;

struct Device {
    conn: Connection,
    engine: Engine<MemoryTransport, PlainCodec>,
}

/// A device that has imported the household's line. `with_two` decides whether it has
/// seen the second channel yet. Row ids are deliberately offset per device.
fn device(server: &MemoryTransport, name: &str, offset: i64, with_two: bool) -> Device {
    let conn = aurora_db::open_memory().unwrap();
    conn.execute_batch(&format!(
        "INSERT INTO providers (id,name,kind,base_url,username,created_at)
              VALUES ({p},'Line','xtream','http://panel.example/','viewer',0);
         INSERT INTO channels (id,provider_id,provider_key,name,match_key,last_seen_at)
              VALUES ({c1},{p},'501','BBC One','bbcone',0);
         INSERT INTO movies (id,provider_id,provider_key,title,match_key,url,last_seen_at)
              VALUES ({m},{p},'9001','Heat','heat','http://x/heat.mkv',0);
         INSERT INTO series (id,provider_id,provider_key,title,match_key,last_seen_at)
              VALUES ({s},{p},'77','Lost','lost',0);
         INSERT INTO episodes (id,series_id,season,episode,url)
              VALUES ({e},{s},1,2,'http://x/lost.mkv');",
        p = 1 + offset,
        c1 = 10 + offset,
        m = 20 + offset,
        s = 30 + offset,
        e = 40 + offset,
    ))
    .unwrap();
    if with_two {
        import_second_channel(&conn, offset);
    }
    Device {
        conn,
        engine: Engine::new(server.clone(), PlainCodec, name),
    }
}

fn import_second_channel(conn: &Connection, offset: i64) {
    conn.execute(
        "INSERT INTO channels (id,provider_id,provider_key,name,match_key,last_seen_at)
              VALUES (?1,?2,'502','BBC Two','bbctwo',0)",
        [11 + offset, 1 + offset],
    )
    .unwrap();
}

impl Device {
    fn join(&mut self) {
        self.engine.join(&mut self.conn).unwrap();
    }
    fn sync(&mut self) -> aurora_sync::Report {
        self.engine.sync(&mut self.conn).unwrap()
    }
    fn count(&self, sql: &str) -> i64 {
        self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }
    fn favorites(&self) -> i64 {
        self.count("SELECT COUNT(*) FROM favorites")
    }
}

/// Two devices on one line, both joined: A first, then B.
fn household() -> (MemoryTransport, Device, Device) {
    let server = MemoryTransport::new();
    let mut a = device(&server, "pc", 0, true);
    let mut b = device(&server, "tv", 100, true);
    a.join();
    b.join();
    a.sync();
    (server, a, b)
}

#[test]
fn joining_adopts_the_households_provider_and_profile() {
    let (_, a, b) = household();
    let id = |d: &Device, t: &str| -> String {
        d.conn
            .query_row(&format!("SELECT sync_id FROM {t} LIMIT 1"), [], |r| {
                r.get(0)
            })
            .unwrap()
    };
    assert_eq!(id(&a, "providers"), id(&b, "providers"));
    assert_eq!(id(&a, "profiles"), id(&b, "profiles"));
    assert_eq!(
        b.count("SELECT COUNT(*) FROM providers"),
        1,
        "not duplicated"
    );
    assert_eq!(
        b.count("SELECT COUNT(*) FROM profiles"),
        1,
        "not duplicated"
    );
}

#[test]
fn a_favourite_on_one_device_appears_on_the_other() {
    let (_, mut a, mut b) = household();
    lists::toggle_favorite(&a.conn, 1, 10, None, 1).unwrap();
    a.sync();
    b.sync();
    // Channel 10 on the PC is channel 110 on the TV.
    assert_eq!(
        b.count("SELECT COUNT(*) FROM favorites WHERE item_id = 110 AND item_kind = 'channel'"),
        1
    );
    // Applying it did not queue it to be sent back.
    assert_eq!(db::outbox_len(&b.conn).unwrap(), 0);
}

#[test]
fn a_removal_is_not_resurrected_by_the_other_devices_older_copy() {
    let (server, mut a, mut b) = household();
    lists::toggle_favorite(&a.conn, 1, 10, None, 1).unwrap();
    a.sync();
    b.sync();

    // The TV removes it. The PC is offline and edits something else meanwhile.
    lists::toggle_favorite(&b.conn, 1, 110, None, 2).unwrap();
    b.sync();
    server.set_offline(true);
    lists::toggle_my_list(&a.conn, 1, lists::ItemKind::Movie, 20, 3).unwrap();
    assert!(a.engine.sync(&mut a.conn).is_err());
    server.set_offline(false);

    a.sync();
    b.sync();
    assert_eq!(a.favorites(), 0);
    assert_eq!(b.favorites(), 0);
    assert_eq!(b.count("SELECT COUNT(*) FROM my_list"), 1);
}

#[test]
fn offline_edits_wait_in_the_outbox_and_go_when_back_online() {
    let (server, mut a, mut b) = household();
    server.set_offline(true);
    lists::toggle_favorite(&a.conn, 1, 10, None, 1).unwrap();
    assert!(a.engine.sync(&mut a.conn).is_err());
    assert_eq!(db::outbox_len(&a.conn).unwrap(), 1);

    server.set_offline(false);
    a.sync();
    assert_eq!(db::outbox_len(&a.conn).unwrap(), 0);
    b.sync();
    assert_eq!(b.favorites(), 1);
}

#[test]
fn a_finished_film_stays_finished_against_a_stale_position() {
    let (server, mut a, mut b) = household();
    progress::save(&a.conn, 1, progress::ItemKind::Movie, 20, 600, 7000, 1).unwrap();
    a.sync();
    b.sync();

    // The PC goes offline mid-film. The TV finishes it.
    server.set_offline(true);
    progress::save(&a.conn, 1, progress::ItemKind::Movie, 20, 900, 7000, 5).unwrap();
    server.set_offline(false);
    progress::save(&b.conn, 1, progress::ItemKind::Movie, 120, 6990, 7000, 3).unwrap();
    b.sync();
    // The PC's later mid-film save arrives afterwards.
    a.sync();
    b.sync();

    for d in [&a, &b] {
        assert_eq!(
            d.count("SELECT completed FROM watch_progress WHERE item_kind = 'movie'"),
            1,
            "{} lost the finish",
            d.engine.device()
        );
    }
}

#[test]
fn the_device_that_saw_the_other_change_wins_even_against_a_fast_clock() {
    let (_, mut a, mut b) = household();
    // The PC's clock runs far ahead.
    let ahead = db::clock(&a.conn).unwrap() + (120_000 << 16);
    db::observe_clock(&a.conn, ahead).unwrap();
    aurora_db::repo::profiles::rename(&a.conn, 1, "From the PC").unwrap();
    a.sync();

    // The TV pulls that, and renames afterwards: its rename is later in causal order,
    // although its wall clock is two minutes behind the PC's.
    b.sync();
    aurora_db::repo::profiles::rename(&b.conn, 1, "From the TV").unwrap();
    b.sync();
    a.sync();
    let name: String = a
        .conn
        .query_row("SELECT name FROM profiles", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "From the TV");
}

#[test]
fn concurrent_edits_to_different_fields_of_a_profile_both_survive() {
    let (server, mut a, mut b) = household();
    server.set_offline(true);
    aurora_db::repo::profiles::rename(&a.conn, 1, "Sam").unwrap();
    aurora_db::repo::profiles::set_limits(&b.conn, 1, Some(12), false, Some(90)).unwrap();
    server.set_offline(false);
    a.sync();
    b.sync();
    a.sync();
    for d in [&a, &b] {
        let (name, rating): (String, Option<String>) = d
            .conn
            .query_row("SELECT name, max_rating FROM profiles", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(name, "Sam");
        assert_eq!(rating.as_deref(), Some("12"));
    }
}

#[test]
fn a_reference_to_something_not_imported_yet_is_parked_then_applied() {
    let server = MemoryTransport::new();
    let mut a = device(&server, "pc", 0, true);
    let mut b = device(&server, "tv", 100, false);
    a.join();
    b.join();

    lists::toggle_favorite(&a.conn, 1, 11, None, 1).unwrap();
    a.sync();
    let report = b.sync();
    assert_eq!(report.parked, 1);
    assert_eq!(b.favorites(), 0);

    // The TV's next refresh imports the channel; the parked favourite lands.
    import_second_channel(&b.conn, 100);
    assert_eq!(b.engine.retry_pending(&mut b.conn).unwrap(), 1);
    assert_eq!(
        b.count("SELECT COUNT(*) FROM favorites WHERE item_id = 111"),
        1
    );
    assert_eq!(b.count("SELECT COUNT(*) FROM sync_pending"), 0);
}

#[test]
fn personal_settings_travel_and_device_settings_do_not() {
    let (_, mut a, mut b) = household();
    settings::set(&a.conn, "filter.english_only", &true).unwrap();
    settings::set(&a.conn, "timeshift.folder", &"D:\\Timeshift").unwrap();
    a.sync();
    b.sync();
    assert_eq!(
        settings::get::<bool>(&b.conn, "filter.english_only").unwrap(),
        Some(true)
    );
    assert_eq!(
        settings::get::<String>(&b.conn, "timeshift.folder").unwrap(),
        None
    );
}

#[test]
fn three_devices_converge_whatever_order_they_sync_in() {
    let server = MemoryTransport::new();
    let mut a = device(&server, "pc", 0, true);
    let mut b = device(&server, "tv", 100, true);
    let mut c = device(&server, "phone", 200, true);
    a.join();
    b.join();
    c.join();

    lists::toggle_favorite(&a.conn, 1, 10, Some("Sport"), 1).unwrap();
    lists::toggle_my_list(&b.conn, 1, lists::ItemKind::Series, 130, 1).unwrap();
    lists::toggle_liked(&c.conn, 1, lists::ItemKind::Movie, 220, 1).unwrap();
    c.sync();
    a.sync();
    b.sync();
    c.sync();
    a.sync();

    for d in [&a, &b, &c] {
        assert_eq!(d.favorites(), 2, "{}", d.engine.device());
        assert_eq!(d.count("SELECT COUNT(*) FROM my_list"), 1);
    }
}

#[test]
fn an_episodes_progress_finds_the_same_episode_under_other_row_ids() {
    let (_, mut a, mut b) = household();
    progress::save(&a.conn, 1, progress::ItemKind::Episode, 40, 300, 2400, 1).unwrap();
    a.sync();
    b.sync();
    assert_eq!(
        b.count(
            "SELECT position_secs FROM watch_progress WHERE item_kind='episode' AND item_id=140"
        ),
        300
    );
}

#[test]
fn a_damaged_record_is_skipped_not_fatal() {
    let (server, mut a, mut b) = household();
    use aurora_sync::transport::{Envelope, SyncTransport};
    server
        .push(&[Envelope {
            doc_id: "favorite:{}".into(),
            payload: b"not json".to_vec(),
            writer: "someone".into(),
        }])
        .unwrap();
    lists::toggle_favorite(&a.conn, 1, 10, None, 1).unwrap();
    a.sync();
    let report = b.sync();
    assert_eq!(report.undecodable, 1);
    assert_eq!(b.favorites(), 1);
}

#[test]
fn a_device_that_never_joins_captures_nothing() {
    let server = MemoryTransport::new();
    let d = device(&server, "pc", 0, true);
    lists::toggle_favorite(&d.conn, 1, 10, None, 1).unwrap();
    assert_eq!(db::outbox_len(&d.conn).unwrap(), 0);
    assert!(server.is_empty());
}

#[test]
fn opening_a_film_pulls_its_progress_directly() {
    let (_, mut a, mut b) = household();
    // The TV stops at 41:00; the phone (here, the PC) opens the film before its next
    // scheduled pull.
    progress::save(&b.conn, 1, progress::ItemKind::Movie, 120, 2460, 7000, 1).unwrap();
    b.sync();
    let key: String = b
        .conn
        .query_row(
            "SELECT sync_key FROM sync_shadow WHERE sync_key LIKE 'progress:%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(a.engine.refresh_one(&mut a.conn, &key).unwrap());
    assert_eq!(
        a.count("SELECT position_secs FROM watch_progress WHERE item_id = 20"),
        2460
    );
}

#[test]
fn devices_sync_through_the_encrypted_codec_and_the_server_sees_no_plaintext() {
    use aurora_sync::transport::SyncTransport;
    use aurora_sync::vault::{VaultCodec, VaultKey};

    let server = MemoryTransport::new();
    let key = VaultKey::generate().unwrap();
    let mk = |name: &str, offset: i64| {
        let d = device(&server, name, offset, true);
        (d, Engine::new(server.clone(), VaultCodec::new(&key), name))
    };
    let (mut a, ea) = mk("pc", 0);
    let (mut b, eb) = mk("tv", 100);
    ea.join(&mut a.conn).unwrap();
    eb.join(&mut b.conn).unwrap();
    lists::toggle_favorite(&a.conn, 1, 10, None, 1).unwrap();
    ea.sync(&mut a.conn).unwrap();
    eb.sync(&mut b.conn).unwrap();
    assert_eq!(b.favorites(), 1);

    for env in server.pull(None, 10_000).unwrap().records {
        let text = String::from_utf8_lossy(&env.payload);
        assert!(!text.contains("BBC") && !text.contains("501") && !text.contains("Favorites"));
        assert!(
            !env.doc_id.contains(':'),
            "doc id leaks the key: {}",
            env.doc_id
        );
    }
}
