//! libmpv backend — Windows only.
//!
//! This is the implementation that satisfies README C2 ("all playback is in-process").
//! mpv renders into a child HWND of the Tauri window, positioned *behind* the WebView2
//! so the React UI composites on top of live video. See docs/ARCHITECTURE.md.
//!
//! IMPORTANT: this module compiles on Windows in CI but has NOT been run against a real
//! display or a real stream — that is the Phase 0 spike in docs/ROADMAP.md, which needs
//! a Windows machine. Treat every timing claim here as unverified until then.

#![cfg(windows)]

use std::collections::HashMap;
use std::ffi::c_void;

use libmpv2::{events::Event, Mpv};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SetWindowPos, ShowWindow, HWND_BOTTOM, SWP_NOACTIVATE, SW_SHOW,
    WINDOW_EX_STYLE, WS_CHILD, WS_VISIBLE,
};

use crate::backend::{LoadOptions, PlayerBackend};
use crate::error::{PlaybackError, PlayerError};
use crate::state::{Aspect, PlaybackStats, PlayerState, PlayerStatus, Track, TrackKind};

/// mpv's end-file reason for a playback error.
const MPV_END_FILE_REASON_ERROR: u32 = 4;

/// Owned copies of the mpv events we care about, so the event-context borrow can be
/// released before handling them.
enum Drained {
    StartFile,
    FileLoaded,
    EndFile(u32),
    Property(String),
}

pub struct MpvBackend {
    mpv: Mpv,
    /// The child window mpv draws into. Owned by us, parented to the Tauri window.
    video_hwnd: Option<HWND>,
    state: PlayerState,
}

// SAFETY: Mpv is internally synchronized and the HWND is only touched from the thread
// that owns the backend. The backend is moved into a Mutex by the app layer.
unsafe impl Send for MpvBackend {}

impl MpvBackend {
    pub fn new() -> Result<Self, PlayerError> {
        let mpv = Mpv::with_initializer(|init| {
            // Hardware decoding first, software fallback — README C4.
            init.set_property("hwdec", "d3d11va-copy,dxva2-copy,auto-safe,no")?;
            init.set_property("vo", "gpu-next")?;
            init.set_property("gpu-api", "d3d11")?;
            init.set_property("gpu-context", "d3d11")?;

            // Audio: WASAPI, with passthrough left to the settings layer.
            init.set_property("ao", "wasapi")?;
            init.set_property("audio-channels", "auto-safe")?;
            init.set_property("volume-max", "200")?;

            // Resilience — streams die constantly (README §6.1).
            init.set_property("keep-open", "yes")?;
            init.set_property("network-timeout", "12")?;
            init.set_property(
                "stream-lavf-o",
                "reconnect=1,reconnect_streamed=1,\
                                                reconnect_delay_max=5",
            )?;

            // Subtitles: styled ASS, and never auto-load from the filesystem for a
            // network stream.
            init.set_property("sub-auto", "fuzzy")?;
            init.set_property("sub-ass-override", "yes")?;
            init.set_property("blend-subtitles", "yes")?;

            // Deinterlacing is decided per stream; SD cable feeds are often interlaced.
            init.set_property("deinterlace", "auto")?;

            // The app owns all UI. mpv draws nothing but video.
            init.set_property("osc", "no")?;
            init.set_property("osd-level", "0")?;
            init.set_property("input-default-bindings", "no")?;
            init.set_property("input-vo-keyboard", "no")?;
            init.set_property("terminal", "no")?;
            Ok(())
        })
        .map_err(|e| PlayerError::Init(e.to_string()))?;

        Ok(Self {
            mpv,
            video_hwnd: None,
            state: PlayerState::default(),
        })
    }

    /// Create the child HWND mpv renders into and hand it to mpv via `wid`.
    ///
    /// Z-order matters: the video window goes to the BOTTOM so the WebView2 (a sibling
    /// child of the same parent) paints above it. The WebView2 is configured with a
    /// transparent background, so the UI floats over the video and still receives every
    /// mouse and key event.
    pub fn attach(&mut self, parent: HWND, width: u32, height: u32) -> Result<(), PlayerError> {
        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                windows::core::w!("STATIC"),
                windows::core::w!(""),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                width as i32,
                height as i32,
                parent,
                None,
                None,
                None,
            )
            .map_err(|e| PlayerError::Init(format!("CreateWindowExW failed: {e}")))?;

            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetWindowPos(
                hwnd,
                HWND_BOTTOM,
                0,
                0,
                width as i32,
                height as i32,
                SWP_NOACTIVATE,
            );

