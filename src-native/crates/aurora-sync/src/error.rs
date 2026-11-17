use thiserror::Error;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error(transparent)]
    Db(#[from] aurora_db::DbError),

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error("a sync record could not be read: {0}")]
    Malformed(String),

    /// The server could not be reached, or refused for a reason a retry may fix.
    /// The outbox keeps everything; nothing is lost.
    #[error("sync is offline: {0}")]
    Offline(String),

    /// The daily free-plan quota is spent (§3.7). Treated as offline until it resets.
    #[error("the sync service's daily quota is used up")]
    QuotaExhausted,

    /// An account call was refused for a reason the person can act on: wrong password,
    /// an email already registered, a weak password. The text is ready to show.
    #[error("{0}")]
    Account(String),

    /// The account's session expired or was revoked. Needs the viewer to sign in.
    #[error("sync needs you to sign in again")]
    Unauthenticated,

    /// A record that will not decode: written by a newer app, or damaged.
    #[error("a sync record could not be decrypted or decoded")]
    Undecodable,

    #[error("the sync service answered unexpectedly: {0}")]
    Protocol(String),
}

impl From<serde_json::Error> for SyncError {
    fn from(e: serde_json::Error) -> Self {
        SyncError::Malformed(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, SyncError>;
