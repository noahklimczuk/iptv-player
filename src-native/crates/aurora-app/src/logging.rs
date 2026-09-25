//! Where the log goes, and how to get at it (README §18).
//!
//! A release build sets `windows_subsystem = "windows"`, so it has no console and
//! anything written to stdout goes nowhere — including the one line that distinguishes
//! "libmpv would not load" from "the video is behind the window". So a release logs to
//! a file beside its data, and a debug build keeps the console it already has.
//!
//! Two things that file used to get wrong. It was created with `File::create` on every
//! launch, which truncates — so the sequence that matters most, "it crashed, I
//! relaunched to report it", threw the evidence away at exactly the wrong moment. And
//! there was no way to hand it to anyone: the path is inside `%LOCALAPPDATA%`, which
//! is not somewhere a person goes.

use std::path::{Path, PathBuf};

/// The current log, and the one before it. Two generations rather than a rolling set:
/// the question asked of this file is almost always about the launch that just failed
/// or the one before it, and twenty stale copies in a data directory is its own
/// nuisance.
pub const LOG_NAME: &str = "aurora.log";
pub const PREVIOUS_LOG_NAME: &str = "aurora.log.1";

/// Keep the last run's log, then start a new one.
///
/// Returns the path being written to. Rename failures are deliberately ignored: not
/// keeping the previous log is a worse outcome than not starting this one, but neither
/// is a reason to refuse to launch.
pub fn rotate(data_dir: &Path) -> PathBuf {
    let current = data_dir.join(LOG_NAME);
    if current.exists() {
        let _ = std::fs::rename(&current, data_dir.join(PREVIOUS_LOG_NAME));
    }
    current
}

/// Both log files that exist, newest first.
pub fn existing(data_dir: &Path) -> Vec<PathBuf> {
    [LOG_NAME, PREVIOUS_LOG_NAME]
        .iter()
        .map(|n| data_dir.join(n))
        .filter(|p| p.exists())
        .collect()
}

/// Copy the logs somewhere a person can attach them to a message.
///
/// Returns what was written. The destination is the viewer's own choice of folder,
/// which is the whole point — a path under `%LOCALAPPDATA%` is not somewhere anyone is
/// going to go looking.
pub fn export_to(data_dir: &Path, destination: &Path) -> std::io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(destination)?;
    let mut written = Vec::new();
    for source in existing(data_dir) {
        let Some(name) = source.file_name() else {
            continue;
        };
        let target = destination.join(name);
        std::fs::copy(&source, &target)?;
        written.push(target);
    }
    Ok(written)
}

/// What the Settings screen shows about the log, and where the export went.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogExport {
    /// Absolute paths of the files written, so the viewer can be told where to look.
    pub files: Vec<String>,
    pub folder: String,
}

/// What the Settings screen should tell the viewer about this launch.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    /// Where the log and the library live, so a support message can name it.
    pub data_dir: String,
    /// Set when the library could not be read at startup and was started again. The
    /// viewer's favourites, watch progress and recordings metadata went with it, so
    /// they should be told rather than left to find an empty library.
    pub library_was_replaced: Option<String>,
    /// Whether provider passwords survive a restart on this platform.
    pub credentials_persist: bool,
}

#[tauri::command(async)]
pub fn app_diagnostics(
    services: tauri::State<'_, crate::services::Services>,
) -> crate::error::Result<Diagnostics> {
    Ok(Diagnostics {
        data_dir: services.data_dir.to_string_lossy().into_owned(),
        library_was_replaced: match &services.opened {
            aurora_db::Opened::Intact => None,
            aurora_db::Opened::Replaced { corrupt_copy } => {
                Some(corrupt_copy.to_string_lossy().into_owned())
            }
        },
        credentials_persist: services.credentials.is_persistent(),
    })
}

/// Copy the logs to a folder beside the library, and say where they went.
///
/// Deliberately not a path the UI chooses: a command that writes wherever it is told
/// is a way to overwrite something. `<data>/logs-export` is beside everything else the
/// app owns, and the path comes back so the viewer can be shown it.
#[tauri::command(async)]
pub fn logs_export(
    services: tauri::State<'_, crate::services::Services>,
) -> crate::error::Result<LogExport> {
    let destination = services.data_dir.join("logs-export");
    let files = export_to(&services.data_dir, &destination)
        .map_err(|e| crate::AppError::Other(format!("could not export the logs: {e}")))?;
    if files.is_empty() {
        return Err(crate::AppError::Other(
            "There is no log to export yet. Debug builds log to the console instead.".to_string(),
        ));
    }
    Ok(LogExport {
        files: files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        folder: destination.to_string_lossy().into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aurora-log-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The bug: `File::create` truncated on every launch, so relaunching to report a
    /// crash was what destroyed the record of it.
    #[test]
    fn the_previous_run_is_kept_as_generation_one() {
        let dir = scratch("rotate");
        std::fs::write(dir.join(LOG_NAME), "the run that crashed").unwrap();

        let path = rotate(&dir);
        assert_eq!(path, dir.join(LOG_NAME));
        assert_eq!(
            std::fs::read_to_string(dir.join(PREVIOUS_LOG_NAME)).unwrap(),
            "the run that crashed"
        );
    }

    #[test]
    fn only_two_generations_are_kept() {
        let dir = scratch("two");
        for run in 0..4 {
            rotate(&dir);
            std::fs::write(dir.join(LOG_NAME), format!("run {run}")).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(dir.join(LOG_NAME)).unwrap(),
            "run 3"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(PREVIOUS_LOG_NAME)).unwrap(),
            "run 2"
        );
        let stray: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(stray.len(), 2, "{stray:?}");
    }

    #[test]
    fn a_first_run_has_nothing_to_rotate() {
        let dir = scratch("first");
        rotate(&dir);
        assert!(!dir.join(PREVIOUS_LOG_NAME).exists());
        assert!(existing(&dir).is_empty());
    }

    #[test]
    fn export_copies_both_generations_and_leaves_the_originals() {
        let dir = scratch("export");
        std::fs::write(dir.join(LOG_NAME), "now").unwrap();
        std::fs::write(dir.join(PREVIOUS_LOG_NAME), "before").unwrap();
        let out = dir.join("Desktop");

        let written = export_to(&dir, &out).unwrap();
        assert_eq!(written.len(), 2);
        assert_eq!(std::fs::read_to_string(out.join(LOG_NAME)).unwrap(), "now");
        assert_eq!(
            std::fs::read_to_string(out.join(PREVIOUS_LOG_NAME)).unwrap(),
            "before"
        );
        // Copied, not moved: the app is still writing to one of them.
        assert!(dir.join(LOG_NAME).exists());
    }

    #[test]
    fn exporting_with_no_logs_yet_is_not_an_error() {
        let dir = scratch("none");
        assert!(export_to(&dir, &dir.join("out")).unwrap().is_empty());
    }
}
