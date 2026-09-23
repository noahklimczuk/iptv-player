//! Asking GitHub whether there is a newer build than the one running.
//!
//! The app ships as an installer from a release, not a store, so nothing tells a
//! viewer that a fix exists — this does. It only *checks*: it reports what is
//! published and leaves installing to the person, because downloading and executing
//! an installer on the strength of an HTTP response is a different kind of decision
//! and wants signature verification behind it (docs/DECISIONS.md D17).
//!
//! The comparison is `aurora_core::version`, deliberately: a string compare would
//! decide 0.9.0 is newer than 0.10.0 and then never offer another update.

use std::sync::Arc;

use aurora_core::neterr::{ErrorAction, ErrorCode, NetFailure};
use aurora_core::version::Version;
use serde::{Deserialize, Serialize};

use crate::http::HttpClient;

pub const DEFAULT_API_BASE_URL: &str = "https://api.github.com";

/// How long a successful check stays fresh.
///
/// Six hours rather than every launch: a viewer who restarts the app four times in an
/// evening has not become more likely to find a new build, and GitHub's unauthenticated
/// allowance is per address, shared with everything else on the network.
pub const CHECK_INTERVAL_SECS: i64 = 6 * 60 * 60;

/// Whether a background check is due.
///
/// A clock that moved backwards makes `last` look like the future; that counts as due
/// rather than blocking checks until real time catches up, which on a machine whose
/// clock was wrong by a year would be forever.
pub fn due(last_checked: Option<i64>, now: i64) -> bool {
    match last_checked {
        None => true,
        Some(last) => now < last || now - last >= CHECK_INTERVAL_SECS,
    }
}

/// Where builds come from. A constant rather than a setting: pointing the updater at
/// an arbitrary host is how an update check becomes a way to install something else.
pub const DEFAULT_REPO: &str = "noahklimczuk/iptv-player";

/// A published build, once its tag has been understood as a version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: Version,
    pub tag: String,
    /// The release notes, as written. Shown to the viewer, so it is worth saying that
    /// this is text from the release page and nothing interprets it.
    pub notes: String,
    /// The release page, which is what "Download" opens.
    pub page_url: String,
    /// The Windows installer, when the release carries one.
    pub installer_url: Option<String>,
    pub installer_bytes: Option<u64>,
    pub published_at: Option<String>,
}

/// The answer to "is there anything newer?".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub current: Version,
    /// The newest published release, or `None` when nothing is published yet or the
    /// newest release is not tagged with a version this understands.
    pub latest: Option<Release>,
    /// Whether `latest` is actually newer than `current`. Kept as its own field so the
    /// UI never has to re-derive an ordering the host already worked out.
    pub available: bool,
}

/* ── What GitHub sends ──────────────────────────────────────────────────────── */

#[derive(Debug, Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Debug, Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Updates {
    http: Arc<HttpClient>,
    repo: String,
    api_base_url: String,
}

impl Updates {
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self {
            http,
            repo: DEFAULT_REPO.into(),
            api_base_url: DEFAULT_API_BASE_URL.into(),
        }
    }

    /// Point at a different API host. Tests use it; nothing in the app does.
    pub fn with_api_base_url(mut self, base: impl Into<String>) -> Self {
        self.api_base_url = base.into();
        self
    }

    pub fn with_repo(mut self, repo: impl Into<String>) -> Self {
        self.repo = repo.into();
        self
    }

    /// The newest published release, or `None` if there is nothing usable to report.
    ///
    /// "Nothing usable" covers two cases that are emphatically not errors: a
    /// repository with no releases yet, and a newest release whose tag is not a
    /// version — this project published `latest-windows` for a while, and an updater
    /// that threw an error at the sight of it would be wrong rather than careful.
    pub fn latest(&self) -> Result<Option<Release>, NetFailure> {
        let url = format!("{}/repos/{}/releases/latest", self.api_base_url, self.repo);

        let body = match self.http.fetch_string(&url) {
            Ok(body) => body,
            // 404 is GitHub's way of saying there are no releases at all.
            Err(failure) if failure.code == ErrorCode::NotFound => return Ok(None),
            Err(failure) => return Err(reword(failure)),
        };

        let release: ApiRelease = serde_json::from_str(&body).map_err(|e| NetFailure {
            code: ErrorCode::Unknown,
            message: "Couldn't read the update information".into(),
            cause: format!("GitHub's answer was not the JSON this expected: {e}"),
            actions: vec![ErrorAction::Retry],
            retryable: true,
        })?;

        let Some(version) = Version::parse(&release.tag_name) else {
            tracing::debug!(tag = %release.tag_name, "latest release is not version-tagged");
            return Ok(None);
        };

        // The single-file installer, which is the only asset worth pointing at. The MSI
        // and the portable zip are for people who already know which they want.
        let installer = release
            .assets
            .iter()
            .find(|a| a.name.to_ascii_lowercase().ends_with(".exe"));

        Ok(Some(Release {
            version,
            page_url: release.html_url.unwrap_or_else(|| {
                format!(
                    "https://github.com/{}/releases/tag/{}",
                    self.repo, release.tag_name
                )
            }),
            tag: release.tag_name,
            notes: release.body.unwrap_or_default(),
            installer_url: installer.map(|a| a.browser_download_url.clone()),
            installer_bytes: installer.and_then(|a| a.size),
            published_at: release.published_at,
        }))
    }

    /// Compare what is published against what is running.
    pub fn check(&self, current: Version) -> Result<UpdateCheck, NetFailure> {
        let latest = self.latest()?;
        let available = latest.as_ref().is_some_and(|r| r.version > current);
        Ok(UpdateCheck {
            current,
            latest,
            available,
        })
    }
}

