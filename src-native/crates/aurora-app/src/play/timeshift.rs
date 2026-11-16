//! The live rewind buffer's settings and its folder (README §7.6, docs/PLAYER.md E21–E24).
//!
//! The engine does the buffering; this decides whether a channel gets a buffer, how
//! big, and where, and looks after what is left on disk.

use std::fs;
use std::path::{Path, PathBuf};

use aurora_core::timeshift::{Budget, DEFAULT_BYTES, DEFAULT_SECS};
use aurora_db::repo::settings;
use aurora_db::rusqlite::Connection;
use aurora_video::Timeshift;
use serde::{Deserialize, Serialize};

use crate::error::Result;

// The stored names. They predate this module and are kept so a viewer's settings
// survive the update.
pub const ENABLED_KEY: &str = "timeshift.enabled";
pub const BYTES_KEY: &str = "timeshift.bytes";
pub const SECS_KEY: &str = "timeshift.secs";
pub const FOLDER_KEY: &str = "timeshift.folder";

/// On unless turned off: rewinding live TV is a headline feature.
pub const ENABLED_BY_DEFAULT: bool = true;

/// `TimeshiftSettings` in `shared/ipc.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeshiftSettings {
    pub enabled: bool,
    pub bytes: u64,
    pub secs: u32,
    pub folder: String,
    /// What mpv says its cache file holds now.
    pub bytes_on_disk: u64,
    /// What "Empty buffer" could free: everything but the file in use.
    pub reclaimable_bytes: u64,
}

/// `timeshift.setSettings`'s arguments. Absent fields are left as they are.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub enabled: Option<bool>,
    pub bytes: Option<u64>,
    pub secs: Option<u32>,
    /// Empty restores the default folder.
    pub folder: Option<String>,
}

/// The default folder: beside the library.
pub fn default_folder(data_dir: &Path) -> PathBuf {
    data_dir.join("timeshift")
}

pub fn folder(db: &Connection, data_dir: &Path) -> Result<PathBuf> {
    Ok(settings::get::<String>(db, FOLDER_KEY)?
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_folder(data_dir)))
}

fn budget(db: &Connection) -> Result<Budget> {
    Ok(Budget::clamped(
        settings::get_or(db, BYTES_KEY, DEFAULT_BYTES)?,
        settings::get_or(db, SECS_KEY, DEFAULT_SECS)?,
    ))
}

/// The buffer a channel tuned now gets, or `None` when the viewer turned it off
/// (E22: read at every tune, so a change applies to the next one).
pub fn for_live(db: &Connection, data_dir: &Path) -> Result<Option<Timeshift>> {
    if !settings::get_or(db, ENABLED_KEY, ENABLED_BY_DEFAULT)? {
        return Ok(None);
    }
    let dir = folder(db, data_dir)?;
    if let Err(e) = fs::create_dir_all(&dir) {
        // mpv would fail to open its cache there, and with it the channel. A channel
        // without rewind beats no channel.
        tracing::warn!("no rewind buffer: the folder {dir:?} cannot be made: {e}");
        return Ok(None);
    }
    Ok(Some(Timeshift {
        budget: budget(db)?,
        dir,
    }))
}

/// The settings, with what is on disk. `in_use` is mpv's cache file size while a
/// buffered stream plays.
pub fn read(db: &Connection, data_dir: &Path, in_use: Option<u64>) -> Result<TimeshiftSettings> {
    let b = budget(db)?;
    let dir = folder(db, data_dir)?;
    Ok(TimeshiftSettings {
        enabled: settings::get_or(db, ENABLED_KEY, ENABLED_BY_DEFAULT)?,
        bytes: b.bytes,
        secs: b.secs,
        folder: dir.to_string_lossy().into_owned(),
        bytes_on_disk: in_use.unwrap_or(0),
        reclaimable_bytes: leftovers(&dir).iter().map(|(_, n)| n).sum(),
    })
}

