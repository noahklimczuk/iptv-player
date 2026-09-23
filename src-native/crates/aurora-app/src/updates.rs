//! Telling the viewer a newer build exists (README §23).
//!
//! The app installs from a GitHub release rather than a store, so nothing would
//! otherwise tell someone that the bug they hit was fixed a week ago. This checks and
//! reports; installing stays a decision the person makes, in a browser, on a page
//! they can read first (docs/DECISIONS.md D17).

use aurora_core::version::Version;
use aurora_db::repo::settings;
use aurora_ingest::updates::{self, UpdateCheck, Updates};
use serde::{Deserialize, Serialize};
use tauri::State;

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
    let db = services.db.lock();
    Ok(UpdateStatus {
        check,
        automatic: automatic(&db),
        last_checked_at: last_checked(&db),
        releases_url: RELEASES_URL,
    })
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

    #[test]
    fn the_releases_url_points_at_the_repository_builds_come_from() {
        assert!(RELEASES_URL.starts_with("https://github.com/"));
        assert!(RELEASES_URL.contains(updates::DEFAULT_REPO));
    }
}
