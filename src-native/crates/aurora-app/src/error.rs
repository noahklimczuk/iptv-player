use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Core(#[from] aurora_core::CoreError),
    #[error(transparent)]
    Db(#[from] aurora_db::DbError),
    #[error(transparent)]
    Player(#[from] aurora_player::PlayerError),
    /// A newer tune replaced this one before it finished.
    ///
    /// Not a failure the viewer should be told about: they pressed a second button,
    /// and the answer to the first request is simply no longer interesting. The UI
    /// recognises this message and stays quiet.
    #[error("superseded by a newer request")]
    Superseded,
    #[error("{0}")]
    Other(String),
}

/// Tauri needs a serializable error. Credentials must never reach this string
/// (README C10) — the services redact before constructing one.
impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
