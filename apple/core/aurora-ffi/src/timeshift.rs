//! Pause and rewind live TV (README §7.6): Windows' `aurora-app/src/play/timeshift.rs`
//! for the settings and the folder, and `aurora_core::timeshift` -- the same crate
//! Windows uses -- for the window the viewer can reach.
//!
//! As on Windows there is no buffer of Aurora's own. mpv's on-disk demuxer cache is the
//! buffer; `aurora_video::Request::settings` turns it on when a tune carries a
//! `Timeshift`, and this decides whether a channel gets one, how big, and where. What is
//! left is arithmetic over what mpv reports, done here so the iPhone, the Apple TV and
//! Windows draw the same window from the same numbers.

use std::fs;
use std::path::{Path, PathBuf};

use aurora_core::timeshift::{
    Budget, Reading, Window, DEFAULT_BYTES, DEFAULT_SECS, LIVE_EDGE_SECS,
};
use aurora_db::repo::settings;
use aurora_db::rusqlite::Connection;
use aurora_video::engine::LIVE_MARGIN_SECS;
use aurora_video::request::Timeshift;

use crate::error::Result;
use crate::AuroraCore;

// Windows' stored names, so a viewer's settings mean the same on both. They are
// per-device settings (`aurora_core::sync_settings`): sync never carries them across.
pub const ENABLED_KEY: &str = "timeshift.enabled";
pub const BYTES_KEY: &str = "timeshift.bytes";
pub const SECS_KEY: &str = "timeshift.secs";

/// On unless turned off, as on Windows: rewinding live TV is a headline feature.
pub const ENABLED_BY_DEFAULT: bool = true;

/// The sizes and limits the settings screen offers (`TimeshiftPanel`'s `SIZES` and
/// `MINUTES`).
pub const BUFFER_SIZES_GB: [u64; 4] = [1, 2, 5, 10];
pub const REWIND_LIMITS_MIN: [u32; 4] = [15, 30, 60, 180];

/// What the settings screen offers: buffer sizes in GB and rewind limits in minutes.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TimeshiftChoices {
    pub sizes_gb: Vec<u64>,
    pub limits_min: Vec<u32>,
}

#[uniffi::export]
pub fn timeshift_choices() -> TimeshiftChoices {
    TimeshiftChoices {
        sizes_gb: BUFFER_SIZES_GB.to_vec(),
        limits_min: REWIND_LIMITS_MIN.to_vec(),
    }
}

/// The buffer a channel gets: its caps. `None` on a tune means no rewind.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct RewindBudget {
    pub bytes: u64,
    pub secs: u32,
}

impl From<Budget> for RewindBudget {
    fn from(b: Budget) -> Self {
        Self {
            bytes: b.bytes,
            secs: b.secs,
        }
    }
}

impl From<RewindBudget> for Budget {
    /// Clamped: a budget that crossed the bridge is outside data like any other.
    fn from(b: RewindBudget) -> Self {
        Budget::clamped(b.bytes, b.secs)
    }
}

/// What the player read from mpv a moment ago (Windows' `Engine::refresh_window`).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct RewindReading {
    /// `time-pos`.
    pub position_secs: f64,
    /// `demuxer-cache-state` → `cache-end`: the newest moment buffered.
    pub live_secs: Option<f64>,
    /// The playhead when this channel was tuned: nothing before it was ever buffered.
    pub tuned_at_secs: Option<f64>,
    /// `video-bitrate` plus `audio-bitrate`; zero when not known yet.
    pub bitrate_bps: u64,
    /// `demuxer-cache-state` → the first `seekable-ranges` start: what mpv still holds.
    pub seekable_start_secs: Option<f64>,
}

/// The reachable span of a buffered channel and where the playhead is in it.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct RewindWindow {
    pub start_secs: f64,
    pub position_secs: f64,
    pub live_secs: f64,
    /// How far behind live the viewer is; zero within `LIVE_EDGE_SECS` of the edge, as
    /// Windows' `behindLive` says it.
    pub behind_secs: f64,
    /// How much is held: the readout beside the bar ("12:04 buffered").
    pub held_secs: f64,
}

impl From<Window> for RewindWindow {
    fn from(w: Window) -> Self {
        let behind = w.delay_secs();
        Self {
            start_secs: w.start_secs,
            position_secs: w.position_secs,
            live_secs: w.live_secs,
            behind_secs: if behind > LIVE_EDGE_SECS { behind } else { 0.0 },
            held_secs: w.span_secs(),
        }
    }
}

impl From<RewindWindow> for Window {
    fn from(w: RewindWindow) -> Self {
        Window {
            start_secs: w.start_secs,
            position_secs: w.position_secs,
            live_secs: w.live_secs,
        }
    }
}

