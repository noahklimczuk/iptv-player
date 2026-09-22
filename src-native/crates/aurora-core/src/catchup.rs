//! Turning "watch this from the start" into a URL (README §7.5).
//!
//! Catch-up is the least standardised thing a provider does. Four conventions are in
//! common use, they disagree about everything, and the `catchup-source` attribute is a
//! template language nobody wrote down — so this is pure, and every shape that turned up
//! in the wild has a test rather than a comment claiming it works.
//!
//! The channel's `catchup_mode`, `catchup_source` and `catchup_days` come from the
//! playlist (`m3u::parse`) or from an Xtream listing; this decides what to do with them.

use time::OffsetDateTime;

/// How a provider expects a past programme to be requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Xtream Codes panels: `/streaming/timeshift.php?...&start=…&duration=…`.
    Xtream,
    /// Append the `catchup-source` template to the stream URL.
    Append,
    /// Append `?utc=<start>&lutc=<now>`, the Kodi-era convention.
    Shift,
    /// Flussonic: the last path segment becomes `timeshift_abs-<unix>.m3u8`.
    Flussonic,
}

impl Mode {
    /// Recognise the spellings playlists actually carry.
    ///
    /// `default` is deliberately `Shift`: it is what the oldest and most widespread
    /// players do with it, so a provider writing `catchup="default"` expects `?utc=`.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "xc" | "xtream" | "xtream-codes" | "stalker" => Some(Mode::Xtream),
            "append" => Some(Mode::Append),
            "default" | "shift" | "timeshift" | "1" => Some(Mode::Shift),
            "flussonic" | "flussonic-hls" | "flussonic-ts" | "fs" => Some(Mode::Flussonic),
            _ => None,
        }
    }
}

/// Everything needed to build one catch-up URL.
#[derive(Debug, Clone)]
pub struct Request<'a> {
    /// The channel's live stream URL.
    pub stream_url: &'a str,
    pub mode: Mode,
    /// The provider's `catchup-source` template, if it gave one.
    pub source: Option<&'a str>,
    /// Programme airtime, unix seconds.
    pub start: i64,
    pub stop: i64,
    pub now: i64,
}

/// Whether a programme is still inside the provider's catch-up window.
///
/// `days` of 0 means the provider never said, which is not the same as "no catch-up" —
/// the channel would not be flagged at all in that case — so it is treated as
/// unrestricted rather than as a reason to refuse.
pub fn is_available(start: i64, now: i64, days: u16) -> bool {
    if start > now {
        // Not yet aired. There is nothing to catch up on.
        return false;
    }
    if days == 0 {
        return true;
    }
    now - start <= i64::from(days) * 86_400
}

/// Build the URL to play a past programme from, or `None` if this provider's
/// configuration does not say how.
pub fn url_for(req: &Request<'_>) -> Option<String> {
    if req.stream_url.trim().is_empty() {
        return None;
    }
    // A template always wins, whatever the mode: a provider that bothered to send one
    // is describing its own server more accurately than any convention can.
    if let Some(template) = req.source.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(apply_source(req.stream_url, template, req));
    }

    match req.mode {
        Mode::Shift | Mode::Append => Some(append_query(
            req.stream_url,
            &format!("utc={}&lutc={}", req.start, req.now),
        )),
        Mode::Flussonic => Some(flussonic_url(req.stream_url, req.start)),
        Mode::Xtream => xtream_url(req.stream_url, req),
    }
}

/// A `catchup-source` is either a whole URL or a fragment to bolt onto the stream URL.
fn apply_source(stream_url: &str, template: &str, req: &Request<'_>) -> String {
    let expanded = expand(template, req);
    let lower = expanded.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return expanded;
    }
    if let Some(query) = expanded.strip_prefix('?') {
        return append_query(stream_url, query);
    }
    if expanded.starts_with('&') {
        return append_query(stream_url, expanded.trim_start_matches('&'));
    }
    // A path fragment.
    format!("{}{expanded}", stream_url.trim_end_matches('/'))
}

