//! Provider passwords and API keys, synced through the vault (`docs/APPLE_PORT_AND_SYNC.md`
//! §3.4, `docs/DECISIONS.md` D34).
//!
//! **Off unless the household turns it on** ([`SHARE_KEY`]). Every other record syncs
//! because it is the viewer's library; a password is a different promise, and the Sync
//! panel and `PRIVACY.md` made the opposite one. So nothing here is sent until somebody
//! chooses to, and turning it off stops the sending.
//!
//! **Plaintext never reaches SQLite.** These live in the platform credential store, and a
//! record that carried them through `sync_shadow` or `sync_pending` would put them in the
//! library file in the clear. So:
//!
//! - the shadow keeps a [`fingerprint`] in place of the value: a keyed hash under the
//!   vault, enough to merge on (only the clocks decide) and useless to anybody holding
//!   just the file;
//! - `sync_state` keeps the same fingerprint per secret ([`seen`]), which is how a round
//!   notices that one changed in the credential store, where no trigger can see it;
//! - a record that has to wait -- a password for a provider this device has not got yet --
//!   is parked [`seal`]ed: encrypted with the vault, as it travels.
//!
//! **A host that has no store to put them in** does not understand the kind: its engine
//! notes such records as unreadable (§3.8), and fetches them again once it has one.

use std::collections::BTreeMap;

use aurora_db::repo::settings;
use rusqlite::{params, Connection};
use serde_json::{json, Value};

use crate::codec::Codec;
use crate::error::{Result, SyncError};
use crate::record::{Clocked, Record};

/// The record kind.
pub const KIND: &str = "secret";

/// Whether this household shares sign-ins and keys. A personal setting, so it syncs: it
/// is one decision for the household, and the passwords it sends reach every device
/// anyway.
pub const SHARE_KEY: &str = "sync.secrets";

/// `sync_state` keys, one per secret: the fingerprint last sent or applied here.
const SEEN_PREFIX: &str = "secret.";
/// What [`seen`] holds for a secret that existed here and is now gone.
const GONE: &str = "-";
/// How a parked record's value is marked as sealed.
const SEALED: &str = "sealed:";

/// The secrets a host can share, by name: `provider:<sync id>`, `tmdb`, `gemini`.
///
/// `conn` is sync's own connection, so an implementation may read and write the library
/// without taking the app's lock.
pub trait SharedSecrets: Send + Sync {
    /// Every secret this device holds that could be shared, with its value.
    fn list(&self, conn: &Connection) -> Vec<(String, String)>;
    /// Keep one that arrived from another device, or forget it (`None`). `Ok(false)`
    /// when what it belongs to is not on this device yet; the engine parks it.
    fn store(&self, conn: &Connection, name: &str, value: Option<&str>) -> Result<bool>;
}

/// Whether the household shares sign-ins and keys.
pub fn sharing(conn: &Connection) -> Result<bool> {
    Ok(settings::get::<bool>(conn, SHARE_KEY)?.unwrap_or(false))
}

/// Turn sharing on or off.
pub fn set_sharing(conn: &Connection, on: bool) -> Result<()> {
    Ok(settings::set(conn, SHARE_KEY, &on)?)
}

/// The record's identity: just the name, which every device spells the same.
pub fn ident(name: &str) -> Value {
    json!({ "name": name })
}

/// The name in an ident.
pub fn name_of(ident: &Value) -> Result<&str> {
    ident
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| SyncError::Protocol("a secret record without a name".into()))
}

/// A keyed hash of one secret's value, under the vault: the same value always gives the
/// same answer on every device in the household, and nobody without the vault key can
/// test a guess against it.
pub fn fingerprint<C: Codec>(codec: &C, name: &str, value: &str) -> String {
    format!(
        "fp:{}",
        codec.doc_id(&format!("secret-value\u{0}{name}\u{0}{value}"))
    )
}

/// Every secret's fingerprint as last sent or applied here.
pub fn seen(conn: &Connection) -> Result<BTreeMap<String, String>> {
    let mut stmt = conn.prepare("SELECT key, value FROM sync_state WHERE key LIKE 'secret.%'")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(k, v)| k.strip_prefix(SEEN_PREFIX).map(|n| (n.to_string(), v)))
        .collect())
}

/// Note what this device now knows a secret to be, so the next round does not send it
/// back.
pub fn note_seen(conn: &Connection, name: &str, fingerprint: Option<&str>) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_state (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![format!("{SEEN_PREFIX}{name}"), fingerprint.unwrap_or(GONE)],
    )?;
    Ok(())
}

/// Queue a secret to be sent. Goes through `sync_enqueue` like every trigger, so it is
/// dropped when sync is off or a pulled change is being written.
pub fn enqueue(conn: &Connection, name: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_enqueue (kind, ident) VALUES (?1, ?2)",
        params![KIND, ident(name).to_string()],
    )?;
    Ok(())
}

