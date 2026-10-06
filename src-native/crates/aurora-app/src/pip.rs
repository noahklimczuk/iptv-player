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
}

pub struct Pip {
    player: Arc<Mutex<Box<dyn PlayerBackend>>>,
    enabled: AtomicBool,
    geometry: Mutex<Geometry>,
    size: Mutex<(u32, u32)>,
}

impl Pip {
    pub fn new(player: Arc<Mutex<Box<dyn PlayerBackend>>>) -> Self {
        Self {
            player,
            enabled: AtomicBool::new(false),
            geometry: Mutex::new(Geometry::default()),
            size: Mutex::new((1280, 720)),
        }
    }

    pub fn set_window(&self, width: u32, height: u32) {
        *self.size.lock() = (width.max(1), height.max(1));
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    fn rect(&self) -> Rect {
        let (w, h) = *self.size.lock();
        self.geometry.lock().rect(w, h)
    }

    pub fn state(&self) -> PipView {
        PipView {
            enabled: self.is_enabled(),
            corner: self.geometry.lock().corner,
            rect: self.rect(),
        }
    }

    /// Put the surface where the current mode says it belongs.
    ///
    /// The one function that touches the backend's geometry, so "full window" and "in a
    /// corner" cannot drift apart.
    fn apply(&self) -> Result<PipView> {
        let (w, h) = *self.size.lock();
        let view = self.state();
        let mut player = self.player.lock();
        if view.enabled {
            player.place(view.rect)?;
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

    /// The window changed size. Called for every resize, PiP on or off.
    pub fn relayout(&self, width: u32, height: u32) -> Result<PipView> {
        self.set_window(width, height);
        self.apply()
    }
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

#[tauri::command(async)]
pub fn pip_set_corner(services: State<'_, Services>, args: CornerArgs) -> Result<PipView> {
    let corner = match args.corner.as_deref() {
        None => None,
        Some(name) => Some(Corner::parse(name).ok_or_else(|| {
            crate::AppError::Other(format!("unknown corner {name:?}"))
        })?),
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
        assert!(back.rect.width > 0, "a bigger window gives the picture back");
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
            fn resize(&mut self, _: u32, _: u32) -> std::result::Result<(), aurora_player::PlayerError> {
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
