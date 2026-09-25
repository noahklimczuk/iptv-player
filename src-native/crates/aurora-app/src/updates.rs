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
use aurora_ingest::selfupdate;
use aurora_ingest::updates::{self, AssetKind, UpdateCheck, Updates};
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
    /// Whether this build can install an update over itself at all. False only off
    /// Windows, where there is neither an installer to run nor a `.exe` to swap.
    pub can_install: bool,
    /// Which kind of copy this is, so the UI can say what pressing Install will do.
    /// A portable copy replaces its own files and restarts; an installed one runs the
    /// installer.
    pub kind: AssetKind,
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

#[tauri::command(async)]
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
        kind: asset_kind(),
    }
}

/// How this copy of the app replaces itself.
///
/// The distinction is not cosmetic. Running the NSIS installer over a portable copy
/// installs into Program Files and leaves the folder the viewer is actually running
/// untouched — two installations, neither updated. And unzipping the portable archive
/// over an installed copy would leave the installer's own registry entries describing
/// a version that is no longer there. Each kind of copy has exactly one right answer,
/// and `Release::asset` refuses rather than substituting the other.
pub fn asset_kind() -> AssetKind {
    if crate::portable_dir().is_some() {
        AssetKind::Portable
    } else {
        AssetKind::Installer
    }
}

/// Whether this copy can update itself at all.
///
/// Both kinds can, now. It is false only off Windows, where there is neither an
/// installer to run nor a `.exe` to swap — which is every developer machine this is
/// built on, and is why the button has to be able to say so rather than fail late.
pub fn can_install() -> bool {
    cfg!(windows)
}

/// Where a portable copy lives, which is the folder being replaced.
///
/// `portable_dir()` is the *data* folder beside the executable; the install folder is
/// its parent, which is where `aurora-app.exe` and `mpv-2.dll` actually sit.
pub fn install_dir() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.to_path_buf()))
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
#[tauri::command(async)]
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

    // Whichever asset matches how this copy was installed. Never the other one: see
    // `asset_kind`.
    let kind = asset_kind();
    // Logged before it can fail, because every refusal below is invisible from a bug
    // report otherwise. "It says there is an update and the button does nothing" is
    // the shape these arrive in, and the two things worth knowing — which copy this
    // is, and which assets the release actually carries — are both decided here.
    tracing::info!(
        ?kind,
        version = %release.version,
        installer = release.installer_url.is_some(),
        portable = release.portable_url.is_some(),
        "starting an update download"
    );
    let (url, expected) = release
        .asset(kind)
        .map(|(url, expected)| (url.to_string(), expected))
        .ok_or_else(|| {
            AppError::Other(match kind {
                AssetKind::Installer => "This release does not publish a Windows installer".into(),
                AssetKind::Portable => {
                    "This release does not publish a portable build, so this copy \
                     cannot update itself from it"
                        .to_string()
                }
            })
        })
        .inspect_err(|e: &AppError| tracing::warn!("update download refused: {e}"))?;
    let version = release.version.to_string();

    // Named from the version rather than from anything the response said, so there is
    // no filename from the network anywhere near the filesystem.
    let dest = services.data_dir.join("updates").join(match kind {
        AssetKind::Installer => format!("Aurora-TV-{version}-x64-setup.exe"),
        AssetKind::Portable => format!("Aurora-TV-{version}-portable.zip"),
    });

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
    let stage_dir = services
        .data_dir
        .join("updates")
        .join(selfupdate::STAGE_DIR);
    std::thread::Builder::new()
        .name("aurora-update-download".into())
        .spawn(move || {
            let mut last = std::time::Instant::now();
            let outcome =
                updates::download_asset(&http, &url, &dest, &expected, &mut |received, _total| {
                    state.advance(received);
                    if last.elapsed() >= PROGRESS_INTERVAL {
                        last = std::time::Instant::now();
                        let _ = app.emit("update.download", &state.get());
                    }
                });

            let finished = match outcome {
                Ok(done) => {
                    // One update at a time: 40 MB per version left behind would add up
                    // on a machine that updates often.
                    forget_other_installers(&done.path);

                    // A portable update is unpacked now rather than at install time.
                    // Extraction is where the archive gets checked for the things a
                    // digest cannot see — a path that escapes the folder, an archive
                    // that is not Aurora — and finding that out while the viewer is
                    // watching a progress bar is much better than finding it out after
                    // they have pressed Install and the app is closing.
                    if kind == AssetKind::Portable {
                        match selfupdate::stage_zip(&done.path, &stage_dir) {
                            Ok(staged) => {
                                tracing::info!(
                                    version = %version,
                                    files = staged.files.len(),
                                    "update staged"
                                );
                            }
                            Err(e) => {
                                tracing::warn!("could not stage the update: {}", e.cause);
                                let _ = std::fs::remove_file(&done.path);
                                return finish(
                                    &app,
                                    &state,
                                    Download {
                                        status: DownloadStatus::Failed,
                                        version: Some(version),
                                        message: Some(e.message),
                                        ..Default::default()
                                    },
                                );
                            }
                        }
                    }

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
            finish(&app, &state, finished);
        })
        .map_err(|e| AppError::Other(format!("cannot start the download: {e}")))?;

    Ok(started)
}

/// Publish the download's final state and tell the UI. One place, because the failure
/// paths above each need it and each would otherwise forget the emit.
fn finish(app: &tauri::AppHandle, state: &Arc<Downloads>, download: Download) {
    let _ = app.emit("update.download", &state.set(download));
}

/// Whether a name in the updates folder is a downloaded release asset.
///
/// Deliberately narrow, and matched on the names *this* code chose rather than on
/// anything a response said: `updates_download` builds the filename from the version
/// it is fetching, so these are the only two shapes it can produce, plus the partial
/// file `fetch_verified` writes on the way.
fn is_download_artifact(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".part")
        || (lower.starts_with("aurora-tv-") && (lower.ends_with(".exe") || lower.ends_with(".zip")))
}

