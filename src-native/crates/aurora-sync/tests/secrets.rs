//! Sign-ins and API keys through sync (§3.4, D34), end to end through the encrypted codec.
//!
//! Three promises are checked here rather than assumed: nothing is sent until the
//! household turns it on; a secret reaches the other device's credential store and never
//! its library file in the clear; and a device that cannot keep secrets skips them and
//! fetches them once it can.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use aurora_db::repo::sync as db;
use aurora_db::rusqlite::Connection;
use aurora_sync::engine::Engine;
use aurora_sync::secrets::{self, SharedSecrets};
use aurora_sync::transport::{MemoryTransport, SyncTransport};
use aurora_sync::vault::{VaultCodec, VaultKey};

/// A credential store in memory. `accepts` says whether what a secret belongs to is on
/// this device: false stands for a password of a provider not imported here yet.
#[derive(Default)]
struct Store {
    values: Mutex<BTreeMap<String, String>>,
    refuses: AtomicBool,
}

impl Store {
    fn put(&self, name: &str, value: &str) {
        self.values
            .lock()
            .unwrap()
            .insert(name.into(), value.into());
    }
    fn remove(&self, name: &str) {
        self.values.lock().unwrap().remove(name);
    }
    fn get(&self, name: &str) -> Option<String> {
        self.values.lock().unwrap().get(name).cloned()
    }
}

