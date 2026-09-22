use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlayerError {
    #[error("libmpv could not be initialised: {0}")]
    Init(String),
    #[error("mpv command failed: {0}")]
    Command(String),
    #[error("no media loaded")]
    NoMedia,
}

/// The playback-facing name for the shared taxonomy in `aurora_core::neterr`.
/// Kept as an alias so the IPC contract (`shared/ipc.ts`) is unchanged.
pub use aurora_core::neterr::{ErrorAction, ErrorCode, NetFailure as PlaybackError};
