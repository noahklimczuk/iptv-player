//! The one error type Swift sees.
//!
//! Every variant carries a sentence meant for a person, because that is what the
//! Windows app's errors are: `AppError::Other` strings are written to be shown, and the
//! shared crates already put the provider's own words (`NetFailure::message`) in theirs.
//! Swift gets the category as the enum case, so it can choose an icon or a retry
//! button, and the sentence to put under it.
//!
//! Credentials never reach these strings (README C10). The shared crates redact before
//! they build a message, and nothing here formats a URL or a password into one.

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum AuroraError {
    /// The provider or the network refused: a wrong password, a panel that is down, a
    /// timeout.
    #[error("{message}")]
    Network { message: String },
    /// The library file could not be read or written.
    #[error("{message}")]
    Database { message: String },
    /// The Keychain refused, or a provider's password is missing from it.
    #[error("{message}")]
    Credentials { message: String },
    /// The thing asked for is not in the library: a channel id from a stale list, a
    /// provider that was removed.
    #[error("{message}")]
    NotFound { message: String },
    /// The parental gate said no (`parental.rs`). `refusal` says what would open it:
    /// the parental PIN for a lock, the day's time or a guarded change, another profile
    /// for something over this one's limit. The message is Windows' own (`AppError`).
    #[error("{message}")]
    Parental { refusal: Refusal, message: String },
    /// Everything else, already worded for the viewer.
    #[error("{message}")]
    Other { message: String },
}

/// Why the parental gate refused, as Windows' four `AppError` variants for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Refusal {
    /// Over the age limit of the profile watching, or an adult category it hides. The
    /// parental PIN does not open it; another profile might.
    NotAllowed,
    /// A channel or category a parent locked. The PIN opens it.
    Locked,
    /// Today's viewing time for the profile watching is used up. The PIN gives more.
    TimeUp,
    /// A change to parental controls, profiles or Settings that needs the PIN.
    PinRequired,
}

impl Refusal {
    /// Windows' words for it, `aurora-app/src/error.rs`, checked by `mirrors.rs`.
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::NotAllowed => "This profile can't watch that. Switch to a profile that can.",
            Self::Locked => "This is locked. Enter the parental PIN to watch it.",
            Self::TimeUp => {
                "Today's viewing time is used up. Enter the parental PIN to keep watching."
            }
            Self::PinRequired => "Enter the parental PIN to change this.",
        }
    }
}

impl From<Refusal> for AuroraError {
    fn from(refusal: Refusal) -> Self {
        Self::Parental {
            refusal,
            message: refusal.message().to_string(),
        }
    }
}

pub type Result<T> = std::result::Result<T, AuroraError>;

impl AuroraError {
    pub(crate) fn other(message: impl Into<String>) -> Self {
        Self::Other {
            message: message.into(),
        }
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound {
            message: message.into(),
        }
    }

    pub(crate) fn credentials(message: impl Into<String>) -> Self {
        Self::Credentials {
            message: message.into(),
        }
    }
}

impl From<aurora_db::DbError> for AuroraError {
    fn from(e: aurora_db::DbError) -> Self {
        Self::Database {
            message: e.to_string(),
        }
    }
}

impl From<aurora_db::rusqlite::Error> for AuroraError {
    fn from(e: aurora_db::rusqlite::Error) -> Self {
        aurora_db::DbError::from(e).into()
    }
}

impl From<aurora_core::CoreError> for AuroraError {
    fn from(e: aurora_core::CoreError) -> Self {
        Self::other(e.to_string())
    }
}

impl From<aurora_core::neterr::NetFailure> for AuroraError {
    /// The same two parts the Windows app shows, joined the same way
    /// (`aurora-app/src/providers.rs`, `refresh_provider`).
    fn from(e: aurora_core::neterr::NetFailure) -> Self {
        Self::Network {
            message: format!("{}: {}", e.message, e.cause),
        }
    }
}

impl From<aurora_ingest::credentials::CredentialError> for AuroraError {
    fn from(e: aurora_ingest::credentials::CredentialError) -> Self {
        Self::credentials(e.to_string())
    }
}