/// Substitute the placeholders `catchup-source` templates use.
///
/// Longest token first, because `${duration}` and `${d}` share a prefix and `${start}`
/// is a prefix of `${start-year}` — replacing the short one first corrupts the long one.
pub fn expand(template: &str, req: &Request<'_>) -> String {
    let duration = (req.stop - req.start).max(0);
    let offset = (req.now - req.start).max(0);
    let t = OffsetDateTime::from_unix_timestamp(req.start).ok();

    let (year, month, day, hour, minute, second) = match t {
        Some(t) => (
            t.year(),
            t.month() as u8,
            t.day(),
            t.hour(),
            t.minute(),
            t.second(),
        ),
        None => (1970, 1, 1, 0, 0, 0),
    };

    // Ordered: every token that is a prefix of another comes after it.
    let pairs: Vec<(String, String)> = vec![
        ("start-year".into(), format!("{year:04}")),
        ("start-month".into(), format!("{month:02}")),
        ("start-day".into(), format!("{day:02}")),
        ("start-hour".into(), format!("{hour:02}")),
        ("start-minute".into(), format!("{minute:02}")),
        ("start-second".into(), format!("{second:02}")),
        ("duration".into(), duration.to_string()),
        ("timestamp".into(), req.start.to_string()),
        ("offset".into(), offset.to_string()),
        ("start".into(), req.start.to_string()),
        ("lutc".into(), req.now.to_string()),
        ("utc".into(), req.start.to_string()),
        ("end".into(), req.stop.to_string()),
        ("now".into(), req.now.to_string()),
        ("Y".into(), format!("{year:04}")),
        ("m".into(), format!("{month:02}")),
        ("d".into(), format!("{day:02}")),
        ("H".into(), format!("{hour:02}")),
        ("M".into(), format!("{minute:02}")),
        ("S".into(), format!("{second:02}")),
    ];

    let mut out = template.to_string();
    for (token, value) in pairs {
        // Both bracket styles are in the wild, sometimes in the same playlist.
        out = out.replace(&format!("${{{token}}}"), &value);
        out = out.replace(&format!("{{{token}}}"), &value);
    }
    out
}

fn append_query(url: &str, query: &str) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}{query}")
}

/// Flussonic wants the playlist segment replaced, not a query parameter.
fn flussonic_url(stream_url: &str, start: i64) -> String {
    let (before_query, query) = match stream_url.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (stream_url, None),
    };
    let trimmed = before_query.trim_end_matches('/');

    // Replace a trailing playlist segment (index.m3u8, mono.m3u8, video.m3u8…), or add
    // one when the URL stops at the stream name.
    let rebuilt = match trimmed.rsplit_once('/') {
        Some((head, last)) if last.to_ascii_lowercase().ends_with(".m3u8") => {
            format!("{head}/timeshift_abs-{start}.m3u8")
        }
        _ => format!("{trimmed}/timeshift_abs-{start}.m3u8"),
    };
    match query {
        Some(q) => format!("{rebuilt}?{q}"),
        None => rebuilt,
    }
}

/// Xtream panels want `timeshift.php`, which needs the credentials and stream id back
/// out of the live URL — they are in it, as `/live/<user>/<pass>/<id>.<ext>`.
fn xtream_url(stream_url: &str, req: &Request<'_>) -> Option<String> {
    let (base, username, password, stream_id) = split_xtream_stream_url(stream_url)?;
    let start = OffsetDateTime::from_unix_timestamp(req.start).ok()?;
    let stamp = format!(
        "{:04}-{:02}-{:02}:{:02}-{:02}",
        start.year(),
        start.month() as u8,
        start.day(),
        start.hour(),
        start.minute()
    );
    let duration_min = ((req.stop - req.start).max(60) + 59) / 60;
    Some(format!(
        "{base}/streaming/timeshift.php?username={username}&password={password}\
         &stream={stream_id}&start={stamp}&duration={duration_min}"
    ))
}

