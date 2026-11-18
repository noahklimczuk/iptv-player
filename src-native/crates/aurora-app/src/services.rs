//! Service container shared by every command handler.

use std::path::PathBuf;
use std::sync::Arc;

use aurora_db::rusqlite::Connection;
use aurora_ingest::artwork;
use aurora_ingest::credentials::{default_store, CredentialStore};
use aurora_ingest::http::{HttpClient, HttpConfig};
use aurora_ingest::recorder::StreamRecorder;
use parking_lot::Mutex;

pub struct Services {
    /// Single writer connection. Reads go through the same lock for now; the read pool
    /// is a Phase 11 optimisation (docs/ROADMAP.md).
    pub db: Arc<crate::dblock::TimedMutex<Connection>>,
    /// A read-only connection for what the screens ask for (`aurora_db::open_reader`).
    ///
    /// Not blocked by `db`: a refresh or a metadata pass writing through the writer no
    /// longer makes every page wait for it. Only for reads -- it refuses writes. Use
    /// [`Services::read`], which falls back to the writer when there is no reader.
    ///
    /// A few of them, because reads queue behind each other too: Home's build holds
    /// one for half a second on a large library, and the guide's now-and-next asked
    /// at the same moment should not wait for it.
    pub readers: Arc<Vec<crate::dblock::TimedMutex<Connection>>>,
    /// The player: what plays, where the picture goes, the sleep timer and the seek
    /// preview (docs/PLAYER.md §2). PiP is this same player in a corner.
    pub player: Arc<crate::play::player::Player>,
    pub http: Arc<HttpClient>,
    /// Provider passwords. Never touches the database (README C10).
    pub credentials: Arc<dyn CredentialStore>,
    /// The recording scheduler. Holds its own handle on `db`.
    pub dvr: Arc<crate::dvr::Dvr>,
    /// Multi-view (README §7.4). Holds one backend per open tile, so it is empty — and
    /// costs nothing — until a mosaic is opened.
    pub mosaic: Arc<crate::mosaic::Mosaic>,
    /// Downloaded posters and backdrops. Deletable at any time — the library stores
    /// remote URLs, so this is only an accelerator.
    pub artwork: Arc<artwork::Cache>,
    /// Artwork being fetched right now, so two screens asking for the same poster in
    /// the same second cost one download rather than two.
    pub warming: Arc<Mutex<std::collections::HashSet<String>>>,
    /// The update installer being fetched, if any (docs/DECISIONS.md D17).
    pub updates: Arc<crate::updates::Downloads>,
    /// The home feed last built, so its rails can be paged without rebuilding them
    /// (`crate::home`). Never held while `db` is being locked.
    pub home: Arc<Mutex<Option<crate::home::HomeCache>>>,
    /// Cross-device sync (`crate::sync`). Has its own connection to the library.
    pub sync: Arc<crate::sync::Sync>,
    pub data_dir: PathBuf,
    /// What opening the library took. `Replaced` means the viewer's favourites, watch
    /// progress and recordings metadata are gone and a refresh is needed, which is
    /// worth saying out loud rather than letting them find an empty library.
    pub opened: aurora_db::Opened,
}

/// How many read connections the screens share. Each costs a file handle and its own
/// page cache (`open_reader` sets 64 MiB as a ceiling, filled only as it is used).
const READERS: usize = 3;

