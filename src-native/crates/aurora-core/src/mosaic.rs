//! Multi-view: several channels at once, and whether the line can carry them
//! (README §7.4).
//!
//! Two things live here, both of them arithmetic, so both can be tested without a
//! display or a subscription.
//!
//! The first is where the tiles go. Each tile is its own video surface positioned
//! inside the window, so the layout has to tile the area *exactly* — a rounding error
//! is not a rounding error on screen, it is a one-pixel stripe of whatever is behind
//! the player showing through between two tiles. So edges are computed proportionally
//! and shared between neighbours, rather than each tile being given a width and the
//! last one taking whatever is left.
//!
//! The second is the warning the feature is mostly about. A 3×3 mosaic opens nine
//! streams, and the subscription this project was measured against allows **one**:
//! `max_connections = 1`, which makes every layout here impossible on it. A provider
//! does not queue the tenth stream, it refuses it — or cuts one already running — so
//! the only kind thing is to say so before opening rather than draw eight error tiles.
//! Recordings in flight are counted too, because the DVR competes for the same line.

use serde::{Deserialize, Serialize};

/// Where one tile goes, in physical pixels relative to the window's client area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// The layouts README §7.4 asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Layout {
    /// Four equal tiles.
    Grid2x2,
    /// Nine equal tiles.
    Grid3x3,
    /// One large tile over three quarters of the width, three stacked beside it.
    OnePlusThree,
    /// One large tile over the top-left, five around it in the remaining cells of a
    /// three-by-three grid.
    OnePlusFive,
}

impl Layout {
    /// Every layout, in the order a picker should offer them: cheapest line first.
    ///
    /// Ordered by how many connections they need rather than by how they look, because
    /// that is the axis a viewer is actually choosing on once they have hit the limit
    /// once.
    pub const ALL: [Layout; 4] = [
        Layout::Grid2x2,
        Layout::OnePlusThree,
        Layout::OnePlusFive,
        Layout::Grid3x3,
    ];

    /// The stable name used in the IPC payload and in a saved layout's row.
    pub fn as_str(self) -> &'static str {
        match self {
            Layout::Grid2x2 => "grid2x2",
            Layout::Grid3x3 => "grid3x3",
            Layout::OnePlusThree => "onePlusThree",
            Layout::OnePlusFive => "onePlusFive",
        }
    }

    pub fn parse(s: &str) -> Option<Layout> {
        Layout::ALL.into_iter().find(|l| l.as_str() == s)
    }

    /// How many tiles — and therefore how many streams, and how many connections.
    pub fn tiles(self) -> usize {
        match self {
            Layout::Grid2x2 | Layout::OnePlusThree => 4,
            Layout::OnePlusFive => 6,
            Layout::Grid3x3 => 9,
        }
    }

    /// Tile geometry for a client area, tile 0 first.
    ///
    /// Tile 0 is the large one in the `1+n` layouts and the top-left one in the grids,
    /// which is why it is also the tile audio starts on: in every layout it is the one
    /// the eye is already on.
    ///
    /// A client area too small to divide is not an error — a window can be dragged to
    /// any size, including one narrower than the layout has columns. Tiles collapse to
    /// zero width rather than overlapping, and a zero-sized surface draws nothing,
    /// which is the right answer for a window nobody can see anything in anyway.
    pub fn rects(self, width: u32, height: u32) -> Vec<Rect> {
        // Proportional edges, shared between neighbours: `edge(w, 1, 3)` and
        // `edge(w, 2, 3)` are the two interior boundaries of a three-column split, so
        // neighbouring tiles meet exactly on them.
        let col = |i: u32, of: u32| edge(width, i, of);
        let row = |i: u32, of: u32| edge(height, i, of);

        match self {
            Layout::Grid2x2 => grid(2, 2, &col, &row),
            Layout::Grid3x3 => grid(3, 3, &col, &row),
            Layout::OnePlusThree => {
                // Four columns wide so the large tile takes three of them, and three
                // rows so the small ones stack evenly beside it.
                let mut out = vec![cell(0, 0, col(3, 4), row(3, 3))];
                for r in 0..3 {
                    out.push(cell(col(3, 4), row(r, 3), col(4, 4), row(r + 1, 3)));
                }
                out
            }
            Layout::OnePlusFive => {
                // The large tile is the top-left two-by-two of a three-by-three grid;
                // the five small ones are the right column top to bottom, then the two
                // remaining cells of the bottom row left to right.
                let mut out = vec![cell(0, 0, col(2, 3), row(2, 3))];
                for r in 0..3 {
                    out.push(cell(col(2, 3), row(r, 3), col(3, 3), row(r + 1, 3)));
                }
                for c in 0..2 {
                    out.push(cell(col(c, 3), row(2, 3), col(c + 1, 3), row(3, 3)));
                }
                out
            }
        }
    }
}

