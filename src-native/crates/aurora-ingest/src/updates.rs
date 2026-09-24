//! Asking GitHub whether there is a newer build than the one running, and fetching it.
//!
//! The app ships as an installer from a release, not a store, so nothing tells a
//! viewer that a fix exists — this does, and then downloads it. What makes the second
//! half defensible is that GitHub publishes a SHA-256 for every release asset in the
//! same authenticated API response that names the version: the installer is checked
//! against that digest before anything is allowed to run it (docs/DECISIONS.md D17).
//!
//! The comparison is `aurora_core::version`, deliberately: a string compare would
//! decide 0.9.0 is newer than 0.10.0 and then never offer another update.

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aurora_core::neterr::{ErrorAction, ErrorCode, NetFailure};
use aurora_core::version::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    /// The installer's SHA-256, lowercase hex, as GitHub published it beside the
    /// asset. `None` on a release old enough to predate the API field — which is a
    /// reason to refuse to install it, not a reason to skip the check.
    pub installer_sha256: Option<String>,
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
    /// `"sha256:<hex>"`. GitHub added this to the assets API; an older release will
    /// not have it.
    #[serde(default)]
    digest: Option<String>,
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
            installer_sha256: installer
                .and_then(|a| a.digest.as_deref())
                .and_then(parse_sha256),
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

/// Read GitHub's `"sha256:<hex>"` asset digest.
///
/// Anything else — another algorithm, a truncated hex string — reads as absent rather
/// than as something to compare against, because a digest that is not a SHA-256 cannot
/// be checked and pretending otherwise is worse than admitting it.
fn parse_sha256(digest: &str) -> Option<String> {
    let hex = digest.strip_prefix("sha256:")?;
    let ok = hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit());
    ok.then(|| hex.to_ascii_lowercase())
}

/// A ceiling on an installer, so a wrong URL cannot fill the disk.
///
/// The real one is around 40 MB; this is an order of magnitude of headroom and still
/// far below anything that would be a problem.
pub const MAX_INSTALLER_BYTES: u64 = 512 * 1024 * 1024;

/// Whether a URL is an asset of this repository's own releases.
///
/// The URL comes from GitHub's API rather than from the UI, but it is about to be
/// written to disk and executed, so it is checked against the one shape it may have.
/// The trailing slash in the prefix is load-bearing: without it
/// `https://github.com.example.invalid/...` and `https://github.com@example.invalid/...`
/// both pass.
pub fn is_release_asset_url(url: &str, repo: &str) -> bool {
    let prefix = format!("https://github.com/{repo}/releases/download/");
    url.starts_with(&prefix) && !url.contains("..")
}

/// What the release said the installer should be.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Expected {
    pub bytes: Option<u64>,
    /// Lowercase hex SHA-256. Without one, [`download_installer`] refuses.
    pub sha256: Option<String>,
}

/// A verified installer on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
}

/// Fetch an installer and prove it is the one the release describes.
///
/// Written to a `.part` file and only renamed into place once the length and the digest
/// both match, so a half-finished or wrong download is never a file something else
/// could decide to run. A failed check leaves nothing behind at all.
///
/// `progress` is called on every chunk with the bytes so far and the expected total;
/// throttling that into something a progress bar can use is the caller's job.
pub fn download_installer(
    http: &HttpClient,
    url: &str,
    dest: &Path,
    expected: &Expected,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<Downloaded, NetFailure> {
    let Some(want_sha) = expected.sha256.as_ref() else {
        return Err(refuse(
            "This release does not publish a checksum",
            "Aurora will not download and run an installer it cannot verify. The \
             release page in a browser is the way to install this one.",
        ));
    };
    if !is_release_asset_url(url, DEFAULT_REPO) {
        return Err(refuse(
            "That download is not from Aurora's releases",
            "The update would have come from somewhere other than this project's own \
             GitHub releases, so it was not fetched.",
        ));
    }
    fetch_verified(http, url, dest, expected, want_sha, progress)
}

/// Fetch, hash and verify, with no opinion about where the URL came from.
///
/// Split out so the two refusals above are in one place and cannot be forgotten, and so
/// the tests can drive this against a local server rather than against GitHub.
fn fetch_verified(
    http: &HttpClient,
    url: &str,
    dest: &Path,
    expected: &Expected,
    want_sha: &str,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<Downloaded, NetFailure> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| io_failure("create the download folder", &e))?;
    }
    let part = dest.with_extension("part");
    let mut reader = http.fetch_reader(url)?;
    let mut file =
        BufWriter::new(File::create(&part).map_err(|e| io_failure("open the download file", &e))?);

    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut total: u64 = 0;

    loop {
        let read = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                let _ = fs::remove_file(&part);
                return Err(NetFailure::classify(&e.to_string()));
            }
        };
        total += read as u64;
        if total > MAX_INSTALLER_BYTES {
            let _ = fs::remove_file(&part);
            return Err(refuse(
                "That download is implausibly large",
                "It passed the size an Aurora installer could be, so it was stopped.",
            ));
        }
        hasher.update(&buf[..read]);
        if let Err(e) = file.write_all(&buf[..read]) {
            let _ = fs::remove_file(&part);
            return Err(io_failure("write the download", &e));
        }
        progress(total, expected.bytes);
    }

    if let Err(e) = file.flush() {
        let _ = fs::remove_file(&part);
        return Err(io_failure("finish writing the download", &e));
    }
    drop(file);

    let sha: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    if let Some(want) = expected.bytes {
        if want != total {
            let _ = fs::remove_file(&part);
            return Err(refuse(
                "The update did not download completely",
                &format!("Expected {want} bytes and got {total}."),
            ));
        }
    }
    if !sha.eq_ignore_ascii_case(want_sha) {
        let _ = fs::remove_file(&part);
        return Err(refuse(
            "The update failed its checksum",
            "What arrived is not the file GitHub published for this release, so it was \
             deleted rather than kept.",
        ));
    }

    fs::rename(&part, dest).map_err(|e| io_failure("store the download", &e))?;
    Ok(Downloaded {
        path: dest.to_path_buf(),
        bytes: total,
        sha256: sha,
    })
}