            let wid = hwnd.0 as i64;
            self.mpv
                .set_property("wid", wid)
                .map_err(|e| PlayerError::Init(format!("mpv rejected wid: {e}")))?;
            self.video_hwnd = Some(hwnd);
        }
        Ok(())
    }

    /// Pump mpv's event queue. Called from a dedicated thread by the app layer; every
    /// interesting transition is folded into `self.state` for the UI to mirror.
    pub fn pump(&mut self, timeout_secs: f64) {
        // mpv's event context borrows the handle mutably for as long as it lives, and
        // every handler below needs to read properties off that same handle. So drain
        // the queue into owned values first, then release the borrow and act on them.
        let drained = {
            let ctx = self.mpv.event_context_mut();
            let mut out: Vec<Drained> = Vec::new();
            while let Some(Ok(event)) = ctx.wait_event(timeout_secs) {
                out.push(match event {
                    Event::StartFile => Drained::StartFile,
                    Event::FileLoaded => Drained::FileLoaded,
                    Event::EndFile(reason) => Drained::EndFile(reason),
                    Event::PropertyChange { name, .. } => Drained::Property(name.to_string()),
                    _ => continue,
                });
            }
            out
        };

        for event in drained {
            match event {
                Drained::StartFile => self.state.status = PlayerStatus::Loading,
                Drained::FileLoaded => {
                    self.state.status = PlayerStatus::Playing;
                    self.state.error = None;
                    self.refresh_tracks();
                }
                Drained::EndFile(reason) => {
                    // MPV_END_FILE_REASON_ERROR.
                    if reason == MPV_END_FILE_REASON_ERROR {
                        let raw = self
                            .mpv
                            .get_property::<String>("error-string")
                            .unwrap_or_else(|_| "unknown".into());
                        self.state.error = Some(PlaybackError::classify(&raw));
                        self.state.status = PlayerStatus::Error;
                    } else {
                        self.state.status = PlayerStatus::Idle;
                    }
                }
                Drained::Property(name) => match name.as_str() {
                    "time-pos" => {
                        self.state.position_secs = self.mpv.get_property("time-pos").unwrap_or(0.0);
                    }
                    "duration" => {
                        self.state.duration_secs = self.mpv.get_property("duration").unwrap_or(0.0);
                    }
                    "paused-for-cache" => {
                        let stalled: bool =
                            self.mpv.get_property("paused-for-cache").unwrap_or(false);
                        if stalled {
                            self.state.status = PlayerStatus::Buffering;
                        } else if self.state.status == PlayerStatus::Buffering {
                            self.state.status = PlayerStatus::Playing;
                        }
                    }
                    _ => {}
                },
            }
        }
        self.refresh_stats();
    }

    fn refresh_tracks(&mut self) {
        let count: i64 = self.mpv.get_property("track-list/count").unwrap_or(0);
        let mut audio = Vec::new();
        let mut subs = Vec::new();

        for i in 0..count {
            let kind: String = match self.mpv.get_property(&format!("track-list/{i}/type")) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let track = Track {
                id: self
                    .mpv
                    .get_property(&format!("track-list/{i}/id"))
                    .unwrap_or(i),
                kind: if kind == "audio" {
                    TrackKind::Audio
                } else {
                    TrackKind::Subtitle
                },
                title: self.mpv.get_property(&format!("track-list/{i}/title")).ok(),
                language: self.mpv.get_property(&format!("track-list/{i}/lang")).ok(),
                codec: self.mpv.get_property(&format!("track-list/{i}/codec")).ok(),
                channels: self
                    .mpv
                    .get_property(&format!("track-list/{i}/demux-channel-count"))
                    .ok()
                    .map(|n: i64| format!("{n}ch")),
                default: self
                    .mpv
                    .get_property(&format!("track-list/{i}/default"))
                    .unwrap_or(false),
            };
            match kind.as_str() {
                "audio" => audio.push(track),
                "sub" => subs.push(track),
                _ => {}
            }
        }
        self.state.audio_tracks = audio;
        self.state.subtitle_tracks = subs;
        self.state.active_audio_track = self.mpv.get_property("aid").ok();
        self.state.active_subtitle_track = self.mpv.get_property("sid").ok();
    }

    fn refresh_stats(&mut self) {
        let w: Option<i64> = self.mpv.get_property("width").ok();
        let h: Option<i64> = self.mpv.get_property("height").ok();
        self.state.stats = Some(PlaybackStats {
            resolution: match (w, h) {
                (Some(w), Some(h)) if w > 0 => Some(format!("{w}x{h}")),
                _ => None,
            },
            video_codec: self.mpv.get_property("video-codec").ok(),
            audio_codec: self.mpv.get_property("audio-codec-name").ok(),
            fps: self.mpv.get_property("container-fps").ok(),
            bitrate_kbps: self
                .mpv
                .get_property::<i64>("video-bitrate")
                .ok()
                .map(|b| (b / 1000) as u32),
            dropped_frames: self
                .mpv
                .get_property::<i64>("frame-drop-count")
                .unwrap_or(0)
                .max(0) as u64,
            buffer_secs: self
                .mpv
                .get_property("demuxer-cache-duration")
                .unwrap_or(0.0),
            hw_decoder: self.mpv.get_property("hwdec-current").ok(),
        });
    }

    fn cmd(&self, args: &[&str]) -> Result<(), PlayerError> {
        self.mpv
            .command(args[0], &args[1..])
            .map_err(|e| PlayerError::Command(format!("{}: {e}", args[0])))
    }
}

