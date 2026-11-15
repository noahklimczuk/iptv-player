//! Playback abstraction.
//!
//! `PlayerBackend` is the seam between the app and libmpv (docs/DECISIONS.md D3).
//! `NullBackend` compiles everywhere and drives tests and the Linux dev server;
//! `MpvBackend` is Windows-only and is what ships.

pub mod backend;
pub mod error;
pub mod frames;
pub mod state;
pub mod watchdog;

#[cfg(windows)]
pub mod mpv;

pub use backend::{Engine, NullBackend, PlayerBackend};
pub use error::{PlaybackError, PlayerError};
pub use frames::{Frame, FrameSource};
pub use state::{Aspect, PlayerState, PlayerStatus, Track, TrackKind};
pub use watchdog::{Phase, Verdict, Watchdog};

/// A silent second player on one local file, for the seek preview's frames.
///
/// Local files only: it opens what it is given, and given a provider's URL it would
/// spend the line's one connection on a thumbnail. `None` where there is no libmpv, or
/// the file will not open.
pub fn open_file_frames(path: &std::path::Path) -> Option<Box<dyn FrameSource>> {
    #[cfg(windows)]
    {
        match mpv::FileFrames::open(path) {
            Ok(f) => return Some(Box::new(f)),
            Err(e) => tracing::warn!("no seek preview frames for {}: {e}", path.display()),
        }
    }
    let _ = path;
    None
}

/// Construct the best backend for this platform.
pub fn create_backend() -> Box<dyn PlayerBackend> {
    #[cfg(windows)]
    {
        match mpv::MpvBackend::new() {
            Ok(b) => {
                // Logged on success as well as failure. A log that only ever mentions
                // libmpv when it breaks cannot distinguish "it loaded" from "this
                // build predates the check", which is exactly the question someone
                // staring at an empty window needs answered.
                let engine = b.engine();
                tracing::info!(
                    version = engine.version.as_deref().unwrap_or("unknown"),
                    "video engine: libmpv"
                );
                return Box::new(b);
            }
            Err(e) => {
                tracing::error!("libmpv unavailable, falling back to null backend: {e}");
            }
        }
    }
    tracing::info!("video engine: none — this build decodes no picture");
    Box::new(NullBackend::default())
}
