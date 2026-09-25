//! Service container shared by every command handler.

use std::path::PathBuf;
use std::sync::Arc;

use aurora_db::rusqlite::Connection;
use aurora_ingest::artwork;
use aurora_ingest::credentials::{default_store, CredentialStore};
use aurora_ingest::http::{HttpClient, HttpConfig};
use aurora_ingest::recorder::StreamRecorder;
use aurora_player::{create_backend, PlayerBackend};
use parking_lot::Mutex;

pub struct Services {
    /// Single writer connection. Reads go through the same lock for now; the read pool
    /// is a Phase 11 optimisation (docs/ROADMAP.md).
    pub db: Arc<Mutex<Connection>>,
    pub player: Arc<Mutex<Box<dyn PlayerBackend>>>,
    /// Owns tuning, failover and the state heartbeat (README §7.14).
    pub playback: Arc<crate::playback::Playback>,
    pub http: Arc<HttpClient>,
    /// Provider passwords. Never touches the database (README C10).
    pub credentials: Arc<dyn CredentialStore>,
    /// The recording scheduler. Holds its own handle on `db`.
    pub dvr: Arc<crate::dvr::Dvr>,
    /// Downloaded posters and backdrops. Deletable at any time — the library stores
    /// remote URLs, so this is only an accelerator.
    pub artwork: Arc<artwork::Cache>,
    /// The update installer being fetched, if any (docs/DECISIONS.md D17).
    pub updates: Arc<crate::updates::Downloads>,
    pub data_dir: PathBuf,
    /// What opening the library took. `Replaced` means the viewer's favourites, watch
    /// progress and recordings metadata are gone and a refresh is needed, which is
    /// worth saying out loud rather than letting them find an empty library.
    pub opened: aurora_db::Opened,
}

impl Services {
    pub fn new(data_dir: PathBuf) -> Result<Self, crate::AppError> {
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| crate::AppError::Other(format!("cannot create data dir: {e}")))?;
        // Not `open`: a corrupt file propagated straight out of here, which Tauri
        // turns into a start-up failure — no window, no dialog, and no console in a
        // release build to say why. The app simply did not launch. `open_or_recover`
        // moves the unusable file aside and starts a fresh one, and what it had to do
        // is kept so Settings can say so rather than the library quietly being empty.
        let (db, opened) = aurora_db::open_or_recover(data_dir.join("library.db"))?;
        if let aurora_db::Opened::Replaced { corrupt_copy } = &opened {
            tracing::error!(
                "the library could not be read and has been started again; the old \
                 file is at {}",
                corrupt_copy.display()
            );
        }

        // A library imported before this build — or before the classifier last changed
        // its mind — has no language or quality on its rows, so the filters would have
        // nothing to read. Sync does this too; this is for the copy already on disk.
        let mut db = db;
        match aurora_db::repo::filtering::reclassify_if_stale(&mut db) {
            Ok(0) => {}
            Ok(n) => tracing::info!("classified {n} library rows for filtering"),
            // Not being able to classify is not a reason to refuse to start: the
            // filters simply hide nothing until the next sync.
            Err(e) => tracing::warn!("could not classify the library: {e}"),
        }

        let http = HttpClient::new(HttpConfig::default())
            .map_err(|e| crate::AppError::Other(e.message))?;

        let db = Arc::new(Mutex::new(db));

        // The recordings folder is configurable because a DVR fills a disk; the
        // default sits beside the library so a portable install stays portable.
        let folder: PathBuf =
            aurora_db::repo::settings::get::<String>(&db.lock(), crate::dvr::FOLDER_KEY)?
                .map(PathBuf::from)
                .unwrap_or_else(|| data_dir.join("Recordings"));
        let max_concurrent =
            max_connections(&db.lock()).unwrap_or(crate::dvr::DEFAULT_MAX_CONCURRENT);

        let player: Arc<Mutex<Box<dyn PlayerBackend>>> = Arc::new(Mutex::new(create_backend()));

        Ok(Self {
            playback: Arc::new(crate::playback::Playback::new(
                Arc::clone(&db),
                Arc::clone(&player),
                data_dir.clone(),
            )),
            artwork: Arc::new(artwork::Cache::new(data_dir.join("artwork"))),
            updates: Arc::new(crate::updates::Downloads::default()),
            dvr: Arc::new(
                crate::dvr::Dvr::new(Arc::clone(&db), Arc::new(StreamRecorder), folder)
                    .with_max_concurrent(max_concurrent),
            ),
            db,
            player,
            http: Arc::new(http),
            credentials: Arc::from(default_store()),
            data_dir,
            opened,
        })
    }
}

/// The tightest connection limit across enabled providers, which is what actually caps
/// simultaneous recordings — exceeding it gets the line cut, not queued.
fn max_connections(db: &Connection) -> Option<usize> {
    db.query_row(
        "SELECT MIN(max_connections) FROM providers WHERE enabled = 1 AND max_connections > 0",
        [],
        |r| r.get::<_, Option<i64>>(0),
    )
    .ok()
    .flatten()
    .map(|n| n.max(1) as usize)
}
