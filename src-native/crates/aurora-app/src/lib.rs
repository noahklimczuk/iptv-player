//! Aurora TV host: owns the services and exposes them over typed IPC.
//!
//! README §3: the UI never touches the network or the database directly, and playback
//! state is owned here and mirrored to the UI.

pub mod commands;
pub mod dvr;
pub mod error;
pub mod metadata;
pub mod profiles;
pub mod providers;
pub mod services;
pub mod window;

pub use error::AppError;
