//! Where the picture goes in the window: all of it, a PiP corner, or a rectangle the
//! page measured (docs/PLAYER.md H21–H23).
//!
//! Pure geometry in physical pixels. The app moves the video surface to
//! [`Placement::surface`] after every change.
//!
//! PiP is the same player, moved into a corner. Not a second stream: a provider line
//! commonly allows one connection, and a second would knock the first off.

use serde::{Deserialize, Serialize};

/// `MosaicRect` in `shared/ipc.ts`: physical pixels of the client area.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    fn right(&self) -> i64 {
        i64::from(self.x) + i64::from(self.width)
    }
    fn bottom(&self) -> i64 {
        i64::from(self.y) + i64::from(self.height)
    }
}

/// `PipCorner` in `shared/ipc.ts`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Corner {
    TopLeft,
    TopRight,
    #[default]
    BottomRight,
    BottomLeft,
}

impl Corner {
    /// The next corner clockwise (H21).
    pub fn clockwise(self) -> Self {
        match self {
            Self::TopLeft => Self::TopRight,
            Self::TopRight => Self::BottomRight,
            Self::BottomRight => Self::BottomLeft,
            Self::BottomLeft => Self::TopLeft,
        }
    }
}

/// `PipView` in `shared/ipc.ts`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipView {
    pub enabled: bool,
    pub corner: Corner,
    /// Where the PiP tile is, or would be.
    pub rect: Rect,
    /// Where the page's inlay actually went, after trimming to the window.
    pub inlay: Option<Rect>,
}

/// The PiP tile's width as a share of the window's.
pub const PIP_SHARE: f64 = 0.28;
/// Never smaller than this, so a small window still gets a watchable tile.
pub const PIP_MIN_WIDTH: u32 = 320;
/// An inlay smaller than this either way is not worth a picture.
pub const MIN_INLAY: u32 = 16;

#[derive(Debug, Clone, Default)]
pub struct Placement {
    window: (u32, u32),
    pip: bool,
    corner: Corner,
    inlay: Option<Rect>,
}