/// The window, from a reading (`Engine::refresh_window`): the budget's bounds, then
/// what mpv still holds as the hard limit whatever the budget allows.
#[uniffi::export]
pub fn rewind_window(budget: RewindBudget, reading: RewindReading) -> RewindWindow {
    let position = reading.position_secs;
    let r = Reading {
        position_secs: position,
        live_secs: reading.live_secs.unwrap_or(position),
        tuned_at_secs: reading.tuned_at_secs.unwrap_or(position),
        bitrate_bps: reading.bitrate_bps,
    };
    let mut w = Budget::from(budget).window(&r);
    if let Some(s) = reading.seekable_start_secs.filter(|s| s.is_finite()) {
        w.start_secs = w.start_secs.max(s).min(w.position_secs);
    }
    w.into()
}

/// Where an absolute seek lands: held inside the buffer (E20), and never nearer the
/// newest data than "Back to live" goes.
///
/// Windows holds a seek at `live_secs` itself, but that is the last byte mpv has: asked
/// for it, mpv answers "Cannot seek in this stream" and stays put, so +30 a little
/// behind live did nothing (seen on the Apple TV simulator). Landing where
/// `rewind_live_target` lands makes a skip past live the same as "Back to live".
#[uniffi::export]
pub fn rewind_seek_target(window: RewindWindow, target_secs: f64) -> f64 {
    let held = Window::from(window).clamp(target_secs);
    if held.is_finite() {
        held.min(rewind_live_target(window))
    } else {
        held
    }
}

/// Where "Back to live" lands: a moment short of the newest data, since asking mpv for
/// its very last byte is how a live picture freezes (`Seek::Live`).
#[uniffi::export]
pub fn rewind_live_target(window: RewindWindow) -> f64 {
    (window.live_secs - LIVE_MARGIN_SECS).max(window.start_secs)
}

/// The Pause live TV settings (Windows' `TimeshiftSettings`, without the folder: on
/// Apple it is the app's own cache folder, not the viewer's to choose).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TimeshiftSettings {
    pub enabled: bool,
    pub bytes: u64,
    pub secs: u32,
    /// What "Empty buffer" could free: files left by an earlier run.
    pub reclaimable_bytes: u64,
}

/// A change to them; absent fields stay as they are.
#[derive(Debug, Clone, Default, uniffi::Record)]
pub struct TimeshiftChange {
    pub enabled: Option<bool>,
    pub bytes: Option<u64>,
    pub secs: Option<u32>,
}

fn budget(db: &Connection) -> Result<Budget> {
    Ok(Budget::clamped(
        settings::get_or(db, BYTES_KEY, DEFAULT_BYTES)?,
        settings::get_or(db, SECS_KEY, DEFAULT_SECS)?,
    ))
}

/// The buffer a channel tuned now gets, or `None` when the viewer turned it off (E22:
/// read at every tune, so a change applies to the next one).
pub(crate) fn for_live(db: &Connection, dir: &Path) -> Result<Option<Timeshift>> {
    if !settings::get_or(db, ENABLED_KEY, ENABLED_BY_DEFAULT)? {
        return Ok(None);
    }
    if let Err(e) = fs::create_dir_all(dir) {
        // mpv would fail to open its cache there, and with it the channel. A channel
        // without rewind beats no channel.
        tracing::warn!("no rewind buffer: the folder {dir:?} cannot be made: {e}");
        return Ok(None);
    }
    Ok(Some(Timeshift {
        budget: budget(db)?,
        dir: dir.to_path_buf(),
    }))
}

/// Files in the folder: leftovers. mpv's own cache file is never among them -- it is
/// unlinked as soon as it is opened -- so these are from a run that crashed.
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

#[uniffi::export]
impl AuroraCore {
    /// Where the rewind buffer goes. Swift points it at the app's Caches folder, which
    /// is not backed up and which the system may empty -- right for a buffer, wrong for
    /// the library beside which Windows keeps it.
    pub fn use_buffer_folder(&self, path: String) {
        *self.buffer_dir.lock() = Some(PathBuf::from(path));
    }

    pub fn timeshift_settings(&self) -> Result<TimeshiftSettings> {
        let db = self.db.lock();
        let b = budget(&db)?;
        Ok(TimeshiftSettings {
            enabled: settings::get_or(&db, ENABLED_KEY, ENABLED_BY_DEFAULT)?,
            bytes: b.bytes,
            secs: b.secs,
            reclaimable_bytes: leftovers(&self.buffer_folder())
                .iter()
                .map(|(_, n)| n)
                .sum(),
        })
    }

