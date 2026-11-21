//! A backup file: everything sync would carry, written to a file instead of a server.
//!
//! For the people who do not use sync -- moving to a new PC, a reinstall, a second copy
//! set up the same way. It holds profiles, the parental settings, favourites and lists,
//! progress, per-show preferences, the viewer's own channel names and groups, rules,
//! recording rules, reminders, multi-view layouts and every setting, each named in the
//! device-independent terms sync uses (`mv:{provider}:{stream}`), so it restores onto a
//! library whose row ids are different.
//!
//! It never holds a password or a key. Those live in the system's credential store and
//! only travel through sync, end-to-end encrypted, when the household chooses to share
//! them; a file on a desktop or a USB stick is not that. A restored provider asks for its
//! password again.
//!
//! Reading is [`aurora_db::repo::sync::snapshot`] and [`local::read`], which change
//! nothing: sync's own state, and anything waiting to go to the server, are untouched.
//! Restoring is [`local::write`] for each record, in [`local::KINDS`] order so providers
//! and profiles exist before what names them. Something that names a channel or a film
//! not imported yet -- the usual case on a fresh install, where the provider comes back
//! before its library does -- waits in `restore_pending` and is tried again by
//! [`retry_parked`] after each import.
//!
//! A restore writes the backup's values over this device's for every record it holds,
//! and leaves alone anything it does not mention: nothing is deleted. With sync on, what
//! it writes is captured like any local change and goes to the household.

use std::collections::BTreeMap;

use aurora_db::repo::sync as db;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Result, SyncError};
use crate::local::{self, Applied};
use crate::record::{Fields, Record};

/// What the file says it is, so a restore can refuse anything else.
pub const FORMAT: &str = "aurora-backup";
/// The newest layout this build writes and reads. A newer one is refused, not guessed at.
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub format: String,
    pub version: u32,
    /// Unix seconds.
    pub created_at: i64,
    /// The Aurora that wrote it, for a person reading the file.
    pub app_version: String,
    pub records: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub kind: String,
    pub ident: Value,
    pub fields: Fields,
}

/// How a restore went.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Restored {
    pub applied: usize,
    /// Waiting for an import: they name something this library does not have yet.
    pub waiting: usize,
    /// Of a kind this build does not know, from a newer Aurora.
    pub skipped: usize,
    /// Providers that came back without their password.
    pub providers: usize,
}

impl Backup {
    /// How many records of each kind, for the screen to say what a file holds.
    pub fn counts(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for e in &self.records {
            *out.entry(e.kind.clone()).or_insert(0) += 1;
        }
        out
    }

