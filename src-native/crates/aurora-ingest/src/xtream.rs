//! Xtream Codes client (README §4.1).
//!
//! The endpoint surface is small but the *forks* are not: fields go missing, numbers
//! arrive as strings, arrays arrive as null, and a failed auth is frequently an HTML
//! page with HTTP 200. `aurora_core::xtream` already models that leniency; this module
//! is the transport plus the flow on top.

use aurora_core::neterr::{ErrorAction, ErrorCode, NetFailure};
use aurora_core::xtream::{
    parse_json, AuthResponse, Category, LiveStream, SeriesListing, VodStream,
};

use crate::http::{redact, HttpClient};

pub struct XtreamClient<'a> {
    http: &'a HttpClient,
    base_url: String,
    username: String,
    password: String,
}

/// What the account itself says about the subscription, surfaced in Settings.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountStatus {
    pub active: bool,
    pub expires_at: Option<i64>,
    pub days_until_expiry: Option<i64>,
    pub max_connections: Option<u32>,
    pub active_connections: Option<u32>,
    pub is_trial: bool,
}

impl<'a> XtreamClient<'a> {
    pub fn new(http: &'a HttpClient, base_url: &str, username: &str, password: &str) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            username: username.to_string(),
            password: password.to_string(),
        }
    }

    fn api_url(&self, action: Option<&str>) -> String {
        let mut url = format!(
            "{}/player_api.php?username={}&password={}",
            self.base_url,
            urlencode(&self.username),
            urlencode(&self.password)
        );
        if let Some(action) = action {
            url.push_str("&action=");
            url.push_str(action);
        }
        url
    }

    /// The name an error should use for a request, since the URL carries credentials.
    fn label(action: Option<&str>) -> &str {
        action.unwrap_or("the account check")
    }

    /// Fetch the body of one API call. Transport failures only — parsing is the
    /// caller's, because how much a bad payload matters depends on what was asked for.
    fn fetch(&self, action: Option<&str>) -> Result<String, NetFailure> {
        let url = self.api_url(action);
        // Info, not debug: when an import fails against a real panel, the sequence of
        // requests is the whole diagnosis, and nobody hits a problem with the log level
        // already turned up. Redacted — the URL carries the password.
        tracing::info!(url = %redact(&url), "xtream request");
        self.http.fetch_string(&url)
    }

    fn get<T: serde::de::DeserializeOwned>(&self, action: Option<&str>) -> Result<T, NetFailure> {
        let body = self.fetch(action)?;
        parse_json(&body).map_err(|e| provider_failure(Self::label(action), &e.to_string()))
    }

    /// A list whose absence is not worth failing the import over.
    ///
    /// Categories are group *labels* and nothing else — a channel with no category is
    /// a channel in no group, not a channel that cannot be imported. Panels really do
    /// answer these with an empty body, and aborting a forty-thousand-entry import
    /// because the group names did not arrive is the wrong trade.
    ///
    /// A transport failure still propagates: a panel that has stopped answering is a
    /// different thing from one that answered with nothing.
    fn get_optional_list<T: serde::de::DeserializeOwned>(
        &self,
        action: &str,
    ) -> Result<Vec<T>, NetFailure> {
        let body = self.fetch(Some(action))?;
        match parse_json::<Vec<T>>(&body) {
            Ok(list) => Ok(list),
            Err(e) => {
                tracing::warn!(action, cause = %e, "no usable categories; importing without groups");
                Ok(Vec::new())
            }
        }
    }

    /// Authenticate and read the account's own view of itself.
    ///
    /// A panel that answers but reports an inactive line is an authentication
    /// failure as far as the user is concerned, so it is reported as one rather
    /// than as an empty library later.
    pub fn authenticate(&self, now_unix: i64) -> Result<AccountStatus, NetFailure> {
        let auth: AuthResponse = self.get(None)?;

        if auth.user_info.username.is_none() && auth.user_info.status.is_none() {
            return Err(NetFailure {
                code: ErrorCode::Unauthorized,
                message: "Your provider did not accept those details".into(),
                cause: "The panel answered but returned no account information, which \
                        usually means the username or password is wrong."
                    .into(),
                actions: vec![ErrorAction::OpenSettings],
                retryable: false,
            });
        }

        if !auth.is_active() {
            let status = auth
                .user_info
                .status
                .clone()
                .unwrap_or_else(|| "unknown".into());
            return Err(NetFailure {
                code: ErrorCode::Unauthorized,
                message: format!("This subscription is not active ({status})"),
                cause: "The provider recognised the account but will not serve it. It may \
                        have expired or been suspended."
                    .into(),
                actions: vec![ErrorAction::OpenSettings],
                retryable: false,
            });
        }

        Ok(AccountStatus {
            active: true,
            expires_at: auth.user_info.exp_date.map(|d| d as i64),
            days_until_expiry: auth.days_until_expiry(now_unix),
            max_connections: auth.user_info.max_connections,
            active_connections: auth.user_info.active_cons,
            is_trial: auth.user_info.is_trial.unwrap_or(0) > 0,
        })
    }

    pub fn live_categories(&self) -> Result<Vec<Category>, NetFailure> {
        self.get_optional_list("get_live_categories")
    }

    pub fn vod_categories(&self) -> Result<Vec<Category>, NetFailure> {
        self.get_optional_list("get_vod_categories")
    }

    pub fn series_categories(&self) -> Result<Vec<Category>, NetFailure> {
        self.get_optional_list("get_series_categories")
    }

    pub fn live_streams(&self) -> Result<Vec<LiveStream>, NetFailure> {
        self.get(Some("get_live_streams"))
    }

    pub fn vod_streams(&self) -> Result<Vec<VodStream>, NetFailure> {
        self.get(Some("get_vod_streams"))
    }

    pub fn series(&self) -> Result<Vec<SeriesListing>, NetFailure> {
        self.get(Some("get_series"))
    }

    /// The provider's own EPG endpoint, for the XMLTV importer.
    pub fn xmltv_url(&self) -> String {
        format!(
            "{}/xmltv.php?username={}&password={}",
            self.base_url,
            urlencode(&self.username),
            urlencode(&self.password)
        )
    }

    /// Build the playable URL for a stream (delegates to the shared builder so the
    /// shape stays in one place).
    pub fn stream_url(
        &self,
        kind: aurora_core::model::MediaKind,
        stream_id: u32,
        extension: Option<&str>,
    ) -> String {
        aurora_core::xtream::stream_url(
            &self.base_url,
            &self.username,
            &self.password,
            kind,
            stream_id,
            extension,
        )
    }
}

