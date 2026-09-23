//! Aurora TV core domain logic.
//!
//! Pure, platform-independent, I/O-free. Everything here compiles and tests on any host,
//! which is what keeps the majority of a Windows-only application verifiable in CI.
//! See `docs/DECISIONS.md` D2.

pub mod catchup;
pub mod classify;
pub mod dvr;
pub mod epg_match;
pub mod error;
pub mod fsname;
pub mod lang;
pub mod m3u;
pub mod markers;
pub mod model;
pub mod neterr;
pub mod parental;
pub mod pin;
pub mod rules;
pub mod series;
pub mod sources;
pub mod timeshift;
pub mod title;
pub mod tmdb;
pub mod version;
pub mod xmltv;
pub mod xtream;

pub use error::{CoreError, Result};
