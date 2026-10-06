//! Picture-in-picture: where the small picture goes (README §6.2, §7.2).
//!
//! The geometry only, so it can be tested without a window. Everything here is the
//! same shape as `mosaic`: a rectangle in physical pixels of the client area, which is
//! what positions the real video surface and therefore also what the UI has to draw its
//! frame around.
//!
//! **It is the same stream, moved.** Unlike a mosaic tile, PiP opens nothing: the main
//! player's existing surface is placed into a corner instead of filling the window, so
//! it costs no extra provider connection and cannot be refused. That is the whole
//! reason this module is arithmetic and not a service.
//!
//! The one rule worth stating: a corner tile must stay **inside** the window and keep
//! its aspect ratio, and those two can disagree — a window shorter than the tile's
//! height has to win, because a picture hanging off the bottom edge is worse than a
//! smaller one.

use serde::{Deserialize, Serialize};

use crate::mosaic::Rect;

/// Which corner the small picture sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    /// Clockwise from the top left, which is what a "next corner" button walks.
    pub const ALL: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomRight,
        Corner::BottomLeft,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Corner::TopLeft => "topLeft",
            Corner::TopRight => "topRight",
            Corner::BottomLeft => "bottomLeft",
            Corner::BottomRight => "bottomRight",
        }
    }

    pub fn parse(s: &str) -> Option<Corner> {
        Corner::ALL.into_iter().find(|c| c.as_str() == s)
    }

    /// The next corner clockwise.
    pub fn next(self) -> Corner {
        let at = Corner::ALL.iter().position(|c| *c == self).unwrap_or(0);
        Corner::ALL[(at + 1) % Corner::ALL.len()]
    }
}

/// How big the small picture is, and how far from the edges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    pub corner: Corner,
    /// Fraction of the window's width the tile takes.
    ///
    /// A fraction rather than a pixel size, so it is the same *relative* size on a
    /// 1366×768 laptop and a 4K television — a 480-pixel tile is a third of one screen
    /// and an eighth of the other.
    pub width_fraction: f32,
    /// Gap between the tile and the window edges, in physical pixels.
    pub margin: u32,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            // Bottom right: the corner least likely to cover a heading, and where every
            // other application puts this.
            corner: Corner::BottomRight,
            width_fraction: 0.28,
            margin: 24,
        }
    }
}

/// Narrower than this and there is nothing to see, so the fraction stops applying.
pub const MIN_WIDTH: u32 = 160;

/// 16:9, which is what practically every channel is. A tile that matched the stream's
/// own aspect would change size when the viewer zapped, which is worse than a letterbox.
const ASPECT_W: u32 = 16;
const ASPECT_H: u32 = 9;

impl Geometry {
    /// Where the tile goes in a client area of `width` × `height`.
    ///
    /// Returns a zero-sized rect when the window is too small to hold a tile and its
    /// margins at all. A zero-sized surface draws nothing, which is the right answer for
    /// a window with no room in it — and the caller can tell, which is why it is not
    /// clamped up to `MIN_WIDTH` regardless.
    pub fn rect(&self, width: u32, height: u32) -> Rect {
        let margin = self.margin;
        // Two margins on each axis: one against the window edge, one against the
        // content the tile is floating over.
        let Some(room_w) = width.checked_sub(margin * 2) else {
            return ZERO;
        };
        let Some(room_h) = height.checked_sub(margin * 2) else {
            return ZERO;
        };
        if room_w == 0 || room_h == 0 {
            return ZERO;
        }

        let fraction = self.width_fraction.clamp(0.1, 0.9);
        let wanted = (f64::from(width) * f64::from(fraction)).round() as u64;
        // `max` then `min`, not `clamp`: in a window narrower than `MIN_WIDTH` plus its
        // margins the minimum is larger than the maximum, and `clamp` panics on that —
        // which in a release build is an abort, so dragging the window thin would close
        // the app. The window always wins.
        let mut tile_w = wanted
            .max(u64::from(MIN_WIDTH))
            .min(u64::from(room_w)) as u32;
        let mut tile_h = (u64::from(tile_w) * u64::from(ASPECT_H) / u64::from(ASPECT_W)) as u32;

        // The window can be shorter than the tile is tall — a wide, squat window, or one
        // dragged down to a sliver. Height wins, because a picture hanging off the edge
        // is worse than a smaller one, and the width follows so the aspect holds.
        if tile_h > room_h {
            tile_h = room_h;
            tile_w = (u64::from(tile_h) * u64::from(ASPECT_W) / u64::from(ASPECT_H)) as u32;
            tile_w = tile_w.min(room_w);
        }

        if tile_w == 0 || tile_h == 0 {
            return ZERO;
        }

        let (x, y) = match self.corner {
            Corner::TopLeft => (margin, margin),
            Corner::TopRight => (width - margin - tile_w, margin),
            Corner::BottomLeft => (margin, height - margin - tile_h),
            Corner::BottomRight => (width - margin - tile_w, height - margin - tile_h),
        };

        Rect {
            x: x as i32,
            y: y as i32,
            width: tile_w,
            height: tile_h,
        }
    }
}

const ZERO: Rect = Rect {
    x: 0,
    y: 0,
    width: 0,
    height: 0,
};

#[cfg(test)]
mod tests {
    use super::*;

    const SIZES: [(u32, u32); 5] = [
        (1920, 1080),
        (1366, 768),
        (3840, 2160),
        (1001, 997),
        (640, 480),
    ];