/// Delete the *other* downloads beside the one just verified.
///
/// One update at a time: 40 MB per version left behind adds up on a machine that
/// updates often.
///
/// This used to remove every entry in the folder that was not the file being kept, and
/// it shares that folder with `staged/`, `previous/` and the apply breadcrumb. It got
/// away with it only because `remove_file` refuses a directory — so the staged update
/// and the rollback copy survived by accident rather than by intent, and one
/// `remove_dir_all` here would have deleted the update it had just prepared, or the
/// only copy of the version being replaced. Matching names is the difference between
/// housekeeping and a blast radius.
fn forget_other_installers(keep: &std::path::Path) {
    let Some(dir) = keep.parent() else { return };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep {
            continue;
        }
        // Never a directory, whatever it is called: `staged/` holds an update that is
        // ready to apply and `previous/` holds the only copy of the version it
        // replaced.
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if is_download_artifact(name) {
            let _ = std::fs::remove_file(&path);
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
            "Aurora can only update itself on Windows. This build is for development.".into(),
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

#[tauri::command(async)]
pub fn updates_install(app: tauri::AppHandle, services: State<'_, Services>) -> Result<()> {
    let kind = asset_kind();
    let state = services.updates.get();

    // A portable update is already unpacked, so what has to still be there is the
    // staged folder rather than the archive it came from.
    let stage_dir = services
        .data_dir
        .join("updates")
        .join(selfupdate::STAGE_DIR);
    let present = match kind {
        AssetKind::Installer => state.path.as_ref().is_some_and(|p| p.exists()),
        AssetKind::Portable => selfupdate::staged(&stage_dir).is_some(),
    };

    if let Some(reason) = refusal(
        &state,
        present,
        can_install(),
        services.dvr.active_ids().len(),
    ) {
        // Same reasoning as the download refusal: "Install does nothing" is
        // undiagnosable from a log that never mentions the attempt.
        tracing::warn!(?kind, present, "update install refused: {reason}");
        return Err(AppError::Other(reason));
    }

    // Finalise anything the scheduler is holding before the process goes away, the
    // same way closing the window does. Before either branch, because both of them
    // end with this process exiting.
    services.dvr.shutdown(crate::now_unix());

    match kind {
        // Nothing to run: the new files are already on disk, and the swap happens on
        // the way back up, when no file is open. `restart` relaunches the same path —
        // which by then holds the new binary.
        AssetKind::Portable => {
            tracing::info!("update staged, restarting to apply it");
            app.restart();
        }
        AssetKind::Installer => {
            let path = state.path.expect("checked by the refusal above");
            launch_installer(&path).map_err(AppError::Other)?;
            tracing::info!("installer launched, exiting for it");
            app.exit(0);
            Ok(())
        }
    }
}

/// Swap in a staged update, before anything is holding a file open.
///
/// Called from `main` ahead of Tauri, which is the only moment this is safe and simple:
/// the app has opened no library, no log and no video surface, so every file it is
/// about to replace is closed. On Windows the running `.exe` still cannot be deleted —
/// but it can be renamed, which is what `selfupdate::apply` does and why it moves
/// everything aside rather than removing it.
///
/// Returns whether anything was applied. The caller relaunches when it was, because
/// this process is still the old binary: the new one is on disk, not running.
pub fn apply_staged_update(data_dir: &std::path::Path) -> bool {
    let updates_dir = data_dir.join("updates");
    let previous = updates_dir.join(selfupdate::PREVIOUS_DIR);
    // This runs before the logger exists — it has to, because the logger opens a file
    // in the folder being replaced. So the one moment worth having a record of is the
    // one moment `tracing` cannot reach. Leave a breadcrumb instead; `report_last_apply`
    // picks it up once there is somewhere for it to go.
    let breadcrumb = updates_dir.join(APPLY_BREADCRUMB);

    // Last launch's displaced copy. Removed now rather than at the end of the update
    // that made it, because "the new version started" is the only evidence worth
    // waiting for, and this line running at all is that evidence.
    selfupdate::forget_previous(&previous);

    let Some(staged) = selfupdate::staged(&updates_dir.join(selfupdate::STAGE_DIR)) else {
        return false;
    };
    let Some(install_dir) = install_dir() else {
        tracing::error!("cannot work out where this copy lives; not applying the update");
        return false;
    };

    match selfupdate::apply(&staged, &install_dir, &previous) {
        Ok(count) => {
            let _ = std::fs::write(&breadcrumb, format!("applied {count} files"));
            true
        }
        Err(e) => {
            // The old copy is intact — `apply` rolls back — so carrying on as the
            // version we already were is both possible and the right answer. A failed
            // update must never be why somebody's television stops working.
            let _ = std::fs::write(&breadcrumb, format!("FAILED: {} ({})", e.message, e.cause));
            false
        }
    }
}

/// What the last swap left behind, written before there was a logger.
const APPLY_BREADCRUMB: &str = "last-apply.txt";

/// Log what the pre-launch update swap did, now that there is somewhere to log it.
///
/// Read once and deleted, so a line appears in the log of the launch it belongs to and
/// not in every launch after it.
pub fn report_last_apply(data_dir: &std::path::Path) {
    let breadcrumb = data_dir.join("updates").join(APPLY_BREADCRUMB);
    let Ok(what) = std::fs::read_to_string(&breadcrumb) else {
        return;
    };
    let _ = std::fs::remove_file(&breadcrumb);
    if what.starts_with("FAILED") {
        tracing::error!("the staged update was not applied: {what}");
    } else {
        tracing::info!("a staged update was applied before launch: {what}");
    }
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

#[tauri::command(async)]
pub fn updates_set_automatic(
    services: State<'_, Services>,
    args: SetAutomaticArgs,
) -> Result<bool> {
    let db = services.db.lock();
    settings::set(&db, AUTOMATIC_KEY, &args.enabled)?;
    Ok(args.enabled)
}

/// Open the releases page in the viewer's browser.
#[tauri::command(async)]
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

    /// This replaces a test that asserted the opposite: that a portable copy is
    /// refused and told the release page has the new zip. That was the behaviour, and
    /// it was the gap — the build that is easiest to update was the only one that
    /// could not update itself. `refusal` no longer knows what kind of copy this is;
    /// `asset_kind` decides which file to fetch, and both kinds install.
    #[test]
    fn being_portable_is_no_longer_a_reason_to_refuse() {
        let ready = ready(Some(PathBuf::from("s.exe")));
        assert_eq!(
            refusal(&ready, true, true, 0),
            None,
            "a verified download with nothing recording must install"
        );
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

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aurora-app-upd-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &std::path::Path, body: &[u8]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    /// A staged update waiting in the data folder, as `updates_download` leaves one.
    fn stage(data_dir: &std::path::Path, exe_body: &[u8]) {
        let stage = data_dir
            .join("updates")
            .join(aurora_ingest::selfupdate::STAGE_DIR);
        write(&stage.join(aurora_ingest::selfupdate::APP_EXE), exe_body);
        write(&stage.join(aurora_ingest::selfupdate::READY_MARKER), b"");
    }

    /// The housekeeping that runs after every download shares a folder with the
    /// staged update and the rollback copy.
    ///
    /// It used to delete every entry that was not the file being kept, and survived
    /// only because `remove_file` refuses a directory. That is luck, not a design: the
    /// two things in there are an update that is ready to apply and the only copy of
    /// the version it replaces, and losing either is losing somebody's television.
    #[test]
    fn housekeeping_keeps_the_staged_update_and_the_rollback_copy() {
        let dir = scratch("housekeeping");
        let updates = dir.join("updates");

        let keep = updates.join("Aurora-TV-0.9.0-portable.zip");
        write(&keep, b"the new one");
        // An older download, and a partial one: these are what it is for.
        write(&updates.join("Aurora-TV-0.8.1-portable.zip"), b"last time");
        write(
            &updates.join("Aurora-TV-0.8.0-x64-setup.exe"),
            b"older still",
        );
        write(
            &updates.join("Aurora-TV-0.9.0-portable.part"),
            b"half a download",
        );
        // And the things it must not touch.
        stage(&dir, b"staged exe");
        write(
            &updates.join("previous").join("aurora-app.exe"),
            b"the old one",
        );
        write(&updates.join(APPLY_BREADCRUMB), b"applied 3 files");

        forget_other_installers(&keep);

        assert!(keep.exists(), "the download just verified was deleted");
        assert!(
            !updates.join("Aurora-TV-0.8.1-portable.zip").exists(),
            "an older download should be cleaned up"
        );
        assert!(
            !updates.join("Aurora-TV-0.8.0-x64-setup.exe").exists(),
            "an older installer should be cleaned up"
        );
        assert!(
            !updates.join("Aurora-TV-0.9.0-portable.part").exists(),
            "a partial download should be cleaned up"
        );

        // The three that matter.
        assert!(
            updates
                .join(aurora_ingest::selfupdate::STAGE_DIR)
                .join(aurora_ingest::selfupdate::APP_EXE)
                .exists(),
            "the staged update was deleted"
        );
        assert!(
            updates.join("previous").join("aurora-app.exe").exists(),
            "the rollback copy was deleted"
        );
        assert!(
            updates.join(APPLY_BREADCRUMB).exists(),
            "the apply breadcrumb was deleted before it could be read"
        );
    }

    /// What counts as ours to delete. Named after the files `updates_download` itself
    /// writes, so nothing else in the folder can be caught by accident.
    #[test]
    fn only_this_code_s_own_downloads_are_cleaned_up() {
        for ours in [
            "Aurora-TV-0.9.0-portable.zip",
            "Aurora-TV-0.9.0-x64-setup.exe",
            "aurora-tv-0.9.0-portable.zip",
            "Aurora-TV-0.9.0-portable.part",
            "anything.part",
        ] {
            assert!(is_download_artifact(ours), "{ours} should be cleaned up");
        }
        for theirs in [
            APPLY_BREADCRUMB,
            "staged",
            "previous",
            "aurora.log",
            "library.db",
            "notes.txt",
        ] {
            assert!(
                !is_download_artifact(theirs),
                "{theirs} is not this code's to delete"
            );
        }
    }

    /// With nothing staged, startup must be a no-op — this runs on every launch.
    #[test]
    fn a_launch_with_nothing_staged_applies_nothing() {
        let dir = scratch("nostage");
        assert!(!apply_staged_update(&dir));
    }

    /// The breadcrumb exists because the swap happens before the logger does. Without
    /// it the one thing worth a log line is the one thing that cannot write one.
    #[test]
    fn what_the_swap_did_is_reported_once_and_then_forgotten() {
        let dir = scratch("breadcrumb");
        let breadcrumb = dir.join("updates").join(APPLY_BREADCRUMB);
        write(&breadcrumb, b"applied 3 files");

        report_last_apply(&dir);
        assert!(
            !breadcrumb.exists(),
            "the note would be repeated on every launch after this one"
        );
        // And with nothing to say, it says nothing rather than failing.
        report_last_apply(&dir);
    }

    /// A download that did not finish must not be applied: the marker is written last,
    /// so its absence means half an application is on disk.
    #[test]
    fn a_half_finished_download_is_never_applied() {
        let dir = scratch("partial");
        let stage = dir
            .join("updates")
            .join(aurora_ingest::selfupdate::STAGE_DIR);
        write(&stage.join(aurora_ingest::selfupdate::APP_EXE), b"half");
        // No ready marker.

        assert!(!apply_staged_update(&dir));
        assert!(
            !stage.exists(),
            "the partial download was left to be retried"
        );
    }

    /// The previous copy is deleted at the *start* of the next launch rather than at
    /// the end of the update that made it, because a version that has started is the
    /// only evidence that the update worked.
    #[test]
    fn the_previous_copy_survives_until_something_has_started() {
        let dir = scratch("previous");
        let previous = dir
            .join("updates")
            .join(aurora_ingest::selfupdate::PREVIOUS_DIR);
        write(
            &previous.join(aurora_ingest::selfupdate::APP_EXE),
            b"old app",
        );

        // A launch with nothing staged still clears what the last update displaced.
        assert!(!apply_staged_update(&dir));
        assert!(!previous.exists());
    }

    /// Staging happens whatever the outcome, so the refusal has to reflect what is
    /// actually on disk rather than what was downloaded.
    #[test]
    fn a_staged_update_is_what_a_portable_copy_installs() {
        let dir = scratch("staged");
        stage(&dir, b"new app");

        let found = aurora_ingest::selfupdate::staged(
            &dir.join("updates")
                .join(aurora_ingest::selfupdate::STAGE_DIR),
        );
        assert!(found.is_some(), "the staged update was not found");
    }

    /// The refusals, in the order a viewer can act on them.
    #[test]
    fn nothing_installs_while_a_recording_is_running() {
        // A recording cannot be taken again later; an update can.
        let reason =
            refusal(&ready(Some(PathBuf::from("/tmp/x.exe"))), true, true, 2).expect("a refusal");
        assert!(reason.contains("2 recordings are in progress"), "{reason}");
    }

    #[test]
    fn nothing_installs_without_a_verified_download() {
        let reason = refusal(&Download::default(), false, true, 0).expect("a refusal");
        assert!(reason.contains("no verified update"), "{reason}");
    }

    #[test]
    fn a_platform_that_cannot_update_itself_says_so_rather_than_failing_late() {
        let reason =
            refusal(&ready(Some(PathBuf::from("/tmp/x.exe"))), true, false, 0).expect("a refusal");
        assert!(reason.contains("only update itself on Windows"), "{reason}");
        // Notably *not* the old message, which told a portable copy to go to a browser.
        assert!(!reason.contains("release page"), "{reason}");
    }

    #[test]
    fn the_releases_url_points_at_the_repository_builds_come_from() {
        assert!(RELEASES_URL.starts_with("https://github.com/"));
        assert!(RELEASES_URL.contains(updates::DEFAULT_REPO));
    }
}
