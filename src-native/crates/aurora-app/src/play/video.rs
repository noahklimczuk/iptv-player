//! Starting the main engine on the app's window (docs/PLAYER.md E29–E30).

use std::sync::Arc;

use aurora_video::none::NoVideo;
use aurora_video::Engine;
use serde::Serialize;

use super::player::VideoSurface;

/// What the session plays on and where it draws.
pub struct Video {
    pub engine: Arc<Engine>,
    pub surface: Option<Box<dyn VideoSurface>>,
    /// The window's client size, physical pixels.
    pub window: (u32, u32),
}

impl Video {
    /// No picture at all: tests, and a machine without libmpv (E30).
    pub fn none(why: Option<String>) -> Self {
        Self {
            engine: Arc::new(Engine::start(NoVideo::new(why))),
            surface: None,
            window: (1280, 720),
        }
    }
}

/// `videoEngine` in Settings → About.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub name: String,
    pub version: Option<String>,
    pub renders_video: bool,
}

pub fn info(engine: &Engine) -> EngineInfo {
    let (name, version) = engine.describe();
    EngineInfo {
        name,
        version,
        renders_video: engine.renders_video(),
    }
}

/// Open the engine drawing into a surface under `window`'s web view.
///
/// Never fails: anything that goes wrong leaves an engine that says it renders no
/// video, and why, rather than an app that will not start.
#[cfg(windows)]
pub fn start(window: &tauri::WebviewWindow) -> Video {
    use aurora_video::libmpv::{self, Config, LibMpv};
    use aurora_video::surface::{Rect, Surface};

    // The picture is behind the web view, so the web view must let it through.
    let _ = window.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
    let size = window
        .inner_size()
        .map(|s| (s.width.max(1), s.height.max(1)))
        .unwrap_or((1280, 720));
    if !libmpv::loadable() {
        tracing::error!("no video engine: {} could not be loaded", libmpv::DLL);
        return Video {
            window: size,
            ..Video::none(Some(format!("{} could not be loaded", libmpv::DLL)))
        };
    }
    let hwnd = match window.hwnd() {
        Ok(h) => h.0 as isize,
        Err(e) => {
            tracing::error!("no video surface: the window has no handle: {e}");
            return Video {
                window: size,
                ..Video::none(Some(format!("the window has no handle: {e}")))
            };
        }
    };
    let rect = Rect {
        x: 0,
        y: 0,
        width: size.0,
        height: size.1,
    };
    let surface = match Surface::create(hwnd, rect) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("no video surface: {e}");
            return Video {
                window: size,
                ..Video::none(Some(e))
            };
        }
    };
    if let Err(e) = surface.set_visible(true) {
        tracing::warn!("{e}");
    }
    match LibMpv::open(&Config {
        wid: Some(surface.wid()),
        headless: false,
    }) {
        Ok(mpv) => {
            tracing::info!(width = size.0, height = size.1, "video engine ready");
            Video {
                engine: Arc::new(Engine::start(mpv)),
                surface: Some(Box::new(surface)),
                window: size,
            }
        }
        Err(e) => {
            tracing::error!("no video engine: {e}");
            Video {
                window: size,
                ..Video::none(Some(e))
            }
        }
    }
}

#[cfg(not(windows))]
pub fn start(window: &tauri::WebviewWindow) -> Video {
    let size = window
        .inner_size()
        .map(|s| (s.width.max(1), s.height.max(1)))
        .unwrap_or((1280, 720));
    Video {
        window: size,
        ..Video::none(Some("video playback is built for Windows only".into()))
    }
}
