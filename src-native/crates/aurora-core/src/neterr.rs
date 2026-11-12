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
    /// The server understood the request and would not accept it as sent.
    ///
    /// Its own code rather than borrowed from `NotFound`, because the two differ in the
    /// thing that matters: a 404 is worth reporting as a broken source, and a 400 is the
    /// request being wrong, which no retry and no other source will fix. Added when
    /// every rejected Gemini call was surfacing as "Your provider didn't respond"
    /// because 400 matched no branch at all.
    BadRequest,
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
    /// The request never reached the provider: something between this device and them
    /// answered it instead.
    ///
    /// Its own code rather than `ServerError` or `Unknown` because the thing to do about
    /// it is in neither of those sentences. The provider is working and the title is
    /// fine; a filter -- DNS on the router, the ISP's, a "safe browsing" service -- is
    /// serving a web page in place of the stream, and no retry and no other title will
    /// change that. See `diagnose_stream`.
    Blocked,
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
    // `reqwest`'s own `Display`: "http status server error (555 unknown status) for url
    // (...)". The number is inside the parenthesis rather than after the lead-in, so the
    // walk above steps past it and a 555 came out as `Unknown` -- "Something went wrong
    // with that request" (R-32).
    if let Some(at) = lower.find("http status") {
        if let Some(open) = lower[at..].find('(') {
            let after = at + open + 1;
            let digits: String = lower[after..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if digits.len() == 3 {
                if let Ok(code) = digits.parse() {
                    return Some(code);
                }
            }
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
        } else if is(400) {
            // The one status the classifier had no answer for, which is how every
            // rejected Gemini request became "Your provider didn't respond". A 400 is
            // the *request* being wrong, so retrying an identical one cannot help --
            // `retryable` follows from the code below.
            (
                ErrorCode::BadRequest,
                "That request was rejected",
                "The server would not accept it as sent. Nothing about retrying will change that; the log has what it said.",
            )
        } else if is(403) || l.contains("forbidden") {
            (
                ErrorCode::Forbidden,
                "Your provider refused the request",
                "This often means the line has hit its connection limit, or that what was \
                 asked for is not part of your package.",
            )
        } else if is(404) || l.contains("not found") {
            (
                ErrorCode::NotFound,
                // Not "this stream": the same classifier answers for a provider being
                // checked in the setup wizard, where nothing is playing and the address
                // itself is what came back 404.
                "Your provider doesn't have that",
                "It has probably been removed or renumbered. For a title, a library \
                 refresh may fix it; for an address, check it against what your provider \
                 gave you.",
            )
        } else if is(429) || l.contains("too many requests") {
            (
                ErrorCode::RateLimited,
                "Your provider is rate-limiting this connection",
                "Too many requests in a short time. Waiting a moment usually clears it.",
            )
        } else if status == Some(888) {
            // 888, which is not a status code at all.
            //
            // Measured against this panel on 2026-10-25, with the account reporting
            // `status: Active`, `auth: 1` and `active_cons: 0`, from one IP in one
            // minute:
            //
            //   * a VOD stream  -> HTTP 200, bytes flowing;
            //   * three live streams -> 888, 888, 503.
            //
            // So it is not the credentials, not the line being full, not the address and
            // not the client: the same request that works for a film is refused for a
            // channel. Every user agent tried got the same answer, including VLC's and
            // libavformat's, and sending none at all got a 503 instead -- which is the
            // same refusal wearing a standard number.
            //
            // Reported as "live tv is completely broken", and it arrived as **"Something
            // went wrong with that request -- Aurora could not tell what from the error
            // it got back"**, because 888 is in no family any branch names. That answer
            // is worse than the problem: it tells a viewer their app is confused when
            // what happened is that their provider said no.
            //
            // **And it passes.** The same channel that answered 888 to every client
            // tried was serving `video/mp2t` with a 200 about twenty minutes later,
            // nothing having changed at this end. So the wording says "right now"
            // and suggests waiting: a refusal that clears by itself must not be
            // reported as a subscription problem to go and argue about.
            //
            // Deliberately *not* filed as a connection limit, which is where 555 goes.
            // The two look alike and are not: 555 arrives with the line's one slot
            // already in use, and this arrives with none. Guessing "you are watching
            // elsewhere" at somebody who is not would send them looking for a device
            // that is not on.
            (
                ErrorCode::Refused,
                "Your provider is not serving this channel right now",
                "The panel refused it outright rather than failing to answer. That \
                 is usually their live service having a moment: films and shows \
                 often keep working while it lasts, and it has been seen to clear \
                 by itself within the hour. Another source may work meanwhile.",
            )
        } else if status == Some(555)
            || l.contains("connection limit")
            || l.contains("max connections")
        {
            (
                ErrorCode::ConnectionLimit,
                "You're already watching on another device",
                "This subscription allows a limited number of simultaneous streams. \
                 Stop the other stream, or see Settings for how many this line allows.",
            )
        } else if l.contains("refused") {
            (
                ErrorCode::Refused,
                "Nothing is listening at that address",
                "The server refused the connection. The address or port may be wrong, \
                 or the provider may be down.",
            )
        } else if l.contains("stopped sending data") {
            // A stall, which is a timeout about something that was *working*.
            //
            // It used to fall into the branch below and be told "the address may be
            // missing a port. Panels usually publish one, like :8080 or :2082" --
            // setup advice, shown to somebody whose picture was on a second ago. The
            // address cannot be the problem: it resolved, connected and delivered
            // video. Reported from live viewing, where it is the commonest thing on
            // screen.
            //
            // Matched before the general timeout branch because the watchdog's
            // reason contains "timed out" as well, and the first arm wins.
            (
                ErrorCode::Timeout,
                "The stream stopped",
                "It was playing and then the data stopped arriving. That is usually \
                 the provider or the connection having a bad moment, and it often \
                 comes back on its own.",
            )
        } else if l.contains("timed out") || l.contains("timeout") {
            (
                ErrorCode::Timeout,
                // This is the one the setup wizard shows most, where there is no stream
                // yet — "The stream didn't respond in time" was describing something that
                // did not exist, under a form that had just been filled in.
                "Your provider didn't respond in time",
                "The server may be overloaded, or the address may be missing a port. \
                 Panels usually publish one, like :8080 or :2082.",
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
        // 555 is checked above, not here. It is not a standard status, so no branch
        // named it and it fell in with the 5xx family -- and on this subscription it is
        // the commonest failure there is: the line allows one connection and reports the
        // limit as HTTP 555 with an empty body. So opening multi-view, starting a
        // recording while watching, or zapping faster than the panel frees the slot all
        // produced "Your provider's server had an error. This is on their end. It
        // usually resolves by itself." -- blaming the provider for the viewer's own
        // second stream, promising it would fix itself, and offering a Retry that cannot
        // work while the first stream is open. Aurora already had the right words
        // (`ConnectionLimit`) and reached them only by finding the literal text
        // "connection limit", which this panel never sends (R-32).
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
            // What hyper says when a server accepts a connection and then hangs up
            // without sending a reply — which is how a panel that is overloaded, or
            // blocking this device, actually behaves. Reported for a provider check as
            // "didn't respond", which is both vaguer than the truth and points at the
            // wrong thing: the host answered, it just would not say anything.
            || l.contains("closed before message completed")
            || l.contains("incompletemessage")
            || l.contains("empty reply")
        {
            (
                ErrorCode::Dropped,
                // True of a live stream that dies mid-programme and of a panel that hangs
                // up on a request: in both, something was there and then was not.
                "Your provider closed the connection",
                "It accepted the connection and then hung up without answering. That \
                 usually means the panel is overloaded or is refusing this device, and \
                 it is often temporary. For a channel, another source frequently works.",
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
            // Nothing matched, so the raw text is the only thing that could explain this
            // failure -- and until now it was dropped on the floor here. An assistant
            // that answered "Your provider didn't respond" to every Gemini problem was
            // this branch: the generic wording is not wrong for a stream, and for
            // everything else it is a dead end with no way to find out more.
            //
            // Logged at `warn` with the text that fell through, so the next unrecognised
            // failure is diagnosable from a log rather than from a rebuild.
            tracing::warn!("unclassified network failure: {l}");
            (
                ErrorCode::Unknown,
                // Not "this channel", and not "this stream": the same classifier answers
                // for a film, an episode, and a provider being checked before anything is
                // playing at all.
                "Something went wrong with that request",
                "Aurora could not tell what from the error it got back; the log has the details.",
            )
        };

        let retryable = !matches!(
            code,
            ErrorCode::DrmProtected
                | ErrorCode::Unauthorized
                | ErrorCode::NotFound
                | ErrorCode::BadRequest
                | ErrorCode::Blocked
        );

        let mut actions = Vec::new();
        if retryable {
            actions.push(ErrorAction::Retry);
        }
        // Only where another source could exist.
        //
        // This was pushed unconditionally, for every code -- four lines above a comment
        // reasoning carefully that a `BadRequest` must not offer `ReportBroken` "because
        // the source is not what is wrong", which is equally an argument against
        // offering to try a different one. And this classifier is shared, which is the
        // point of it: the assistant's rejected Gemini call comes through here, as does
        // a provider check in the setup wizard where no stream exists at all. Both
        // presented a button whose only honest meaning is "this channel has several
        // URLs; try the next" (R-13).
        //
        // What is left out: a rejected request, which no other source will accept any
        // differently; credentials the provider refused, which are the same credentials
        // on every source; and a line at its connection limit, which is a property of
        // the subscription rather than of the address.
        if !matches!(
            code,
            ErrorCode::BadRequest | ErrorCode::Unauthorized | ErrorCode::ConnectionLimit
        ) {
            actions.push(ErrorAction::TryAnotherSource);
        }
        if matches!(code, ErrorCode::Unauthorized | ErrorCode::ConnectionLimit) {
            actions.push(ErrorAction::OpenSettings);
        }
        if matches!(code, ErrorCode::NotFound | ErrorCode::UnsupportedCodec) {
            actions.push(ErrorAction::ReportBroken);
        }
        // Deliberately not `ReportBroken` for a 400: the source is not what is wrong.
        if matches!(code, ErrorCode::BadRequest) {
            actions.push(ErrorAction::OpenSettings);
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

/// Explain a failed stream load from what the URL answers when asked directly.
///
/// `classify` works on the words mpv produced, and there is one important failure it
/// cannot see: the request never reaches the provider. An Xtream panel redirects a VOD
/// request to one of its streaming servers, and something can take that second hop over
/// and answer it with HTTP 200 and an HTML page. The player gets a successful download
/// of a web page where a film should be, and says the file is broken. The file was never
/// reached.
///
/// Where that something sits is worth not guessing at. Measured on 2026-11-12, the whole
/// of `Riverdale` -- all 138 episodes, every season -- is pinned to one streaming server
/// that answers *every* request, including `/` and a path that cannot exist, with a 302
/// to a "security" interstitial; a TCP connect to it takes as long as an ICMP round trip
/// does, so nothing on this side of the link is answering early, and six asks for the
/// same episode never moved off that machine. Ten films sampled the same afternoon were
/// spread over six other servers and nine of them served video. So the filter is as
/// likely to be at the provider's end as on the viewer's network, and the sentence
/// `diagnose_stream` returns says which to suspect from what else plays rather than
/// sending everybody to their router.
///
/// Returns `None` when the probe shows nothing a viewer could act on, so the caller
/// keeps mpv's own classification. Deliberately quiet rather than guessing: a probe
/// that merely failed differently is not evidence of interception.
///
/// `requested_host` is the host Aurora asked for; `final_host` is where the request
/// ended up after every redirect. A provider legitimately redirects between its own
/// hosts, so the comparison is on the registrable tail rather than the exact name.
pub fn diagnose_stream(
    status: u16,
    content_type: &str,
    requested_host: &str,
    final_host: &str,
) -> Option<NetFailure> {
    let served_video = content_type.starts_with("video/")
        || content_type.starts_with("audio/")
        || content_type.contains("mpegurl")
        || content_type.contains("octet-stream")
        || content_type.contains("mp2t");
    let served_page = content_type.contains("html") || content_type.contains("xhtml");
    let ok = (200..300).contains(&status);

    // The request ended somewhere that is not the provider's and not a relative of it.
    let hijacked = !final_host.is_empty()
        && !requested_host.is_empty()
        && registrable(final_host) != registrable(requested_host);

    if ok && served_page && hijacked {
        return Some(NetFailure {
            code: ErrorCode::Blocked,
            message: "A security filter answered instead of the stream".into(),
            cause: format!(
                "The request for this title ended at {final_host}, which returned a \
                 warning page instead of video. The panel keeps each title on one \
                 streaming server, and every request to this one gets that answer -- \
                 so retrying, or trying another episode of the same show, lands on \
                 it again. If the rest of your library plays, that machine is \
                 blocked at the provider's end and only they can move the title; if \
                 everything does this, the filter is on this network: a router web \
                 protection, a DNS service, or one your ISP applies."
            ),
            actions: vec![ErrorAction::OpenSettings],
            // Every attempt lands on the same blocked machine: the panel pins a title to
            // one streaming server, which is why these titles fail identically for ever
            // while their neighbours are fine. Offering Retry would be a button that
            // cannot work.
            retryable: false,
        });
    }

    if ok && served_page {
        return Some(NetFailure {
            code: ErrorCode::Blocked,
            message: "Your provider sent a web page, not video".into(),
            cause: "The address answered successfully, but with a page rather than a \
                    stream. That is usually a provider error notice, or a portal asking \
                    something of this connection."
                .into(),
            retryable: true,
            actions: vec![ErrorAction::Retry, ErrorAction::ReportBroken],
        });
    }

    // A non-2xx probe is better explained by the status than by anything here, and a
    // 2xx that really did serve video means the failure was mpv's to explain.
    if !ok {
        return Some(NetFailure::classify(&format!("HTTP error {status}")));
    }
    let _ = served_video;
    None
}

/// The registrable tail of a host: the last two labels, lower-cased.
///
/// `cdn7.panel.example` and `panel.example` are the same provider; `www.safebrowse.io`
/// is not. Crude on purpose -- this decides the wording of an error message, and a
/// public-suffix list for that would be a dependency and a data file to keep current.
/// An IP address has no tail worth taking, so it is compared whole.
fn registrable(host: &str) -> String {
    if host.parse::<std::net::IpAddr>().is_ok() {
        return host.to_ascii_lowercase();
    }
    let labels: Vec<&str> = host.trim_end_matches('.').split('.').collect();
    let tail = if labels.len() >= 2 {
        labels[labels.len() - 2..].join(".")
    } else {
        host.to_string()
    };
    tail.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Try another source" is offered only where another source could help.
    ///
    /// It was pushed for every code, including the ones where it cannot mean anything:
    /// a rejected request is rejected the same way everywhere, refused credentials are
    /// the same credentials on every URL, and a connection limit belongs to the
    /// subscription rather than to the address. This classifier is shared, so those
    /// buttons appeared on the assistant's failures and in the setup wizard, where no
    /// stream exists at all (R-13).
    #[test]
    fn another_source_is_offered_only_where_there_could_be_one() {
        let has = |raw: &str| {
            NetFailure::classify(raw)
                .actions
                .contains(&ErrorAction::TryAnotherSource)
        };

        // A channel that would not play: another URL for it is exactly the answer.
        assert!(
            has("HTTP 404"),
            "a missing stream should offer another source"
        );
        assert!(has("connection refused"));
        assert!(has("connection timed out"));
        assert!(has("dns"));

        // And the three where it is noise.
        assert!(!has("HTTP 400"), "a rejected request has no other source");
        assert!(
            !has("HTTP 401"),
            "refused credentials are the same everywhere"
        );
        assert!(
            !has("HTTP 555"),
            "a connection limit is the line, not the address"
        );

        // The ones that are left still carry something to do.
        let rejected = NetFailure::classify("HTTP 400");
        assert!(
            rejected.actions.contains(&ErrorAction::OpenSettings),
            "{rejected:?}"
        );
        let unauthorized = NetFailure::classify("HTTP 401");
        assert!(unauthorized.actions.contains(&ErrorAction::OpenSettings));
    }

    /// HTTP 555 with an empty body, which is how this subscription says "that is your
    /// second stream".
    ///
    /// Established directly against the panel: hold one stream open, ask for another on
    /// the same line, and the answer is `555`, `text/html`, zero bytes. 555 is not a
    /// standard status, so no branch named it and it fell in with the 5xx family -- the
    /// viewer was told "Your provider's server had an error. This is on their end. It
    /// usually resolves by itself", about their own second stream, with a Retry that
    /// could not work until they closed the first one (R-32).
    #[test]
    fn a_555_is_the_line_being_busy_not_the_provider_being_broken() {
        // Every form the three call sites actually produce.
        for text in [
            "HTTP 555",
            "server returned 555",
            "http error 555",
            "HTTP status server error (555 unknown status) for url (http://host/live)",
        ] {
            let f = NetFailure::classify(text);
            assert_eq!(
                f.code,
                ErrorCode::ConnectionLimit,
                "{text:?} was read as {:?}: {}",
                f.code,
                f.message
            );
            assert!(
                f.actions.contains(&ErrorAction::OpenSettings),
                "{text:?} offers no way to see the limit: {:?}",
                f.actions
            );
            assert!(
                !f.message.to_lowercase().contains("their end")
                    && !f.cause.to_lowercase().contains("their end"),
                "{text:?} still blames the provider: {} / {}",
                f.message,
                f.cause
            );
        }

        // An ordinary 5xx is still an ordinary 5xx.
        let real = NetFailure::classify("server returned 502 bad gateway");
        assert_eq!(real.code, ErrorCode::ServerError);
    }

    /// No viewer-facing sentence carries a run of whitespace left over from wrapping.
    ///
    /// Two `cause` strings did -- eighteen spaces each, from a hand-wrapped line that
    /// was later joined -- and the error panel renders a cause as a paragraph, so the
    /// gap was on screen. Nothing was broken; it looked broken, which for an error
    /// message is the same thing (R-12).
    #[test]
    fn nothing_the_viewer_reads_has_a_gap_in_it() {
        for text in [
            "HTTP 400",
            "HTTP 401",
            "HTTP 404",
            "HTTP 429",
            "HTTP 500",
            "HTTP 555",
            "connection refused",
            "timed out",
            "widevine",
            "unrecognisable nonsense",
        ] {
            let f = NetFailure::classify(text);
            for (what, sentence) in [("message", &f.message), ("cause", &f.cause)] {
                assert!(
                    !sentence.contains("  "),
                    "the {what} for {text:?} has a double space in it: {sentence:?}"
                );
            }
        }
    }

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

    /// A panel that accepts the connection and then hangs up without answering.
    ///
    /// Observed against a real subscription: `curl` reports "Empty reply from server" and
    /// hyper reports "connection closed before message completed", neither of which any
    /// branch recognised — so it fell through to "didn't respond", which points at the
    /// wrong thing. The host answered; it just would not say anything.
    #[test]
    fn a_host_that_accepts_and_then_hangs_up_is_not_a_silent_host() {
        for raw in [
            "error sending request; connection closed before message completed",
            "hyper::Error(IncompleteMessage)",
            "empty reply from server",
        ] {
            let e = NetFailure::classify(raw);
            assert_eq!(e.code, ErrorCode::Dropped, "for {raw:?}");
            assert!(
                e.cause.contains("hung up"),
                "the cause should say what actually happened: {e:?}"
            );
            // Retryable, because this is overload or throttling far more often than it
            // is anything permanent.
            assert!(e.retryable, "for {raw:?}");
        }
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

    /// These messages are shown by the setup wizard too, where nothing is playing and
    /// there is no stream to describe — only an address somebody has just typed.
    /// A stall and a failed connection are both "timed out" and are not the same thing.
    ///
    /// The watchdog says "the stream stopped sending data: timed out" for a stream that
    /// *was playing*, and that string contains "timed out", so it fell into the general
    /// timeout arm and the viewer was told the address might be missing a port --
    /// advice for a connection that never happened, shown about one that had been
    /// delivering video a second earlier. Reported from live viewing.
    #[test]
    fn a_stall_is_not_told_to_check_its_port() {
        let stalled = NetFailure::classify("the stream stopped sending data: timed out");
        assert_eq!(stalled.code, ErrorCode::Timeout);
        assert!(
            !stalled.cause.contains("8080") && !stalled.cause.contains("port"),
            "a stream that was playing was told to check its address: {}",
            stalled.cause
        );
        assert!(stalled.message.contains("stopped"), "{}", stalled.message);

        // And the one that never started still gets the advice that helps it, which is
        // what the branch below this one exists for.
        let never = NetFailure::classify("the stream did not start playing: timed out");
        assert_eq!(never.code, ErrorCode::Timeout);
        assert!(never.cause.contains("port"), "{}", never.cause);
        assert_ne!(never.message, stalled.message);
    }

    /// 888 is a refusal, not a confusion.
    ///
    /// Measured against the real panel: the account was `Active` with `active_cons: 0`,
    /// a VOD stream returned HTTP 200 with bytes, and live channels returned 888. It
    /// reached the viewer as "Aurora could not tell what from the error it got back",
    /// which blames the app for the provider saying no.
    #[test]
    fn a_non_standard_refusal_is_still_a_refusal() {
        for raw in ["HTTP 888", "server returned 888", "http error 888: "] {
            let f = NetFailure::classify(raw);
            assert_eq!(f.code, ErrorCode::Refused, "{raw}");
            assert!(
                !f.message.contains("could not tell"),
                "{raw} still reads as a confusion: {}",
                f.message
            );
            assert!(
                f.message.to_lowercase().contains("provider"),
                "{}",
                f.message
            );
        }
    }

    /// And it is not mistaken for the connection limit, which looks just like it.
    ///
    /// 555 arrives when the line's one slot is already in use; 888 arrived with none in
    /// use. Telling somebody they are "watching on another device" when they are not
    /// sends them looking for a device that is not on.
    #[test]
    fn a_refusal_is_not_reported_as_a_second_device() {
        let refused = NetFailure::classify("HTTP 888");
        let limit = NetFailure::classify("HTTP 555");
        assert_eq!(limit.code, ErrorCode::ConnectionLimit);
        assert_ne!(refused.code, limit.code);
        assert!(
            !refused.message.contains("another device"),
            "{}",
            refused.message
        );
    }

    /// "The stream didn't respond in time", under a provider form, is a sentence about
    /// something that does not exist yet.
    #[test]
    fn a_provider_being_checked_is_not_described_as_a_stream() {
        for raw in [
            "connection timed out",
            "server returned 404 not found",
            "server returned 403 forbidden",
            "something nobody has seen before",
        ] {
            let e = NetFailure::classify(raw);
            let said = format!("{} {}", e.message, e.cause).to_lowercase();
            assert!(
                !e.message.to_lowercase().contains("stream"),
                "a provider check would say {:?} about a stream that does not exist",
                e.message
            );
            assert!(!said.is_empty());
        }
    }

    /// The commonest reason a panel does not answer, and the one worth naming: a bare
    /// hostname with no port. Panels publish one far more often than not.
    #[test]
    fn a_timeout_suggests_the_thing_that_is_usually_wrong() {
        let e = NetFailure::classify("connection timed out");
        assert_eq!(e.code, ErrorCode::Timeout);
        assert!(e.cause.contains("port"), "{e:?}");
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

    /// The failure that looks like success.
    ///
    /// Measured against the real subscription: 5.3% of sampled films and episodes ended
    /// at a 3,303-byte `text/html` page served from a CDN rather than at the provider.
    /// The panel redirects a VOD request to one of its streaming servers, and a filter
    /// between the device and that server answers the second hop itself -- with a 200.
    /// mpv downloads the page, cannot demux it, and reports a broken file for a title it
    /// never reached.
    #[test]
    fn a_stream_answered_by_a_filter_page_says_so_rather_than_blaming_the_file() {
        let f = diagnose_stream(200, "text/html", "panel.example", "www.safebrowse.io")
            .expect("an HTML page from another host is not a diagnosis to stay quiet about");
        assert_eq!(f.code, ErrorCode::Blocked);
        assert!(
            f.cause.contains("safebrowse.io"),
            "the host that answered is the one actionable fact: {}",
            f.cause
        );
        // Every attempt lands on the same blocked machine, so a Retry button would be a
        // button that cannot work.
        assert!(!f.retryable);
        assert!(!f.actions.contains(&ErrorAction::Retry));
    }

    /// The shape the real subscription produces, down to the status code.
    ///
    /// Riverdale, every episode of it: the panel 302s to one streaming server, and that
    /// server answers the ranged request with `206` and an HTML warning page rather than
    /// a `200`. A diagnosis that only looked for `200` would have let the whole show fall
    /// through to mpv's "broken file", which is the sentence this exists to replace.
    #[test]
    fn a_ranged_request_answered_with_a_warning_page_is_diagnosed_too() {
        let f = diagnose_stream(
            206,
            "text/html",
            "64582429.max-jbnott.online",
            "www.safebrowse.io",
        )
        .expect("206 plus a page is the same interception as 200 plus a page");
        assert_eq!(f.code, ErrorCode::Blocked);
        assert!(!f.retryable, "the server answers every request this way");
        // The viewer is told how to tell a blocked server from a blocked network, which
        // is the difference between reporting a title and reconfiguring a router.
        assert!(
            f.cause.contains("rest of your library"),
            "the sentence does not say how to tell the two apart: {}",
            f.cause
        );
    }

    /// A provider redirecting between its own machines is not interception.
    ///
    /// This is the ordinary case: the panel answers on `panel.example` and serves from
    /// `cdn7.panel.example`. Comparing hosts exactly would have called every working
    /// stream on the subscription blocked.
    #[test]
    fn a_providers_own_streaming_server_is_not_a_filter() {
        assert!(diagnose_stream(200, "video/mp4", "panel.example", "cdn7.panel.example").is_none());
        // And the bare IPs these panels actually redirect to, which have no tail to take.
        assert!(
            diagnose_stream(206, "video/x-matroska", "panel.example", "45.134.21.65").is_none()
        );
    }

    /// A page served by the provider itself is still not video, and still worth saying.
    #[test]
    fn a_page_from_the_provider_is_reported_as_a_page() {
        let f = diagnose_stream(
            200,
            "text/html; charset=utf-8",
            "panel.example",
            "panel.example",
        )
        .expect("a web page where a film should be is always worth a sentence");
        assert_eq!(f.code, ErrorCode::Blocked);
        // Unlike the hijack: the provider may simply have been having a moment.
        assert!(f.retryable);
    }

    /// Video that arrived means the failure was mpv's to explain, so this says nothing.
    #[test]
    fn a_stream_that_served_video_is_left_to_mpv_to_explain() {
        for content_type in [
            "video/mp4",
            "video/x-matroska",
            "audio/mpeg",
            "application/octet-stream",
            "application/vnd.apple.mpegurl",
            "video/mp2t",
        ] {
            assert!(
                diagnose_stream(206, content_type, "panel.example", "panel.example").is_none(),
                "{content_type} was second-guessed"
            );
        }
    }

    /// A non-2xx probe is explained by its status, through the classifier that already
    /// has the words for it.
    #[test]
    fn a_probe_that_was_refused_is_explained_by_its_status() {
        let gone = diagnose_stream(404, "text/html", "panel.example", "panel.example")
            .expect("a 404 is a diagnosis");
        assert_eq!(gone.code, ErrorCode::NotFound);
        assert!(!gone.retryable);

        let busy = diagnose_stream(403, "text/html", "panel.example", "panel.example")
            .expect("a 403 is a diagnosis");
        assert_eq!(busy.code, ErrorCode::Forbidden);
    }
}
