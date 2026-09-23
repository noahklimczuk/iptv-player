//! Aurora TV host: owns the services and exposes them over typed IPC.
//!
//! README §3: the UI never touches the network or the database directly, and playback
//! state is owned here and mirrored to the UI.

pub mod commands;
pub mod dvr;
pub mod error;
pub mod library;
pub mod metadata;
pub mod playback;
pub mod playlist;
pub mod profiles;
pub mod providers;
pub mod services;
pub mod updates;
pub mod window;

pub use error::AppError;

/// Wall-clock seconds. One definition, because four copies of it drifting apart is a
/// class of bug nobody finds.
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