fn provider_failure(request: &str, detail: &str) -> NetFailure {
    let lower = detail.to_ascii_lowercase();
    // The classic fork behaviour: an error page served with HTTP 200.
    if lower.contains("html") {
        return NetFailure {
            code: ErrorCode::Unauthorized,
            message: "Your provider returned a web page instead of data".into(),
            cause: "That usually means the credentials were rejected, or the panel is \
                    down for maintenance."
                .into(),
            actions: vec![ErrorAction::Retry, ErrorAction::OpenSettings],
            retryable: false,
        };
    }
    NetFailure {
        code: ErrorCode::Unknown,
        message: "Your provider sent something Aurora could not read".into(),
        // Naming the request matters more than it looks: six different calls can fail
        // this way, and without the name the message says nothing about which panel
        // feature is broken or whether the import got anywhere at all.
        cause: format!("The response to {request} was not valid JSON. {detail}"),
        actions: vec![ErrorAction::Retry, ErrorAction::ReportBroken],
        retryable: true,
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_empty_categories_response_does_not_stop_the_import() {
        // A real panel did this: the account check answered, and one of the category
        // calls came back with nothing at all, which aborted the whole first import.
        // Group names are decoration; the channels behind them are not.
        let server = TestServer::start(|_, req| {
            if req.path.contains("action=get_live_categories") {
                Reply::ok("")
            } else {
                Reply::ok(r#"[{"stream_id":1,"name":"CNN","num":101}]"#)
            }
        });
        let http = client();
        let xtream = XtreamClient::new(&http, &server.url(""), "u", "p");

        assert!(xtream.live_categories().expect("categories").is_empty());
        assert_eq!(xtream.live_streams().expect("streams").len(), 1);
    }

    #[test]
    fn categories_that_are_an_error_page_are_also_survivable() {
        let server = TestServer::start(|_, req| {
            if req.path.contains("categories") {
                Reply::ok("<html><body>nope</body></html>")
            } else {
                Reply::ok("[]")
            }
        });
        let http = client();
        let xtream = XtreamClient::new(&http, &server.url(""), "u", "p");
        assert!(xtream.vod_categories().expect("categories").is_empty());
    }

    #[test]
    fn a_panel_that_stops_answering_still_fails_the_import() {
        // The distinction that makes the tolerance safe: nothing-in-the-body is
        // survivable, a panel that has gone away is not.
        let server = TestServer::always(Reply::status(500));
        let http = client();
        let xtream = XtreamClient::new(&http, &server.url(""), "u", "p");
        assert!(xtream.live_categories().is_err());
    }

    #[test]
    fn an_empty_stream_list_names_the_request_that_failed() {
        // Six calls can fail this way. A message that does not say which one leaves
        // nobody able to tell whether the import got anywhere.
        let server = TestServer::start(|_, req| {
            if req.path.contains("action=get_live_streams") {
                Reply::ok("")
            } else {
                Reply::ok("[]")
            }
        });
        let http = client();
        let xtream = XtreamClient::new(&http, &server.url(""), "u", "p");

        let failure = xtream.live_streams().expect_err("should fail");
        assert!(
            failure.cause.contains("get_live_streams"),
            "the cause must name the request, got {:?}",
            failure.cause
        );
    }

    #[test]
    fn the_account_check_is_named_too() {
        let server = TestServer::always(Reply::ok(""));
        let http = client();
        let xtream = XtreamClient::new(&http, &server.url(""), "u", "p");

        let failure = xtream.authenticate(0).expect_err("should fail");
        assert!(
            failure.cause.contains("the account check"),
            "got {:?}",
            failure.cause
        );
    }

    use super::*;
    use crate::http::{HttpClient, HttpConfig};
    use crate::testserver::{Reply, TestServer};

    fn client() -> HttpClient {
        HttpClient::new(HttpConfig {
            max_attempts: 1,
            ..Default::default()
        })
        .unwrap()
    }

    const ACTIVE: &str = r#"{"user_info":{"username":"alice","status":"Active",
        "exp_date":"864000","max_connections":"2","active_cons":1,"is_trial":"0"}}"#;

    #[test]
    fn authenticates_and_reports_the_account() {
        let server = TestServer::always(Reply::ok(ACTIVE));
        let http = client();
        let x = XtreamClient::new(&http, &server.url(""), "alice", "hunter2");

        let status = x.authenticate(0).unwrap();
        assert!(status.active);
        assert_eq!(status.days_until_expiry, Some(10));
        assert_eq!(status.max_connections, Some(2));
        assert_eq!(status.active_connections, Some(1));
        assert!(!status.is_trial);
    }

    #[test]
    fn credentials_are_sent_url_encoded() {
        let server = TestServer::always(Reply::ok(ACTIVE));
        let http = client();
        let x = XtreamClient::new(&http, &server.url(""), "a@b.com", "p w&x");
        x.authenticate(0).unwrap();

        let path = &server.requests()[0].path;
        assert!(path.contains("a%40b.com"), "{path}");
        assert!(path.contains("p%20w%26x"), "{path}");
    }

    #[test]
    fn an_inactive_line_is_an_authentication_failure() {
        let server = TestServer::always(Reply::ok(
            r#"{"user_info":{"username":"a","status":"Expired"}}"#,
        ));
        let http = client();
        let x = XtreamClient::new(&http, &server.url(""), "a", "b");

        let err = x.authenticate(0).unwrap_err();
        assert_eq!(err.code, ErrorCode::Unauthorized);
        assert!(err.message.contains("Expired"), "{}", err.message);
        assert!(!err.retryable);
    }

    #[test]
    fn an_empty_user_info_is_a_rejected_login_not_an_empty_library() {
        let server = TestServer::always(Reply::ok(r#"{"user_info":{}}"#));
        let http = client();
        let x = XtreamClient::new(&http, &server.url(""), "a", "b");

        let err = x.authenticate(0).unwrap_err();
        assert_eq!(err.code, ErrorCode::Unauthorized);
        assert!(err.cause.contains("username or password"), "{}", err.cause);
    }

    #[test]
    fn an_html_error_page_with_http_200_is_explained() {
        let server = TestServer::always(Reply::ok("<html><body>Access denied</body></html>"));
        let http = client();
        let x = XtreamClient::new(&http, &server.url(""), "a", "b");

        let err = x.authenticate(0).unwrap_err();
        assert!(err.message.contains("web page"), "{}", err.message);
        assert!(!err.retryable);
    }

    #[test]
    fn lists_survive_fork_shaped_json() {
        // Numbers as strings, null where an array belongs, missing fields.
        let body = r#"[
            {"stream_id":"101","name":"CNN","tv_archive":"1","num":202},
            {"stream_id":102},
            {"stream_id":103,"name":"BBC","epg_channel_id":null}
        ]"#;
        let server = TestServer::always(Reply::ok(body));
        let http = client();
        let x = XtreamClient::new(&http, &server.url(""), "a", "b");

        let streams = x.live_streams().unwrap();
        assert_eq!(streams.len(), 3);
        assert_eq!(streams[0].stream_id, Some(101));
        assert!(streams[0].has_catchup());
        assert_eq!(
            streams[1].name, None,
            "a missing name must not abort the import"
        );
    }

    #[test]
    fn each_action_hits_its_endpoint() {
        let server = TestServer::always(Reply::ok("[]"));
        let http = client();
        let x = XtreamClient::new(&http, &server.url(""), "a", "b");

        x.live_categories().unwrap();
        x.vod_streams().unwrap();
        x.series().unwrap();

        let paths: Vec<String> = server.requests().iter().map(|r| r.path.clone()).collect();
        assert!(
            paths[0].contains("action=get_live_categories"),
            "{:?}",
            paths
        );
        assert!(paths[1].contains("action=get_vod_streams"), "{:?}", paths);
        assert!(paths[2].contains("action=get_series"), "{:?}", paths);
    }

    #[test]
    fn a_401_surfaces_as_bad_credentials() {
        let server = TestServer::always(Reply::status(401));
        let http = client();
        let x = XtreamClient::new(&http, &server.url(""), "a", "b");
        assert_eq!(x.authenticate(0).unwrap_err().code, ErrorCode::Unauthorized);
    }

    #[test]
    fn builds_the_epg_and_stream_urls() {
        let http = client();
        let x = XtreamClient::new(&http, "http://example.com/", "alice", "pw");

        let epg = x.xmltv_url();
        assert!(epg.starts_with("http://example.com/xmltv.php"), "{epg}");
        assert!(epg.contains("username=alice"));

        let live = x.stream_url(aurora_core::model::MediaKind::Live, 12, None);
        assert_eq!(live, "http://example.com/live/alice/pw/12.ts");
    }

    #[test]
    fn a_trailing_slash_on_the_base_url_does_not_double_up() {
        let http = client();
        let x = XtreamClient::new(&http, "http://example.com///", "a", "b");
        assert!(!x.xmltv_url().contains("///xmltv"), "{}", x.xmltv_url());
    }
}
