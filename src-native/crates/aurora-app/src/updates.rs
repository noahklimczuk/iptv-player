//! Telling the viewer a newer build exists, and installing it (README §23).
//!
//! The app installs from a GitHub release rather than a store, so nothing would
//! otherwise tell someone that the bug they hit was fixed a week ago. This checks,
//! reports, downloads the installer and hands it to Windows — never silently, and
//! never without checking the file against the SHA-256 GitHub published for it
//! (docs/DECISIONS.md D17).

use std::path::PathBuf;
use std::sync::Arc;

use aurora_core::version::Version;
use aurora_db::repo::settings;
use aurora_ingest::updates::{self, Expected, UpdateCheck, Updates};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::error::Result;
use crate::services::Services;
use crate::AppError;

/// What this binary was built as.
///
/// Comes from the workspace `version`, which the release build rewrites before it
/// compiles — so the number here and the number on the release are the same by
/// construction rather than by anyone remembering (docs/BUILDING.md).
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Where "Get the update" goes.
///
/// A constant, and deliberately not the `html_url` the API returned: a command that
/// opens whatever URL it is handed is a way to make the app launch something else.
/// Nothing reaches this from the network or from the UI, so there is no URL to
/// validate and nothing to get wrong.
pub const RELEASES_URL: &str = "https://github.com/noahklimczuk/iptv-player/releases/latest";

const LAST_CHECKED_KEY: &str = "updates.lastCheckedAt";
const LAST_RESULT_KEY: &str = "updates.lastResult";
const AUTOMATIC_KEY: &str = "updates.automatic";

/// Everything the Updates section of Settings renders, from one call.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    #[serde(flatten)]
    pub check: UpdateCheck,
    /// Whether the background check on launch is switched on.
    pub automatic: bool,
    pub last_checked_at: Option<i64>,
    pub releases_url: &'static str,
    /// How far the installer has got, if the viewer asked for it.
    pub download: Download,
    /// Whether this build can install an update over itself at all. False for a
    /// portable copy, which the installer would not replace, and false off Windows.
    pub can_install: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadStatus {
    #[default]
    Idle,
    Downloading,
    /// On disk and matching the digest GitHub published. Ready to run.
    Ready,
    Failed,
}

/// The installer download, as the Updates panel sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub status: DownloadStatus,
    /// What is being fetched, or what is waiting to be installed.
    pub version: Option<String>,
    pub received_bytes: u64,
    pub total_bytes: Option<u64>,
    /// Why it failed, in words the viewer can act on.
    pub message: Option<String>,
    /// Where it landed. Never sent to the UI: the install command finds the file from
    /// here rather than being handed a path, so there is no path to get wrong.
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

/// The installer download, shared between the command that starts it and the thread
/// that does it.
#[derive(Default)]
pub struct Downloads(Mutex<Download>);

impl Downloads {
    pub fn get(&self) -> Download {
        self.0.lock().clone()
    }

    fn set(&self, download: Download) -> Download {
        *self.0.lock() = download.clone();
        download
    }

    /// Record progress without disturbing the rest of the state.
    fn advance(&self, received: u64) {
        let mut state = self.0.lock();
        state.received_bytes = received;
    }
}

/// The running version, or `0.0.0` if the manifest somehow held something unparseable.
///
/// Falling back rather than panicking: a version this cannot read is a reason to offer
/// every update, not a reason to refuse to start.
pub fn current() -> Version {
    Version::parse(CURRENT).unwrap_or(Version::new(0, 0, 0))
}

pub fn automatic(conn: &aurora_db::rusqlite::Connection) -> bool {
    settings::get_or(conn, AUTOMATIC_KEY, true).unwrap_or(true)
}

pub fn last_checked(conn: &aurora_db::rusqlite::Connection) -> Option<i64> {
    settings::get(conn, LAST_CHECKED_KEY).ok().flatten()
}

/// The last answer GitHub gave, re-judged against the version running now.
///
/// Re-judging matters: the cached answer was written by the build that was running
/// then. After an update installs, a stored "0.2.0 is available" is about a version
/// this binary now *is*, and replaying it verbatim would offer an update to itself.
fn cached(conn: &aurora_db::rusqlite::Connection) -> Option<UpdateCheck> {
    let stored: UpdateCheck = settings::get(conn, LAST_RESULT_KEY).ok().flatten()?;
    let current = current();
    let available = stored.latest.as_ref().is_some_and(|r| r.version > current);
    Some(UpdateCheck {
        current,
        latest: stored.latest,
        available,
    })
}

