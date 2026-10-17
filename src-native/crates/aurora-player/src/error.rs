use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlayerError {
    #[error("libmpv could not be initialised: {0}")]
    Init(String),
    #[error("mpv command failed: {0}")]
    Command(String),
    #[error("no media loaded")]
    NoMedia,
}

/// The playback-facing name for the shared taxonomy in `aurora_core::neterr`.
/// Kept as an alias so the IPC contract (`shared/ipc.ts`) is unchanged.
pub use aurora_core::neterr::{ErrorAction, ErrorCode, NetFailure as PlaybackError};

/// What a playback failure says, with the credentials taken out of it.
///
/// mpv's own message plus the last few lines of mpv's log, which is where the HTTP
/// status lives: `mpv_error_string` gives "loading failed" and ffmpeg gives "Server
/// returned 403 Forbidden", and only one of those tells the viewer anything. The string
/// this builds goes three places — `tracing::warn!`, `PlaybackError::classify`, and from
/// there into the OSD as the cause line under the error.
///
/// Every part of it goes through `redact` first. Neither site did: `redact` was not
/// referenced once anywhere in `aurora-player`, because it lived in `aurora-ingest`
/// beside `reqwest` and this crate cannot depend on that. ffmpeg's open failures name
/// the URL they failed to open — `Failed to open http://host/user/pass/1234` — at error
/// level, which is above the `warn` filter `LOG_LEVEL` sets, so the line was captured,
/// written to `aurora.log`, and shown on screen. `docs/PRIVACY.md` says that credentials
/// "do not appear in a log" and that "every URL written to the log goes through
/// `http::redact` first"; neither half was true of the player (R-62).
pub fn describe_failure<'a>(message: &str, recent: impl IntoIterator<Item = &'a str>) -> String {
    let context: Vec<String> = recent
        .into_iter()
        .map(aurora_core::redact::redact_in_text)
        .collect();
    let message = aurora_core::redact::redact_in_text(message);
    if context.is_empty() {
        message
    } else {
        format!("{message} [mpv: {}]", context.join(" / "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The URL ffmpeg names when a stream refuses, as this subscription serves it.
    const LEAK: &str =
        "Failed to open http://panel.example/bob/hunter2/1234: Server returned 403 Forbidden";

    #[test]
    fn a_failed_stream_does_not_write_the_password_to_the_log() {
        let described = describe_failure("loading failed", [LEAK]);
        assert!(!described.contains("hunter2"), "{described}");
        assert!(!described.contains("bob"), "{described}");
        // And still says the thing that makes the message worth having.
        assert!(described.contains("403"), "{described}");
    }

    /// The same for the message half, which does not come through the log at all.
    #[test]
    fn the_message_is_redacted_as_well_as_the_context() {
        let described = describe_failure("cannot open http://alice:hunter2@panel.example/x", []);
        assert!(!described.contains("hunter2"), "{described}");
    }

    /// Query-parameter credentials, which is the other form a panel serves.
    #[test]
    fn a_query_parameter_password_is_taken_out_too() {
        let described = describe_failure(
            "loading failed",
            ["http://panel.example/get.php?username=bob&password=hunter2 -> 403"],
        );
        assert!(!described.contains("hunter2"), "{described}");
    }

    /// With nothing captured the message stands alone, which is what the old code did.
    #[test]
    fn no_log_context_means_no_brackets() {
        assert_eq!(describe_failure("loading failed", []), "loading failed");
    }
}
