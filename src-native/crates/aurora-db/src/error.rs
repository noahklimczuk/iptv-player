use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error("migration {version} failed: {source}")]
    Migration {
        version: u32,
        #[source]
        source: rusqlite::Error,
    },

    #[error(
        "database is newer than this build (schema v{found}, supported v{supported}) — \
             update Aurora TV or restore a backup"
    )]
    SchemaTooNew { found: u32, supported: u32 },

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, DbError>;
