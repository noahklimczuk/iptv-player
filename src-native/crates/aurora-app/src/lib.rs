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
pub mod supervise;
pub mod timeshift;
pub mod updates;
pub mod window;

pub use error::AppError;

/// Where a portable copy keeps its data, or `None` for an installed one (README §13).
///
/// A `portable.txt` beside the exe is the marker. One definition, because two places
/// now need the answer: where the database goes, and whether an installer could
/// replace this copy at all (docs/DECISIONS.md D17).
pub fn portable_dir() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
        .filter(|dir| dir.join("portable.txt").exists())
        .map(|dir| dir.join("data"))
}

/// Wall-clock seconds. One definition, because four copies of it drifting apart is a
/// class of bug nobody finds.
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