impl Services {
    /// The connection for a read: the reader when there is one, else the writer.
    ///
    /// Only for code that never writes. The reader is opened read-only, so a write
    /// through it fails with "attempt to write a readonly database" rather than
    /// silently going anywhere.
    #[track_caller]
    pub fn read(&self) -> crate::dblock::TimedGuard<'_, Connection> {
        if self.readers.is_empty() {
            return self.db.lock();
        }
        // A free one if there is one; otherwise queue on one, spread by thread so
        // concurrent callers do not all pile onto the first.
        for r in self.readers.iter() {
            if let Some(guard) = r.try_lock() {
                return guard;
            }
        }
        let n = self.readers.len();
        let slot = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::thread::current().id().hash(&mut h);
            (h.finish() as usize) % n
        };
        self.readers[slot].lock()
    }

    /// `video` is the engine opened on the app's window ([`crate::play::video::start`]).
    pub fn new(
        data_dir: PathBuf,
        video: crate::play::video::Video,
    ) -> Result<Self, crate::AppError> {
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

        // Nobody has said who is watching yet, and nothing the parental PIN opened last
        // time is still open. Until the picker answers, the strictest profile's limits
        // apply (`aurora_db::repo::parental`). A failure here is logged, not fatal: the
        // gate it leaves is the previous session's, which is a profile somebody chose.
        if let Err(e) = aurora_db::repo::parental::clear_session(&db) {
            tracing::warn!("could not reset who is watching: {e}");
        }

        let http = HttpClient::new(HttpConfig::default())
            .map_err(|e| crate::AppError::Other(e.message))?;

        let db = Arc::new(crate::dblock::TimedMutex::new(db));
        // After the writer, which created and migrated the file. A reader that cannot
        // be opened costs speed, not correctness: reads go through the writer as they
        // always did.
        let readers = (0..READERS)
            .map_while(
                |_| match aurora_db::open_reader(data_dir.join("library.db")) {
                    Ok(conn) => Some(crate::dblock::TimedMutex::new(conn)),
                    Err(e) => {
                        tracing::warn!(
                            "could not open a read connection, reads share the writer: {e}"
                        );
                        None
                    }
                },
            )
            .collect::<Vec<_>>();
        let readers = Arc::new(readers);

        // The recordings folder is configurable because a DVR fills a disk; the
        // default sits beside the library so a portable install stays portable.
        let folder: PathBuf =
            aurora_db::repo::settings::get::<String>(&db.lock(), crate::dvr::FOLDER_KEY)?
                .map(PathBuf::from)
                .unwrap_or_else(|| data_dir.join("Recordings"));
        // No cap is set here on purpose. It was read once, at this point, and on a
        // first run that is before the provider exists — see `Dvr::max_concurrent`,
        // which asks the database per tick instead.
        let credentials: Arc<dyn CredentialStore> = Arc::from(default_store());

        let session = crate::play::session::Session::new(
            video.engine,
            Arc::clone(&db),
            Arc::clone(&credentials),
            data_dir.clone(),
        );
        let player = Arc::new(crate::play::player::Player::new(
            session,
            Arc::clone(&db),
            data_dir.clone(),
            video.surface,
            video.window,
        ));

        let dvr = Arc::new(crate::dvr::Dvr::new(
            Arc::clone(&db),
            Arc::new(StreamRecorder),
            folder,
            Arc::clone(&credentials),
        ));

        // One engine per tile, made on demand, each with its own mpv and its own child
        // surface (docs/PLAYER.md E32): one long-lived instance for the main surface,
        // short-lived ones for the tiles.
        let mosaic = Arc::new(crate::mosaic::Mosaic::new(
            Arc::clone(&db),
            Arc::clone(&dvr),
            crate::play::tile::EngineTile::factory(),
            data_dir.clone(),
            {
                let player = Arc::clone(&player);
                Arc::new(move |width, height| player.sink_under_tiles(width, height))
            },
            Arc::clone(&credentials),
        ));

        let http = Arc::new(http);
        let sync = Arc::new(crate::sync::Sync::new(
            data_dir.join("library.db"),
            &db.lock(),
            Arc::clone(&credentials),
        ));

        Ok(Self {
            mosaic,
            artwork: Arc::new(artwork::Cache::new(data_dir.join("artwork"))),
            warming: Arc::new(Mutex::new(std::collections::HashSet::new())),
            updates: Arc::new(crate::updates::Downloads::default()),
            home: Arc::new(Mutex::new(None)),
            sync,
            dvr,
            db,
            readers,
            player,
            http,
            credentials,
            data_dir,
            opened,
        })
    }
}
