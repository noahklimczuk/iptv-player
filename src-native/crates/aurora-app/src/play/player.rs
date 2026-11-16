//! The player service: the session, where its picture goes, the sleep timer and the
//! seek preview, behind one handle the commands and the app's loop share.

use std::path::PathBuf;
use std::sync::Arc;

use aurora_db::rusqlite::Connection;
use aurora_video::{ItemKind, PlayerState, Status};
use parking_lot::Mutex;

use super::placement::{Corner, PipView, Placement, Rect};
use super::preview::{self, Frames, SeekPreview};
use super::resolve::What;
use super::session::Session;
use super::sleep::{Playing, SleepAction, SleepAfter, SleepDeadline, SleepState, SleepTimer};
use super::{guide, timeshift};
use crate::dblock::TimedMutex;
use crate::error::{AppError, Result};

/// Where the picture is drawn. A trait so the service is testable without a window.
pub trait VideoSurface: Send + Sync {
    fn place(&self, rect: Rect) -> std::result::Result<(), String>;
}

#[cfg(windows)]
impl VideoSurface for aurora_video::surface::Surface {
    fn place(&self, r: Rect) -> std::result::Result<(), String> {
        aurora_video::surface::Surface::place(
            self,
            aurora_video::surface::Rect {
                x: r.x,
                y: r.y,
                width: r.width,
                height: r.height,
            },
        )
    }
}

pub struct Player {
    pub session: Arc<Session>,
    db: Arc<TimedMutex<Connection>>,
    data_dir: PathBuf,
    surface: Option<Box<dyn VideoSurface>>,
    placement: Mutex<Placement>,
    sleep: Mutex<SleepTimer>,
    frames: Mutex<Frames>,
}

impl Player {
    pub fn new(
        session: Arc<Session>,
        db: Arc<TimedMutex<Connection>>,
        data_dir: PathBuf,
        surface: Option<Box<dyn VideoSurface>>,
        window: (u32, u32),
    ) -> Self {
        Self {
            session,
            db,
            data_dir,
            surface,
            placement: Mutex::new(Placement::new(window.0, window.1)),
            sleep: Mutex::new(SleepTimer::default()),
            frames: Mutex::new(Frames::default()),
        }
    }

    /// Move the surface to where the placement says. Called after every change.
    fn place(&self, rect: Rect) {
        if let Some(s) = &self.surface {
            if let Err(e) = s.place(rect) {
                tracing::warn!("{e}");
            }
        }
    }

    /// The surface's rectangle as things stand, for multi-view to sink it under its
    /// tiles.
    pub fn surface_rect(&self) -> Rect {
        self.placement.lock().surface()
    }

    /// Multi-view's tiles are open: send the surface, full size, beneath them. Every
    /// placement sends a surface to the bottom of the z-order, so placing this one
    /// after the tiles is what leaves them on top of it.
    pub fn sink_under_tiles(&self, width: u32, height: u32) {
        self.place(Rect {
            x: 0,
            y: 0,
            width: width.max(1),
            height: height.max(1),
        });
    }

    pub fn place_surface(&self) {
        let r = self.surface_rect();
        self.place(r);
    }

    /* ── Playing ─────────────────────────────────────────────────────────── */

    pub fn play(&self, what: What, start_secs: Option<f64>) -> Result<PlayerState> {
        self.session.play(what, start_secs)
    }

    /// Stop. The picture gets the whole window back for next time: PiP and any inlay
    /// belong to what was playing.
    pub fn stop(&self) -> Result<PlayerState> {
        let state = self.session.stop()?;
        let r = {
            let mut p = self.placement.lock();
            p.reset();
            p.surface()
        };
        self.place(r);
        Ok(state)
    }

    /* ── Placement (H21–H23) ─────────────────────────────────────────────── */

    pub fn pip(&self) -> PipView {
        self.placement.lock().view()
    }

    fn placed<T>(&self, change: impl FnOnce(&mut Placement) -> T) -> T {
        let (out, r) = {
            let mut p = self.placement.lock();
            let out = change(&mut p);
            (out, p.surface())
        };
        self.place(r);
        out
    }

    pub fn pip_toggle(&self, mosaic_open: bool) -> Result<PipView> {
        self.placed(|p| p.toggle(mosaic_open))
            .map_err(AppError::Other)
    }

    pub fn pip_set_enabled(&self, on: bool, mosaic_open: bool) -> Result<PipView> {
        self.placed(|p| p.set_enabled(on, mosaic_open))
            .map_err(AppError::Other)
    }

    pub fn pip_set_corner(&self, corner: Option<Corner>) -> PipView {
        self.placed(|p| p.set_corner(corner))
    }

    pub fn inlay(&self, rect: Rect, mosaic_open: bool) -> Result<PipView> {
        self.placed(|p| p.place(rect, mosaic_open))
            .map_err(AppError::Other)
    }

    pub fn clear_inlay(&self) -> PipView {
        self.placed(Placement::clear)
    }

    pub fn resized(&self, width: u32, height: u32) {
        self.placed(|p| p.resize(width, height));
    }

