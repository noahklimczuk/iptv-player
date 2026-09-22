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
    pub http: Arc<HttpClient>,
    /// Provider passwords. Never touches the database (README C10).
    pub credentials: Arc<dyn CredentialStore>,
    /// The recording scheduler. Holds its own handle on `db`.
    pub dvr: Arc<crate::dvr::Dvr>,
    /// Downloaded posters and backdrops. Deletable at any time — the library stores
    /// remote URLs, so this is only an accelerator.
    pub artwork: Arc<artwork::Cache>,
    pub data_dir: PathBuf,
}

impl Services {
    pub fn new(data_dir: PathBuf) -> Result<Self, crate::AppError> {
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| crate::AppError::Other(format!("cannot create data dir: {e}")))?;
        let db = aurora_db::open(data_dir.join("library.db"))?;

        if !aurora_db::integrity_check(&db)? {
            tracing::error!("database failed its integrity check");
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

        Ok(Self {
            artwork: Arc::new(artwork::Cache::new(data_dir.join("artwork"))),
            dvr: Arc::new(
                crate::dvr::Dvr::new(Arc::clone(&db), Arc::new(StreamRecorder), folder)
                    .with_max_concurrent(max_concurrent),
            ),
            db,
            player: Arc::new(Mutex::new(create_backend())),
            http: Arc::new(http),
            credentials: Arc::from(default_store()),
            data_dir,
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