/// Store a change, clamped into the documented range, and return what was stored.
pub fn change(
    db: &Connection,
    data_dir: &Path,
    c: Change,
    in_use: Option<u64>,
) -> Result<TimeshiftSettings> {
    if let Some(on) = c.enabled {
        settings::set(db, ENABLED_KEY, &on)?;
    }
    if c.bytes.is_some() || c.secs.is_some() {
        let now = budget(db)?;
        let b = Budget::clamped(c.bytes.unwrap_or(now.bytes), c.secs.unwrap_or(now.secs));
        settings::set(db, BYTES_KEY, &b.bytes)?;
        settings::set(db, SECS_KEY, &b.secs)?;
    }
    if let Some(f) = c.folder {
        let f = f.trim();
        if f.is_empty() {
            settings::clear(db, settings::GLOBAL, FOLDER_KEY)?;
        } else {
            settings::set(db, FOLDER_KEY, &f.to_string())?;
        }
    }
    read(db, data_dir, in_use)
}

/// The files in `dir`: leftovers from earlier runs.
///
/// mpv's own cache file is never among them. It is created, opened and unlinked at
/// once, so the open handle keeps the bytes and the folder keeps no name; that is why
/// the buffer in use is read from mpv (`bytes_on_disk`), never from a listing.
fn leftovers(dir: &Path) -> Vec<(PathBuf, u64)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file().then(|| (e.path(), m.len()))
        })
        .collect()
}

/// Delete the leftovers; the bytes freed. A file still held open is skipped: Windows
/// refuses, and it counts for nothing.
pub fn clear(dir: &Path) -> u64 {
    leftovers(dir)
        .into_iter()
        .filter(|(p, _)| fs::remove_file(p).is_ok())
        .map(|(_, n)| n)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::timeshift::{MAX_BYTES, MIN_SECS};

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aurora-play-ts-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn by_default_a_channel_gets_a_buffer_beside_the_library() {
        let db = aurora_db::open_memory().unwrap();
        let data = tempdir("default");
        let t = for_live(&db, &data).unwrap().expect("a buffer");
        assert_eq!(t.dir, data.join("timeshift"));
        assert_eq!(t.budget, Budget::default());
        assert!(t.dir.is_dir());
    }

    #[test]
    fn e22_turned_off_means_the_next_channel_gets_none() {
        let db = aurora_db::open_memory().unwrap();
        let data = tempdir("off");
        change(
            &db,
            &data,
            Change {
                enabled: Some(false),
                ..Change::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(for_live(&db, &data).unwrap(), None);
    }

    #[test]
    fn a_budget_outside_the_range_is_clamped_and_reported_as_stored() {
        let db = aurora_db::open_memory().unwrap();
        let data = tempdir("clamp");
        let s = change(
            &db,
            &data,
            Change {
                bytes: Some(u64::MAX),
                secs: Some(1),
                ..Change::default()
            },
            None,
        )
        .unwrap();
        assert_eq!((s.bytes, s.secs), (MAX_BYTES, MIN_SECS));
        assert_eq!(read(&db, &data, None).unwrap(), s);
    }

    #[test]
    fn an_empty_folder_restores_the_default() {
        let db = aurora_db::open_memory().unwrap();
        let data = tempdir("folder");
        let elsewhere = data.join("elsewhere");
        let s = change(
            &db,
            &data,
            Change {
                folder: Some(elsewhere.to_string_lossy().into()),
                ..Change::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(PathBuf::from(&s.folder), elsewhere);
        let s = change(
            &db,
            &data,
            Change {
                folder: Some("  ".into()),
                ..Change::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(PathBuf::from(&s.folder), data.join("timeshift"));
    }

    #[test]
    fn emptying_frees_the_leftovers_and_reports_what_it_freed() {
        let dir = tempdir("clear");
        fs::write(dir.join("old"), vec![0u8; 100]).unwrap();
        fs::write(dir.join("older"), vec![0u8; 7]).unwrap();
        assert_eq!(leftovers(&dir).iter().map(|f| f.1).sum::<u64>(), 107);
        assert_eq!(clear(&dir), 107);
        assert!(leftovers(&dir).is_empty());
        assert_eq!(clear(&dir), 0);
    }
}
