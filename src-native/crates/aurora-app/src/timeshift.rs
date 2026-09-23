//! The timeshift buffer's settings, and what it is costing (README §7.6, §15).
//!
//! There is no writer here. The buffer is mpv's own on-disk demuxer cache
//! (docs/DECISIONS.md D21), so what this module owns is the description of it: the four
//! numbers the settings panel edits, and the two questions a cache panel asks of any
//! cache — how much disk is it using, and empty it (README §13).

use std::fs;
use std::path::{Path, PathBuf};

use aurora_core::timeshift::Budget;
use aurora_db::repo::settings;
use aurora_db::rusqlite::Connection;
use aurora_player::backend::TimeshiftCache;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::Result;
use crate::services::Services;

pub const ENABLED_KEY: &str = "timeshift.enabled";
pub const BYTES_KEY: &str = "timeshift.bytes";
pub const SECS_KEY: &str = "timeshift.secs";
pub const FOLDER_KEY: &str = "timeshift.folder";

/// On unless turned off. Pressing pause on live TV and having it work is what people
/// expect of a television, and the default gigabyte is a modest thing to spend on it.
pub const DEFAULT_ENABLED: bool = true;

/// What the settings panel shows and edits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub enabled: bool,
    /// Rewind budget in bytes, and in seconds. Both are real; the tighter one decides
    /// what the viewer can reach (`aurora_core::timeshift::Budget`).
    pub bytes: u64,
    pub secs: u32,
    pub folder: String,
    /// What the buffer is using on disk at this moment.
    pub bytes_on_disk: u64,
}

/// Where the buffer lives. Beside the library by default, so a portable install stays
/// portable and an SSD-versus-spinning-disk choice is still available to anyone who
/// wants it.
pub fn folder(db: &Connection, data_dir: &Path) -> Result<PathBuf> {
    Ok(settings::get::<String>(db, FOLDER_KEY)?
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| data_dir.join("timeshift")))
}

pub fn budget(db: &Connection) -> Result<Budget> {
    let bytes = settings::get_or(db, BYTES_KEY, aurora_core::timeshift::DEFAULT_BYTES)?;
    let secs = settings::get_or(db, SECS_KEY, aurora_core::timeshift::DEFAULT_SECS)?;
    Ok(Budget::clamped(bytes, secs))
}

pub fn read(db: &Connection, data_dir: &Path) -> Result<Settings> {
    let dir = folder(db, data_dir)?;
    let budget = budget(db)?;
    Ok(Settings {
        enabled: settings::get_or(db, ENABLED_KEY, DEFAULT_ENABLED)?,
        bytes: budget.bytes,
        secs: budget.secs,
        bytes_on_disk: bytes_on_disk(&dir),
        folder: dir.to_string_lossy().into_owned(),
    })
}

/// What to hand the player when tuning a live channel, or `None` when nothing should be
/// kept.
///
/// A buffer that cannot be written is not a reason to refuse to play: the folder is
/// created here, and if that fails the channel tunes without rewind and says so in the
/// log.
pub fn cache_for(db: &Connection, data_dir: &Path) -> Result<Option<TimeshiftCache>> {
    if !settings::get_or(db, ENABLED_KEY, DEFAULT_ENABLED)? {
        return Ok(None);
    }
    let dir = folder(db, data_dir)?;
    if let Err(e) = fs::create_dir_all(&dir) {
        tracing::warn!(
            "timeshift is on but {} cannot be created ({e}); playing without a buffer",
            dir.display()
        );
        return Ok(None);
    }
    Ok(Some(TimeshiftCache {
        budget: budget(db)?,
        dir,
    }))
}

