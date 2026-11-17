//! How a record becomes what the server stores, and back.
//!
//! The server only ever sees a document id and opaque bytes. [`PlainCodec`] makes those
//! the record key and its JSON, which is what the in-memory tests use. The end-to-end
//! encrypted codec (§3.5) replaces the id with an HMAC of the key and the bytes with an
//! AEAD ciphertext, behind the same trait, so the engine does not change.

use crate::error::{Result, SyncError};
use crate::record::Record;

pub trait Codec: Send + Sync {
    /// The server-side id of the record with this key.
    fn doc_id(&self, key: &str) -> String;
    fn encode(&self, record: &Record) -> Result<Vec<u8>>;
    /// Decode bytes stored under `doc_id`. An error here means "skip this record",
    /// never "stop syncing" (§3.8).
    fn decode(&self, doc_id: &str, bytes: &[u8]) -> Result<Record>;
}

/// No encryption: the key is the id and the record is plain JSON. For tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct PlainCodec;

impl Codec for PlainCodec {
    fn doc_id(&self, key: &str) -> String {
        key.to_string()
    }

    fn encode(&self, record: &Record) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(record)?)
    }

    fn decode(&self, doc_id: &str, bytes: &[u8]) -> Result<Record> {
        let record: Record = serde_json::from_slice(bytes).map_err(|_| SyncError::Undecodable)?;
        if record.key != doc_id {
            return Err(SyncError::Undecodable);
        }
        Ok(record)
    }
}
