//! Settings → Account and sync → Backup: save everything sync would carry to a file, and
//! restore it (`aurora_sync::backup`), for the people who do not use sync.
//!
//! Saving writes to `Documents\Aurora TV\Backups`, named by the date. The folder is the
//! host's choice, not the page's: nothing the UI sends decides where a file is written
//! or which folder is opened. Restoring takes the file's text, which the page reads from
//! a file the viewer picked, so the host never opens a path the page names either.

use std::collections::BTreeMap;
use std::path::PathBuf;

use aurora_sync::backup::{self, Backup, Restored};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::error::{AppError, Result};
use crate::services::Services;

/// What a backup holds, for the screen to say before anything is written.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSummary {
    pub created_at: i64,
    pub app_version: String,
    /// Records by kind: `favorite`, `profile`, `setting`, …
    pub counts: BTreeMap<String, usize>,
}

impl From<&Backup> for BackupSummary {
    fn from(b: &Backup) -> Self {
        Self {
            created_at: b.created_at,
            app_version: b.app_version.clone(),
            counts: b.counts(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSaved {
    /// Where it went, to show the viewer.
    pub path: String,
    pub summary: BackupSummary,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupTextArgs {
    pub text: String,
}

/// The largest file a restore reads. A real backup is a few hundred kilobytes; this is
/// to refuse a wrong file early, not to fit a big one.
const MAX_BYTES: usize = 50 * 1024 * 1024;

fn parse(text: &str) -> Result<Backup> {
    if text.len() > MAX_BYTES {
        return Err(AppError::Other(
            "that file is far too large to be an Aurora backup".into(),
        ));
    }
    Backup::from_json(text).map_err(|e| AppError::Other(e.to_string()))
}

/// `Documents\Aurora TV\Backups`.
fn folder(app: &AppHandle) -> Result<PathBuf> {
    use tauri::Manager;
    let docs = app
        .path()
        .document_dir()
        .map_err(|e| AppError::Other(format!("could not find the Documents folder: {e}")))?;
    Ok(docs.join("Aurora TV").join("Backups"))
}

/// `aurora-tv-backup-2026-11-21-1405.json`, in local time.
fn file_name(t: time::OffsetDateTime) -> String {
    format!(
        "aurora-tv-backup-{:04}-{:02}-{:02}-{:02}{:02}.json",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute()
    )
}

/// Save a backup. Behind the Settings lock: the file holds the household's parental
/// settings.
#[tauri::command(async)]
pub fn backup_save(app: AppHandle, services: State<'_, Services>) -> Result<BackupSaved> {
    let file = {
        let mut db = services.db.lock();
        crate::profiles::require_settings(&db)?;
        backup::export(&mut db, crate::now_unix(), crate::updates::CURRENT)
            .map_err(|e| AppError::Other(e.to_string()))?
    };
    let dir = folder(&app)?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Other(format!("could not make {}: {e}", dir.display())))?;
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let path = dir.join(file_name(now));
    let text = file.to_json().map_err(|e| AppError::Other(e.to_string()))?;
    // Written beside, then moved into place, so a full disk never leaves half a backup
    // under the real name.
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, text)
        .and_then(|()| std::fs::rename(&partial, &path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&partial);
            AppError::Other(format!("could not save the backup: {e}"))
        })?;
    tracing::info!(records = file.records.len(), "backup saved");
    Ok(BackupSaved {
        path: path.display().to_string(),
        summary: BackupSummary::from(&file),
    })
}

/// What a backup file holds, without restoring anything.
#[tauri::command(async)]
pub fn backup_inspect(args: BackupTextArgs) -> Result<BackupSummary> {
    Ok(BackupSummary::from(&parse(&args.text)?))
}

/// Restore a backup. Behind the Settings lock, and behind the parental PIN when there is
/// one: a backup carries the parental settings, PIN included, so restoring an old one is
/// a way to change them.
#[tauri::command(async)]
pub fn backup_restore(services: State<'_, Services>, args: BackupTextArgs) -> Result<Restored> {
    let file = parse(&args.text)?;
    let done = {
        let mut db = services.db.lock();
        crate::profiles::require_settings(&db)?;
        crate::profiles::require_parent(&db)?;
        backup::restore(&mut db, &file, crate::now_unix())
            .map_err(|e| AppError::Other(e.to_string()))?
    };
    tracing::info!(
        applied = done.applied,
        waiting = done.waiting,
        skipped = done.skipped,
        "backup restored"
    );
    // With sync on, what was restored goes to the household now rather than next round.
    services.sync.wake();
    Ok(done)
}

/// Open the backups folder in Explorer.
#[tauri::command(async)]
pub fn backup_open_folder(app: AppHandle) -> Result<()> {
    let dir = folder(&app)?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Other(format!("could not make {}: {e}", dir.display())))?;
    open_folder(&dir).map_err(AppError::Other)
}

#[cfg(windows)]
fn open_folder(dir: &std::path::Path) -> std::result::Result<(), String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let path = HSTRING::from(dir.as_os_str());
    let result = unsafe {
        ShellExecuteW(
            None,
            windows::core::w!("explore"),
            PCWSTR(path.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        return Err(format!(
            "Windows would not open the folder (ShellExecute returned {})",
            result.0 as isize
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn open_folder(dir: &std::path::Path) -> std::result::Result<(), String> {
    Err(format!(
        "open {} by hand: there is no file manager to ask here",
        dir.display()
    ))
}

/// Try again what a restore could not place yet. Called after every import.
pub fn retry_parked(services: &Services) {
    let mut db = services.db.lock();
    match backup::retry_parked(&mut db, crate::now_unix()) {
        Ok(0) => {}
        Ok(n) => tracing::info!(
            records = n,
            "restored records found their place after an import"
        ),
        Err(e) => tracing::warn!("could not place restored records after an import: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_backup_is_named_by_the_minute_it_was_made() {
        let at = time::OffsetDateTime::from_unix_timestamp(1_795_270_000).unwrap();
        assert_eq!(file_name(at), "aurora-tv-backup-2026-11-21-1406.json");
    }

    #[test]
    fn the_wrong_file_is_refused_before_anything_is_read_into_it() {
        assert!(parse("{}")
            .unwrap_err()
            .to_string()
            .contains("not an Aurora backup"));
        let huge = "x".repeat(MAX_BYTES + 1);
        assert!(parse(&huge).unwrap_err().to_string().contains("too large"));
    }
}