/// A refusal that retrying will not change.
fn refuse(message: &str, cause: &str) -> NetFailure {
    NetFailure {
        code: ErrorCode::Unknown,
        message: message.into(),
        cause: cause.into(),
        actions: vec![ErrorAction::ReportBroken],
        retryable: false,
    }
}

fn io_failure(what: &str, e: &std::io::Error) -> NetFailure {
    NetFailure {
        code: ErrorCode::Unknown,
        message: format!("Couldn't {what}"),
        cause: e.to_string(),
        actions: vec![ErrorAction::Retry],
        retryable: true,
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

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aurora-upd-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn sha256_of(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// The installer URL of a real release, which is the only shape that may be fetched.
    fn real_url() -> String {
        format!("https://github.com/{DEFAULT_REPO}/releases/download/v0.5.0/Aurora-TV-0.5.0-x64-setup.exe")
    }

    #[test]
    fn a_published_digest_is_carried_through_the_release() {
        let digest = format!("sha256:{}", "a".repeat(64));
        let asset = format!(
            r#"{{"name":"setup.exe",
                 "browser_download_url":"https://github.com/owner/repo/releases/download/v1/s.exe",
                 "size":42, "digest":"{digest}"}}"#
        );
        let server = TestServer::always(Reply::ok(release_json("v0.2.0", &asset).into_bytes()));
        let release = updates(&server).latest().unwrap().unwrap();
        assert_eq!(
            release.installer_sha256.as_deref(),
            Some("a".repeat(64).as_str())
        );
    }

    #[test]
    fn a_digest_that_is_not_a_sha256_reads_as_absent() {
        // Better to say "no checksum" — which refuses the download — than to compare
        // against something that cannot be a SHA-256.
        assert_eq!(parse_sha256("sha256:abc"), None);
        assert_eq!(parse_sha256("md5:{}"), None);
        assert_eq!(
            parse_sha256(&format!("sha256:{}", "A".repeat(64))),
            Some("a".repeat(64))
        );
        assert_eq!(parse_sha256(&format!("sha256:{}", "z".repeat(64))), None);
    }

    #[test]
    fn only_this_projects_release_assets_may_be_downloaded() {
        assert!(is_release_asset_url(&real_url(), DEFAULT_REPO));

        // A different repository, a different host, and the two lookalikes the
        // trailing slash exists to stop.
        for url in [
            "https://github.com/someone/else/releases/download/v1/setup.exe",
            "https://example.invalid/noahklimczuk/iptv-player/releases/download/v1/s.exe",
            "https://github.com.example.invalid/noahklimczuk/iptv-player/releases/download/v1/s.exe",
            "https://github.com@example.invalid/noahklimczuk/iptv-player/releases/download/v1/s.exe",
            "http://github.com/noahklimczuk/iptv-player/releases/download/v1/s.exe",
        ] {
            assert!(!is_release_asset_url(url, DEFAULT_REPO), "{url} must be refused");
        }
    }

    /// Drive the fetch half directly: the shipped entry point only accepts GitHub
    /// release URLs, and a test server is not GitHub.
    fn fetch(
        server: &TestServer,
        dest: &Path,
        expected: &Expected,
    ) -> Result<Downloaded, NetFailure> {
        let sha = expected.sha256.clone().unwrap_or_default();
        fetch_verified(
            &client(),
            &server.url("/setup.exe"),
            dest,
            expected,
            &sha,
            &mut |_, _| {},
        )
    }

    #[test]
    fn an_installer_is_kept_only_once_it_matches_what_was_published() {
        let body = vec![7u8; 300_000]; // more than one chunk
        let server = TestServer::always(Reply::ok(body.clone()));
        let dir = tempdir("ok");
        let dest = dir.join("setup.exe");

        let mut seen: Vec<u64> = Vec::new();
        let out = fetch_verified(
            &client(),
            &server.url("/setup.exe"),
            &dest,
            &Expected {
                bytes: Some(body.len() as u64),
                sha256: Some(sha256_of(&body)),
            },
            &sha256_of(&body),
            &mut |done, total| {
                assert_eq!(total, Some(body.len() as u64));
                seen.push(done);
            },
        )
        .expect("a matching download is kept");

        assert_eq!(out.bytes, body.len() as u64);
        assert_eq!(out.sha256, sha256_of(&body));
        assert_eq!(fs::read(&dest).unwrap(), body, "byte for byte");
        assert!(
            !dest.with_extension("part").exists(),
            "the part file is gone"
        );
        assert!(seen.len() > 1, "progress was reported as it went");
        assert_eq!(seen.last().copied(), Some(body.len() as u64));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_download_that_fails_its_checksum_is_deleted_rather_than_kept() {
        let body = vec![1u8; 50_000];
        let server = TestServer::always(Reply::ok(body.clone()));
        let dir = tempdir("badsha");
        let dest = dir.join("setup.exe");

        let err = fetch(
            &server,
            &dest,
            &Expected {
                bytes: Some(body.len() as u64),
                // The right length, the wrong file: exactly the case a length check
                // alone would wave through.
                sha256: Some("c".repeat(64)),
            },
        )
        .unwrap_err();

        assert!(err.message.contains("checksum"), "{}", err.message);
        assert!(!dest.exists(), "nothing executable may be left behind");
        assert!(!dest.with_extension("part").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_download_of_the_wrong_length_is_refused_before_the_digest_is_blamed() {
        let body = vec![2u8; 4_000];
        let server = TestServer::always(Reply::ok(body.clone()));
        let dir = tempdir("badlen");
        let dest = dir.join("setup.exe");

        let err = fetch(
            &server,
            &dest,
            &Expected {
                bytes: Some(9_999),
                sha256: Some(sha256_of(&body)),
            },
        )
        .unwrap_err();

        assert!(err.message.contains("completely"), "{}", err.message);
        assert!(err.cause.contains("9999"), "{}", err.cause);
        assert!(!dest.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_connection_cut_mid_download_leaves_nothing_to_run() {
        // The recorder keeps a cut stream on purpose — a partial recording still
        // plays. A partial installer is the opposite: it must not survive.
        let server = TestServer::always(Reply::Truncated {
            announced: 100_000,
            send: vec![3u8; 40_000],
        });
        let dir = tempdir("cut");
        let dest = dir.join("setup.exe");

        let err = fetch(
            &server,
            &dest,
            &Expected {
                bytes: Some(100_000),
                sha256: Some("d".repeat(64)),
            },
        )
        .unwrap_err();

        assert!(!err.message.is_empty());
        assert!(!dest.exists(), "a truncated installer must not be kept");
        assert!(!dest.with_extension("part").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_release_without_a_checksum_is_not_downloaded_at_all() {
        let dir = tempdir("nosha");
        let err = download_installer(
            &client(),
            &real_url(),
            &dir.join("setup.exe"),
            &Expected {
                bytes: Some(10),
                sha256: None,
            },
            &mut |_, _| {},
        )
        .unwrap_err();
        assert!(err.message.contains("checksum"), "{}", err.message);
        assert!(!dir.join("setup.exe").exists());
        assert!(!err.retryable, "retrying cannot make a checksum appear");
    }

    #[test]
    fn a_download_from_anywhere_but_this_projects_releases_is_refused() {
        let dir = tempdir("host");
        let err = download_installer(
            &client(),
            "https://example.invalid/setup.exe",
            &dir.join("setup.exe"),
            &Expected {
                bytes: Some(1),
                sha256: Some("b".repeat(64)),
            },
            &mut |_, _| {},
        )
        .unwrap_err();
        assert!(err.message.contains("not from Aurora"), "{}", err.message);
    }

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
