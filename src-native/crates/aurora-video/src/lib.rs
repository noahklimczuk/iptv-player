//! Aurora's video engine (docs/PLAYER.md §1).
//!
//! [`Engine`] is the handle: it owns a thread, the thread owns mpv, and everything the
//! rest of the app wants from mpv goes through the handle. [`Driver`] is the seam the
//! thread talks to mpv through, so the engine's rules are tested with
//! [`fake::FakeMpv`] on any machine.

pub mod driver;
pub mod engine;
pub mod fake;
pub mod frame;
#[cfg(windows)]
pub mod libmpv;
pub mod model;
pub mod none;
pub mod request;
pub mod stall;
#[cfg(windows)]
pub mod surface;

pub use driver::{Driver, Event, Node};
pub use engine::{seek_target, Engine, Seek, VideoError};
pub use frame::Frame;
pub use model::{Aspect, Report, Stats, Status, Track, TrackKind};
pub use request::{Request, Timeshift};
