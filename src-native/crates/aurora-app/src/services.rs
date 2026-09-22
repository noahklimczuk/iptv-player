//! Service container shared by every command handler.

use std::path::PathBuf;
use std::sync::Arc;

use aurora_db::rusqlite::Connection;
use aurora_player::{create_backend, PlayerBackend};
use parking_lot::Mutex;

pub struct Services {
    /// Single writer connection. Reads go through the same lock for now; the read pool
    /// is a Phase 11 optimisation (docs/ROADMAP.md).
    pub db: Arc<Mutex<Connection>>,
    pub player: Arc<Mutex<Box<dyn PlayerBackend>>>,
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

        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            player: Arc::new(Mutex::new(create_backend())),
            data_dir,
        })
    }
}
