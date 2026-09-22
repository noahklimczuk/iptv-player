use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("malformed playlist at line {line}: {reason}")]
    Playlist { line: usize, reason: String },

    #[error("malformed XMLTV: {0}")]
    Xmltv(String),

    #[error("provider returned an unexpected payload: {0}")]
    Provider(String),

    #[error("invalid rule pattern {pattern:?}: {reason}")]
    Rule { pattern: String, reason: String },

    #[error(transparent)]
    Xml(#[from] quick_xml::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, CoreError>;