    /// Store a change, clamped into the documented range, and return what was stored.
    pub fn set_timeshift(&self, change: TimeshiftChange) -> Result<TimeshiftSettings> {
        {
            let db = self.db.lock();
            if let Some(on) = change.enabled {
                settings::set(&db, ENABLED_KEY, &on)?;
            }
            if change.bytes.is_some() || change.secs.is_some() {
                let now = budget(&db)?;
                let b = Budget::clamped(
                    change.bytes.unwrap_or(now.bytes),
                    change.secs.unwrap_or(now.secs),
                );
                settings::set(&db, BYTES_KEY, &b.bytes)?;
                settings::set(&db, SECS_KEY, &b.secs)?;
            }
        }
        self.timeshift_settings()
    }

    /// Delete the leftovers; the bytes freed.
    pub fn empty_timeshift(&self) -> u64 {
        leftovers(&self.buffer_folder())
            .into_iter()
            .filter(|(p, _)| fs::remove_file(p).is_ok())
            .map(|(_, n)| n)
            .sum()
    }
}

impl AuroraCore {
    pub(crate) fn buffer_folder(&self) -> PathBuf {
        self.buffer_dir
            .lock()
            .clone()
            .unwrap_or_else(|| self.data_dir.join("timeshift"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::timeshift::{MAX_BYTES, MIN_SECS};

    #[test]
    fn by_default_a_channel_gets_a_buffer_in_the_folder_it_was_given() {
        let (core, dir) = crate::testing::core();
        let buffers = dir.path().join("Caches/timeshift");
        core.use_buffer_folder(buffers.display().to_string());
        let t = for_live(&core.db.lock(), &core.buffer_folder())
            .unwrap()
            .expect("a buffer");
        assert_eq!(t.dir, buffers);
        assert_eq!(t.budget, Budget::default());
        assert!(buffers.is_dir());
    }

    #[test]
    fn turned_off_means_the_next_channel_gets_none() {
        let (core, _dir) = crate::testing::core();
        core.set_timeshift(TimeshiftChange {
            enabled: Some(false),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            for_live(&core.db.lock(), &core.buffer_folder()).unwrap(),
            None
        );
    }

    #[test]
    fn a_change_is_held_to_the_documented_range() {
        let (core, _dir) = crate::testing::core();
        let s = core
            .set_timeshift(TimeshiftChange {
                bytes: Some(u64::MAX),
                secs: Some(1),
                ..Default::default()
            })
            .unwrap();
        assert_eq!((s.bytes, s.secs), (MAX_BYTES, MIN_SECS));
        assert!(s.enabled);
    }

    #[test]
    fn leftovers_are_counted_and_emptied() {
        let (core, dir) = crate::testing::core();
        let buffers = dir.path().join("buf");
        fs::create_dir_all(&buffers).unwrap();
        fs::write(buffers.join("old"), vec![0u8; 1000]).unwrap();
        core.use_buffer_folder(buffers.display().to_string());
        assert_eq!(core.timeshift_settings().unwrap().reclaimable_bytes, 1000);
        assert_eq!(core.empty_timeshift(), 1000);
        assert_eq!(core.timeshift_settings().unwrap().reclaimable_bytes, 0);
    }

    fn reading(position: f64, live: f64, tuned_at: f64) -> RewindReading {
        RewindReading {
            position_secs: position,
            live_secs: Some(live),
            tuned_at_secs: Some(tuned_at),
            bitrate_bps: 0,
            seekable_start_secs: None,
        }
    }

    #[test]
    fn the_window_opens_at_the_tune_and_says_how_far_behind() {
        let b = RewindBudget::from(Budget::default());
        // Ten minutes on, paused two minutes ago.
        let w = rewind_window(b, reading(1480.0, 1600.0, 1000.0));
        assert_eq!(w.start_secs, 1000.0);
        assert_eq!(w.held_secs, 600.0);
        assert_eq!(w.behind_secs, 120.0);
        // Within the live edge counts as live.
        assert_eq!(
            rewind_window(b, reading(1597.0, 1600.0, 1000.0)).behind_secs,
            0.0
        );
        // What mpv has let go of is gone, whatever the budget allows.
        let mut r = reading(1480.0, 1600.0, 1000.0);
        r.seekable_start_secs = Some(1200.0);
        assert_eq!(rewind_window(b, r).start_secs, 1200.0);
    }

    #[test]
    fn seeks_stay_inside_the_buffer_and_live_lands_short_of_the_edge() {
        let w = rewind_window(
            RewindBudget::from(Budget::default()),
            reading(1480.0, 1600.0, 1000.0),
        );
        assert_eq!(rewind_seek_target(w, 2000.0), 1600.0 - LIVE_MARGIN_SECS);
        assert_eq!(rewind_seek_target(w, 1500.0), 1500.0);
        assert_eq!(rewind_seek_target(w, 0.0), 1000.0);
        assert_eq!(rewind_live_target(w), 1600.0 - LIVE_MARGIN_SECS);
    }
}
