//! Cross-device sync for Aurora (`docs/APPLE_PORT_AND_SYNC.md` §3).
//!
//! The local database stays the source of truth on every device. Sync is a background
//! reconciliation on top of it:
//!
//! - **Capture** is in `aurora-db` (migration 18): triggers put every change to synced
//!   data in `sync_outbox`, in the transaction that made it.
//! - [`record`] is the unit that travels: one favourite, one film's progress, one
//!   profile, with a hybrid-logical clock per field, and the merge rule for two copies.
//! - [`local`] reads a record out of this device's tables and writes one back, turning
//!   device-independent keys (`mv:{provider}:{stream}`) into local row ids and parking
//!   what cannot be resolved yet.
//! - [`engine`] pushes the outbox and pulls what other devices wrote, through a
//!   [`transport::SyncTransport`] and a [`codec::Codec`].
//!
//! The transport and codec are traits so the merge rules can be tested as ordinary unit
//! tests -- several simulated devices, offline edits, skewed clocks -- against
//! [`transport::MemoryTransport`] with no network.

pub mod codec;
pub mod engine;
pub mod error;
pub mod local;
pub mod record;
pub mod transport;
pub mod vault;

pub use engine::{Engine, Report};
pub use error::{Result, SyncError};
pub use record::Record;