/// A rectangle from two corners, clamped so a reversed pair cannot wrap a `u32`.
fn cell(x0: i32, y0: i32, x1: i32, y1: i32) -> Rect {
    Rect {
        x: x0,
        y: y0,
        width: (x1 - x0).max(0) as u32,
        height: (y1 - y0).max(0) as u32,
    }
}

/// The `i`th of `of` proportional boundaries across `total`.
///
/// `total * i / of` rather than `i * (total / of)`: the second form loses the remainder
/// at every boundary, so a 1000-pixel window split three ways would give three
/// 333-pixel tiles and leave a 1-pixel seam down the edge.
fn edge(total: u32, i: u32, of: u32) -> i32 {
    ((u64::from(total) * u64::from(i)) / u64::from(of)) as i32
}

fn grid(
    cols: u32,
    rows: u32,
    col: &dyn Fn(u32, u32) -> i32,
    row: &dyn Fn(u32, u32) -> i32,
) -> Vec<Rect> {
    let mut out = Vec::with_capacity((cols * rows) as usize);
    for r in 0..rows {
        for c in 0..cols {
            out.push(cell(
                col(c, cols),
                row(r, rows),
                col(c + 1, cols),
                row(r + 1, rows),
            ));
        }
    }
    out
}

/// What is already using the line, and what is about to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Demand {
    /// Tiles the layout wants to open.
    pub tiles: usize,
    /// Recordings in flight. The DVR holds a connection for the whole of one, and a
    /// mosaic that cuts somebody's recording to draw a ninth tile has made the wrong
    /// trade on their behalf.
    pub recordings: usize,
}

/// Whether the line can carry a layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "verdict", rename_all = "camelCase")]
pub enum Budget {
    /// It fits, with the numbers so the UI can show them without recomputing.
    Fits { needed: usize, limit: usize },
    /// It does not. `over` is how many tiles would have to go.
    Exceeds {
        needed: usize,
        limit: usize,
        over: usize,
        recordings: usize,
    },
    /// No enabled provider has said what its limit is.
    ///
    /// Not treated as "fine": an M3U playlist carries no `max_connections` at all, so
    /// this is the ordinary case rather than the exotic one, and the only honest answer
    /// is that opening six streams may work and may get the line cut. The UI says that
    /// in words and lets the viewer try.
    Unknown { needed: usize },
}

impl Budget {
    /// `limit` is the tightest `max_connections` across enabled providers, or `None`
    /// where none of them declares one.
    pub fn check(demand: Demand, limit: Option<usize>) -> Budget {
        let needed = demand.tiles + demand.recordings;
        match limit {
            None => Budget::Unknown { needed },
            Some(limit) if needed > limit => Budget::Exceeds {
                needed,
                limit,
                over: needed - limit,
                recordings: demand.recordings,
            },
            Some(limit) => Budget::Fits { needed, limit },
        }
    }

    /// Whether opening would exceed a limit the provider actually declared.
    pub fn is_refused(&self) -> bool {
        matches!(self, Budget::Exceeds { .. })
    }