/// Total bytes in the buffer folder. An absent folder is an empty buffer, not an error.
pub fn bytes_on_disk(dir: &Path) -> u64 {
    let Ok(read) = fs::read_dir(dir) else {
        return 0;
    };
    read.filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// Empty the buffer, returning the bytes reclaimed.
///
/// Whatever mpv currently has open stays: Windows refuses to delete a file in use, and
/// that refusal is the right answer — the file being written is the programme the viewer
/// is watching.
pub fn clear(dir: &Path) -> u64 {
    let Ok(read) = fs::read_dir(dir) else {
        return 0;
    };
    read.filter_map(|e| e.ok())
        .filter_map(|entry| {
            let bytes = entry.metadata().ok().filter(|m| m.is_file())?.len();
            fs::remove_file(entry.path()).ok().map(|()| bytes)
        })
        .sum()
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetArgs {
    pub enabled: Option<bool>,
    pub bytes: Option<u64>,
    pub secs: Option<u32>,
    /// An empty string puts the folder back to the default beside the library.
    pub folder: Option<String>,
}

/// Apply what the panel changed and answer with what the buffer now is.
///
/// Returns the stored values rather than the requested ones, because they are not always
/// the same: a budget outside the documented range is clamped into it, and a panel that
/// showed the request back would be lying about what will happen.
pub fn write(db: &Connection, data_dir: &Path, args: &SetArgs) -> Result<Settings> {
    if let Some(enabled) = args.enabled {
        settings::set(db, ENABLED_KEY, &enabled)?;
    }
    if args.bytes.is_some() || args.secs.is_some() {
        let current = budget(db)?;
        let wanted = Budget::clamped(
            args.bytes.unwrap_or(current.bytes),
            args.secs.unwrap_or(current.secs),
        );
        settings::set(db, BYTES_KEY, &wanted.bytes)?;
        settings::set(db, SECS_KEY, &wanted.secs)?;
    }
    if let Some(folder) = &args.folder {
        let trimmed = folder.trim();
        if trimmed.is_empty() {
            settings::clear(db, settings::GLOBAL, FOLDER_KEY)?;
        } else {
            settings::set(db, FOLDER_KEY, &trimmed.to_string())?;
        }
    }
    read(db, data_dir)
}

#[tauri::command]
pub fn timeshift_settings(services: State<'_, Services>) -> Result<Settings> {
    let db = services.db.lock();
    read(&db, &services.data_dir)
}

/// Changes apply to the next channel tuned.
///
/// Deliberately not to the stream already playing: mpv sizes its cache when a file is
/// loaded, and re-loading to apply a settings change would black out a programme
/// somebody is watching.
#[tauri::command]
pub fn timeshift_set_settings(services: State<'_, Services>, args: SetArgs) -> Result<Settings> {
    let db = services.db.lock();
    write(&db, &services.data_dir, &args)
}

/// Empty the buffer. Returns the bytes reclaimed.
#[tauri::command]
pub fn timeshift_clear(services: State<'_, Services>) -> Result<u64> {
    let dir = {
        let db = services.db.lock();
        folder(&db, &services.data_dir)?
    };
    Ok(clear(&dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::timeshift::{MAX_BYTES, MIN_SECS};

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aurora-ts-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_defaults_are_the_buffer_the_spec_describes() {
        let db = aurora_db::open_memory().unwrap();
        let s = read(&db, Path::new("/data")).unwrap();
        assert!(s.enabled);
        assert_eq!(s.bytes, aurora_core::timeshift::DEFAULT_BYTES);
        assert_eq!(s.secs, 1800);
        assert!(s.folder.ends_with("timeshift"), "{}", s.folder);
        assert_eq!(s.bytes_on_disk, 0, "nothing buffered yet");
    }

    #[test]
    fn a_budget_outside_the_range_is_stored_clamped_and_reported_clamped() {
        let db = aurora_db::open_memory().unwrap();
        let applied = write(
            &db,
            Path::new("/data"),
            &SetArgs {
                bytes: Some(500 * 1024 * 1024 * 1024),
                secs: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(applied.bytes, MAX_BYTES);
        assert_eq!(applied.secs, MIN_SECS);
        // And the stored value is the clamped one, not the request: the next tune reads
        // the database, not this reply.
        assert_eq!(read(&db, Path::new("/data")).unwrap().bytes, MAX_BYTES);
    }

    #[test]
    fn changing_one_setting_leaves_the_others_alone() {
        let db = aurora_db::open_memory().unwrap();
        write(
            &db,
            Path::new("/data"),
            &SetArgs {
                bytes: Some(4 * 1024 * 1024 * 1024),
                ..Default::default()
            },
        )
        .unwrap();
        let after = write(
            &db,
            Path::new("/data"),
            &SetArgs {
                secs: Some(600),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(after.bytes, 4 * 1024 * 1024 * 1024, "the size survived");
        assert_eq!(after.secs, 600);
    }

    #[test]
    fn an_empty_folder_means_the_default_again() {
        let db = aurora_db::open_memory().unwrap();
        let moved = write(
            &db,
            Path::new("/data"),
            &SetArgs {
                folder: Some("/mnt/fast/buffer".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(moved.folder, "/mnt/fast/buffer");

        let back = write(
            &db,
            Path::new("/data"),
            &SetArgs {
                folder: Some("   ".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(back.folder.ends_with("timeshift"), "{}", back.folder);
    }

    #[test]
    fn turning_it_off_means_a_tune_is_given_no_buffer() {
        let dir = tempdir("off");
        let db = aurora_db::open_memory().unwrap();
        assert!(cache_for(&db, &dir).unwrap().is_some());

        write(
            &db,
            &dir,
            &SetArgs {
                enabled: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(cache_for(&db, &dir).unwrap().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tuning_creates_the_folder_and_carries_the_stored_budget() {
        let dir = tempdir("create");
        let db = aurora_db::open_memory().unwrap();
        write(
            &db,
            &dir,
            &SetArgs {
                bytes: Some(2 * 1024 * 1024 * 1024),
                secs: Some(900),
                ..Default::default()
            },
        )
        .unwrap();

        let cache = cache_for(&db, &dir).unwrap().unwrap();
        assert_eq!(cache.budget.bytes, 2 * 1024 * 1024 * 1024);
        assert_eq!(cache.budget.secs, 900);
        assert!(cache.dir.exists(), "mpv needs somewhere to write");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_cache_reports_its_size_and_can_be_emptied() {
        let dir = tempdir("size");
        fs::write(dir.join("cache-1.tmp"), vec![0u8; 4096]).unwrap();
        fs::write(dir.join("cache-2.tmp"), vec![0u8; 2048]).unwrap();
        fs::create_dir_all(dir.join("nested")).unwrap();

        assert_eq!(bytes_on_disk(&dir), 6144, "directories are not contents");
        assert_eq!(clear(&dir), 6144);
        assert_eq!(bytes_on_disk(&dir), 0);
        assert!(dir.exists(), "the folder itself stays");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_that_does_not_exist_is_an_empty_buffer_not_an_error() {
        let missing = std::env::temp_dir().join("aurora-ts-nothing-here");
        let _ = fs::remove_dir_all(&missing);
        assert_eq!(bytes_on_disk(&missing), 0);
        assert_eq!(clear(&missing), 0);
    }

    #[test]
    fn a_value_an_older_build_wrote_differently_falls_back_to_the_default() {
        let db = aurora_db::open_memory().unwrap();
        // The settings table is JSON, so a build that stored "1GB" as text is possible.
        settings::set(&db, BYTES_KEY, &"1GB".to_string()).unwrap();
        assert_eq!(
            read(&db, Path::new("/data")).unwrap().bytes,
            aurora_core::timeshift::DEFAULT_BYTES
        );
    }
}
