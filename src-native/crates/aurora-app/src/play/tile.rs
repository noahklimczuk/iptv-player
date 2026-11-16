//! One multi-view tile's video: its own engine, drawing into its own window
//! (docs/PLAYER.md E32).
//!
//! A tile is opened when it gets a stream and dropped when it loses one. Dropping it
//! shuts its mpv down, which closes the provider connection — the one thing a mosaic on
//! a line with a connection limit cannot afford to leave open.
//!
//! The mosaic talks to tiles through [`TileVideo`], which is only what multi-view
//! needs: load a stream, say how it is going, move, and mute. Tests drive the mosaic
//! with fakes of it; the app uses [`EngineTile`].

use std::sync::Arc;

use aurora_core::mosaic::Rect;
use aurora_video::{Engine, Report, Request};

/// A tile's player, as the mosaic sees it.
pub trait TileVideo: Send {
    /// Ask for a stream. `Err` is a refusal before anything was opened; a stream that
    /// fails to open or dies later shows up in [`report`](Self::report) as an error.
    fn load(&mut self, request: Request) -> Result<(), String>;
    fn report(&self) -> Report;
    fn place(&mut self, rect: Rect);
    fn set_muted(&mut self, muted: bool);
}

/// Makes a tile's player, given the window to draw in (`0` when there is none, as in
/// tests) and where in it.
pub type TileFactory = Arc<dyn Fn(isize, Rect) -> Result<Box<dyn TileVideo>, String> + Send + Sync>;

/// A tile on the real engine.
///
/// Field order is drop order: the engine (and the mpv drawing into the surface) goes
/// first, then the surface it was drawing into.
pub struct EngineTile {
    engine: Engine,
    #[cfg_attr(not(windows), allow(dead_code))]
    surface: Option<Box<dyn TileSurface>>,
}

/// Where a tile's picture goes. A trait so a tile without a window (tests, and any
/// platform without one) is the same type.
pub trait TileSurface: Send + Sync {
    fn place(&self, rect: Rect) -> Result<(), String>;
}

#[cfg(windows)]
impl TileSurface for aurora_video::surface::Surface {
    fn place(&self, r: Rect) -> Result<(), String> {
        aurora_video::surface::Surface::place(self, to_surface(r))
    }
}

#[cfg(windows)]
fn to_surface(r: Rect) -> aurora_video::surface::Rect {
    aurora_video::surface::Rect {
        x: r.x,
        y: r.y,
        width: r.width.max(1),
        height: r.height.max(1),
    }
}

impl EngineTile {
    /// A tile on any engine, with no window of its own.
    pub fn headless(engine: Engine) -> Self {
        Self {
            engine,
            surface: None,
        }
    }

    /// A tile drawing into a new child of `parent`, on libmpv.
    #[cfg(windows)]
    pub fn open(parent: isize, rect: Rect) -> Result<Self, String> {
        use aurora_video::libmpv::{Config, LibMpv};
        use aurora_video::surface::Surface;

        if parent == 0 {
            return Err("This tile has no window to draw in.".into());
        }
        let surface = Surface::create(parent, to_surface(rect))
            .map_err(|e| format!("This tile has no video surface: {e}"))?;
        surface
            .set_visible(true)
            .map_err(|e| format!("This tile's video surface could not be shown: {e}"))?;
        let mpv = LibMpv::open(&Config {
            wid: Some(surface.wid()),
            headless: false,
        })
        .map_err(|e| format!("This tile could not start a player: {e}"))?;
        Ok(Self {
            engine: Engine::start(mpv),
            surface: Some(Box::new(surface)),
        })
    }

    #[cfg(not(windows))]
    pub fn open(_parent: isize, _rect: Rect) -> Result<Self, String> {
        Err("Multi-view plays video on Windows only.".into())
    }

    /// The factory the app gives the mosaic.
    pub fn factory() -> TileFactory {
        Arc::new(|parent, rect| Ok(Box::new(EngineTile::open(parent, rect)?) as Box<dyn TileVideo>))
    }
}

impl TileVideo for EngineTile {
    fn load(&mut self, request: Request) -> Result<(), String> {
        self.engine
            .load(request)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn report(&self) -> Report {
        self.engine.report()
    }

    fn place(&mut self, rect: Rect) {
        if let Some(surface) = &self.surface {
            if let Err(e) = surface.place(rect) {
                tracing::warn!("a mosaic tile could not be placed: {e}");
            }
        }
    }

    fn set_muted(&mut self, muted: bool) {
        if let Err(e) = self.engine.set_muted(muted) {
            tracing::warn!("a mosaic tile could not be muted: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_video::fake::FakeMpv;
    use aurora_video::Status;
    use std::time::{Duration, Instant};

    fn until(tile: &EngineTile, want: Status) -> Report {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let r = tile.report();
            if r.status == want {
                return r;
            }
            assert!(Instant::now() < deadline, "never {want:?}: {r:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn request(url: &str) -> Request {
        Request {
            url: url.into(),
            live: true,
            cache_secs: 8,
            ..Request::default()
        }
    }

    #[test]
    fn a_tile_loads_reports_and_mutes_through_its_own_engine() {
        let mpv = FakeMpv::new();
        let mut tile = EngineTile::headless(Engine::start(mpv.driver()));
        tile.load(request("http://p.test/live/1.ts")).unwrap();
        mpv.plays();
        until(&tile, Status::Playing);
        tile.set_muted(true);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !tile.report().muted {
            assert!(Instant::now() < deadline, "never muted");
            std::thread::sleep(Duration::from_millis(10));
        }
        // No window: placing is a no-op rather than an error.
        tile.place(Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        });
    }

    #[test]
    fn a_stream_that_dies_is_an_error_in_the_report() {
        let mpv = FakeMpv::new();
        let mut tile = EngineTile::headless(Engine::start(mpv.driver()));
        tile.load(request("http://p.test/live/1.ts")).unwrap();
        mpv.fail("HTTP error 403 Forbidden");
        let r = until(&tile, Status::Error);
        assert!(r.error.is_some(), "an error with no reason: {r:?}");
    }

    #[test]
    fn an_empty_address_is_refused_before_anything_opens() {
        let mpv = FakeMpv::new();
        let mut tile = EngineTile::headless(Engine::start(mpv.driver()));
        assert!(tile.load(request("  ")).is_err());
    }

    #[cfg(not(windows))]
    #[test]
    fn without_windows_the_factory_says_so_rather_than_drawing_nowhere() {
        let rect = Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        };
        assert!((EngineTile::factory())(1, rect).is_err());
    }
}
