//! Push the outbox, pull what other devices wrote, merge, apply.
//!
//! The order of a sync is push, then pull, then a retry of parked records. Pushing first
//! means the server holds this device's latest before it reads others', which keeps a
//! round of edits on two devices to one round trip each.
//!
//! **Pushing** reads each outbox row's record as it is *now*, diffed against its shadow so
//! that only fields this device changed carry the new clock, and uploads it. The shadow
//! and the outbox are updated only after the server accepted the batch, so a failed push
//! changes nothing locally and simply happens again.
//!
//! **Pulling** merges each remote record into the shadow. If the same record also has an
//! unpushed local change, the local change is folded in before the row is written -- the
//! row gets whichever field values win -- but the shadow takes only the remote side, so
//! the next push still sees the local change as a change and sends it.

use std::time::{SystemTime, UNIX_EPOCH};

use aurora_core::sync_settings;
use aurora_db::repo::sync as db;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::codec::Codec;
use crate::error::Result;
use crate::local::{self, Applied};
use crate::record::Record;
use crate::transport::{Envelope, SyncTransport};

/// Firestore's commit limit, and a sensible page.
pub const BATCH: usize = 500;

/// Where the pull cursor is kept in `sync_state`.
pub const CURSOR: &str = "cursor";
/// This device's id in `sync_state`, set when it joins.
pub const DEVICE: &str = "device";
const APPLYING: &str = "applying";

/// What one sync did. Feeds the Sync panel and Diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub pushed: usize,
    pub pulled: usize,
    pub applied: usize,
    pub parked: usize,
    /// Records that would not decode: a newer app's, or damaged. Skipped, never applied.
    pub undecodable: usize,
    /// Local providers or profiles that took the id of a matching remote one.
    pub adopted: usize,
}

pub struct Engine<T: SyncTransport, C: Codec> {
    transport: T,
    codec: C,
    device: String,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn load(conn: &Connection, table: &str, key: &str) -> Result<Option<Record>> {
    let raw: Option<String> = conn
        .query_row(
            &format!("SELECT record FROM {table} WHERE sync_key = ?1"),
            [key],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match raw {
        Some(raw) => Some(serde_json::from_str(&raw)?),
        None => None,
    })
}

fn save_shadow(conn: &Connection, record: &Record) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_shadow (sync_key, record) VALUES (?1, ?2)
         ON CONFLICT (sync_key) DO UPDATE SET record = excluded.record",
        params![record.key, serde_json::to_string(record)?],
    )?;
    Ok(())
}

/// Whether a record carries a setting that stays on the device it was made on.
fn device_local(kind: &str, ident: &Value) -> bool {
    kind == "setting"
        && !ident
            .get("key")
            .and_then(Value::as_str)
            .is_some_and(sync_settings::syncs)
}

impl<T: SyncTransport, C: Codec> Engine<T, C> {
    pub fn new(transport: T, codec: C, device: impl Into<String>) -> Self {
        Self {
            transport,
            codec,
            device: device.into(),
        }
    }

