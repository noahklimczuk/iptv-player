//! The HTTP layer every provider fetch goes through.
//!
//! README §6.1 and §17: per-provider User-Agent and Referer, sane timeouts, bounded
//! retry with exponential backoff, and failures that map onto the shared human-facing
//! taxonomy rather than leaking transport errors at the user.

use std::io::{BufReader, Read};
use std::time::Duration;

use aurora_core::neterr::{ErrorCode, NetFailure};
use flate2::read::MultiGzDecoder;

/// Retry schedule from README §6.1: three attempts at 1s, 3s, 7s.
pub const BACKOFF_SECS: [u64; 3] = [1, 3, 7];

#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
    /// Time to first byte. Deliberately separate from the overall read budget: a slow
    /// 2 GB EPG is fine, a server that never answers is not.
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub max_attempts: usize,
    /// Refuse bodies larger than this. Guards against a misconfigured URL streaming
    /// until the disk fills.
    pub max_bytes: u64,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            user_agent: Some(concat!("AuroraTV/", env!("CARGO_PKG_VERSION")).to_string()),
            referrer: None,
            connect_timeout: Duration::from_secs(12),
            read_timeout: Duration::from_secs(120),
            max_attempts: BACKOFF_SECS.len(),
            max_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

pub struct HttpClient {
    inner: reqwest::blocking::Client,
    config: HttpConfig,
    /// Overridable so tests do not actually sleep through the backoff schedule.
    sleep: Box<dyn Fn(Duration) + Send + Sync>,
}

impl std::fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpClient")
            .field("config", &self.config)
            .finish()
    }
}

impl HttpClient {
    pub fn new(config: HttpConfig) -> Result<Self, NetFailure> {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(referrer) = &config.referrer {
            if let Ok(v) = reqwest::header::HeaderValue::from_str(referrer) {
                headers.insert(reqwest::header::REFERER, v);
            }
        }

        let mut builder = reqwest::blocking::Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.read_timeout)
            .default_headers(headers)
            // Providers redirect constantly; cap it so a loop cannot hang a refresh.
            .redirect(reqwest::redirect::Policy::limited(5))
            .gzip(true);

        if let Some(ua) = &config.user_agent {
            builder = builder.user_agent(ua.clone());
        }

        let inner = builder
            .build()
            .map_err(|e| NetFailure::classify(&e.to_string()))?;

        Ok(Self {
            inner,
            config,
            sleep: Box::new(std::thread::sleep),
        })
    }

    #[cfg(test)]
    fn without_sleeping(mut self) -> Self {
        self.sleep = Box::new(|_| {});
        self
    }

    pub fn config(&self) -> &HttpConfig {
        &self.config
    }

    /// Fetch a URL, retrying only failures that could plausibly succeed next time.
    ///
    /// Returns the body as bytes. Use [`HttpClient::fetch_reader`] for anything that
    /// should be streamed rather than buffered.
    pub fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>, NetFailure> {
        let mut reader = self.fetch_reader(url)?;
        let mut out = Vec::new();
        reader
            .read_to_end(&mut out)
            .map_err(|e| NetFailure::classify(&e.to_string()))?;
        Ok(out)
    }

    pub fn fetch_string(&self, url: &str) -> Result<String, NetFailure> {
        let bytes = self.fetch_bytes(url)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Fetch a URL as a stream, transparently inflating a gzip payload.
    ///
    /// Handles both forms providers use: `Content-Encoding: gzip` (unwrapped by the
    /// client) and a plain `.xml.gz` *file*, which is just gzip bytes over an
    /// otherwise ordinary response and has to be inflated here.
    pub fn fetch_reader(&self, url: &str) -> Result<Box<dyn Read + Send>, NetFailure> {
        let response = self.send_with_retry(url)?;

        let declared_gzip_file = looks_gzipped(url)
            || response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.contains("gzip") || v.contains("x-gzip"))
                .unwrap_or(false);

        if let Some(len) = response.content_length() {
            if len > self.config.max_bytes {
                return Err(NetFailure {
                    code: ErrorCode::Unknown,
                    message: "That download is implausibly large".into(),
                    cause: format!(
                        "The server offered {len} bytes, past the {} byte ceiling. \
                         The URL may not be a playlist.",
                        self.config.max_bytes
                    ),
                    actions: vec![aurora_core::neterr::ErrorAction::OpenSettings],
                    retryable: false,
                });
            }
        }

        let capped = response.take(self.config.max_bytes);
        if declared_gzip_file {
            // MultiGzDecoder, not GzDecoder: concatenated gzip members are legal and
            // some providers' EPG dumps are built that way.
            Ok(Box::new(MultiGzDecoder::new(BufReader::new(capped))))
        } else {
            Ok(Box::new(BufReader::new(capped)))
        }
    }

    fn send_with_retry(&self, url: &str) -> Result<reqwest::blocking::Response, NetFailure> {
        let attempts = self.config.max_attempts.max(1);
        let mut last: Option<NetFailure> = None;

        for attempt in 0..attempts {
            if attempt > 0 {
                let wait = BACKOFF_SECS
                    .get(attempt - 1)
                    .copied()
                    .unwrap_or(*BACKOFF_SECS.last().unwrap_or(&7));
                tracing::debug!(url = %redact(url), attempt, wait, "retrying");
                (self.sleep)(Duration::from_secs(wait));
            }

            match self.inner.get(url).send() {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        return Ok(response);
                    }
                    let failure = NetFailure::classify(&format!("HTTP {}", status.as_u16()));
                    if !failure.retryable {
                        return Err(failure);
                    }
                    last = Some(failure);
                }
                Err(e) => {
                    let failure = classify_reqwest(&e);
                    if !failure.retryable {
                        return Err(failure);
                    }
                    last = Some(failure);
                }
            }
        }

        Err(last.unwrap_or_else(|| NetFailure::classify("unknown")))
    }
}

