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
    /// The host resolved and answered, but nothing is listening. Distinct from `Dns`:
    /// one sends you to your network settings, the other to the address you typed.
    Refused,
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
    /// The stream was playing and the far end went away: an ordinary EOF on a live feed,
    /// a reset connection, a provider that hung up. Distinct from `Timeout` because
    /// nothing timed out and distinct from `ServerError` because nothing errored — and
    /// worth its own sentence, since it is the commonest way an IPTV stream dies.
    Dropped,
    Unknown,
}

/// The HTTP status in a failure description, when there is one.
///
/// Looked for behind a word that means "status", never as a bare three-digit number.
/// `MpvBackend::fail` now attaches mpv's own log lines to every failure, and those
/// carry numbers of their own: `bitrate: 403 kb/s` on a healthy stream would otherwise
/// classify a stall as "your provider refused this stream".
fn http_status(lower: &str) -> Option<u16> {
    const LEAD_INS: [&str; 7] = [
        "server returned ",
        "http error ",
        "http status ",
        "status code ",
        "status: ",
        "http/1.1 ",
        "http ",
    ];
    for lead in LEAD_INS {
        let mut from = 0;
        while let Some(at) = lower[from..].find(lead) {
            let after = from + at + lead.len();
            let digits: String = lower[after..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if digits.len() == 3 {
                if let Ok(code) = digits.parse() {
                    return Some(code);
                }
            }
            from = after;
        }
    }
    // `403 Forbidden` and friends: the number is identified by the word beside it.
    for (code, word) in [
        (401u16, "401 unauthorized"),
        (403, "403 forbidden"),
        (404, "404 not found"),
        (429, "429 too many requests"),
        (500, "500 internal server error"),
        (502, "502 bad gateway"),
        (503, "503 service unavailable"),
        (504, "504 gateway timeout"),
    ] {
        if lower.contains(word) {
            return Some(code);
        }
    }
    None
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
        let status = http_status(&l);
        let is = |code: u16| status == Some(code);

        let (code, message, cause) = if is(401) || l.contains("unauthorized") {
            (
                ErrorCode::Unauthorized,
                "Your provider rejected these credentials",
                "The username or password may have changed, or the line may have expired.",
            )
        } else if is(403) || l.contains("forbidden") {
            (
                ErrorCode::Forbidden,
                "Your provider refused this stream",
                "This often means the line has hit its connection limit, or this channel \
                 is not part of your package.",
            )
        } else if is(404) || l.contains("not found") {
            (
                ErrorCode::NotFound,
                "This stream no longer exists",
                "The provider has probably removed or renumbered it. A library refresh \
                 may fix it.",
            )
        } else if is(429) || l.contains("too many requests") {
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
                ErrorCode::Refused,
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
        } else if matches!(status, Some(500..=599))
            || l.contains("bad gateway")
            || l.contains("service unavailable")
            || l.contains("internal server error")
        {
            (
                ErrorCode::ServerError,
                "Your provider's server had an error",
                "This is on their end. It usually resolves by itself.",
            )
        } else if l.contains("widevine")
            || l.contains("playready")
            || l.contains("fairplay")
            || l.contains("drm")
            || l.contains("cenc")
        {
            // Deliberately *not* the bare word "encrypted". Ordinary HLS carries
            // AES-128, says so in mpv's log, and plays perfectly — ffmpeg decrypts it
            // without being asked. Matching that word told people a channel they could
            // watch was protected and gave them no retry button, which is the worst
            // combination available: a wrong diagnosis and no way past it.
            (
                ErrorCode::DrmProtected,
                "This stream is DRM-protected",
                "Aurora TV does not decrypt protected streams and cannot play this.",
            )
        } else if l.contains("closed the connection")
            || l.contains("connection reset")
            || l.contains("broken pipe")
            || l.contains("end of file")
        {
            (
                ErrorCode::Dropped,
                "The stream ended unexpectedly",
                "The provider dropped the connection. This is usually temporary, and \
                 another source for the same channel often works.",
            )
        } else if l.contains("codec")
            || l.contains("no video")
            || l.contains("unsupported")
            || l.contains("unrecognized file format")
            || l.contains("invalid data found")
            || l.contains("protocol not found")
        {
            (
                ErrorCode::UnsupportedCodec,
                "This stream uses a format that couldn't be decoded",
                "The codec may be unsupported, or the stream may be corrupt.",
            )
        } else {
            (
                ErrorCode::Unknown,
                // Not "this channel": the same classifier answers for a film and an
                // episode, and being told a channel did not respond when you pressed
                // play on a film reads as the app having lost track of what it is doing.
                "This stream didn't respond",
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
        if matches!(code, ErrorCode::Dropped) {
            // Worth reporting only if it keeps happening, which the viewer is the one
            // who knows. The button records the source as bad so the next tune starts
            // somewhere else.
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

    /// Ordinary HLS says this and plays fine — ffmpeg decrypts AES-128 without being
    /// asked. Calling it DRM told people a channel they could watch was protected, and
    /// took the retry button away while doing it.
    #[test]
    fn aes_128_hls_is_not_drm() {
        for line in [
            "ffmpeg: [hls] using aes-128 encrypted segments",
            "ffmpeg: encryption method aes-128",
        ] {
            let e = NetFailure::classify(line);
            assert_ne!(e.code, ErrorCode::DrmProtected, "misdiagnosed: {line}");
            assert!(e.retryable, "left with no way past it: {line}");
        }
    }

    /// The real phrasings, as ffmpeg and mpv actually write them.
    #[test]
    fn real_provider_failures_are_read_correctly() {
        let cases = [
            (
                "ffmpeg: server returned 403 forbidden",
                ErrorCode::Forbidden,
            ),
            ("ffmpeg: server returned 404 not found", ErrorCode::NotFound),
            (
                "ffmpeg: server returned 401 unauthorized",
                ErrorCode::Unauthorized,
            ),
            (
                "ffmpeg: server returned 503 service unavailable",
                ErrorCode::ServerError,
            ),
            ("http error 429 too many requests", ErrorCode::RateLimited),
            ("stream: connection reset by peer", ErrorCode::Dropped),
            (
                "ffmpeg: invalid data found when processing input",
                ErrorCode::UnsupportedCodec,
            ),
        ];
        for (raw, want) in cases {
            assert_eq!(NetFailure::classify(raw).code, want, "for {raw:?}");
        }
    }

    /// `MpvBackend::fail` attaches mpv's own log lines to every failure, and those are
    /// full of numbers. A healthy stream's bitrate must not be read as an HTTP status.
    #[test]
    fn a_number_in_the_log_is_not_a_status_code() {
        let e = NetFailure::classify(
            "the stream stopped sending data: timed out \
             [mpv: ffmpeg: stream 0: video 1920x1080, bitrate: 403 kb/s / \
             ffmpeg: audio bitrate 404 kb/s]",
        );
        assert_eq!(
            e.code,
            ErrorCode::Timeout,
            "a bitrate was read as an HTTP status: {e:?}"
        );
    }

    /// The commonest way an IPTV stream dies: it just ends. Retryable, and worth trying
    /// another source for, because that is usually all it takes.
    #[test]
    fn a_provider_hanging_up_is_its_own_failure() {
        let e = NetFailure::classify("the provider closed the connection");
        assert_eq!(e.code, ErrorCode::Dropped);
        assert!(e.retryable);
        assert!(e.actions.contains(&ErrorAction::TryAnotherSource));
        assert!(!e.message.contains("DRM"));
    }

    /// Both watchdog verdicts are timeouts, and timeouts are retryable — which is what
    /// makes `playback::tick` roll over to the next source for a stream that went quiet.
    #[test]
    fn the_watchdogs_verdicts_are_retryable_timeouts() {
        for reason in [
            "the stream did not start playing: timed out",
            "the stream stopped sending data: timed out",
        ] {
            let e = NetFailure::classify(reason);
            assert_eq!(e.code, ErrorCode::Timeout, "for {reason:?}");
            assert!(e.retryable);
        }
    }

    /// A film that will not play must not be described as a channel.
    #[test]
    fn an_unrecognised_failure_does_not_claim_to_be_a_channel() {
        let e = NetFailure::classify("something nobody has seen before");
        assert_eq!(e.code, ErrorCode::Unknown);
        assert!(!e.message.to_lowercase().contains("channel"), "{e:?}");
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
