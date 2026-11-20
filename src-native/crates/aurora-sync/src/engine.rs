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

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use aurora_core::sync_settings;
use aurora_db::repo::sync as db;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::codec::Codec;
use crate::error::Result;
use crate::local::{self, Applied};
use crate::record::{Fields, Record};
use crate::secrets::{self, SharedSecrets};
use crate::transport::{Envelope, SyncTransport};

/// Firestore's commit limit, and a sensible page.
pub const BATCH: usize = 500;

/// Where the pull cursor is kept in `sync_state`.
pub const CURSOR: &str = "cursor";

/// How many skipped records one round fetches again. Each costs a read or two, and a
/// household that skipped thousands should spend a few rounds on them, not one day's
/// quota in one go.
pub const REREAD_PER_ROUND: usize = 50;

/// What this build can read: the record format and every kind of record it knows.
/// An engine with somewhere to keep secrets reads one kind more ([`Engine::reader`]).
///
/// A record that would not decode is noted under this (`aurora_db::repo::sync::
/// note_unreadable`). An update that reads more -- a new record version, a new kind --
/// has a different answer, and that difference is what makes it fetch the record again
/// (§3.8). The same build asking again would only be skipped again.
pub fn reader() -> String {
    format!(
        "v{}:{}",
        crate::vault::RECORD_VERSION,
        local::KINDS.join(",")
    )
}
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
    /// Where provider passwords and API keys live on this device, when the host has
    /// somewhere to put them (`secrets`). Without one, those records are unreadable here.
    secrets: Option<Arc<dyn SharedSecrets>>,
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
            secrets: None,
        }
    }

    /// Give the engine the host's credential store, so it can sync sign-ins and keys
    /// (§3.4) -- still only while the household has [`secrets::SHARE_KEY`] on.
    pub fn with_secrets(mut self, secrets: Option<Arc<dyn SharedSecrets>>) -> Self {
        self.secrets = secrets;
        self
    }

    /// What this engine can read: [`reader`], and secrets when it has a store for them.
    /// Gaining a store changes the answer, which is what fetches the secrets it skipped.
    pub fn reader(&self) -> String {
        match self.secrets {
            Some(_) => format!("{},{}", reader(), secrets::KIND),
            None => reader(),
        }
    }

    /// Whether records of this kind can be applied here.
    fn understands(&self, kind: &str) -> bool {
        local::KINDS.contains(&kind) || (kind == secrets::KIND && self.secrets.is_some())
    }

    /// Queue the sign-ins and keys that changed since the last round, when the household
    /// shares them.
    fn capture_secrets(&self, conn: &Connection) -> Result<()> {
        if let Some(store) = &self.secrets {
            if db::is_enabled(conn)? && secrets::sharing(conn)? {
                secrets::capture(conn, store.as_ref(), &self.codec)?;
            }
        }
        Ok(())
    }

    /// A secret's current value, as record fields, or `None` when this device has none.
    fn secret_fields(&self, conn: &Connection, ident: &Value) -> Result<Option<Fields>> {
        let Some(store) = &self.secrets else {
            return Ok(None);
        };
        let name = secrets::name_of(ident)?;
        Ok(store
            .list(conn)
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| Fields::from([("value".to_string(), Value::String(v))])))
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
        self.capture_secrets(conn)?;
        self.push(conn, &mut report)?;
        Ok(report)
    }

    /// One round: push, pull, retry parked records, and read again what an older build
    /// skipped.
    pub fn sync(&self, conn: &mut Connection) -> Result<Report> {
        let mut report = Report::default();
        self.capture_secrets(conn)?;
        self.push(conn, &mut report)?;
        self.pull(conn, &mut report)?;
        report.applied += self.retry_pending(conn)?;
        report.applied += self.reread_unreadable(conn)?;
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
                // A secret queued before sharing was turned off goes nowhere.
                let secret = row.kind == secrets::KIND;
                if device_local(&row.kind, &ident)
                    || (secret && (self.secrets.is_none() || !secrets::sharing(conn)?))
                {
                    done.push((row.sync_key.clone(), row.hlc, None));
                    continue;
                }
                let shadow = load(conn, "sync_shadow", &row.sync_key)?;
                let fields = if secret {
                    self.secret_fields(conn, &ident)?
                } else {
                    local::read(conn, &row.kind, &ident)?
                };
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
                    save_shadow(&tx, &secrets::storable(&self.codec, record))?;
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
        let reader = self.reader();
        loop {
            let page = self.transport.pull(cursor.as_deref(), BATCH)?;
            let tx = conn.transaction()?;
            for env in &page.records {
                report.pulled += 1;
                // This device's own write: its shadow already says at least as much.
                if env.writer == self.device {
                    continue;
                }
                // Skipped, and noted: the cursor moves past it, so the note is the only
                // way an update that can read it will know to ask for it again.
                let Ok(remote) = self.codec.decode(&env.doc_id, &env.payload) else {
                    report.undecodable += 1;
                    db::note_unreadable(&tx, &env.doc_id, &reader, now())?;
                    tracing::warn!(doc = %env.doc_id, "skipping a sync record that will not decode");
                    continue;
                };
                if !self.understands(&remote.kind) {
                    report.undecodable += 1;
                    db::note_unreadable(&tx, &env.doc_id, &reader, now())?;
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
        if remote.kind == secrets::KIND {
            return self.apply_secret(tx, remote);
        }
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

    /// A sign-in or key from another device: merged on the clocks like any record, kept in
    /// the credential store rather than a table, and never written to SQLite in the clear.
    fn apply_secret(&self, tx: &Connection, remote: &Record) -> Result<Applied> {
        let Some(store) = &self.secrets else {
            return Ok(Applied::Done);
        };
        let name = secrets::name_of(&remote.ident)?.to_string();
        let shadow = load(tx, "sync_shadow", &remote.key)?;
        let pending = load(tx, "sync_pending", &remote.key)?
            .map(|p| secrets::unseal(&self.codec, p))
            .transpose()?;
        let known = match (shadow.as_ref(), pending.as_ref()) {
            (Some(s), Some(p)) => Some(s.merge(p)),
            (Some(s), None) => Some(s.clone()),
            (None, Some(p)) => Some(p.clone()),
            (None, None) => None,
        };
        let merged = known.map_or_else(|| remote.clone(), |k| k.merge(remote));

        // What this device holds already is a fingerprint in the shadow, and a change made
        // here and not sent yet is in the outbox: either way there is nothing to store.
        let unsent_here: Option<i64> = tx
            .query_row(
                "SELECT hlc FROM sync_outbox WHERE sync_key = ?1",
                [&remote.key],
                |r| r.get(0),
            )
            .optional()?;
        let newest = merged.max_clock();
        let mine_is_newer = unsent_here.is_some_and(|hlc| hlc > newest);
        let value = if merged.alive.value {
            secrets::plain_value(&merged)
        } else {
            None
        };
        let arrived = !merged.alive.value || value.is_some();
        if mine_is_newer || !arrived {
            save_shadow(tx, &secrets::storable(&self.codec, &merged))?;
            tx.execute(
                "DELETE FROM sync_pending WHERE sync_key = ?1",
                [&remote.key],
            )?;
            return Ok(Applied::Done);
        }

        db::set_state(tx, APPLYING, "1")?;
        let stored = store.store(tx, &name, value);
        db::clear_state(tx, APPLYING)?;
        if !stored? {
            tx.execute(
                "INSERT INTO sync_pending (sync_key, record, parked_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (sync_key) DO UPDATE SET record = excluded.record",
                params![
                    merged.key,
                    serde_json::to_string(&secrets::seal(&self.codec, &merged)?)?,
                    now()
                ],
            )?;
            return Ok(Applied::Parked);
        }
        let fp = value.map(|v| secrets::fingerprint(&self.codec, &name, v));
        secrets::note_seen(tx, &name, fp.as_deref())?;
        save_shadow(tx, &secrets::storable(&self.codec, &merged))?;
        tx.execute(
            "DELETE FROM sync_pending WHERE sync_key = ?1",
            [&remote.key],
        )?;
        Ok(Applied::Done)
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
            let record: Record = secrets::unseal(&self.codec, serde_json::from_str(&raw)?)?;
            if self.apply_remote(&tx, &record)? == Applied::Done {
                applied += 1;
            }
        }
        tx.commit()?;
        Ok(applied)
    }

    /// Fetch again the records an older build skipped, now that this one may read them
    /// (§3.8). Returns how many applied.
    ///
    /// Only records noted by a different [`reader`]: this build failing on its own notes
    /// again would cost a read each round for nothing. A record this build cannot read
    /// either becomes its note, and one gone from the server is forgotten.
    pub fn reread_unreadable(&self, conn: &mut Connection) -> Result<usize> {
        let reader = self.reader();
        let ids = db::unreadable_by_others(conn, &reader, REREAD_PER_ROUND)?;
        let mut applied = 0;
        for doc_id in ids {
            let mut merged: Option<Record> = None;
            let mut unreadable = false;
            for env in self.transport.get(&doc_id)? {
                if env.writer == self.device {
                    continue;
                }
                match self.codec.decode(&env.doc_id, &env.payload) {
                    Ok(copy) if self.understands(&copy.kind) => {
                        merged = Some(match merged {
                            Some(m) => m.merge(&copy),
                            None => copy,
                        });
                    }
                    _ => unreadable = true,
                }
            }
            let tx = conn.transaction()?;
            if unreadable {
                db::note_unreadable(&tx, &doc_id, &reader, now())?;
            } else {
                db::forget_unreadable(&tx, &doc_id)?;
            }
            if let Some(remote) = merged {
                db::observe_clock(&tx, remote.max_clock())?;
                if !device_local(&remote.kind, &remote.ident)
                    && self.apply_remote(&tx, &remote)? == Applied::Done
                {
                    applied += 1;
                }
            }
            tx.commit()?;
        }
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