    pub fn device(&self) -> &str {
        &self.device
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// First sync on this device: take in what the household already has, then send
    /// what this device has.
    ///
    /// Pulling before capture is on lets a provider or profile that exists on both sides
    /// adopt the household's id (see [`local::adopt`]) *before* the snapshot enqueues this
    /// device's rows under it -- so the same favourite on both sides is one record, not two.
    pub fn join(&self, conn: &mut Connection) -> Result<Report> {
        if db::is_enabled(conn)? {
            return self.sync(conn);
        }
        let mut report = Report::default();
        self.pull(conn, &mut report)?;
        db::enable(conn)?;
        db::set_state(conn, DEVICE, &self.device)?;
        self.push(conn, &mut report)?;
        Ok(report)
    }

    /// One round: push, pull, retry parked records.
    pub fn sync(&self, conn: &mut Connection) -> Result<Report> {
        let mut report = Report::default();
        self.push(conn, &mut report)?;
        self.pull(conn, &mut report)?;
        report.applied += self.retry_pending(conn)?;
        Ok(report)
    }

    pub fn push(&self, conn: &mut Connection, report: &mut Report) -> Result<()> {
        loop {
            let rows = db::outbox(conn, BATCH)?;
            if rows.is_empty() {
                return Ok(());
            }
            let mut envelopes = Vec::new();
            let mut done: Vec<(String, i64, Option<Record>)> = Vec::new();
            for row in &rows {
                let ident: Value = serde_json::from_str(&row.ident)?;
                if device_local(&row.kind, &ident) {
                    done.push((row.sync_key.clone(), row.hlc, None));
                    continue;
                }
                let shadow = load(conn, "sync_shadow", &row.sync_key)?;
                let fields = local::read(conn, &row.kind, &ident)?;
                let record = Record::capture(
                    shadow.as_ref(),
                    &row.sync_key,
                    &row.kind,
                    &ident,
                    fields.as_ref(),
                    row.hlc,
                    &self.device,
                );
                let unchanged = match &shadow {
                    Some(s) => !record.differs_from(s),
                    // Created and removed again before it was ever sent.
                    None => !record.alive.value,
                };
                if unchanged {
                    done.push((row.sync_key.clone(), row.hlc, None));
                    continue;
                }
                envelopes.push(Envelope {
                    doc_id: self.codec.doc_id(&record.key),
                    payload: self.codec.encode(&record)?,
                    writer: self.device.clone(),
                });
                done.push((row.sync_key.clone(), row.hlc, Some(record)));
            }

            if !envelopes.is_empty() {
                if let Err(e) = self.transport.push(&envelopes) {
                    for (key, _, _) in &done {
                        db::record_attempt(conn, key)?;
                    }
                    return Err(e);
                }
            }

            let tx = conn.transaction()?;
            let mut acknowledged = 0;
            for (key, hlc, record) in &done {
                if let Some(record) = record {
                    save_shadow(&tx, record)?;
                }
                if db::acknowledge(&tx, key, *hlc)? {
                    acknowledged += 1;
                }
            }
            tx.commit()?;
            report.pushed += envelopes.len();

            // Every row re-stamped between read and acknowledge: they go next round
            // rather than spinning here.
            if acknowledged == 0 || rows.len() < BATCH {
                return Ok(());
            }
        }
    }

    pub fn pull(&self, conn: &mut Connection, report: &mut Report) -> Result<()> {
        let mut cursor = db::get_state(conn, CURSOR)?;
        let mut adopted_ids = Vec::new();
        loop {
            let page = self.transport.pull(cursor.as_deref(), BATCH)?;
            let tx = conn.transaction()?;
            for env in &page.records {
                report.pulled += 1;
                // This device's own write: its shadow already says at least as much.
                if env.writer == self.device {
                    continue;
                }
                let Ok(remote) = self.codec.decode(&env.doc_id, &env.payload) else {
                    report.undecodable += 1;
                    tracing::warn!(doc = %env.doc_id, "skipping a sync record that will not decode");
                    continue;
                };
                if !local::KINDS.contains(&remote.kind.as_str()) {
                    report.undecodable += 1;
                    continue;
                }
                db::observe_clock(&tx, remote.max_clock())?;
                if device_local(&remote.kind, &remote.ident) {
                    continue;
                }
                if let Some(old) = local::adopt(&tx, &remote)? {
                    report.adopted += 1;
                    adopted_ids.push(old);
                }
                match self.apply_remote(&tx, &remote)? {
                    Applied::Done => report.applied += 1,
                    Applied::Parked => report.parked += 1,
                }
            }
            if let Some(c) = &page.cursor {
                db::set_state(&tx, CURSOR, c)?;
            }
            tx.commit()?;
            cursor = page.cursor;
            if !page.more {
                break;
            }
        }

        // A local provider or profile that took a remote id leaves outbox rows naming its
        // old id. Drop them and enqueue everything again under the new one; the push
        // diffs against the shadow, so only what really differs is sent.
        if !adopted_ids.is_empty() && db::is_enabled(conn)? {
            for old in &adopted_ids {
                conn.execute("DELETE FROM sync_outbox WHERE instr(ident, ?1) > 0", [old])?;
            }
            db::enable(conn)?;
        }
        Ok(())
    }

    /// Merge one remote record into this device and write the result.
    fn apply_remote(&self, tx: &Connection, remote: &Record) -> Result<Applied> {
        let shadow = load(tx, "sync_shadow", &remote.key)?;
        let pending = load(tx, "sync_pending", &remote.key)?;
        let known = match (shadow.as_ref(), pending.as_ref()) {
            (Some(s), Some(p)) => Some(s.merge(p)),
            (Some(s), None) => Some(s.clone()),
            (None, Some(p)) => Some(p.clone()),
            (None, None) => None,
        };
        let merged = known.map_or_else(|| remote.clone(), |k| k.merge(remote));

        // An unpushed local change to the same record: fold it in, so the row keeps any
        // field this device changed more recently.
        let outbox_hlc: Option<i64> = tx
            .query_row(
                "SELECT hlc FROM sync_outbox WHERE sync_key = ?1",
                [&remote.key],
                |r| r.get(0),
            )
            .optional()?;
        let target = match outbox_hlc {
            Some(hlc) => {
                let fields = local::read(tx, &remote.kind, &remote.ident)?;
                let mine = Record::capture(
                    shadow.as_ref(),
                    &remote.key,
                    &remote.kind,
                    &remote.ident,
                    fields.as_ref(),
                    hlc,
                    &self.device,
                );
                merged.merge(&mine)
            }
            None => merged.clone(),
        };

        db::set_state(tx, APPLYING, "1")?;
        let outcome = local::write(
            tx,
            &remote.kind,
            &remote.ident,
            target.live_fields().as_ref(),
            now(),
        );
        db::clear_state(tx, APPLYING)?;

        match outcome? {
            Applied::Done => {
                save_shadow(tx, &merged)?;
                tx.execute(
                    "DELETE FROM sync_pending WHERE sync_key = ?1",
                    [&remote.key],
                )?;
                Ok(Applied::Done)
            }
            Applied::Parked => {
                tx.execute(
                    "INSERT INTO sync_pending (sync_key, record, parked_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT (sync_key) DO UPDATE SET record = excluded.record",
                    params![merged.key, serde_json::to_string(&merged)?, now()],
                )?;
                Ok(Applied::Parked)
            }
        }
    }

    /// Try every parked record again. Call after an import; returns how many applied.
    pub fn retry_pending(&self, conn: &mut Connection) -> Result<usize> {
        let parked: Vec<String> = conn
            .prepare("SELECT record FROM sync_pending")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        if parked.is_empty() {
            return Ok(0);
        }
        let tx = conn.transaction()?;
        let mut applied = 0;
        for raw in parked {
            let record: Record = serde_json::from_str(&raw)?;
            if self.apply_remote(&tx, &record)? == Applied::Done {
                applied += 1;
            }
        }
        tx.commit()?;
        Ok(applied)
    }

    /// Fetch one record straight from the server and apply it, ahead of the next pull.
    ///
    /// Opening a film pulls its progress this way before offering Resume / Start over, so
    /// stopping on the TV and opening the same episode on the phone resumes at the right
    /// second (§3.6). Returns whether anything was applied.
    pub fn refresh_one(&self, conn: &mut Connection, key: &str) -> Result<bool> {
        let mut merged: Option<Record> = None;
        for env in self.transport.get(&self.codec.doc_id(key))? {
            if env.writer == self.device {
                continue;
            }
            let Ok(copy) = self.codec.decode(&env.doc_id, &env.payload) else {
                continue;
            };
            merged = Some(match merged {
                Some(m) => m.merge(&copy),
                None => copy,
            });
        }
        let Some(remote) = merged else {
            return Ok(false);
        };
        let tx = conn.transaction()?;
        db::observe_clock(&tx, remote.max_clock())?;
        let outcome = self.apply_remote(&tx, &remote)?;
        tx.commit()?;
        Ok(outcome == Applied::Done)
    }
}