    /// The property that matters: the tile is inside the window, on every corner, at
    /// every size. A picture hanging off the edge is clipped by the window manager and
    /// looks like a bug in the player.
    #[test]
    fn the_tile_stays_inside_the_window_in_every_corner() {
        for (w, h) in SIZES {
            for corner in Corner::ALL {
                let g = Geometry {
                    corner,
                    ..Default::default()
                };
                let r = g.rect(w, h);
                assert!(r.width > 0 && r.height > 0, "{corner:?} at {w}x{h}: {r:?}");
                assert!(r.x >= 0 && r.y >= 0, "{corner:?} at {w}x{h}: {r:?}");
                assert!(
                    r.x + r.width as i32 <= w as i32,
                    "{corner:?} at {w}x{h} runs off the right: {r:?}"
                );
                assert!(
                    r.y + r.height as i32 <= h as i32,
                    "{corner:?} at {w}x{h} runs off the bottom: {r:?}"
                );
            }
        }
    }

    #[test]
    fn the_margin_is_honoured_on_the_edges_the_corner_touches() {
        let w = 1920;
        let h = 1080;
        let margin = 24;
        for corner in Corner::ALL {
            let r = Geometry {
                corner,
                margin,
                ..Default::default()
            }
            .rect(w, h);
            let right = w as i32 - (r.x + r.width as i32);
            let bottom = h as i32 - (r.y + r.height as i32);
            match corner {
                Corner::TopLeft => assert_eq!((r.x, r.y), (margin as i32, margin as i32)),
                Corner::TopRight => assert_eq!((right, r.y), (margin as i32, margin as i32)),
                Corner::BottomLeft => assert_eq!((r.x, bottom), (margin as i32, margin as i32)),
                Corner::BottomRight => assert_eq!((right, bottom), (margin as i32, margin as i32)),
            }
        }
    }

    /// 16:9 whatever the window is, because a tile that matched the stream would resize
    /// itself every time the viewer zapped.
    #[test]
    fn the_tile_is_sixteen_by_nine_within_rounding() {
        for (w, h) in SIZES {
            let r = Geometry::default().rect(w, h);
            let ratio = f64::from(r.width) / f64::from(r.height);
            let expected = 16.0 / 9.0;
            assert!(
                (ratio - expected).abs() < 0.02,
                "{w}x{h} gave {}x{} = {ratio:.3}",
                r.width,
                r.height
            );
        }
    }

    #[test]
    fn the_fraction_sets_the_size_relative_to_the_window() {
        let quarter = Geometry {
            width_fraction: 0.25,
            ..Default::default()
        };
        assert_eq!(quarter.rect(1920, 1080).width, 480);
        assert_eq!(quarter.rect(800, 600).width, 200);

        // And it is bounded, so a stored setting of 5.0 cannot fill the window.
        let silly = Geometry {
            width_fraction: 5.0,
            ..Default::default()
        };
        let r = silly.rect(1920, 1080);
        assert!(r.x + r.width as i32 <= 1920, "{r:?}");
    }

    /// A window shorter than the tile. Height has to win or the picture hangs off.
    #[test]
    fn a_squat_window_shrinks_the_tile_rather_than_overflowing_it() {
        let r = Geometry::default().rect(1920, 200);
        assert!(r.height as u32 + 48 <= 200, "{r:?}");
        assert!(r.y >= 0 && r.y + r.height as i32 <= 200, "{r:?}");
        // Aspect still holds, so it is a small picture rather than a squashed one.
        let ratio = f64::from(r.width) / f64::from(r.height);
        assert!((ratio - 16.0 / 9.0).abs() < 0.05, "{r:?}");
    }

    /// Dragged down to nothing. Not a panic and not a negative rectangle.
    #[test]
    fn a_window_with_no_room_gives_nothing_rather_than_panicking() {
        for (w, h) in [(0, 0), (10, 10), (48, 48), (1, 1000)] {
            let r = Geometry::default().rect(w, h);
            assert!(r.x >= 0 && r.y >= 0, "{w}x{h}: {r:?}");
            assert!(
                r.x + r.width as i32 <= w.max(0) as i32 || r.width == 0,
                "{w}x{h}: {r:?}"
            );
        }
    }

    /// Below this there is nothing to see, so the fraction stops and the minimum takes
    /// over — but only while the window has room for it.
    #[test]
    fn a_narrow_window_gets_the_minimum_until_there_is_no_room() {
        // 28% of 400 is 112, under the 160 minimum, and 400 has room for 160.
        let r = Geometry::default().rect(400, 400);
        assert_eq!(r.width, MIN_WIDTH);

        // 180 wide has room for 180 - 48 = 132, which is less than the minimum. The
        // window wins.
        let tight = Geometry::default().rect(180, 400);
        assert!(tight.width <= 132, "{tight:?}");
        assert!(tight.x + tight.width as i32 <= 180, "{tight:?}");
    }

    #[test]
    fn corners_round_trip_and_cycle_clockwise() {
        for corner in Corner::ALL {
            assert_eq!(Corner::parse(corner.as_str()), Some(corner));
        }
        assert_eq!(Corner::parse("middle"), None);

        assert_eq!(Corner::TopLeft.next(), Corner::TopRight);
        assert_eq!(Corner::TopRight.next(), Corner::BottomRight);
        assert_eq!(Corner::BottomRight.next(), Corner::BottomLeft);
        assert_eq!(Corner::BottomLeft.next(), Corner::TopLeft);

        // Four presses come back to where it started.
        let mut c = Corner::BottomRight;
        for _ in 0..4 {
            c = c.next();
        }
        assert_eq!(c, Corner::BottomRight);
    }
}
