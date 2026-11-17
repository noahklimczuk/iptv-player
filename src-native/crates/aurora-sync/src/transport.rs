//! Where records go: the server, behind a trait.
//!
//! The production transport is Cloud Firestore over REST (§3.1). The trait is what lets
//! the merge rules be tested as multi-device simulations with [`MemoryTransport`], and
//! what makes leaving Firebase a new implementation rather than a redesign (§5 risk 4).
//!
//! **Every device writes its own copy of a record.** The server stores one document per
//! (record, writer), and a reader merges every copy it pulls. The server cannot merge --
//! with end-to-end encryption it cannot even read -- so a single shared document per
//! record would be last-*push*-wins: a device pushing its version would replace a newer
//! edit from another device that it had not pulled yet, and that edit would be gone for
//! good. With a copy per writer nothing is ever replaced but a device's own older state,
//! the merge is commutative, and every device converges on the join of all copies in any
//! order, with no transactions or preconditions on the server.

use std::sync::{Arc, Mutex};

use crate::error::{Result, SyncError};

/// One device's copy of a record as the server stores it: the record's opaque id, opaque
/// bytes, and who wrote it. `(doc_id, writer)` is the server-side identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub doc_id: String,
    pub payload: Vec<u8>,
    /// The writing device, in the clear so a device can skip its own echoes cheaply.
    pub writer: String,
}

/// One page of a pull.
#[derive(Debug, Clone, Default)]
pub struct Page {
    pub records: Vec<Envelope>,
    /// Where the next pull starts. Opaque to the engine; stored in `sync_state`.
    pub cursor: Option<String>,
    /// Whether there is more to read after this page.
    pub more: bool,
}

pub trait SyncTransport: Send + Sync {
    /// Write a batch of this device's copies, each replacing the same writer's previous
    /// copy of that record. All-or-nothing: on error, none are assumed written.
    fn push(&self, batch: &[Envelope]) -> Result<()>;

    /// Copies written since `cursor` (from the beginning when `None`), oldest first,
    /// at most `limit` of them.
    fn pull(&self, cursor: Option<&str>, limit: usize) -> Result<Page>;

    /// Every device's copy of one record, for the direct pull before Resume / Start over
    /// (§3.6).
    fn get(&self, doc_id: &str) -> Result<Vec<Envelope>>;
}

#[derive(Debug, Default)]
struct Store {
    /// Every write in server order; a rewrite of a copy moves it to the end, as a new
    /// server timestamp does in Firestore.
    log: Vec<(u64, Envelope)>,
    seq: u64,
    offline: bool,
}

/// An in-memory server shared by simulated devices.
///
/// Clones share one store, the way several devices share one Firestore project.
#[derive(Debug, Clone, Default)]
pub struct MemoryTransport {
    store: Arc<Mutex<Store>>,
}

impl MemoryTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Make every call fail as if the network were down.
    pub fn set_offline(&self, offline: bool) {
        self.store.lock().unwrap().offline = offline;
    }

    /// How many documents the server holds.
    pub fn len(&self) -> usize {
        self.store.lock().unwrap().log.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn check(store: &Store) -> Result<()> {
        if store.offline {
            Err(SyncError::Offline("simulated".into()))
        } else {
            Ok(())
        }
    }
}

impl SyncTransport for MemoryTransport {
    fn push(&self, batch: &[Envelope]) -> Result<()> {
        let mut store = self.store.lock().unwrap();
        Self::check(&store)?;
        for env in batch {
            store
                .log
                .retain(|(_, e)| !(e.doc_id == env.doc_id && e.writer == env.writer));
            store.seq += 1;
            let seq = store.seq;
            store.log.push((seq, env.clone()));
        }
        Ok(())
    }

    fn pull(&self, cursor: Option<&str>, limit: usize) -> Result<Page> {
        let store = self.store.lock().unwrap();
        Self::check(&store)?;
        let after: u64 = cursor.and_then(|c| c.parse().ok()).unwrap_or(0);
        let mut newer = store.log.iter().filter(|(s, _)| *s > after);
        let records: Vec<(u64, Envelope)> = newer.by_ref().take(limit).cloned().collect();
        let more = newer.next().is_some();
        let cursor = records
            .last()
            .map(|(s, _)| s.to_string())
            .or_else(|| cursor.map(str::to_string));
        Ok(Page {
            records: records.into_iter().map(|(_, e)| e).collect(),
            cursor,
            more,
        })
    }

    fn get(&self, doc_id: &str) -> Result<Vec<Envelope>> {
        let store = self.store.lock().unwrap();
        Self::check(&store)?;
        Ok(store
            .log
            .iter()
            .filter(|(_, e)| e.doc_id == doc_id)
            .map(|(_, e)| e.clone())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(id: &str) -> Envelope {
        Envelope {
            doc_id: id.into(),
            payload: id.as_bytes().to_vec(),
            writer: "d".into(),
        }
    }

    #[test]
    fn pulls_page_in_server_order_and_resume_from_the_cursor() {
        let t = MemoryTransport::new();
        t.push(&[env("a"), env("b"), env("c")]).unwrap();
        let p1 = t.pull(None, 2).unwrap();
        assert_eq!(p1.records.len(), 2);
        assert!(p1.more);
        let p2 = t.pull(p1.cursor.as_deref(), 2).unwrap();
        assert_eq!(p2.records[0].doc_id, "c");
        assert!(!p2.more);
        // Nothing new: the cursor stays where it was.
        let p3 = t.pull(p2.cursor.as_deref(), 2).unwrap();
        assert!(p3.records.is_empty());
        assert_eq!(p3.cursor, p2.cursor);
    }

    #[test]
    fn a_rewritten_document_comes_round_again() {
        let t = MemoryTransport::new();
        t.push(&[env("a"), env("b")]).unwrap();
        let seen = t.pull(None, 10).unwrap().cursor;
        t.push(&[env("a")]).unwrap();
        let p = t.pull(seen.as_deref(), 10).unwrap();
        assert_eq!(p.records.len(), 1);
        assert_eq!(p.records[0].doc_id, "a");
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn two_writers_of_one_record_keep_a_copy_each() {
        let t = MemoryTransport::new();
        let mut other = env("a");
        other.writer = "e".into();
        t.push(&[env("a"), other]).unwrap();
        t.push(&[env("a")]).unwrap();
        assert_eq!(t.len(), 2, "a rewrite replaces only the writer's own copy");
        assert_eq!(t.get("a").unwrap().len(), 2);
    }

    #[test]
    fn offline_fails_every_call() {
        let t = MemoryTransport::new();
        t.set_offline(true);
        assert!(matches!(t.push(&[env("a")]), Err(SyncError::Offline(_))));
        assert!(t.pull(None, 1).is_err());
    }
}
