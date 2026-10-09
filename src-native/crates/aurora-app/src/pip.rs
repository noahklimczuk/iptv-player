//! Picture-in-picture (README §6.2, §7.2, §14.1 `P`).
//!
//! The main player's surface, placed into a corner instead of filling the window, so
//! the viewer can browse the guide or the library while the channel keeps playing.
//! §7.2 calls the guide half of this "the single most cable-like detail; do not skip
//! it".
//!
//! **It is the same stream, moved.** No second instance, no second connection, nothing
//! to refuse — which is the whole difference between this and a mosaic tile, and the
//! reason there is no budget check anywhere in here. Turning PiP on cannot fail for any
//! reason a provider has an opinion about.
//!
//! ## Why this owns the resize
//!
//! The surface is a sibling of the WebView and nothing moves it on its own, so the
//! window's `Resized` event has always called `player.resize(w, h)` — which means "fill
//! the client area" and has no origin to put a corner at. With PiP on, that call is
//! exactly wrong: it would snap the small picture back to full-window on the first drag
//! of the window edge. So the resize goes through here instead, and this decides which
//! of the two the surface should get. One place, rather than a flag the resize handler
//! has to remember to read.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use aurora_core::mosaic::Rect;
use aurora_core::pip::{Corner, Geometry};
use aurora_player::PlayerBackend;
use parking_lot::Mutex;
use serde::Serialize;

use crate::error::Result;

/// PiP as the UI draws it.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipView {
    pub enabled: bool,
    pub corner: Corner,
    /// Where the picture is, in physical pixels of the client area.
    ///
    /// Sent even when PiP is off — it is where the tile *would* go, which is what lets
    /// the UI animate into it rather than having it appear.
    pub rect: Rect,
    /// Where a page has asked for the picture to be inlaid, if anywhere.
    ///
    /// The guide's preview panel, today. Echoed back rather than simply remembered by
    /// the caller because the host clamps it: a page that asks for a rectangle half
    /// outside the window gets the one it actually got, and can draw its frame there.
    pub inlay: Option<Rect>,
}

pub struct Pip {
    player: Arc<Mutex<Box<dyn PlayerBackend>>>,
    enabled: AtomicBool,
    geometry: Mutex<Geometry>,
    size: Mutex<(u32, u32)>,
    /// A rectangle a page has asked the picture to sit in.
    ///
    /// This is here rather than in a service of its own because `apply` below is the one
    /// function in the app that moves the surface, and three services each moving it
    /// would be three things to arbitrate between on every window resize. The precedence
    /// is stated once, in `apply`.
    inlay: Mutex<Option<Rect>>,
}

impl Pip {
    pub fn new(player: Arc<Mutex<Box<dyn PlayerBackend>>>) -> Self {
        Self {
            player,
            enabled: AtomicBool::new(false),
            geometry: Mutex::new(Geometry::default()),
            size: Mutex::new((1280, 720)),
            inlay: Mutex::new(None),
        }
    }

