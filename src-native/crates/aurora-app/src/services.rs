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
    /// Multi-view (README §7.4). Holds one backend per open tile, so it is empty — and
    /// costs nothing — until a mosaic is opened.
    pub mosaic: Arc<crate::mosaic::Mosaic>,
    /// Picture-in-picture (README §6.2). The *same* player in a corner rather than a
    /// second one, so unlike a mosaic tile it opens no connection and cannot be refused.
    pub pip: Arc<crate::pip::Pip>,
    /// Downloaded posters and backdrops. Deletable at any time — the library stores
    /// remote URLs, so this is only an accelerator.
    pub artwork: Arc<artwork::Cache>,
    /// Artwork being fetched right now, so two screens asking for the same poster in
    /// the same second cost one download rather than two.
    pub warming: Arc<Mutex<std::collections::HashSet<String>>>,
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

        // Two maintenance passes used to run here, before the window existed:
        // reclassifying the library for the filters, and forgetting metadata answers
        // recorded under an older query. Both are version-gated, so an ordinary launch
        // paid a settings read for each — but the launch *after* an upgrade that bumped
        // either version paid for a scan and an update of every channel, film and show.
        // On the library this was measured against that is some 39,000 rows, and it
        // happened with no window on screen and nothing to say why.
        //
        // They run on a thread after setup now (`main.rs`), which is where work nobody
        // is waiting for belongs. See `catch_up`.
        let db = db;

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

        let dvr = Arc::new(
            crate::dvr::Dvr::new(Arc::clone(&db), Arc::new(StreamRecorder), folder)
                .with_max_concurrent(max_concurrent),
        );

        // One instance per tile, made on demand. `create_backend` rather than a clone of
        // the main player, because each tile needs its own mpv and its own child surface
        // — which is also README §6.1's shape: one long-lived instance for the main
        // surface, short-lived ones for the tiles.
        let mosaic = Arc::new(crate::mosaic::Mosaic::new(
            Arc::clone(&db),
            Arc::clone(&dvr),
            Arc::new(create_backend),
            data_dir.clone(),
            Arc::clone(&player),
        ));

        let pip = Arc::new(crate::pip::Pip::new(Arc::clone(&player)));

        Ok(Self {
            pip,
            playback: Arc::new(crate::playback::Playback::new(
                Arc::clone(&db),
                Arc::clone(&player),
                data_dir.clone(),
            )),
            mosaic,
            artwork: Arc::new(artwork::Cache::new(data_dir.join("artwork"))),
            warming: Arc::new(Mutex::new(std::collections::HashSet::new())),
            updates: Arc::new(crate::updates::Downloads::default()),
            dvr,
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