/// Re-word a transport failure for this context.
///
/// `NetFailure::classify` speaks in terms of the viewer's IPTV provider, because that
/// is what almost every fetch in this app is. "Your provider refused this stream" is
/// nonsense when the fetch was an update check, so the codes that can plausibly come
/// back from GitHub get their own wording and everything else is left alone.
fn reword(failure: NetFailure) -> NetFailure {
    let (message, cause) = match failure.code {
        ErrorCode::Forbidden | ErrorCode::RateLimited => (
            "GitHub is rate-limiting update checks",
            "Unauthenticated checks share an hourly allowance per address. It clears by \
             itself; the release page always works in a browser.",
        ),
        ErrorCode::Dns | ErrorCode::Refused | ErrorCode::Timeout => (
            "Couldn't reach GitHub to check for updates",
            "This usually means no internet connection rather than anything wrong with \
             the app.",
        ),
        ErrorCode::ServerError => (
            "GitHub couldn't answer the update check",
            "Their end, not yours. Worth trying again later.",
        ),
        _ => return failure,
    };
    NetFailure {
        code: failure.code,
        message: message.into(),
        cause: cause.into(),
        actions: vec![ErrorAction::Retry],
        retryable: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpClient, HttpConfig};
    use crate::testserver::{Reply, TestServer};

    fn client() -> Arc<HttpClient> {
        Arc::new(
            HttpClient::new(HttpConfig {
                // One attempt: these tests are about what an answer means, not retries,
                // and the backoff schedule would make them sleep for eleven seconds.
                max_attempts: 1,
                ..Default::default()
            })
            .expect("client"),
        )
    }

    fn updates(server: &TestServer) -> Updates {
        Updates::new(client())
            .with_api_base_url(server.url(""))
            .with_repo("owner/repo")
    }

    fn release_json(tag: &str, assets: &str) -> String {
        format!(
            r#"{{
              "tag_name": "{tag}",
              "body": "Fixed the thing.",
              "html_url": "https://github.com/owner/repo/releases/tag/{tag}",
              "published_at": "2026-09-23T03:13:28Z",
              "assets": [{assets}]
            }}"#
        )
    }

    const INSTALLER: &str = r#"{
      "name": "Aurora-TV-0.2.0-x64-setup.exe",
      "browser_download_url": "https://github.com/owner/repo/releases/download/v0.2.0/setup.exe",
      "size": 38767916
    }"#;

    #[test]
    fn a_first_run_is_always_due_a_check() {
        assert!(due(None, 1_700_000_000));
    }

    #[test]
    fn a_check_just_done_is_not_due_again() {
        let now = 1_700_000_000;
        assert!(!due(Some(now), now));
        assert!(!due(Some(now - CHECK_INTERVAL_SECS + 1), now));
    }

    #[test]
    fn a_stale_check_is_due() {
        let now = 1_700_000_000;
        assert!(due(Some(now - CHECK_INTERVAL_SECS), now));
        assert!(due(Some(now - 7 * 24 * 60 * 60), now));
    }

    #[test]
    fn a_clock_that_went_backwards_does_not_block_checks_forever() {
        let now = 1_700_000_000;
        assert!(due(Some(now + 365 * 24 * 60 * 60), now));
    }

    #[test]
    fn a_newer_release_is_an_update() {
        let server = TestServer::always(Reply::ok(release_json("v0.2.0", INSTALLER)));
        let check = updates(&server)
            .check(Version::new(0, 1, 0))
            .expect("check");

        assert!(check.available);
        let latest = check.latest.expect("a release");
        assert_eq!(latest.version, Version::new(0, 2, 0));
        assert_eq!(latest.tag, "v0.2.0");
        assert_eq!(latest.notes, "Fixed the thing.");
        assert_eq!(latest.installer_bytes, Some(38_767_916));
        assert!(latest.installer_url.is_some());
    }

    #[test]
    fn the_running_version_is_not_an_update() {
        let server = TestServer::always(Reply::ok(release_json("v0.2.0", INSTALLER)));
        let check = updates(&server)
            .check(Version::new(0, 2, 0))
            .expect("check");

        assert!(!check.available);
        // Still reported, so Settings can say which build this is.
        assert!(check.latest.is_some());
    }

    #[test]
    fn a_build_newer_than_the_release_is_not_an_update() {
        // A local build, or a release that was pulled. Never offer to go backwards.
        let server = TestServer::always(Reply::ok(release_json("v0.2.0", INSTALLER)));
        let check = updates(&server)
            .check(Version::new(0, 3, 1))
            .expect("check");
        assert!(!check.available);
    }

    #[test]
    fn ten_is_newer_than_nine() {
        // The string-compare trap, end to end: 0.10.0 must beat 0.9.0.
        let server = TestServer::always(Reply::ok(release_json("v0.10.0", INSTALLER)));
        let check = updates(&server)
            .check(Version::new(0, 9, 0))
            .expect("check");
        assert!(check.available, "0.10.0 should be offered over 0.9.0");
    }

    #[test]
    fn a_repository_with_no_releases_is_not_an_error() {
        let server = TestServer::always(Reply::status(404));
        let check = updates(&server)
            .check(Version::new(0, 1, 0))
            .expect("check");

        assert!(!check.available);
        assert!(check.latest.is_none());
    }

    #[test]
    fn a_release_that_is_not_version_tagged_is_ignored() {
        // This repository really did publish `latest-windows`.
        let server = TestServer::always(Reply::ok(release_json("latest-windows", INSTALLER)));
        let check = updates(&server)
            .check(Version::new(0, 1, 0))
            .expect("check");

        assert!(!check.available);
        assert!(check.latest.is_none());
    }

    #[test]
    fn a_release_with_no_installer_still_reports() {
        let server = TestServer::always(Reply::ok(release_json("v0.2.0", "")));
        let latest = updates(&server)
            .latest()
            .expect("check")
            .expect("a release");

        assert_eq!(latest.installer_url, None);
        assert_eq!(latest.installer_bytes, None);
        // The page is always somewhere to send someone.
        assert!(latest.page_url.contains("releases/tag/v0.2.0"));
    }

    #[test]
    fn the_msi_is_not_mistaken_for_the_installer() {
        let assets = r#"{
          "name": "Aurora-TV-0.2.0-x64.msi",
          "browser_download_url": "https://example.com/a.msi",
          "size": 1
        }"#;
        let server = TestServer::always(Reply::ok(release_json("v0.2.0", assets)));
        let latest = updates(&server)
            .latest()
            .expect("check")
            .expect("a release");
        assert_eq!(latest.installer_url, None);
    }

    #[test]
    fn rate_limiting_says_so_in_its_own_words() {
        let server = TestServer::always(Reply::status(403));
        let failure = updates(&server)
            .check(Version::new(0, 1, 0))
            .expect_err("should fail");

        // Not "Your provider refused this stream", which is what the shared classifier
        // says and which would be gibberish here.
        assert!(
            failure.message.contains("GitHub"),
            "expected GitHub wording, got {:?}",
            failure.message
        );
        assert!(!failure.message.contains("provider"));
    }

    #[test]
    fn nonsense_json_is_a_clean_failure_not_a_panic() {
        let server = TestServer::always(Reply::ok("<html>not json at all</html>"));
        let failure = updates(&server)
            .check(Version::new(0, 1, 0))
            .expect_err("should fail");
        assert_eq!(failure.code, ErrorCode::Unknown);
        assert!(failure.retryable);
    }

    #[test]
    fn a_release_missing_optional_fields_still_parses() {
        let server = TestServer::always(Reply::ok(r#"{"tag_name":"v0.5.0"}"#));
        let latest = updates(&server)
            .latest()
            .expect("check")
            .expect("a release");

        assert_eq!(latest.version, Version::new(0, 5, 0));
        assert_eq!(latest.notes, "");
        // Synthesised from the repo and tag when GitHub does not send one.
        assert_eq!(
            latest.page_url,
            "https://github.com/owner/repo/releases/tag/v0.5.0"
        );
    }

    #[test]
    fn it_asks_for_the_latest_release_of_the_configured_repo() {
        let server = TestServer::always(Reply::ok(release_json("v0.2.0", INSTALLER)));
        updates(&server).latest().expect("check");

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/repos/owner/repo/releases/latest");
    }
}