/// `http://host:port/live/user/pass/1234.ts` → its four parts.
///
/// Also accepts the form without the `/live` segment, which some panels serve.
fn split_xtream_stream_url(url: &str) -> Option<(String, String, String, String)> {
    let without_query = url.split('?').next().unwrap_or(url);
    let scheme_end = without_query.find("://")? + 3;
    let (scheme, rest) = without_query.split_at(scheme_end);

    let mut parts: Vec<&str> = rest.split('/').collect();
    let host = parts.first().copied()?;
    if parts.len() < 4 {
        return None;
    }
    // Drop the host, and the `live` marker when present.
    parts.remove(0);
    if parts
        .first()
        .is_some_and(|p| matches!(p.to_ascii_lowercase().as_str(), "live" | "movie" | "series"))
    {
        parts.remove(0);
    }
    if parts.len() < 3 {
        return None;
    }

    let username = parts[0].to_string();
    let password = parts[1].to_string();
    // `1234.ts` → `1234`.
    let stream_id = parts[2].split('.').next()?.to_string();
    if username.is_empty() || password.is_empty() || stream_id.is_empty() {
        return None;
    }
    Some((format!("{scheme}{host}"), username, password, stream_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-03-14 09:26:53 UTC, so every component is distinguishable.
    const START: i64 = 1_773_480_413;
    const STOP: i64 = START + 3600;
    const NOW: i64 = START + 7200;

    fn req<'a>(stream_url: &'a str, mode: Mode, source: Option<&'a str>) -> Request<'a> {
        Request {
            stream_url,
            mode,
            source,
            start: START,
            stop: STOP,
            now: NOW,
        }
    }

    #[test]
    fn modes_are_recognised_by_the_spellings_playlists_use() {
        assert_eq!(Mode::parse("xc"), Some(Mode::Xtream));
        assert_eq!(Mode::parse("Xtream-Codes"), Some(Mode::Xtream));
        assert_eq!(Mode::parse("append"), Some(Mode::Append));
        assert_eq!(Mode::parse("flussonic"), Some(Mode::Flussonic));
        assert_eq!(Mode::parse(" SHIFT "), Some(Mode::Shift));
        // The oldest and commonest spelling; players have always read it as `?utc=`.
        assert_eq!(Mode::parse("default"), Some(Mode::Shift));
        assert_eq!(Mode::parse("something-else"), None);
        assert_eq!(Mode::parse(""), None);
    }

    #[test]
    fn shift_appends_the_utc_pair() {
        let url = url_for(&req("http://h/live/s.ts", Mode::Shift, None)).unwrap();
        assert_eq!(url, format!("http://h/live/s.ts?utc={START}&lutc={NOW}"));
    }

    #[test]
    fn a_url_that_already_has_a_query_keeps_it() {
        let url = url_for(&req("http://h/s.ts?token=abc", Mode::Shift, None)).unwrap();
        assert!(url.starts_with("http://h/s.ts?token=abc&utc="), "{url}");
        assert_eq!(
            url.matches('?').count(),
            1,
            "only one query separator: {url}"
        );
    }

    #[test]
    fn flussonic_replaces_the_playlist_segment() {
        assert_eq!(
            url_for(&req("http://h/mychannel/index.m3u8", Mode::Flussonic, None)).unwrap(),
            format!("http://h/mychannel/timeshift_abs-{START}.m3u8")
        );
        // Other segment names are equally common.
        assert_eq!(
            url_for(&req("http://h/ch/mono.m3u8", Mode::Flussonic, None)).unwrap(),
            format!("http://h/ch/timeshift_abs-{START}.m3u8")
        );
        // And a URL that stops at the stream name gets the segment added.
        assert_eq!(
            url_for(&req("http://h/mychannel", Mode::Flussonic, None)).unwrap(),
            format!("http://h/mychannel/timeshift_abs-{START}.m3u8")
        );
    }

    #[test]
    fn flussonic_keeps_an_auth_query() {
        let url = url_for(&req(
            "http://h/ch/index.m3u8?token=abc",
            Mode::Flussonic,
            None,
        ))
        .unwrap();
        assert_eq!(
            url,
            format!("http://h/ch/timeshift_abs-{START}.m3u8?token=abc")
        );
    }

    #[test]
    fn xtream_recovers_the_credentials_from_the_stream_url() {
        let url = url_for(&req(
            "http://panel.example.com:8080/live/alice/hunter2/1234.ts",
            Mode::Xtream,
            None,
        ))
        .unwrap();
        assert!(url.starts_with("http://panel.example.com:8080/streaming/timeshift.php?"));
        assert!(url.contains("username=alice"), "{url}");
        assert!(url.contains("password=hunter2"), "{url}");
        assert!(url.contains("stream=1234"), "{url}");
        // Xtream's own date format, not a unix stamp.
        assert!(url.contains("start=2026-03-14:09-26"), "{url}");
        assert!(url.contains("duration=60"), "{url}");
    }

    #[test]
    fn xtream_also_reads_a_url_without_the_live_segment() {
        let url = url_for(&req("http://h/alice/hunter2/9.m3u8", Mode::Xtream, None)).unwrap();
        assert!(url.contains("stream=9"), "{url}");
        assert!(url.contains("username=alice"), "{url}");
    }

    #[test]
    fn a_stream_url_xtream_cannot_be_read_from_yields_no_guess() {
        // Better no button than a URL built from invented credentials.
        assert_eq!(
            url_for(&req("http://h/stream.ts", Mode::Xtream, None)),
            None
        );
        assert_eq!(url_for(&req("not-a-url", Mode::Xtream, None)), None);
    }

    #[test]
    fn an_empty_stream_url_is_never_a_catch_up_url() {
        assert_eq!(url_for(&req("", Mode::Shift, None)), None);
        assert_eq!(
            url_for(&req("   ", Mode::Flussonic, Some("?utc=${start}"))),
            None
        );
    }

    #[test]
    fn a_source_template_overrides_the_mode() {
        // The provider described its own server; that beats any convention.
        let url = url_for(&req(
            "http://h/live/s.ts",
            Mode::Flussonic,
            Some("?utc=${start}&dur=${duration}"),
        ))
        .unwrap();
        assert_eq!(url, format!("http://h/live/s.ts?utc={START}&dur=3600"));
    }

    #[test]
    fn a_source_that_is_a_whole_url_is_used_as_is() {
        let url = url_for(&req(
            "http://h/live/s.ts",
            Mode::Append,
            Some("http://archive.example.com/play?t=${start}"),
        ))
        .unwrap();
        assert_eq!(url, format!("http://archive.example.com/play?t={START}"));
    }

    #[test]
    fn a_source_that_is_a_path_fragment_is_appended() {
        let url = url_for(&req(
            "http://h/live/chan",
            Mode::Append,
            Some("/archive-${start}-${duration}.m3u8"),
        ))
        .unwrap();
        assert_eq!(url, format!("http://h/live/chan/archive-{START}-3600.m3u8"));
    }

    #[test]
    fn a_source_beginning_with_an_ampersand_joins_an_existing_query() {
        let url = url_for(&req(
            "http://h/s.ts?token=abc",
            Mode::Append,
            Some("&utc=${start}"),
        ))
        .unwrap();
        assert_eq!(url, format!("http://h/s.ts?token=abc&utc={START}"));
    }

    #[test]
    fn every_placeholder_spelling_expands() {
        let r = req("http://h/s", Mode::Shift, None);
        // Both bracket styles, in one template, as playlists really do.
        let got = expand(
            "${start}|{utc}|${end}|${duration}|${offset}|{lutc}|${timestamp}",
            &r,
        );
        assert_eq!(
            got,
            format!("{START}|{START}|{STOP}|3600|7200|{NOW}|{START}")
        );
    }

    #[test]
    fn date_component_placeholders_are_zero_padded() {
        let r = req("http://h/s", Mode::Shift, None);
        assert_eq!(
            expand("${start-year}-${start-month}-${start-day}", &r),
            "2026-03-14"
        );
        assert_eq!(
            expand("${start-hour}:${start-minute}:${start-second}", &r),
            "09:26:53"
        );
        // The terse spelling means the same thing.
        assert_eq!(expand("{Y}{m}{d}-{H}{M}{S}", &r), "20260314-092653");
    }

    #[test]
    fn a_long_token_is_not_eaten_by_a_short_one_sharing_its_prefix() {
        let r = req("http://h/s", Mode::Shift, None);
        // `${d}` is a prefix of `${duration}`; expanding it first would leave "14uration".
        assert_eq!(expand("${duration}", &r), "3600");
        assert_eq!(expand("${d}", &r), "14");
        // And `${start}` is a prefix of `${start-day}`.
        assert_eq!(expand("${start-day}", &r), "14");
        assert_eq!(expand("${start}", &r), START.to_string());
    }

    #[test]
    fn an_unknown_placeholder_is_left_alone() {
        let r = req("http://h/s", Mode::Shift, None);
        // Better a URL the provider can reject than one silently missing a parameter.
        assert_eq!(expand("${nonsense}", &r), "${nonsense}");
    }

    #[test]
    fn availability_respects_the_providers_window() {
        let now = 1_000_000;
        let day = 86_400;
        // Inside a seven-day window.
        assert!(is_available(now - 3 * day, now, 7));
        assert!(is_available(now - 7 * day, now, 7));
        // Past it.
        assert!(!is_available(now - 8 * day, now, 7));
    }

    #[test]
    fn a_programme_that_has_not_aired_has_nothing_to_catch_up_on() {
        let now = 1_000_000;
        assert!(!is_available(now + 3600, now, 7));
    }

    #[test]
    fn an_unstated_window_is_not_a_refusal() {
        // A channel is only flagged for catch-up at all if the provider said so; days=0
        // means it did not say how far back, not that there is none.
        let now = 1_000_000;
        assert!(is_available(now - 90 * 86_400, now, 0));
    }
}