    pub fn set_window(&self, width: u32, height: u32) {
        *self.size.lock() = (width.max(1), height.max(1));
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// The window size and the geometry, each copied out before the other is asked for.
    ///
    /// One function, because the bug this replaced was a *nested* lock and the way to not
    /// have one again is to have a single place that takes them. `state` used to read
    ///
    /// ```ignore
    /// PipView {
    ///     corner: self.geometry.lock().corner,
    ///     rect: self.rect(),          // locks `geometry` again
    /// }
    /// ```
    ///
    /// and a temporary inside a struct literal lives until the whole literal is built —
    /// so the guard from `corner` was still held when `rect` asked for it. `parking_lot`
    /// mutexes are not reentrant, so the first call deadlocked, permanently. Every public
    /// method here goes through `state` or `apply`, which means picture-in-picture could
    /// not work at all; and because `relayout` is called from the window's `Resized`
    /// handler, the first resize of the window parked the *main thread*, and with it
    /// wry's custom-protocol handler. That is what shipped in 1.0.1: a window that drew,
    /// a UI that mounted, and then no command, event or asset request ever answering
    /// again — "Opening your library…" forever.
    fn snapshot(&self) -> ((u32, u32), Geometry, Option<Rect>) {
        let size = *self.size.lock();
        let geometry = *self.geometry.lock();
        let inlay = *self.inlay.lock();
        (size, geometry, inlay)
    }

    pub fn state(&self) -> PipView {
        let ((w, h), geometry, inlay) = self.snapshot();
        PipView {
            enabled: self.is_enabled(),
            corner: geometry.corner,
            rect: geometry.rect(w, h),
            inlay,
        }
    }

    /// Put the surface where the current mode says it belongs.
    ///
    /// The one function that touches the backend's geometry, so "full window" and "in a
    /// corner" cannot drift apart.
    fn apply(&self) -> Result<PipView> {
        // One snapshot rather than a size read and then `state`, so the rect handed to
        // the backend is from the same moment as the size it is measured against — and
        // so there stays exactly one place in this file that takes these two locks.
        let ((w, h), geometry, inlay) = self.snapshot();
        let view = PipView {
            enabled: self.is_enabled(),
            corner: geometry.corner,
            rect: geometry.rect(w, h),
            inlay,
        };
        let mut player = self.player.lock();
        // The precedence, in one place.
        //
        // A corner beats an inlay: picture-in-picture is a mode the viewer turned on
        // deliberately and can see, while an inlay is a page asking for the surface
        // because it happens to be open. A page losing its preview to PiP is
        // explainable; PiP silently moving because somebody opened the guide is not.
        if view.enabled {
            player.place(view.rect)?;
        } else if let Some(rect) = inlay {
            player.place(rect)?;
        } else {
            player.resize(w, h)?;
        }
        Ok(view)
    }

    /// `P`: small picture, or back to the whole window.
    pub fn toggle(&self) -> Result<PipView> {
        let was = self.enabled.fetch_xor(true, Ordering::SeqCst);
        match self.apply() {
            Ok(view) => Ok(view),
            Err(e) => {
                // Put the flag back if the surface would not move, so the UI is never
                // drawing a small frame around a full-window picture.
                self.enabled.store(was, Ordering::SeqCst);
                Err(e)
            }
        }
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<PipView> {
        let was = self.enabled.swap(enabled, Ordering::SeqCst);
        match self.apply() {
            Ok(view) => Ok(view),
            Err(e) => {
                self.enabled.store(was, Ordering::SeqCst);
                Err(e)
            }
        }
    }

    /// Move the picture to another corner. Clockwise when `corner` is `None`.
    pub fn set_corner(&self, corner: Option<Corner>) -> Result<PipView> {
        {
            let mut geometry = self.geometry.lock();
            geometry.corner = corner.unwrap_or_else(|| geometry.corner.next());
        }
        self.apply()
    }

    /// Inlay the picture into a rectangle a page has measured, or stop.
    ///
    /// The rectangle arrives in physical pixels of the client area, because that is what
    /// the backend places surfaces in and the conversion from CSS pixels needs the device
    /// pixel ratio, which only the page knows.
    ///
    /// Clamped rather than trusted. A page measures its own layout, and a box that is
    /// mid-animation, mid-scroll or momentarily zero-sized will produce a rectangle that
    /// is partly outside the window — and a surface placed outside its parent is not a
    /// small mistake on Windows, it is a child window nobody can see and no amount of
    /// re-measuring brings back. The clamped rectangle is returned so the page draws its
    /// frame where the picture actually went.
    ///
    /// `None` gives the window back, which is what leaving the page has to do.
    pub fn set_inlay(&self, rect: Option<Rect>) -> Result<PipView> {
        let ((w, h), _, _) = self.snapshot();
        *self.inlay.lock() = rect.and_then(|r| clamp_to_window(r, w, h));
        self.apply()
    }

    /// The window changed size. Called for every resize, PiP on or off.
    pub fn relayout(&self, width: u32, height: u32) -> Result<PipView> {
        self.set_window(width, height);
        self.apply()
    }
}

/// A rectangle trimmed to the client area, or `None` if nothing of it is left.
///
/// Separate and total: every branch has a test, because the failure it prevents is
/// invisible. A surface placed outside its parent window simply is not drawn, and the
/// symptom is "the preview is black" with nothing wrong anywhere a log would look.
fn clamp_to_window(r: Rect, width: u32, height: u32) -> Option<Rect> {
    let (win_w, win_h) = (width as i64, height as i64);
    // The edges, before any of this is a size again.
    let left = r.x as i64;
    let top = r.y as i64;
    let right = left + r.width as i64;
    let bottom = top + r.height as i64;

    let left_in = left.max(0);
    let top_in = top.max(0);
    let right_in = right.min(win_w);
    let bottom_in = bottom.min(win_h);

    // Entirely off one side, or asked for with no area in the first place.
    if right_in <= left_in || bottom_in <= top_in {
        return None;
    }
    Some(Rect {
        x: left_in as i32,
        y: top_in as i32,
        width: (right_in - left_in) as u32,
        height: (bottom_in - top_in) as u32,
    })
}

/* ── Commands ──────────────────────────────────────────────────────────────── */

use tauri::State;

use crate::services::Services;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CornerArgs {
    /// `None` moves clockwise, which is what the button on the tile does.
    pub corner: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnabledArgs {
    pub enabled: bool,
}

#[tauri::command(async)]
pub fn pip_state(services: State<'_, Services>) -> Result<PipView> {
    Ok(services.pip.state())
}

/// Toggle the small picture.
///
/// Refused while a mosaic is open: both want the same surfaces in the same window, and
/// nine tiles with one of them also in a corner is not a thing anybody asked for. The
/// refusal names the way out rather than being silent.
#[tauri::command(async)]
pub fn pip_toggle(services: State<'_, Services>) -> Result<PipView> {
    if services.mosaic.is_open() {
        return Err(crate::AppError::Other(
            "Close multi-view first — it is already using the picture.".into(),
        ));
    }
    services.pip.toggle()
}

#[tauri::command(async)]
pub fn pip_set_enabled(services: State<'_, Services>, args: EnabledArgs) -> Result<PipView> {
    if args.enabled && services.mosaic.is_open() {
        return Err(crate::AppError::Other(
            "Close multi-view first — it is already using the picture.".into(),
        ));
    }
    services.pip.set_enabled(args.enabled)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlayArgs {
    /// Physical pixels, relative to the client area. The page converts from CSS pixels.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Put the picture in a rectangle this page has measured (the guide's preview).
///
/// Refused while a mosaic is open, for the same reason `pip.toggle` is: the tiles are
/// already using the surfaces.
#[tauri::command(async)]
pub fn preview_place(services: State<'_, Services>, args: InlayArgs) -> Result<PipView> {
    if services.mosaic.is_open() {
        return Err(crate::AppError::Other(
            "Close multi-view first — it is already using the picture.".into(),
        ));
    }
    services.pip.set_inlay(Some(Rect {
        x: args.x,
        y: args.y,
        width: args.width,
        height: args.height,
    }))
}

/// Give the window back. Called when the page that asked for an inlay goes away.
///
/// Not refused while a mosaic is open: this is how a page cleans up, and a cleanup that
/// can fail leaves the surface stuck in a rectangle belonging to a screen nobody is
/// looking at any more.
#[tauri::command(async)]
pub fn preview_clear(services: State<'_, Services>) -> Result<PipView> {
    services.pip.set_inlay(None)
}

#[tauri::command(async)]
pub fn pip_set_corner(services: State<'_, Services>, args: CornerArgs) -> Result<PipView> {
    let corner = match args.corner.as_deref() {
        None => None,
        Some(name) => Some(
            Corner::parse(name)
                .ok_or_else(|| crate::AppError::Other(format!("unknown corner {name:?}")))?,
        ),
    };
    services.pip.set_corner(corner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_player::PlayerState;

    /// Where the surface was last put, shared with the test.
    ///
    /// A recording backend rather than `NullBackend` and a downcast: `Pip` holds a
    /// `Box<dyn PlayerBackend>`, the trait has no `as_any`, and adding one so a test
    /// could look inside would be shaping the production seam around the test.
    #[derive(Clone, Default)]
    struct Spy(Arc<Mutex<Option<Rect>>>);

    impl PlayerBackend for Spy {
        fn load(
            &mut self,
            _: &str,
            _: &aurora_player::backend::LoadOptions,
        ) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn stop(&mut self) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn set_paused(&mut self, _: bool) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn seek(&mut self, _: f64, _: bool) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn set_volume(&mut self, _: u32) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn set_muted(&mut self, _: bool) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn set_speed(&mut self, _: f64) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn set_audio_track(
            &mut self,
            _: Option<i64>,
        ) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn set_subtitle_track(
            &mut self,
            _: Option<i64>,
        ) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn set_aspect(
            &mut self,
            _: aurora_player::Aspect,
        ) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn state(&self) -> PlayerState {
            PlayerState::default()
        }
        fn chapters(&self) -> Vec<aurora_core::markers::Chapter> {
            Vec::new()
        }
        fn resize(
            &mut self,
            width: u32,
            height: u32,
        ) -> std::result::Result<(), aurora_player::PlayerError> {
            *self.0.lock() = Some(Rect {
                x: 0,
                y: 0,
                width,
                height,
            });
            Ok(())
        }
        fn place(&mut self, rect: Rect) -> std::result::Result<(), aurora_player::PlayerError> {
            *self.0.lock() = Some(rect);
            Ok(())
        }
    }

    fn pip() -> (Pip, Arc<Mutex<Option<Rect>>>) {
        let seen: Arc<Mutex<Option<Rect>>> = Arc::new(Mutex::new(None));
        let player: Arc<Mutex<Box<dyn PlayerBackend>>> =
            Arc::new(Mutex::new(Box::new(Spy(Arc::clone(&seen)))));
        (Pip::new(player), seen)
    }

    fn placed(seen: &Arc<Mutex<Option<Rect>>>) -> Option<Rect> {
        *seen.lock()
    }

    /// Every entry point returns, on its own thread, within a deadline.
    ///
    /// A deadlock does not fail a Rust test, it hangs it -- and a hung test reads as a
    /// slow machine or a stuck runner rather than as the bug it is. Two of the tests in
    /// this module did hang on the nested `geometry` lock described on `snapshot`, which
    /// is a worse outcome than failing: the suite was killed by a timeout somewhere else,
    /// nobody looked here, and picture-in-picture shipped in a state where the first
    /// window resize parked the host's main thread.
    ///
    /// So this one asks the question directly, with a watchdog, and says which call it
    /// was. It is a cheap guard against reintroducing a lock held across another one --
    /// which is easy to do here by accident, because a `MutexGuard` temporary inside a
    /// struct literal or a method-call chain lives longer than it looks.
    #[test]
    fn every_entry_point_returns_rather_than_deadlocking() {
        use std::sync::mpsc;
        use std::time::Duration;

        /// Generous for work that takes microseconds, and still far short of a hang.
        const DEADLINE: Duration = Duration::from_secs(5);

        /// A named entry point, called through a `&Pip`.
        type Call = (&'static str, fn(&Pip));

        let calls: Vec<Call> = vec![
            ("state", |p| {
                p.state();
            }),
            ("relayout", |p| {
                p.relayout(1920, 1080).unwrap();
            }),
            ("toggle", |p| {
                p.toggle().unwrap();
            }),
            ("set_enabled", |p| {
                p.set_enabled(true).unwrap();
            }),
            ("set_corner", |p| {
                p.set_corner(Some(Corner::TopLeft)).unwrap();
            }),
            ("set_corner clockwise", |p| {
                p.set_corner(None).unwrap();
            }),
            ("set_window then state", |p| {
                p.set_window(1280, 720);
                p.state();
            }),
        ];

        for (name, call) in calls {
            let (tx, rx) = mpsc::channel();
            // A fresh `Pip` per call, on its own thread: a deadlocked thread cannot be
            // killed, so it is left parked and the test ends on the deadline instead.
            std::thread::spawn(move || {
                let (p, _seen) = pip();
                call(&p);
                let _ = tx.send(());
            });
            assert!(
                rx.recv_timeout(DEADLINE).is_ok(),
                "Pip::{name} did not return within {DEADLINE:?} -- it is holding one of                  its locks across another. See `Pip::snapshot`.",
            );
        }
    }

    const WIN: (u32, u32) = (1920, 1080);

    fn rect(x: i32, y: i32, w: u32, h: u32) -> Rect {
        Rect { x, y, width: w, height: h }
    }

    #[test]
    fn a_rectangle_inside_the_window_is_left_alone() {
        let r = rect(1200, 40, 640, 360);
        assert_eq!(clamp_to_window(r, WIN.0, WIN.1), Some(r));
    }

    /// Flush against the right and bottom edges is inside, not outside.
    #[test]
    fn a_rectangle_touching_the_far_edges_is_still_inside() {
        let r = rect(1280, 720, 640, 360);
        assert_eq!(clamp_to_window(r, WIN.0, WIN.1), Some(r));
    }

    #[test]
    fn a_rectangle_hanging_off_the_right_is_trimmed() {
        let got = clamp_to_window(rect(1800, 100, 400, 200), WIN.0, WIN.1);
        assert_eq!(got, Some(rect(1800, 100, 120, 200)));
    }

    /// Negative origins happen: a box measured while a panel is animating in.
    #[test]
    fn a_rectangle_starting_before_the_origin_is_trimmed_and_moved() {
        let got = clamp_to_window(rect(-50, -20, 400, 200), WIN.0, WIN.1);
        assert_eq!(got, Some(rect(0, 0, 350, 180)));
    }

    #[test]
    fn a_rectangle_entirely_outside_is_nothing() {
        for r in [
            rect(1920, 100, 300, 200),
            rect(-400, 100, 300, 200),
            rect(100, 1080, 300, 200),
            rect(100, -300, 300, 200),
        ] {
            assert_eq!(clamp_to_window(r, WIN.0, WIN.1), None, "{r:?}");
        }
    }

    /// A box that has not been laid out yet measures zero, and a zero-sized surface is
    /// not a picture.
    #[test]
    fn a_rectangle_with_no_area_is_nothing() {
        assert_eq!(clamp_to_window(rect(10, 10, 0, 200), WIN.0, WIN.1), None);
        assert_eq!(clamp_to_window(rect(10, 10, 200, 0), WIN.0, WIN.1), None);
    }

    /// The arithmetic is done in `i64`, so a page sending nonsense cannot overflow its
    /// way to a rectangle that passes the checks.
    #[test]
    fn an_absurd_rectangle_does_not_wrap_around() {
        let got = clamp_to_window(rect(i32::MAX - 10, 0, u32::MAX, 100), WIN.0, WIN.1);
        assert_eq!(got, None);
        let got = clamp_to_window(rect(i32::MIN, 0, u32::MAX, 100), WIN.0, WIN.1);
        assert_eq!(got, Some(rect(0, 0, WIN.0, 100)));
    }

    #[test]
    fn an_inlay_puts_the_surface_in_the_rectangle_the_page_asked_for() {
        let (p, seen) = pip();
        p.set_window(WIN.0, WIN.1);
        let view = p.set_inlay(Some(rect(1200, 40, 640, 360))).unwrap();
        assert_eq!(view.inlay, Some(rect(1200, 40, 640, 360)));
        assert_eq!(placed(&seen), Some(rect(1200, 40, 640, 360)));
    }

    #[test]
    fn clearing_an_inlay_gives_the_whole_window_back() {
        let (p, seen) = pip();
        p.set_window(WIN.0, WIN.1);
        p.set_inlay(Some(rect(1200, 40, 640, 360))).unwrap();
        let view = p.set_inlay(None).unwrap();
        assert_eq!(view.inlay, None);
        // Back to the whole client area, not left pinned to the box the page had.
        assert_eq!(placed(&seen), Some(rect(0, 0, WIN.0, WIN.1)));
    }

    /// The precedence `apply` states: a corner the viewer turned on beats a page's inlay.
    #[test]
    fn picture_in_picture_wins_over_a_pages_inlay() {
        let (p, seen) = pip();
        p.set_window(WIN.0, WIN.1);
        p.set_inlay(Some(rect(1200, 40, 640, 360))).unwrap();
        let view = p.set_enabled(true).unwrap();
        // The corner rect, not the inlay.
        assert_eq!(placed(&seen), Some(view.rect));
        assert_ne!(view.rect, rect(1200, 40, 640, 360));
        // And the inlay is remembered, so turning PiP off hands the box back.
        assert_eq!(view.inlay, Some(rect(1200, 40, 640, 360)));
        let off = p.set_enabled(false).unwrap();
        assert_eq!(placed(&seen), Some(rect(1200, 40, 640, 360)), "{off:?}");
    }

    /// A rectangle the page could not possibly have meant leaves the picture alone
    /// rather than putting the surface somewhere it cannot be seen.
    #[test]
    fn an_inlay_off_the_window_is_refused_and_falls_back_to_full() {
        let (p, seen) = pip();
        p.set_window(WIN.0, WIN.1);
        let view = p.set_inlay(Some(rect(5000, 5000, 640, 360))).unwrap();
        assert_eq!(view.inlay, None, "nothing of that rectangle is on screen");
        // So the picture fills the window, which is the safe reading of "nowhere I can
        // put this" — the Spy records a `resize` as the full client area.
        assert_eq!(placed(&seen), Some(rect(0, 0, WIN.0, WIN.1)));
    }

    #[test]
    fn it_starts_off_and_in_the_bottom_right() {
        let (p, _seen) = pip();
        let view = p.state();
        assert!(!view.enabled);
        assert_eq!(view.corner, Corner::BottomRight);
        // The rect is reported even while off, so the UI can animate into it.
        assert!(view.rect.width > 0);
    }

    #[test]
    fn toggling_puts_the_surface_in_the_corner_and_back() {
        let (p, seen) = pip();
        p.set_window(1920, 1080);

        let on = p.toggle().unwrap();
        assert!(on.enabled);
        let expected = Geometry::default().rect(1920, 1080);
        assert_eq!(on.rect, expected);
        assert_eq!(placed(&seen), Some(expected));

        let off = p.toggle().unwrap();
        assert!(!off.enabled);
        // Back to the whole window: `resize` is what fills it, and `NullBackend`
        // records that through `place` too.
        assert_eq!(
            placed(&seen),
            Some(Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080
            })
        );
    }

    #[test]
    fn the_corner_moves_clockwise_and_the_surface_follows() {
        let (p, seen) = pip();
        p.set_window(1600, 900);
        p.set_enabled(true).unwrap();

        let mut corners = vec![p.state().corner];
        for _ in 0..3 {
            let view = p.set_corner(None).unwrap();
            corners.push(view.corner);
            assert_eq!(
                placed(&seen),
                Some(view.rect),
                "the surface did not follow to {:?}",
                view.corner
            );
        }
        assert_eq!(
            corners,
            vec![
                Corner::BottomRight,
                Corner::BottomLeft,
                Corner::TopLeft,
                Corner::TopRight
            ]
        );
    }

    #[test]
    fn a_named_corner_is_honoured() {
        let (p, _seen) = pip();
        let view = p.set_corner(Some(Corner::TopLeft)).unwrap();
        assert_eq!(view.corner, Corner::TopLeft);
        assert_eq!(view.rect.x, 24);
        assert_eq!(view.rect.y, 24);
    }

    /// The reason this owns the resize: with PiP on, "fill the window" is the wrong
    /// answer, and that is what the resize handler used to send unconditionally.
    #[test]
    fn a_resize_keeps_the_picture_in_its_corner() {
        let (p, seen) = pip();
        p.set_window(1920, 1080);
        p.set_enabled(true).unwrap();

        let view = p.relayout(1280, 720).unwrap();
        assert!(view.enabled);
        assert_eq!(view.rect, Geometry::default().rect(1280, 720));
        assert_eq!(placed(&seen), Some(view.rect));
        // And it is still a corner rather than the whole window.
        assert!(view.rect.width < 1280);
    }

    #[test]
    fn a_resize_with_pip_off_fills_the_window() {
        let (p, seen) = pip();
        let view = p.relayout(1024, 768).unwrap();
        assert!(!view.enabled);
        assert_eq!(
            placed(&seen),
            Some(Rect {
                x: 0,
                y: 0,
                width: 1024,
                height: 768
            })
        );
    }

    /// A window with no room for a tile must not leave PiP on with a zero-sized
    /// picture and no way back — the state is still consistent, and the next resize
    /// recovers it.
    #[test]
    fn a_window_too_small_for_a_tile_is_still_consistent() {
        let (p, _seen) = pip();
        p.set_window(40, 40);
        let view = p.set_enabled(true).unwrap();
        assert!(view.enabled);
        assert_eq!(view.rect.width, 0);

        let back = p.relayout(1600, 900).unwrap();
        assert!(
            back.rect.width > 0,
            "a bigger window gives the picture back"
        );
    }

    /// A backend that refuses to move must not leave the flag saying it did.
    #[test]
    fn a_surface_that_will_not_move_leaves_the_state_alone() {
        struct Stuck;
        impl PlayerBackend for Stuck {
            fn load(
                &mut self,
                _: &str,
                _: &aurora_player::backend::LoadOptions,
            ) -> std::result::Result<(), aurora_player::PlayerError> {
                Ok(())
            }
            fn stop(&mut self) -> std::result::Result<(), aurora_player::PlayerError> {
                Ok(())
            }
            fn set_paused(
                &mut self,
                _: bool,
            ) -> std::result::Result<(), aurora_player::PlayerError> {
                Ok(())
            }
            fn seek(
                &mut self,
                _: f64,
                _: bool,
            ) -> std::result::Result<(), aurora_player::PlayerError> {
                Ok(())
            }
            fn set_volume(
                &mut self,
                _: u32,
            ) -> std::result::Result<(), aurora_player::PlayerError> {
                Ok(())
            }
            fn set_muted(
                &mut self,
                _: bool,
            ) -> std::result::Result<(), aurora_player::PlayerError> {
                Ok(())
            }
            fn set_speed(&mut self, _: f64) -> std::result::Result<(), aurora_player::PlayerError> {
                Ok(())
            }
            fn set_audio_track(
                &mut self,
                _: Option<i64>,
            ) -> std::result::Result<(), aurora_player::PlayerError> {
                Ok(())
            }
            fn set_subtitle_track(
                &mut self,
                _: Option<i64>,
            ) -> std::result::Result<(), aurora_player::PlayerError> {
                Ok(())
            }
            fn set_aspect(
                &mut self,
                _: aurora_player::Aspect,
            ) -> std::result::Result<(), aurora_player::PlayerError> {
                Ok(())
            }
            fn state(&self) -> PlayerState {
                PlayerState::default()
            }
            fn chapters(&self) -> Vec<aurora_core::markers::Chapter> {
                Vec::new()
            }
            fn resize(
                &mut self,
                _: u32,
                _: u32,
            ) -> std::result::Result<(), aurora_player::PlayerError> {
                Err(aurora_player::PlayerError::Command("stuck".into()))
            }
            fn place(&mut self, _: Rect) -> std::result::Result<(), aurora_player::PlayerError> {
                Err(aurora_player::PlayerError::Command("stuck".into()))
            }
        }

        let player: Arc<Mutex<Box<dyn PlayerBackend>>> = Arc::new(Mutex::new(Box::new(Stuck)));
        let p = Pip::new(player);

        assert!(p.toggle().is_err());
        assert!(
            !p.is_enabled(),
            "the flag said PiP was on while the picture had not moved"
        );
    }
}
