//! Aurora's shared Rust core, for the Apple app.
//!
//! The Windows app reaches the shared crates through `aurora-app`, a Tauri host. Swift
//! reaches the same crates through this one, over UniFFI. The two hosts are siblings and
//! neither depends on the other: `aurora-app` is not linked here and is not edited by
//! the Apple port (apple/README.md).
//!
//! What lives in this crate is **host glue only**: the handful of steps `aurora-app`
//! performs around the shared crates (minting a credential key before inserting a
//! provider, stamping a refresh, rebuilding a stream address from its stream id). Each
//! one names the `aurora-app` function it mirrors, so when that function changes the
//! counterpart here can be found and checked. Product rules — parsing, matching,
//! failover order, the schema, what mpv is told — stay in the shared crates and are
//! called, never copied.
//!
//! Scope today: the Phase 2 spike of docs/APPLE_PORT_AND_SYNC.md (open a library, add a
//! provider, import it, list its channels, tune one), plus the guide's now/next and
//! schedule, films and series, watch progress, favourite channels, search, catch-up, profiles and My List.

uniffi::setup_scaffolding!();

mod catchup;
mod error;
mod filters;
mod guide;
mod home;
mod library;
mod metadata;
#[cfg(test)]
mod mirrors;
mod mylist;
mod playback;
mod player;
mod playlist;
mod profiles;
mod providers;
mod search;
mod secrets;
mod series;
mod sleep;
mod vod;
mod zap;

use std::path::PathBuf;
use std::sync::Arc;

use aurora_db::rusqlite::Connection;
use aurora_ingest::credentials::CredentialStore;
use aurora_ingest::http::{HttpClient, HttpConfig};
use parking_lot::Mutex;

pub use catchup::CatchupTune;
pub use error::{AuroraError, Result};
pub use filters::{FilterCounts, LibraryFilters, LibraryKind};
pub use guide::{GuideCell, GuideRow, NowNext, Programme};
pub use home::{CatalogItem, Rail, RailItem, RailPage, RailProgress};
pub use library::{Channel, ChannelGroup, FavoriteList};
pub use metadata::{Coverage, Credit, MetadataListener, MetadataReport, MetadataStatus};
pub use mylist::{ItemMarks, ListKind, MyList};
pub use playback::{LiveTune, MpvOption, PlayableSource};
pub use player::{
    AspectChoice, ChapterMark, MarkerKind, MarkerSource, PictureAspect, PlaybackAids, PlayerMenus,
    SeriesPrefs, SkipMarker,
};
pub use profiles::{PinChange, PinOutcome, Profile};
pub use providers::{ImportContent, ImportListener, ImportPhase, ImportReport, Provider};
pub use search::{SearchHit, SearchResults};
pub use secrets::{SecretError, SecretStore};
pub use series::{ListingListener, ListingReport};
pub use sleep::{SleepAfter, SleepPlaying, SleepState};
pub use vod::{
    Browse, BrowseFacets, BrowseSort, Episode, Movie, Series, SeriesResume, Shelf, VodKind, VodTune,
    WatchProgress,
};

/// One open library, and everything needed to fill it and play from it.
///
/// Swift holds one of these for the life of the app. Every method blocks — an import
/// takes minutes — so Swift calls them off the main actor (`Task.detached`), the same way
/// every Windows command runs on Tauri's blocking pool rather than its main thread
/// (docs/ARCHITECTURE.md, Threading).
#[derive(uniffi::Object)]
pub struct AuroraCore {
    /// One connection. The Windows host adds a read pool beside its writer; the spike
    /// does not need one yet, and adding it later changes nothing Swift sees.
    db: Mutex<Connection>,
    /// Shared: the TMDB client keeps its own handle to it.
    http: Arc<HttpClient>,
    credentials: Arc<dyn CredentialStore>,
    data_dir: PathBuf,
    /// The home feed last built (`home.rs`).
    home: Mutex<Option<home::HomeCache>>,
    /// The player's sleep timer (`sleep.rs`).
    sleep: Mutex<sleep::SleepTimer>,
}

#[uniffi::export]
impl AuroraCore {
    /// Open (or create) the library in `data_dir`.
    ///
    /// On Apple `data_dir` is the app's Application Support container, which the system
    /// does not purge. A library that cannot be read is moved aside and started again,
    /// exactly as on Windows (`aurora_db::open_or_recover`).
    #[uniffi::constructor]
    pub fn open(data_dir: String, secrets: Arc<dyn SecretStore>) -> Result<Arc<Self>> {
        let data_dir = PathBuf::from(data_dir);
        std::fs::create_dir_all(&data_dir).map_err(|e| {
            AuroraError::other(format!("Aurora could not create its data folder: {e}"))
        })?;
        let (db, opened) = aurora_db::open_or_recover(data_dir.join("library.db"))?;
        if let aurora_db::Opened::Replaced { corrupt_copy } = &opened {
            tracing::error!(
                "the library could not be read and has been started again; the old \
                 file is at {}",
                corrupt_copy.display()
            );
        }
        let http = Arc::new(HttpClient::new(HttpConfig::default())?);
        Ok(Arc::new(Self {
            db: Mutex::new(db),
            http,
            credentials: Arc::new(secrets::Bridged(secrets)),
            data_dir,
            home: Mutex::new(None),
            sleep: Mutex::new(sleep::SleepTimer::default()),
        }))
    }

    /// Where the library lives. For diagnostics.
    pub fn data_dir(&self) -> String {
        self.data_dir.display().to_string()
    }
}

/// The shared crates' version, for an About screen and bug reports.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Wall-clock seconds, as every timestamp in the library is stored.
pub(crate) fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// A library in a fresh temporary folder, with an in-memory Keychain.
    pub(crate) fn core() -> (Arc<AuroraCore>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let core = AuroraCore::open(
            dir.path().display().to_string(),
            Arc::new(secrets::tests::MemorySecrets::default()),
        )
        .unwrap();
        (core, dir)
    }
}
