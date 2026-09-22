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
    #[error("{0}")]
    Other(String),
}

/// Tauri needs a serializable error. Credentials must never reach this string
/// (README C10) — the services redact before constructing one.
impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