    /* ── Sleep timer (H19–H20) ───────────────────────────────────────────── */

    fn playing_now(&self, now: i64) -> Result<Playing> {
        let state = self.session.state();
        let r = &state.report;
        let mut playing = Playing {
            speed: r.speed,
            ..Playing::default()
        };
        if r.is_live {
            if let Some(ch) = state.channel_id {
                let db = self.db.lock();
                playing.programme_ends = guide::on_at(&db, ch, now)?.map(|p| p.stop);
            }
        } else if r.duration_secs > 0.0 && r.status.is_active() {
            playing.remaining_secs = Some((r.duration_secs - r.position_secs).max(0.0));
        }
        Ok(playing)
    }

    pub fn sleep_state(&self, now: i64) -> SleepState {
        self.sleep.lock().state(now)
    }

    pub fn sleep_set(
        &self,
        after: SleepAfter,
        action: SleepAction,
        now: i64,
    ) -> Result<SleepState> {
        let playing = self.playing_now(now)?;
        self.sleep
            .lock()
            .set(after, action, now, playing)
            .map_err(AppError::Other)
    }

    pub fn sleep_cancel(&self, now: i64) -> SleepState {
        self.sleep.lock().cancel(now)
    }

    pub fn sleep_extend(&self, minutes: u32, now: i64) -> Result<SleepState> {
        self.sleep
            .lock()
            .extend(minutes, now)
            .map_err(AppError::Other)
    }

    /// The deadline, once, when it comes.
    pub fn sleep_due(&self, now: i64) -> Option<SleepDeadline> {
        self.sleep.lock().due(now)
    }

    pub fn sleep_armed(&self) -> bool {
        self.sleep.lock().armed()
    }

    /* ── Seek preview (H24–H25) ──────────────────────────────────────────── */

    /// Take a picture if one is due. Called from a thread of its own: a grab can take
    /// a moment and nothing else should wait for it.
    pub fn preview_tick(&self, now: f64) {
        let title = self.session.plan().map(|p| p.what);
        let report = self.session.engine().report();
        let grab = self.frames.lock().observe(title, &report, now);
        if !grab {
            return;
        }
        let at = report.position_secs;
        let Some(frame) = self.session.engine().grab_frame(preview::MAX_WIDTH) else {
            return;
        };
        if let Some(url) = preview::data_url(&frame) {
            let now_at = self.session.engine().report().position_secs;
            self.frames.lock().keep(title, at, url, now_at);
        }
    }

    pub fn seek_preview(&self, position_secs: f64, now_unix: i64) -> Result<SeekPreview> {
        let state = self.session.state();
        let title = self.session.plan().map(|p| p.what);
        let r = &state.report;
        let mut out = SeekPreview {
            position_secs,
            ..SeekPreview::default()
        };
        if let Some((at, url)) = self.frames.lock().nearest(title, position_secs) {
            out.image = Some(url.to_string());
            out.image_at_secs = Some(at);
        }
        // A picture comes for a moment once it has played steadily on screen.
        out.pending = out.image.is_none()
            && r.status == Status::Playing
            && (position_secs - r.position_secs).abs() < preview::MIN_SPACING_SECS;
        let chapters = self.session.engine().chapters();
        let mut marks = aurora_core::markers::from_chapters(&chapters, r.duration_secs);
        if state.item_kind == Some(ItemKind::Episode) {
            if let Some(id) = state.item_id {
                let db = self.db.lock();
                marks.extend(aurora_db::repo::markers::for_episode(&db, id)?);
            }
        }
        out.label = preview::label(position_secs, &chapters, &marks);
        if r.is_live {
            out.clock_unix = preview::clock_at(r, position_secs, now_unix);
            if let (Some(clock), Some(ch)) = (out.clock_unix, state.channel_id) {
                let db = self.db.lock();
                out.programme = guide::on_at(&db, ch, clock)?.map(|p| p.title);
            }
        }
        Ok(out)
    }

    /* ── Timeshift settings ──────────────────────────────────────────────── */

    fn cache_in_use(&self) -> Option<u64> {
        let r = self.session.engine().report();
        r.timeshift?;
        r.stats.and_then(|s| s.cache_file_bytes)
    }

    pub fn timeshift_settings(&self) -> Result<timeshift::TimeshiftSettings> {
        let in_use = self.cache_in_use();
        let db = self.db.lock();
        timeshift::read(&db, &self.data_dir, in_use)
    }

    pub fn timeshift_change(&self, c: timeshift::Change) -> Result<timeshift::TimeshiftSettings> {
        let in_use = self.cache_in_use();
        let db = self.db.lock();
        timeshift::change(&db, &self.data_dir, c, in_use)
    }

    pub fn timeshift_clear(&self) -> Result<u64> {
        let playing = self.cache_in_use().is_some();
        let dir = {
            let db = self.db.lock();
            timeshift::folder(&db, &self.data_dir)?
        };
        Ok(timeshift::clear(&dir, playing))
    }
}