impl Placement {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            window: (width, height),
            ..Self::default()
        }
    }

    pub fn view(&self) -> PipView {
        PipView {
            enabled: self.pip,
            corner: self.corner,
            rect: self.pip_rect(),
            inlay: self.inlay,
        }
    }

    /// Where the surface goes now. PiP wins over an inlay (H22).
    pub fn surface(&self) -> Rect {
        if self.pip {
            self.pip_rect()
        } else if let Some(r) = self.inlay {
            r
        } else {
            self.full()
        }
    }

    fn full(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: self.window.0,
            height: self.window.1,
        }
    }

    fn pip_rect(&self) -> Rect {
        let (w, h) = self.window;
        let margin = (w / 60).max(16);
        let room = w.saturating_sub(2 * margin).max(1);
        let width = ((f64::from(w) * PIP_SHARE).round() as u32)
            .max(PIP_MIN_WIDTH)
            .min(room);
        let height = (width * 9 / 16).min(h.saturating_sub(2 * margin).max(1));
        let left = margin as i32;
        let top = margin as i32;
        let right = w.saturating_sub(margin + width) as i32;
        let bottom = h.saturating_sub(margin + height) as i32;
        let (x, y) = match self.corner {
            Corner::TopLeft => (left, top),
            Corner::TopRight => (right, top),
            Corner::BottomRight => (right, bottom),
            Corner::BottomLeft => (left, bottom),
        };
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// The window was resized: PiP stays in its corner, an inlay is trimmed again.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.window = (width, height);
        self.inlay = self.inlay.and_then(|r| self.trim(r));
    }

    /// `pip.toggle`. `mosaic_open` refuses turning it on (H23).
    pub fn toggle(&mut self, mosaic_open: bool) -> Result<PipView, String> {
        self.set_enabled(!self.pip, mosaic_open)
    }

    pub fn set_enabled(&mut self, on: bool, mosaic_open: bool) -> Result<PipView, String> {
        if on && mosaic_open {
            return Err(
                "Picture-in-picture is not available while multi-view is open.".into(),
            );
        }
        self.pip = on;
        Ok(self.view())
    }

    /// A named corner, or `None` for the next one clockwise (H21).
    pub fn set_corner(&mut self, corner: Option<Corner>) -> PipView {
        self.corner = corner.unwrap_or_else(|| self.corner.clockwise());
        self.view()
    }

    /// `preview.place` (H22): trimmed to the window; one wholly outside it is dropped
    /// and the picture goes back to the whole window.
    pub fn place(&mut self, wanted: Rect, mosaic_open: bool) -> Result<PipView, String> {
        if mosaic_open {
            return Err("The preview is not available while multi-view is open.".into());
        }
        self.inlay = self.trim(wanted);
        Ok(self.view())
    }

    /// `preview.clear`: never refused.
    pub fn clear(&mut self) -> PipView {
        self.inlay = None;
        self.view()
    }

    /// Leaving the player: PiP and any inlay end, and the picture has the window.
    pub fn reset(&mut self) {
        self.pip = false;
        self.inlay = None;
    }

    fn trim(&self, r: Rect) -> Option<Rect> {
        let (w, h) = (i64::from(self.window.0), i64::from(self.window.1));
        let x0 = i64::from(r.x).clamp(0, w);
        let y0 = i64::from(r.y).clamp(0, h);
        let x1 = r.right().clamp(0, w);
        let y1 = r.bottom().clamp(0, h);
        let (width, height) = ((x1 - x0) as u32, (y1 - y0) as u32);
        (width >= MIN_INLAY && height >= MIN_INLAY).then_some(Rect {
            x: x0 as i32,
            y: y0 as i32,
            width,
            height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect { x, y, width, height }
    }

    #[test]
    fn h21_pip_starts_off_bottom_right_and_toggles() {
        let mut p = Placement::new(1920, 1080);
        let v = p.view();
        assert!(!v.enabled);
        assert_eq!(v.corner, Corner::BottomRight);
        assert_eq!(p.surface(), rect(0, 0, 1920, 1080));
        let v = p.toggle(false).unwrap();
        assert!(v.enabled);
        assert_eq!(p.surface(), v.rect);
        assert!(v.rect.right() <= 1920 && v.rect.bottom() <= 1080);
        assert!(v.rect.x > 960 && v.rect.y > 540, "{v:?}");
        assert!(!p.toggle(false).unwrap().enabled);
    }

    #[test]
    fn h21_corners_go_clockwise_or_by_name() {
        let mut p = Placement::new(1920, 1080);
        let order: Vec<Corner> = (0..4).map(|_| p.set_corner(None).corner).collect();
        assert_eq!(
            order,
            [Corner::BottomLeft, Corner::TopLeft, Corner::TopRight, Corner::BottomRight]
        );
        assert_eq!(p.set_corner(Some(Corner::TopLeft)).corner, Corner::TopLeft);
    }

    #[test]
    fn h21_a_resize_keeps_pip_in_its_corner() {
        let mut p = Placement::new(1920, 1080);
        p.toggle(false).unwrap();
        let before = p.view().rect;
        p.resize(1280, 720);
        let after = p.view().rect;
        let gap = |r: Rect, w: i64, h: i64| (w - r.right(), h - r.bottom());
        assert_eq!(gap(before, 1920, 1080).0 > 0, gap(after, 1280, 720).0 > 0);
        assert!(after.right() <= 1280 && after.bottom() <= 720);
        assert!(after.x > 640 && after.y > 360);
    }

    #[test]
    fn h22_an_inlay_is_trimmed_to_the_window() {
        let mut p = Placement::new(1920, 1080);
        let v = p.place(rect(1500, -100, 600, 400), false).unwrap();
        assert_eq!(v.inlay, Some(rect(1500, 0, 420, 300)));
        assert_eq!(p.surface(), rect(1500, 0, 420, 300));
    }

    #[test]
    fn h22_an_inlay_wholly_off_the_window_falls_back_to_full() {
        let mut p = Placement::new(1920, 1080);
        let v = p.place(rect(3000, 0, 600, 400), false).unwrap();
        assert_eq!(v.inlay, None);
        assert_eq!(p.surface(), rect(0, 0, 1920, 1080));
    }

    #[test]
    fn h22_pip_wins_over_an_inlay_and_clearing_gives_the_window_back() {
        let mut p = Placement::new(1920, 1080);
        p.place(rect(100, 100, 640, 360), false).unwrap();
        p.toggle(false).unwrap();
        assert_eq!(p.surface(), p.view().rect);
        p.toggle(false).unwrap();
        assert_eq!(p.surface(), rect(100, 100, 640, 360));
        p.clear();
        assert_eq!(p.surface(), rect(0, 0, 1920, 1080));
    }

    #[test]
    fn h23_pip_is_refused_while_multi_view_is_open() {
        let mut p = Placement::new(1920, 1080);
        assert!(p.toggle(true).is_err());
        assert!(!p.view().enabled);
        // Turning it off is always allowed.
        p.toggle(false).unwrap();
        assert!(!p.set_enabled(false, true).unwrap().enabled);
    }

    #[test]
    fn a_tiny_window_still_gets_a_tile_inside_it() {
        let p = {
            let mut p = Placement::new(300, 200);
            p.toggle(false).unwrap();
            p
        };
        let r = p.view().rect;
        assert!(r.x >= 0 && r.y >= 0 && r.right() <= 300 && r.bottom() <= 200, "{r:?}");
    }

    #[test]
    fn the_wire_shape_matches_the_contract() {
        let v = serde_json::to_value(Placement::new(1920, 1080).view()).unwrap();
        assert_eq!(v["corner"], "bottomRight");
        assert!(v["inlay"].is_null());
        assert!(v["rect"]["width"].is_number());
    }
}