    /// The largest layout that fits, for the "what *can* I open" half of the warning.
    ///
    /// `None` when even the smallest does not — which is the single-connection case,
    /// and saying "none of them" is more use than offering a 2×2 that will also fail.
    pub fn largest_fitting(recordings: usize, limit: Option<usize>) -> Option<Layout> {
        let mut best: Option<Layout> = None;
        for layout in Layout::ALL {
            let demand = Demand {
                tiles: layout.tiles(),
                recordings,
            };
            if Budget::check(demand, limit).is_refused() {
                continue;
            }
            if best.is_none_or(|b| layout.tiles() > b.tiles()) {
                best = Some(layout);
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property that matters on screen: tiles cover the client area with no gap and
    /// no overlap. A seam is a stripe of the desktop showing between two channels.
    #[test]
    fn every_layout_tiles_the_area_exactly() {
        // Sizes chosen to be awkward: primes, and numbers divisible by neither 2 nor 3.
        for (w, h) in [(1920, 1080), (1000, 1000), (1001, 997), (1366, 768), (7, 5)] {
            for layout in Layout::ALL {
                let rects = layout.rects(w, h);
                assert_eq!(rects.len(), layout.tiles(), "{layout:?} at {w}x{h}");

                let area: u64 = rects
                    .iter()
                    .map(|r| u64::from(r.width) * u64::from(r.height))
                    .sum();
                assert_eq!(
                    area,
                    u64::from(w) * u64::from(h),
                    "{layout:?} at {w}x{h} does not cover the window: {rects:?}"
                );

                for (i, a) in rects.iter().enumerate() {
                    for b in &rects[i + 1..] {
                        assert!(!overlaps(a, b), "{layout:?}: {a:?} overlaps {b:?}");
                    }
                    assert!(a.x >= 0 && a.y >= 0, "{layout:?}: {a:?} is off-window");
                    assert!(
                        a.x + a.width as i32 <= w as i32 && a.y + a.height as i32 <= h as i32,
                        "{layout:?}: {a:?} leaves a {w}x{h} window"
                    );
                }
            }
        }
    }

    fn overlaps(a: &Rect, b: &Rect) -> bool {
        let ax2 = a.x + a.width as i32;
        let ay2 = a.y + a.height as i32;
        let bx2 = b.x + b.width as i32;
        let by2 = b.y + b.height as i32;
        a.x < bx2 && b.x < ax2 && a.y < by2 && b.y < ay2
    }

    /// Tile 0 is the big one, which is also the tile audio starts on.
    #[test]
    fn tile_zero_is_the_largest_in_the_one_plus_layouts() {
        for layout in [Layout::OnePlusThree, Layout::OnePlusFive] {
            let rects = layout.rects(1200, 900);
            let first = u64::from(rects[0].width) * u64::from(rects[0].height);
            for other in &rects[1..] {
                let area = u64::from(other.width) * u64::from(other.height);
                assert!(first > area, "{layout:?}: tile 0 is not the large one");
            }
        }
    }

    #[test]
    fn a_window_too_small_to_divide_collapses_rather_than_overlapping() {
        // Narrower than it has columns. Nothing can be seen either way; what must not
        // happen is two surfaces claiming the same pixels.
        let rects = Layout::Grid3x3.rects(2, 2);
        assert_eq!(rects.len(), 9);
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(!overlaps(a, b), "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn a_zero_sized_window_is_not_a_panic() {
        for layout in Layout::ALL {
            let rects = layout.rects(0, 0);
            assert_eq!(rects.len(), layout.tiles());
            assert!(rects.iter().all(|r| r.width == 0 && r.height == 0));
        }
    }

    #[test]
    fn layout_names_round_trip() {
        for layout in Layout::ALL {
            assert_eq!(Layout::parse(layout.as_str()), Some(layout));
        }
        assert_eq!(Layout::parse("grid4x4"), None);
    }

    /// The case this module mostly exists for: the subscription measured in
    /// `docs/ROADMAP.md` allows one connection, so no layout can be opened on it.
    #[test]
    fn a_single_connection_line_refuses_every_layout() {
        for layout in Layout::ALL {
            let budget = Budget::check(
                Demand {
                    tiles: layout.tiles(),
                    recordings: 0,
                },
                Some(1),
            );
            assert!(
                budget.is_refused(),
                "{layout:?} was allowed on one connection"
            );
        }
        assert_eq!(Budget::largest_fitting(0, Some(1)), None);
    }

    #[test]
    fn a_recording_in_flight_is_counted_against_the_layout() {
        // Four connections and a 2x2 fits exactly — until the DVR is using one.
        let free = Budget::check(
            Demand {
                tiles: 4,
                recordings: 0,
            },
            Some(4),
        );
        assert_eq!(
            free,
            Budget::Fits {
                needed: 4,
                limit: 4
            }
        );

        let busy = Budget::check(
            Demand {
                tiles: 4,
                recordings: 1,
            },
            Some(4),
        );
        assert_eq!(
            busy,
            Budget::Exceeds {
                needed: 5,
                limit: 4,
                over: 1,
                recordings: 1,
            }
        );
    }

    /// An M3U playlist declares no limit, and that is the common case rather than the
    /// odd one. It must not be reported as "fits".
    #[test]
    fn an_undeclared_limit_is_unknown_rather_than_fine() {
        let budget = Budget::check(
            Demand {
                tiles: 9,
                recordings: 0,
            },
            None,
        );
        assert_eq!(budget, Budget::Unknown { needed: 9 });
        assert!(!budget.is_refused(), "unknown must not block the attempt");
    }

    #[test]
    fn the_largest_fitting_layout_is_the_one_with_the_most_tiles() {
        assert_eq!(Budget::largest_fitting(0, Some(9)), Some(Layout::Grid3x3));
        assert_eq!(
            Budget::largest_fitting(0, Some(6)),
            Some(Layout::OnePlusFive)
        );
        // Five connections: 1+5 needs six, so the best is a four-tile layout.
        assert_eq!(
            Budget::largest_fitting(0, Some(5)).map(Layout::tiles),
            Some(4)
        );
        assert_eq!(Budget::largest_fitting(0, Some(3)), None);
        // A recording takes one of the nine.
        assert_eq!(
            Budget::largest_fitting(1, Some(9)),
            Some(Layout::OnePlusFive)
        );
        // Nothing declared: every layout is worth offering.
        assert_eq!(Budget::largest_fitting(0, None), Some(Layout::Grid3x3));
    }
}
