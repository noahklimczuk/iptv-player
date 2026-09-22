//! Human-facing failure classification for anything that talks to a provider.

use serde::{Deserialize, Serialize};

/// A network failure translated for humans (README §17): what happened, the likely
/// cause, and what the user can do about it.
///
/// Shared by playback and ingestion — a 401 from a stream URL and a 401 from
/// `player_api.php` mean the same thing to the person using the app, and should say
/// the same thing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetFailure {
    pub code: ErrorCode,
    pub message: String,
    pub cause: String,
    pub actions: Vec<ErrorAction>,
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    Dns,
    Tls,
    Unauthorized,
    Forbidden,
    NotFound,
    RateLimited,
    ServerError,
    ConnectionLimit,
    Timeout,
    UnsupportedCodec,
    DrmProtected,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorAction {
    Retry,
    TryAnotherSource,
    ReportBroken,
    OpenSettings,
}

impl NetFailure {
    /// Map a raw mpv/network failure onto something a person can act on.
    ///
    /// README §17 requires these to be distinguished rather than collapsed into
    /// "playback failed".
    pub fn classify(raw: &str) -> Self {
        let l = raw.to_ascii_lowercase();

        let (code, message, cause) = if l.contains("401") || l.contains("unauthorized") {
            (
                ErrorCode::Unauthorized,
                "Your provider rejected these credentials",
                "The username or password may have changed, or the line may have expired.",
            )
        } else if l.contains("403") || l.contains("forbidden") {
            (
                ErrorCode::Forbidden,
                "Your provider refused this stream",
                "This often means the line has hit its connection limit, or this channel \
                 is not part of your package.",
            )
        } else if l.contains("404") || l.contains("not found") {
            (
                ErrorCode::NotFound,
                "This stream no longer exists",
                "The provider has probably removed or renumbered it. A library refresh \
                 may fix it.",
            )
        } else if l.contains("429") {
            (
                ErrorCode::RateLimited,
                "Your provider is rate-limiting this connection",
                "Too many requests in a short time. Waiting a moment usually clears it.",
            )
        } else if l.contains("connection limit") || l.contains("max connections") {
            (
                ErrorCode::ConnectionLimit,
                "You're already watching on another device",
                "This subscription allows a limited number of simultaneous streams.",
            )
        } else if l.contains("refused") {
            (
                ErrorCode::Dns,
                "Nothing is listening at that address",
                "The server refused the connection. The address or port may be wrong, \
                 or the provider may be down.",
            )
        } else if l.contains("timed out") || l.contains("timeout") {
            (
                ErrorCode::Timeout,
                "The stream didn't respond in time",
                "The server may be overloaded, or your connection may be unstable.",
            )
        } else if l.contains("getaddrinfo")
            || l.contains("name or service not known")
            || l.contains("dns")
        {
            (
                ErrorCode::Dns,
                "Couldn't reach your provider",
                "The server name didn't resolve. Check your internet connection or DNS.",
            )
        } else if l.contains("ssl") || l.contains("tls") || l.contains("certificate") {
            (
                ErrorCode::Tls,
                "The secure connection failed",
                "The provider's certificate could not be verified.",
            )
        } else if l.contains("5")
            && (l.contains("bad gateway")
                || l.contains("502")
                || l.contains("503")
                || l.contains("500"))
        {
            (
                ErrorCode::ServerError,
                "Your provider's server had an error",
                "This is on their end. It usually resolves by itself.",
            )
        } else if l.contains("encrypted") || l.contains("drm") || l.contains("widevine") {
            (
                ErrorCode::DrmProtected,
                "This stream is DRM-protected",
                "Aurora TV does not decrypt protected streams and cannot play this.",
            )
        } else if l.contains("codec") || l.contains("no video") || l.contains("unsupported") {
            (
                ErrorCode::UnsupportedCodec,
                "This stream uses a format that couldn't be decoded",
                "The codec may be unsupported, or the stream may be corrupt.",
            )
        } else {
            (
                ErrorCode::Unknown,
                "This channel didn't respond",
                "It may be temporarily offline.",
            )
        };

        let retryable = !matches!(
            code,
            ErrorCode::DrmProtected | ErrorCode::Unauthorized | ErrorCode::NotFound
        );

        let mut actions = Vec::new();
        if retryable {
            actions.push(ErrorAction::Retry);
        }
        actions.push(ErrorAction::TryAnotherSource);
        if matches!(code, ErrorCode::Unauthorized | ErrorCode::ConnectionLimit) {
            actions.push(ErrorAction::OpenSettings);
        }
        if matches!(code, ErrorCode::NotFound | ErrorCode::UnsupportedCodec) {
            actions.push(ErrorAction::ReportBroken);
        }

        Self {
            code,
            message: message.to_string(),
            cause: cause.to_string(),
            actions,
            retryable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_auth_from_connection_limit() {
        assert_eq!(
            NetFailure::classify("HTTP 401").code,
            ErrorCode::Unauthorized
        );
        assert_eq!(
            NetFailure::classify("provider reports max connections reached").code,
            ErrorCode::ConnectionLimit
        );
    }

    #[test]
    fn drm_is_never_retryable_and_says_so_plainly() {
        let e = NetFailure::classify("stream is encrypted (widevine)");
        assert_eq!(e.code, ErrorCode::DrmProtected);
        assert!(!e.retryable);
        assert!(!e.actions.contains(&ErrorAction::Retry));
        assert!(e.cause.contains("does not decrypt"));
    }

    #[test]
    fn bad_credentials_offer_settings_not_retry() {
        let e = NetFailure::classify("401 Unauthorized");
        assert!(!e.retryable);
        assert!(e.actions.contains(&ErrorAction::OpenSettings));
    }

    #[test]
    fn dead_streams_can_be_reported() {
        let e = NetFailure::classify("HTTP 404 not found");
        assert_eq!(e.code, ErrorCode::NotFound);
        assert!(e.actions.contains(&ErrorAction::ReportBroken));
    }

    #[test]
    fn transient_failures_are_retryable() {
        for raw in [
            "connection timed out",
            "503 Service Unavailable",
            "getaddrinfo failed",
        ] {
            assert!(NetFailure::classify(raw).retryable, "{raw} should retry");
        }
    }

    #[test]
    fn every_error_has_a_message_a_cause_and_an_action() {
        for raw in [
            "401",
            "403",
            "404",
            "429",
            "timeout",
            "tls",
            "codec",
            "gibberish",
        ] {
            let e = NetFailure::classify(raw);
            assert!(!e.message.is_empty());
            assert!(!e.cause.is_empty());
            assert!(!e.actions.is_empty(), "{raw} produced no actions");
        }
    }
}