impl PlayerBackend for MpvBackend {
    fn load(&mut self, url: &str, options: &LoadOptions) -> Result<(), PlayerError> {
        if url.trim().is_empty() {
            return Err(PlayerError::Command("empty URL".into()));
        }
        // Per-load options are applied before loadfile so they take effect for this URL.
        for (key, value) in options.mpv_options() {
            if let Err(e) = self.mpv.set_property(&key, value.as_str()) {
                tracing::warn!("mpv rejected {key}: {e}");
            }
        }
        self.state = PlayerState {
            status: PlayerStatus::Loading,
            title: options.title.clone(),
            is_live: options.is_live,
            volume: self.state.volume,
            muted: self.state.muted,
            aspect: self.state.aspect,
            ..Default::default()
        };
        self.cmd(&["loadfile", url, "replace"])
    }

    fn stop(&mut self) -> Result<(), PlayerError> {
        let r = self.cmd(&["stop"]);
        self.state.status = PlayerStatus::Idle;
        r
    }

    fn set_paused(&mut self, paused: bool) -> Result<(), PlayerError> {
        self.mpv
            .set_property("pause", paused)
            .map_err(|e| PlayerError::Command(e.to_string()))?;
        self.state.status = if paused {
            PlayerStatus::Paused
        } else {
            PlayerStatus::Playing
        };
        Ok(())
    }

    fn seek(&mut self, position_secs: f64, relative: bool) -> Result<(), PlayerError> {
        let mode = if relative { "relative" } else { "absolute" };
        self.cmd(&["seek", &position_secs.to_string(), mode])
    }

    fn set_volume(&mut self, volume: u32) -> Result<(), PlayerError> {
        let v = volume.min(200);
        self.mpv
            .set_property("volume", v as f64)
            .map_err(|e| PlayerError::Command(e.to_string()))?;
        self.state.volume = v;
        Ok(())
    }

    fn set_muted(&mut self, muted: bool) -> Result<(), PlayerError> {
        self.mpv
            .set_property("mute", muted)
            .map_err(|e| PlayerError::Command(e.to_string()))?;
        self.state.muted = muted;
        Ok(())
    }

    fn set_speed(&mut self, speed: f64) -> Result<(), PlayerError> {
        let s = speed.clamp(0.25, 4.0);
        self.mpv
            .set_property("speed", s)
            .map_err(|e| PlayerError::Command(e.to_string()))?;
        // Keep pitch natural at non-1x speeds (README §6.2).
        let _ = self.mpv.set_property("audio-pitch-correction", true);
        self.state.speed = s;
        Ok(())
    }

    fn set_audio_track(&mut self, track_id: Option<i64>) -> Result<(), PlayerError> {
        match track_id {
            Some(id) => self.mpv.set_property("aid", id),
            None => self.mpv.set_property("aid", "no"),
        }
        .map_err(|e| PlayerError::Command(e.to_string()))?;
        self.state.active_audio_track = track_id;
        Ok(())
    }

    fn set_subtitle_track(&mut self, track_id: Option<i64>) -> Result<(), PlayerError> {
        match track_id {
            Some(id) => self.mpv.set_property("sid", id),
            None => self.mpv.set_property("sid", "no"),
        }
        .map_err(|e| PlayerError::Command(e.to_string()))?;
        self.state.active_subtitle_track = track_id;
        Ok(())
    }

    fn set_aspect(&mut self, aspect: Aspect) -> Result<(), PlayerError> {
        if let Some(v) = aspect.mpv_value() {
            let _ = self.mpv.set_property("video-aspect-override", v);
        }
        let _ = self.mpv.set_property("panscan", aspect.panscan());
        self.state.aspect = aspect;
        Ok(())
    }

    fn state(&self) -> PlayerState {
        self.state.clone()
    }

    /// Keep the video child window exactly on the WebView2's client rect. Called from
    /// the host's WM_SIZE handler so the two never tear apart during a drag or snap.
    fn resize(&mut self, width: u32, height: u32) -> Result<(), PlayerError> {
        if let Some(hwnd) = self.video_hwnd {
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    HWND_BOTTOM,
                    0,
                    0,
                    width as i32,
                    height as i32,
                    SWP_NOACTIVATE,
                );
            }
        }
        Ok(())
    }
}

impl Drop for MpvBackend {
    fn drop(&mut self) {
        if let Some(hwnd) = self.video_hwnd.take() {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}

// Silence unused-import warnings for items kept for the resize/message plumbing the
// host wires up (see docs/ARCHITECTURE.md).
const _: Option<fn(HWND, u32, WPARAM, LPARAM) -> LRESULT> = None;
const _: Option<RECT> = None;
const _: Option<HashMap<String, String>> = None;
const _: Option<*mut c_void> = None;