impl SharedSecrets for Store {
    fn list(&self, _: &Connection) -> Vec<(String, String)> {
        self.values
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
    fn store(&self, _: &Connection, name: &str, value: Option<&str>) -> aurora_sync::Result<bool> {
        if self.refuses.load(Ordering::SeqCst) {
            return Ok(false);
        }
        match value {
            Some(v) => self.put(name, v),
            None => self.remove(name),
        }
        Ok(true)
    }
}

const SECRET: &str = "hunter22-provider-password";

struct Device {
    conn: Connection,
    engine: Engine<MemoryTransport, VaultCodec>,
    store: Arc<Store>,
}

impl Device {
    fn new(server: &MemoryTransport, key: &VaultKey, name: &str, with_store: bool) -> Self {
        let store = Arc::new(Store::default());
        let shared: Option<Arc<dyn SharedSecrets>> = if with_store {
            Some(store.clone())
        } else {
            None
        };
        let mut d = Device {
            conn: aurora_db::open_memory().unwrap(),
            engine: Engine::new(server.clone(), VaultCodec::new(key), name).with_secrets(shared),
            store,
        };
        d.engine.join(&mut d.conn).unwrap();
        d
    }
    fn sync(&mut self) -> aurora_sync::Report {
        self.engine.sync(&mut self.conn).unwrap()
    }
    /// Everything sync keeps in this device's library, as text.
    fn library_text(&self) -> String {
        let mut out = String::new();
        for table in [
            "sync_shadow",
            "sync_pending",
            "sync_outbox",
            "sync_state",
            "settings",
        ] {
            let mut stmt = self
                .conn
                .prepare(&format!("SELECT * FROM {table}"))
                .unwrap();
            let n = stmt.column_count();
            let rows = stmt
                .query_map([], |r| {
                    let mut line = String::new();
                    for i in 0..n {
                        let v: aurora_db::rusqlite::types::Value = r.get(i)?;
                        line.push_str(&format!("{v:?}|"));
                    }
                    Ok(line)
                })
                .unwrap();
            for row in rows {
                out.push_str(&row.unwrap());
                out.push('\n');
            }
        }
        out
    }
}

fn household() -> (MemoryTransport, VaultKey) {
    (MemoryTransport::new(), VaultKey::generate().unwrap())
}

fn secret_records(server: &MemoryTransport, codec: &VaultCodec) -> usize {
    use aurora_sync::codec::Codec;
    server
        .pull(None, 10_000)
        .unwrap()
        .records
        .iter()
        .filter_map(|e| codec.decode(&e.doc_id, &e.payload).ok())
        .filter(|r| r.kind == secrets::KIND)
        .count()
}

#[test]
fn nothing_is_sent_until_the_household_turns_it_on() {
    let (server, key) = household();
    let mut pc = Device::new(&server, &key, "pc", true);
    pc.store.put("tmdb", SECRET);
    pc.sync();
    pc.sync();
    assert_eq!(secret_records(&server, &VaultCodec::new(&key)), 0);

    secrets::set_sharing(&pc.conn, true).unwrap();
    pc.sync();
    assert_eq!(secret_records(&server, &VaultCodec::new(&key)), 1);

    // And off stops it: a new key stays here.
    secrets::set_sharing(&pc.conn, false).unwrap();
    pc.store.put("gemini", "another-key");
    pc.sync();
    assert_eq!(secret_records(&server, &VaultCodec::new(&key)), 1);
}

#[test]
fn a_shared_secret_reaches_the_other_store_and_never_a_library_file_in_the_clear() {
    let (server, key) = household();
    let mut pc = Device::new(&server, &key, "pc", true);
    let mut tv = Device::new(&server, &key, "tv", true);
    secrets::set_sharing(&pc.conn, true).unwrap();
    pc.store.put("provider:abc", SECRET);
    pc.sync();
    tv.sync();

    assert_eq!(tv.store.get("provider:abc").as_deref(), Some(SECRET));
    for (who, d) in [("pc", &pc), ("tv", &tv)] {
        let text = d.library_text();
        assert!(
            !text.contains("hunter22"),
            "{who}'s library holds the secret:\n{text}"
        );
    }
    // The server holds ciphertext only.
    for env in server.pull(None, 10_000).unwrap().records {
        assert!(!String::from_utf8_lossy(&env.payload).contains("hunter22"));
    }
    // Received is not changed: the TV does not send it back.
    tv.sync();
    assert_eq!(db::outbox_len(&tv.conn).unwrap(), 0);
}

#[test]
fn a_change_and_a_removal_travel_both_ways() {
    let (server, key) = household();
    let mut pc = Device::new(&server, &key, "pc", true);
    let mut tv = Device::new(&server, &key, "tv", true);
    secrets::set_sharing(&pc.conn, true).unwrap();
    pc.store.put("tmdb", "first");
    pc.sync();
    tv.sync();
    assert_eq!(tv.store.get("tmdb").as_deref(), Some("first"));

    // Changed on the TV: the PC takes it.
    tv.store.put("tmdb", "second");
    tv.sync();
    pc.sync();
    assert_eq!(pc.store.get("tmdb").as_deref(), Some("second"));

    // Removed on the PC: gone from the TV too.
    pc.store.remove("tmdb");
    pc.sync();
    tv.sync();
    assert_eq!(tv.store.get("tmdb"), None);
    // And settled: nothing more to send on either side.
    pc.sync();
    tv.sync();
    assert_eq!(db::outbox_len(&pc.conn).unwrap(), 0);
    assert_eq!(db::outbox_len(&tv.conn).unwrap(), 0);
}

#[test]
fn a_password_for_a_provider_not_here_yet_waits_sealed_then_applies() {
    let (server, key) = household();
    let mut pc = Device::new(&server, &key, "pc", true);
    let mut tv = Device::new(&server, &key, "tv", true);
    secrets::set_sharing(&pc.conn, true).unwrap();
    pc.store.put("provider:abc", SECRET);
    pc.sync();

    tv.store.refuses.store(true, Ordering::SeqCst);
    let report = tv.sync();
    assert_eq!(report.parked, 1, "{report:?}");
    assert_eq!(tv.store.get("provider:abc"), None);
    let text = tv.library_text();
    assert!(!text.contains("hunter22"), "parked in the clear:\n{text}");

    // The provider arrives (an import); the parked password applies.
    tv.store.refuses.store(false, Ordering::SeqCst);
    tv.engine.retry_pending(&mut tv.conn).unwrap();
    assert_eq!(tv.store.get("provider:abc").as_deref(), Some(SECRET));
}

#[test]
fn a_device_with_nowhere_to_keep_secrets_skips_them_and_reads_them_once_it_can() {
    let (server, key) = household();
    let mut pc = Device::new(&server, &key, "pc", true);
    let mut tv = Device::new(&server, &key, "tv", false);
    secrets::set_sharing(&pc.conn, true).unwrap();
    pc.store.put("tmdb", SECRET);
    pc.sync();

    let report = tv.sync();
    assert_eq!(report.undecodable, 1, "{report:?}");
    assert_eq!(db::unreadable_len(&tv.conn).unwrap(), 1);

    // An update gives this host a store: the next round fetches what it skipped.
    let store = Arc::new(Store::default());
    let shared: Arc<dyn SharedSecrets> = store.clone();
    tv.engine = Engine::new(server.clone(), VaultCodec::new(&key), "tv").with_secrets(Some(shared));
    tv.store = store;
    tv.sync();
    assert_eq!(tv.store.get("tmdb").as_deref(), Some(SECRET));
    assert_eq!(db::unreadable_len(&tv.conn).unwrap(), 0);
}