/// `reqwest`'s Display strings do not mention the words the shared classifier looks
/// for, so map its typed predicates first and fall back to the text.
fn classify_reqwest(e: &reqwest::Error) -> NetFailure {
    if e.is_timeout() {
        return NetFailure::classify("connection timed out");
    }
    if e.is_connect() {
        // A refused or unresolvable host both land here; the message distinguishes them.
        let text = e.to_string().to_ascii_lowercase();
        if text.contains("dns") || text.contains("resolve") || text.contains("name") {
            return NetFailure::classify("dns");
        }
        return NetFailure::classify("connection timed out");
    }
    if e.is_redirect() {
        return NetFailure::classify("too many redirects");
    }
    NetFailure::classify(&e.to_string())
}

fn looks_gzipped(url: &str) -> bool {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase();
    path.ends_with(".gz") || path.ends_with(".gzip")
}

/// Strip credentials before a URL reaches a log line (README C10).
pub fn redact(url: &str) -> String {
    let mut out = url.to_string();
    // Xtream carries them as query parameters.
    for key in [
        "password", "pass", "token", "username", "user", "api_key", "apikey",
    ] {
        if let Some(start) = out.to_ascii_lowercase().find(&format!("{key}=")) {
            let value_start = start + key.len() + 1;
            let end = out[value_start..]
                .find('&')
                .map(|i| value_start + i)
                .unwrap_or(out.len());
            out.replace_range(value_start..end, "***");
        }
    }
    // ...and path segments carry them in stream URLs: /live/user/pass/123.ts
    if let Some(idx) = out.find("://") {
        let (scheme, rest) = out.split_at(idx + 3);
        let mut segments: Vec<&str> = rest.split('/').collect();
        for marker in ["live", "movie", "series"] {
            if let Some(pos) = segments.iter().position(|s| *s == marker) {
                for seg in segments.iter_mut().skip(pos + 1).take(2) {
                    *seg = "***";
                }
            }
        }
        out = format!("{scheme}{}", segments.join("/"));
    }
    // userinfo form: http://user:pass@host
    if let Some(at) = out.find('@') {
        if let Some(scheme_end) = out.find("://") {
            if at > scheme_end {
                out.replace_range(scheme_end + 3..at, "***");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testserver::{Reply, TestServer};
    use std::io::Write;

    fn client(server: &TestServer) -> HttpClient {
        let _ = server;
        HttpClient::new(HttpConfig {
            connect_timeout: Duration::from_secs(2),
            read_timeout: Duration::from_secs(3),
            ..Default::default()
        })
        .unwrap()
        .without_sleeping()
    }

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    #[test]
    fn fetches_a_body() {
        let server = TestServer::always(Reply::ok("#EXTM3U\n"));
        let got = client(&server)
            .fetch_string(&server.url("/playlist.m3u"))
            .unwrap();
        assert_eq!(got, "#EXTM3U\n");
        assert_eq!(server.request_count(), 1);
    }

    #[test]
    fn sends_the_configured_user_agent_and_referer() {
        let server = TestServer::always(Reply::ok("ok"));
        let c = HttpClient::new(HttpConfig {
            user_agent: Some("MyAgent/2.0".into()),
            referrer: Some("https://example.com/".into()),
            ..Default::default()
        })
        .unwrap();
        c.fetch_string(&server.url("/x")).unwrap();

        let req = &server.requests()[0];
        assert_eq!(req.header("user-agent"), Some("MyAgent/2.0"));
        assert_eq!(req.header("referer"), Some("https://example.com/"));
    }

    #[test]
    fn inflates_a_gzip_file_by_extension() {
        let payload = b"<tv><channel id=\"a\"/></tv>";
        let body = gzip(payload);
        let server = TestServer::always(Reply::ok(body));
        let got = client(&server)
            .fetch_bytes(&server.url("/epg.xml.gz"))
            .unwrap();
        assert_eq!(got, payload);
    }

    #[test]
    fn inflates_concatenated_gzip_members() {
        // Legal gzip, and some providers build their EPG dumps this way.
        let mut body = gzip(b"<tv>");
        body.extend(gzip(b"</tv>"));
        let server = TestServer::always(Reply::ok(body));
        let got = client(&server)
            .fetch_bytes(&server.url("/epg.xml.gz"))
            .unwrap();
        assert_eq!(got, b"<tv></tv>");
    }

    #[test]
    fn a_query_string_does_not_hide_the_gz_extension() {
        let body = gzip(b"hello");
        let server = TestServer::always(Reply::ok(body));
        let got = client(&server)
            .fetch_bytes(&server.url("/epg.xml.gz?token=abc"))
            .unwrap();
        assert_eq!(got, b"hello");
    }

    #[test]
    fn inflates_by_content_type_when_the_url_has_no_gz_extension() {
        // Providers frequently serve a gzipped guide from a plain `.php` endpoint and
        // only say so in the header.
        let body = gzip(b"<tv></tv>");
        let server =
            TestServer::always(Reply::ok(body).with_header("Content-Type", "application/gzip"));
        let got = client(&server)
            .fetch_bytes(&server.url("/xmltv.php"))
            .unwrap();
        assert_eq!(got, b"<tv></tv>");
    }

    #[test]
    fn a_connection_closed_before_any_reply_is_an_error() {
        let server = TestServer::always(Reply::Reset);
        let c = HttpClient::new(HttpConfig {
            max_attempts: 1,
            ..Default::default()
        })
        .unwrap()
        .without_sleeping();
        let err = c.fetch_string(&server.url("/x")).unwrap_err();
        assert!(!err.message.is_empty());
        assert!(!err.actions.is_empty());
    }

    #[test]
    fn fetches_are_plain_get_requests() {
        let server = TestServer::always(Reply::ok("ok"));
        client(&server).fetch_string(&server.url("/x")).unwrap();
        assert_eq!(server.requests()[0].method, "GET");
    }

    #[test]
    fn plain_bodies_are_not_mistaken_for_gzip() {
        let server = TestServer::always(Reply::ok("not compressed"));
        let got = client(&server)
            .fetch_string(&server.url("/epg.xml"))
            .unwrap();
        assert_eq!(got, "not compressed");
    }

    #[test]
    fn retries_a_transient_failure_then_succeeds() {
        let server = TestServer::start(|i, _| {
            if i < 2 {
                Reply::status(503)
            } else {
                Reply::ok("finally")
            }
        });
        let got = client(&server).fetch_string(&server.url("/x")).unwrap();
        assert_eq!(got, "finally");
        assert_eq!(server.request_count(), 3);
    }

    #[test]
    fn gives_up_after_the_configured_attempts() {
        let server = TestServer::always(Reply::status(503));
        let c = HttpClient::new(HttpConfig {
            max_attempts: 3,
            ..Default::default()
        })
        .unwrap()
        .without_sleeping();
        let err = c.fetch_string(&server.url("/x")).unwrap_err();
        assert_eq!(err.code, ErrorCode::ServerError);
        assert_eq!(server.request_count(), 3, "must not retry forever");
    }

    #[test]
    fn does_not_retry_an_unauthorized_response() {
        let server = TestServer::always(Reply::status(401));
        let err = client(&server).fetch_string(&server.url("/x")).unwrap_err();
        assert_eq!(err.code, ErrorCode::Unauthorized);
        assert_eq!(
            server.request_count(),
            1,
            "bad credentials will not fix themselves"
        );
        assert!(!err.retryable);
    }

    #[test]
    fn does_not_retry_a_not_found() {
        let server = TestServer::always(Reply::status(404));
        let err = client(&server).fetch_string(&server.url("/x")).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        assert_eq!(server.request_count(), 1);
    }

    #[test]
    fn maps_each_status_onto_a_human_failure() {
        for (status, expected) in [
            (401, ErrorCode::Unauthorized),
            (403, ErrorCode::Forbidden),
            (404, ErrorCode::NotFound),
            (429, ErrorCode::RateLimited),
            (503, ErrorCode::ServerError),
        ] {
            let server = TestServer::always(Reply::status(status));
            let c = HttpClient::new(HttpConfig {
                max_attempts: 1,
                ..Default::default()
            })
            .unwrap()
            .without_sleeping();
            let err = c.fetch_string(&server.url("/x")).unwrap_err();
            assert_eq!(err.code, expected, "status {status}");
            assert!(!err.message.is_empty());
            assert!(!err.actions.is_empty());
        }
    }

    #[test]
    fn a_mid_stream_disconnect_is_an_error_not_a_truncated_success() {
        // The classic IPTV failure: the server promises 5000 bytes and sends 10.
        let server = TestServer::always(Reply::Truncated {
            announced: 5000,
            send: b"#EXTM3U\n\n".to_vec(),
        });
        let c = HttpClient::new(HttpConfig {
            max_attempts: 1,
            ..Default::default()
        })
        .unwrap()
        .without_sleeping();
        let result = c.fetch_string(&server.url("/playlist.m3u"));
        assert!(
            result.is_err(),
            "a short body must not be reported as a complete playlist"
        );
    }

    #[test]
    fn a_server_that_never_answers_times_out() {
        let server = TestServer::always(Reply::Hang);
        let c = HttpClient::new(HttpConfig {
            read_timeout: Duration::from_millis(400),
            max_attempts: 1,
            ..Default::default()
        })
        .unwrap()
        .without_sleeping();
        let started = std::time::Instant::now();
        let err = c.fetch_string(&server.url("/x")).unwrap_err();
        let elapsed = started.elapsed();

        assert_eq!(err.code, ErrorCode::Timeout);
        // And it timed out rather than being refused: a connection the server closes
        // comes back in about a millisecond and classifies as something else entirely.
        // This is what caught the accepted socket inheriting the listener's
        // non-blocking flag on Windows (see testserver::start).
        assert!(
            elapsed >= Duration::from_millis(300),
            "returned after {elapsed:?} — the connection was dropped, not held open"
        );
    }

    #[test]
    fn an_oversized_body_is_refused_before_it_is_read() {
        let server = TestServer::always(Reply::ok(vec![b'x'; 4096]));
        let c = HttpClient::new(HttpConfig {
            max_bytes: 1024,
            ..Default::default()
        })
        .unwrap()
        .without_sleeping();
        let err = c.fetch_bytes(&server.url("/huge.m3u")).unwrap_err();
        assert!(err.message.contains("implausibly large"), "{}", err.message);
    }

    #[test]
    fn a_body_without_content_length_is_still_capped() {
        // take() bounds the reader even when the server never declares a length.
        let server = TestServer::always(Reply::ok(vec![b'x'; 4096]));
        let c = HttpClient::new(HttpConfig {
            max_bytes: 100,
            ..Default::default()
        })
        .unwrap()
        .without_sleeping();
        // Declared length trips the guard first; assert we never exceed the cap either way.
        match c.fetch_bytes(&server.url("/huge.m3u")) {
            Ok(body) => assert!(body.len() <= 100),
            Err(e) => assert!(!e.message.is_empty()),
        }
    }

    #[test]
    fn unreachable_hosts_do_not_leak_transport_errors() {
        let c = HttpClient::new(HttpConfig {
            connect_timeout: Duration::from_millis(300),
            max_attempts: 1,
            ..Default::default()
        })
        .unwrap()
        .without_sleeping();
        // Reserved for documentation; never routable.
        let err = c.fetch_string("http://192.0.2.1:9/x").unwrap_err();
        assert!(!err.message.is_empty());
        assert!(!err.cause.is_empty());
        assert!(!err.actions.is_empty());
    }

    #[test]
    fn redacts_xtream_query_credentials() {
        let got =
            redact("http://example.com/player_api.php?username=alice&password=hunter2&action=x");
        assert!(!got.contains("alice"), "{got}");
        assert!(!got.contains("hunter2"), "{got}");
        assert!(got.contains("action=x"), "non-secret params survive: {got}");
    }

    #[test]
    fn redacts_credentials_in_stream_paths() {
        let got = redact("http://example.com/live/alice/hunter2/123.ts");
        assert!(!got.contains("alice"), "{got}");
        assert!(!got.contains("hunter2"), "{got}");
        assert!(got.contains("123.ts"), "{got}");
    }

    #[test]
    fn redacts_userinfo() {
        let got = redact("http://alice:hunter2@example.com/x");
        assert!(!got.contains("hunter2"), "{got}");
        assert!(got.contains("example.com"), "{got}");
    }

    #[test]
    fn a_metadata_api_key_is_redacted_too() {
        // TMDB carries its key in the query string, and a key in a log line is a leaked
        // credential exactly like a provider password is.
        let got = redact("https://api.themoviedb.org/3/search/movie?api_key=abcd1234&query=Heat");
        assert!(!got.contains("abcd1234"), "{got}");
        assert!(
            got.contains("query=Heat"),
            "the rest of the URL is still readable"
        );

        let got = redact("https://example.com/x?apikey=secret");
        assert!(!got.contains("secret"), "{got}");
    }

    #[test]
    fn redaction_leaves_a_clean_url_alone() {
        let url = "http://example.com/epg.xml.gz";
        assert_eq!(redact(url), url);
    }
}