/// Ask GitHub, then remember both that we asked and what was said.
///
/// The timestamp is written whatever the answer was, including a failure: an offline
/// machine must not retry on a loop, and "we tried six hours ago" is the useful fact
/// either way. The result is only written when there is one, so a failed check leaves
/// the last good answer in place rather than blanking Settings.
pub fn run(services: &Services, now: i64, force: bool) -> Result<UpdateCheck> {
    if !force {
        let db = services.db.lock();
        if !updates::due(last_checked(&db), now) {
            if let Some(check) = cached(&db) {
                return Ok(check);
            }
        }
    }

    let result = Updates::new(services.http.clone()).check(current());

    {
        let db = services.db.lock();
        let _ = settings::set(&db, LAST_CHECKED_KEY, &now);
        if let Ok(check) = &result {
            let _ = settings::set(&db, LAST_RESULT_KEY, check);
        }
    }

    result.map_err(|e| AppError::Other(format!("{}: {}", e.message, e.cause)))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckArgs {
    /// Ask GitHub even if the last answer is still fresh. What the "Check now" button
    /// sends; without it, opening Settings costs no network at all.
    #[serde(default)]
    pub force: bool,
}

#[tauri::command]
pub fn updates_check(
    services: State<'_, Services>,
    args: Option<CheckArgs>,
) -> Result<UpdateStatus> {
    let force = args.unwrap_or_default().force;
    let check = run(&services, crate::now_unix(), force)?;
    Ok(status(&services, check))
}

fn status(services: &Services, check: UpdateCheck) -> UpdateStatus {
    let db = services.db.lock();
    UpdateStatus {
        check,
        automatic: automatic(&db),
        last_checked_at: last_checked(&db),
        releases_url: RELEASES_URL,
        download: services.updates.get(),
        can_install: can_install(),
    }
}

/// Whether an installer could replace this copy of the app.
///
/// A portable build is the interesting case: the NSIS installer would install into
/// Program Files and leave the folder the viewer is actually running untouched, so
/// offering the button would be a way to end up with two copies and update neither.
pub fn can_install() -> bool {
    cfg!(windows) && crate::portable_dir().is_none()
}

/// How often the download reports itself to the UI. A 40 MB file at 256 KB a chunk is
/// a hundred and fifty events; four a second is enough to move a bar.
const PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

/// Start fetching the published installer.
///
/// Takes no URL. The one it uses comes from the release GitHub just described, is
/// checked against this project's own releases, and is verified against the digest
/// published beside it — the same reasoning as `updates_open_releases`, one step
/// further along (docs/DECISIONS.md D17).
#[tauri::command]
pub fn updates_download(app: tauri::AppHandle, services: State<'_, Services>) -> Result<Download> {
    let current = services.updates.get();
    if current.status == DownloadStatus::Downloading {
        return Ok(current);
    }

    let check = run(&services, crate::now_unix(), false)?;
    let release = check
        .latest
        .filter(|_| check.available)
        .ok_or_else(|| AppError::Other("There is no newer build to download".into()))?;

    // Already have it, verified, from an earlier click.
    if current.status == DownloadStatus::Ready
        && current.version.as_deref() == Some(&release.version.to_string())
        && current.path.as_ref().is_some_and(|p| p.exists())
    {
        return Ok(current);
    }

    let url = release.installer_url.clone().ok_or_else(|| {
        AppError::Other("This release does not publish a Windows installer".into())
    })?;
    let version = release.version.to_string();
    let expected = Expected {
        bytes: release.installer_bytes,
        sha256: release.installer_sha256.clone(),
    };

    // Named from the version rather than from anything the response said, so there is
    // no filename from the network anywhere near the filesystem.
    let dest = services
        .data_dir
        .join("updates")
        .join(format!("Aurora-TV-{version}-x64-setup.exe"));

    let started = services.updates.set(Download {
        status: DownloadStatus::Downloading,
        version: Some(version.clone()),
        received_bytes: 0,
        total_bytes: expected.bytes,
        message: None,
        path: None,
    });
    let _ = app.emit("update.download", &started);

    let http = Arc::clone(&services.http);
    let state = Arc::clone(&services.updates);
    std::thread::Builder::new()
        .name("aurora-update-download".into())
        .spawn(move || {
            let mut last = std::time::Instant::now();
            let outcome = updates::download_installer(
                &http,
                &url,
                &dest,
                &expected,
                &mut |received, _total| {
                    state.advance(received);
                    if last.elapsed() >= PROGRESS_INTERVAL {
                        last = std::time::Instant::now();
                        let _ = app.emit("update.download", &state.get());
                    }
                },
            );

            let finished = match outcome {
                Ok(done) => {
                    // One installer at a time: 40 MB per version left behind would add
                    // up on a machine that updates often.
                    forget_other_installers(&done.path);
                    tracing::info!(version = %version, "update downloaded and verified");
                    Download {
                        status: DownloadStatus::Ready,
                        version: Some(version),
                        received_bytes: done.bytes,
                        total_bytes: Some(done.bytes),
                        message: None,
                        path: Some(done.path),
                    }
                }
                Err(e) => {
                    tracing::warn!("update download failed: {} ({})", e.message, e.cause);
                    Download {
                        status: DownloadStatus::Failed,
                        version: Some(version),
                        message: Some(e.message),
                        ..Default::default()
                    }
                }
            };
            let _ = app.emit("update.download", &state.set(finished));
        })
        .map_err(|e| AppError::Other(format!("cannot start the download: {e}")))?;

    Ok(started)
}

/// Delete every other file beside the installer just verified.
fn forget_other_installers(keep: &std::path::Path) {
    let Some(dir) = keep.parent() else { return };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.path() != keep {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Run the downloaded installer and quit so it can replace the files.
///
/// Deliberately visible rather than silent: these builds are not code-signed, so
/// Windows will want to say something about that, and a viewer who is about to have
/// their app replaced should see the thing doing it.
/// Why an install cannot go ahead, or `None` when it can.
///
/// Separate from the command because these are the interesting decisions and a
/// `tauri::AppHandle` is not something a test can conjure. Order matters: the reason
/// reported should be the one the viewer can act on first.
fn refusal(
    download: &Download,
    file_present: bool,
    can_install: bool,
    recordings: usize,
) -> Option<String> {
    if download.status != DownloadStatus::Ready {
        return Some("There is no verified update downloaded yet".into());
    }
    if !file_present {
        return Some("The downloaded update is no longer there".into());
    }
    if !can_install {
        return Some(
            "This copy is portable, so an installer would not replace it. The release \
             page has the new zip."
                .into(),
        );
    }
    // A recording is a thing that cannot be redone. The update can wait.
    if recordings > 0 {
        return Some(format!(
            "{recordings} recording{} in progress. The update can install once {} \
             finished.",
            if recordings == 1 { " is" } else { "s are" },
            if recordings == 1 {
                "it has"
            } else {
                "they have"
            }
        ));
    }
    None
}

#[tauri::command]
pub fn updates_install(app: tauri::AppHandle, services: State<'_, Services>) -> Result<()> {
    let state = services.updates.get();
    let present = state.path.as_ref().is_some_and(|p| p.exists());
    if let Some(reason) = refusal(
        &state,
        present,
        can_install(),
        services.dvr.active_ids().len(),
    ) {
        return Err(AppError::Other(reason));
    }
    let path = state.path.expect("checked by the refusal above");

    launch_installer(&path).map_err(AppError::Other)?;
    // Finalise anything the scheduler is holding before the process goes away, the
    // same way closing the window does.
    services.dvr.shutdown(crate::now_unix());
    tracing::info!("installer launched, exiting for it");
    app.exit(0);
    Ok(())
}

#[cfg(windows)]
fn launch_installer(path: &std::path::Path) -> std::result::Result<(), String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let file = HSTRING::from(path.to_string_lossy().as_ref());
    let verb = HSTRING::from("open");
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        return Err(format!(
            "Windows would not run the installer (ShellExecute returned {})",
            result.0 as isize
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn launch_installer(path: &std::path::Path) -> std::result::Result<(), String> {
    // Compiled everywhere so the Linux job type-checks it; `can_install` has already
    // refused by the time anything could call this.
    Err(format!(
        "there is no Windows installer to run on this platform ({})",
        path.display()
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAutomaticArgs {
    pub enabled: bool,
}

#[tauri::command]
pub fn updates_set_automatic(
    services: State<'_, Services>,
    args: SetAutomaticArgs,
) -> Result<bool> {
    let db = services.db.lock();
    settings::set(&db, AUTOMATIC_KEY, &args.enabled)?;
    Ok(args.enabled)
}

/// Open the releases page in the viewer's browser.
#[tauri::command]
pub fn updates_open_releases() -> Result<()> {
    open_releases_page().map_err(AppError::Other)
}

#[cfg(windows)]
fn open_releases_page() -> std::result::Result<(), String> {
    use windows::core::w;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    // ShellExecuteW's return is a legacy pseudo-HINSTANCE: anything at or below 32 is
    // an error code rather than a handle. The URL is a `w!` literal, so there is no
    // runtime string to build and nothing to escape.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            w!("https://github.com/noahklimczuk/iptv-player/releases/latest"),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        return Err(format!(
            "Windows would not open a browser (ShellExecute returned {})",
            result.0 as isize
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn open_releases_page() -> std::result::Result<(), String> {
    // Exists so the Linux job compiles this crate and catches type errors in
    // everything around it (docs/DECISIONS.md D3). There is no shipped build here.
    tracing::info!(url = RELEASES_URL, "no browser to open on this platform");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_version_is_a_version() {
        // If the manifest ever holds something `Version` cannot read, the updater would
        // silently compare against 0.0.0 and offer every release forever.
        assert!(
            Version::parse(CURRENT).is_some(),
            "workspace version {CURRENT:?} is not major.minor.patch"
        );
    }

    fn ready(path: Option<PathBuf>) -> Download {
        Download {
            status: DownloadStatus::Ready,
            version: Some("0.2.0".into()),
            received_bytes: 10,
            total_bytes: Some(10),
            message: None,
            path,
        }
    }

    #[test]
    fn nothing_installs_until_something_verified_is_on_disk() {
        let nothing = Download::default();
        assert!(refusal(&nothing, false, true, 0)
            .unwrap()
            .contains("no verified update"));

        // Downloaded once, then deleted from under us: the file is what gets run, so
        // its absence is the answer, not the state flag.
        let gone = ready(Some(PathBuf::from("/nowhere/setup.exe")));
        assert!(refusal(&gone, false, true, 0)
            .unwrap()
            .contains("no longer there"));
    }

    #[test]
    fn a_portable_copy_is_told_why_rather_than_offered_an_installer() {
        let message = refusal(&ready(Some(PathBuf::from("s.exe"))), true, false, 0).unwrap();
        assert!(message.contains("portable"), "{message}");
        assert!(message.contains("release page"), "{message}");
    }

    #[test]
    fn a_recording_in_progress_postpones_the_install_and_says_so_in_english() {
        let one = refusal(&ready(Some(PathBuf::from("s.exe"))), true, true, 1).unwrap();
        assert!(one.contains("1 recording is in progress"), "{one}");
        assert!(one.contains("it has finished"), "{one}");

        let several = refusal(&ready(Some(PathBuf::from("s.exe"))), true, true, 3).unwrap();
        assert!(
            several.contains("3 recordings are in progress"),
            "{several}"
        );
        assert!(several.contains("they have finished"), "{several}");
    }

    #[test]
    fn a_verified_download_on_a_quiet_installed_copy_goes_ahead() {
        assert_eq!(
            refusal(&ready(Some(PathBuf::from("s.exe"))), true, true, 0),
            None
        );
    }

    #[test]
    fn the_download_state_reports_progress_without_losing_what_it_is_fetching() {
        let downloads = Downloads::default();
        assert_eq!(downloads.get().status, DownloadStatus::Idle);

        downloads.set(Download {
            status: DownloadStatus::Downloading,
            version: Some("0.2.0".into()),
            total_bytes: Some(1_000),
            ..Default::default()
        });
        downloads.advance(400);

        let state = downloads.get();
        assert_eq!(state.received_bytes, 400);
        assert_eq!(
            state.total_bytes,
            Some(1_000),
            "the total survived the tick"
        );
        assert_eq!(state.version.as_deref(), Some("0.2.0"));
        assert_eq!(state.status, DownloadStatus::Downloading);
    }

    #[test]
    fn the_path_is_never_sent_to_the_ui() {
        // The install command finds the file from the host's own state; a path on the
        // wire is a path something could hand back.
        let json = serde_json::to_string(&ready(Some(PathBuf::from("/tmp/setup.exe")))).unwrap();
        assert!(!json.contains("setup.exe"), "{json}");
        assert!(json.contains("\"status\":\"ready\""), "{json}");
    }

    #[test]
    fn the_releases_url_points_at_the_repository_builds_come_from() {
        assert!(RELEASES_URL.starts_with("https://github.com/"));
        assert!(RELEASES_URL.contains(updates::DEFAULT_REPO));
    }
}