    pub fn from_json(text: &str) -> Result<Self> {
        let probe: Value = serde_json::from_str(text)
            .map_err(|_| SyncError::Malformed("this is not an Aurora backup file".into()))?;
        if probe.get("format").and_then(Value::as_str) != Some(FORMAT) {
            return Err(SyncError::Malformed(
                "this is not an Aurora backup file".into(),
            ));
        }
        let version = probe.get("version").and_then(Value::as_u64).unwrap_or(0);
        if version > u64::from(VERSION) {
            return Err(SyncError::Malformed(
                "this backup was made by a newer Aurora; update this one to restore it".into(),
            ));
        }
        serde_json::from_value(probe)
            .map_err(|e| SyncError::Malformed(format!("this backup file is damaged: {e}")))
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

/// Everything on this device that a backup holds.
pub fn export(conn: &mut Connection, now: i64, app_version: &str) -> Result<Backup> {
    let rows = db::snapshot(conn)?;
    let mut records = Vec::with_capacity(rows.len());
    for (kind, ident) in rows {
        if !local::KINDS.contains(&kind.as_str()) {
            continue;
        }
        let ident: Value = serde_json::from_str(&ident)?;
        // Gone between the snapshot and the read: nothing to keep.
        if let Some(fields) = local::read(conn, &kind, &ident)? {
            records.push(Entry {
                kind,
                ident,
                fields,
            });
        }
    }
    Ok(Backup {
        format: FORMAT.into(),
        version: VERSION,
        created_at: now,
        app_version: app_version.into(),
        records,
    })
}

fn key_of(kind: &str, ident: &Value) -> Result<String> {
    Ok(format!("{kind}:{}", serde_json::to_string(ident)?))
}

fn rank(kind: &str) -> usize {
    local::KINDS
        .iter()
        .position(|k| *k == kind)
        .unwrap_or(usize::MAX)
}

/// Write `backup` onto this device.
pub fn restore(conn: &mut Connection, backup: &Backup, now: i64) -> Result<Restored> {
    let mut out = Restored::default();
    let mut entries: Vec<&Entry> = backup.records.iter().collect();
    // Stable, so records of one kind keep the file's order.
    entries.sort_by_key(|e| rank(&e.kind));

    let mut adopted = Vec::new();
    let tx = conn.transaction()?;
    let hlc = db::clock(&tx)?;
    for e in entries {
        if !local::KINDS.contains(&e.kind.as_str()) {
            out.skipped += 1;
            continue;
        }
        let key = key_of(&e.kind, &e.ident)?;
        // A provider or profile set up again by hand before restoring takes the backup's
        // identity, so what the backup names finds it (the same rule a first sync uses).
        let record = Record::capture(
            None,
            &key,
            &e.kind,
            &e.ident,
            Some(&e.fields),
            hlc,
            "backup",
        );
        if let Some(old) = local::adopt(&tx, &record)? {
            adopted.push(old);
        }
        match local::write(&tx, &e.kind, &e.ident, Some(&e.fields), now)? {
            Applied::Done => {
                out.applied += 1;
                tx.execute("DELETE FROM restore_pending WHERE sync_key = ?1", [&key])?;
            }
            Applied::Parked => {
                out.waiting += 1;
                tx.execute(
                    "INSERT INTO restore_pending (sync_key, kind, ident, fields, parked_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (sync_key) DO UPDATE SET
                        fields = excluded.fields, parked_at = excluded.parked_at",
                    params![
                        key,
                        e.kind,
                        serde_json::to_string(&e.ident)?,
                        serde_json::to_string(&e.fields)?,
                        now
                    ],
                )?;
            }
        }
        if e.kind == "provider" {
            out.providers += 1;
        }
    }
    tx.commit()?;

    // As after a first sync's adoption: outbox rows naming the old id are wrong now, so
    // they go and everything is queued again under the new one.
    if !adopted.is_empty() && db::is_enabled(conn)? {
        for old in &adopted {
            conn.execute("DELETE FROM sync_outbox WHERE instr(ident, ?1) > 0", [old])?;
        }
        db::enable(conn)?;
    }
    Ok(out)
}

/// Try again what a restore could not place yet. Called after every import; returns how
/// many went in.
pub fn retry_parked(conn: &mut Connection, now: i64) -> Result<usize> {
    let parked: Vec<(String, String, String, String)> = conn
        .prepare("SELECT sync_key, kind, ident, fields FROM restore_pending")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<std::result::Result<_, _>>()?;
    if parked.is_empty() {
        return Ok(0);
    }
    let tx = conn.transaction()?;
    let mut placed = 0;
    for (key, kind, ident, fields) in parked {
        let ident: Value = serde_json::from_str(&ident)?;
        let fields: Fields = serde_json::from_str(&fields)?;
        if local::write(&tx, &kind, &ident, Some(&fields), now)? == Applied::Done {
            tx.execute("DELETE FROM restore_pending WHERE sync_key = ?1", [&key])?;
            placed += 1;
        }
    }
    tx.commit()?;
    Ok(placed)
}

/// How many restored records are still waiting for an import.
pub fn waiting(conn: &Connection) -> Result<usize> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM restore_pending", [], |r| r.get(0))?;
    Ok(usize::try_from(n).unwrap_or(0))
}