/// Look for secrets that changed in the credential store since the last round, and queue
/// them. Returns how many were queued.
pub fn capture<C: Codec>(conn: &Connection, store: &dyn SharedSecrets, codec: &C) -> Result<usize> {
    let mut seen = seen(conn)?;
    let mut queued = 0;
    for (name, value) in store.list(conn) {
        let fp = fingerprint(codec, &name, &value);
        if seen.remove(&name).as_deref() != Some(fp.as_str()) {
            enqueue(conn, &name)?;
            note_seen(conn, &name, Some(&fp))?;
            queued += 1;
        }
    }
    // Held here before and gone now: send the removal, once.
    for (name, fp) in seen {
        if fp != GONE {
            enqueue(conn, &name)?;
            note_seen(conn, &name, None)?;
            queued += 1;
        }
    }
    Ok(queued)
}

/// What may be written to SQLite of a secret record: the value replaced by its
/// fingerprint. Every other kind is returned as it is.
pub fn storable<C: Codec>(codec: &C, record: &Record) -> Record {
    if record.kind != KIND {
        return record.clone();
    }
    let mut out = record.clone();
    if let (Ok(name), Some(field)) = (name_of(&record.ident), out.fields.get_mut("value")) {
        if let Some(plain) = field.value.as_str().filter(|v| !v.starts_with("fp:")) {
            field.value = Value::String(fingerprint(codec, name, plain));
        }
    }
    out
}

/// The plaintext value a record carries, if it carries one rather than a fingerprint.
pub fn plain_value(record: &Record) -> Option<&str> {
    record
        .fields
        .get("value")
        .and_then(|f| f.value.as_str())
        .filter(|v| !v.starts_with("fp:") && !v.starts_with(SEALED))
}

/// A record to park, encrypted with the vault as it travels: what goes in
/// `sync_pending` in place of the plaintext.
pub fn seal<C: Codec>(codec: &C, record: &Record) -> Result<Record> {
    let bytes = codec.encode(record)?;
    let mut out = record.clone();
    out.fields = BTreeMap::from([(
        "value".to_string(),
        Clocked {
            value: Value::String(format!("{SEALED}{}", to_hex(&bytes))),
            hlc: record
                .fields
                .get("value")
                .map_or(record.alive.hlc, |f| f.hlc),
        },
    )]);
    Ok(out)
}

/// Undo [`seal`]. A record that was not sealed is returned as it is.
pub fn unseal<C: Codec>(codec: &C, record: Record) -> Result<Record> {
    let Some(hex) = record
        .fields
        .get("value")
        .and_then(|f| f.value.as_str())
        .and_then(|v| v.strip_prefix(SEALED))
    else {
        return Ok(record);
    };
    let bytes = from_hex(hex).ok_or(SyncError::Undecodable)?;
    codec.decode(&codec.doc_id(&record.key), &bytes)
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::PlainCodec;
    use crate::vault::{VaultCodec, VaultKey};

    fn record(value: &str) -> Record {
        Record {
            key: format!("{KIND}:{}", ident("tmdb")),
            kind: KIND.into(),
            ident: ident("tmdb"),
            alive: Clocked {
                value: true,
                hlc: 5,
            },
            fields: BTreeMap::from([(
                "value".to_string(),
                Clocked {
                    value: Value::String(value.into()),
                    hlc: 7,
                },
            )]),
            writer: "pc".into(),
        }
    }

    #[test]
    fn what_reaches_sqlite_never_holds_the_secret() {
        let codec = VaultCodec::new(&VaultKey::generate().unwrap());
        let r = record("hunter22-api-key");

        let kept = storable(&codec, &r);
        let text = serde_json::to_string(&kept).unwrap();
        assert!(!text.contains("hunter22"), "{text}");
        assert_eq!(plain_value(&kept), None);
        // The same value always fingerprints the same, and a different one does not.
        assert_eq!(kept, storable(&codec, &kept));
        assert_ne!(kept, storable(&codec, &record("another-key")));

        let sealed = seal(&codec, &r).unwrap();
        let text = serde_json::to_string(&sealed).unwrap();
        assert!(!text.contains("hunter22"), "{text}");
        assert_eq!(unseal(&codec, sealed).unwrap(), r);
    }

    #[test]
    fn a_fingerprint_depends_on_the_vault_key() {
        let a = VaultCodec::new(&VaultKey::generate().unwrap());
        let b = VaultCodec::new(&VaultKey::generate().unwrap());
        assert_ne!(fingerprint(&a, "tmdb", "k"), fingerprint(&b, "tmdb", "k"));
        assert_eq!(fingerprint(&a, "tmdb", "k"), fingerprint(&a, "tmdb", "k"));
        // And on the name: the same password on two providers is two different secrets.
        assert_ne!(fingerprint(&a, "tmdb", "k"), fingerprint(&a, "gemini", "k"));
    }

    #[test]
    fn hex_round_trips_and_rejects_nonsense() {
        let bytes = [0u8, 1, 0xab, 0xff];
        assert_eq!(from_hex(&to_hex(&bytes)).unwrap(), bytes);
        assert!(from_hex("abc").is_none());
        assert!(from_hex("zz").is_none());
        let _ = PlainCodec;
    }
}
