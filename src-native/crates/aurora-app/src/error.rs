use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Core(#[from] aurora_core::CoreError),
    #[error(transparent)]
    Db(#[from] aurora_db::DbError),
    #[error(transparent)]
    Video(#[from] aurora_video::VideoError),
    /// A newer tune replaced this one before it finished.
    ///
    /// Not a failure the viewer should be told about: they pressed a second button,
    /// and the answer to the first request is simply no longer interesting. The UI
    /// recognises this message and stays quiet.
    #[error("superseded by a newer request")]
    Superseded,
    /// Over the age limit of the profile watching, or an adult category it hides. The
    /// parental PIN does not open it; another profile might. The UI recognises the text.
    #[error("This profile can't watch that. Switch to a profile that can.")]
    NotAllowed,
    /// A channel or category a parent locked. The UI recognises the text, asks for the
    /// parental PIN, and tries again.
    #[error("This is locked. Enter the parental PIN to watch it.")]
    Locked,
    /// A change to parental controls or profiles while a master PIN is set and has not
    /// been entered. The UI recognises the text and asks for it.
    #[error("Enter the parental PIN to change this.")]
    ParentalPinRequired,
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
