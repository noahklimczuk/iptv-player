//! libmpv backend — Windows only.
//!
//! This is the implementation that satisfies README C2 ("all playback is in-process").
//! mpv renders into a child HWND of the Tauri window, positioned *behind* the WebView2
//! so the React UI composites on top of live video. See docs/ARCHITECTURE.md.
//!
//! Run against a real display and a real stream on Windows 11 with libmpv v0.41:
//! 1920x1080 H.264 at 60fps, `hwdec=d3d11va-copy`, composited behind the WebView2, with
//! the surface following a resize. `AUDIT/test-report.md` §11 has the numbers and
//! `tests-host/scenarios/video_surface.py` is the regression test.
//!
//! What that run has *not* covered: a provider's own stream end to end, catch-up,
//! recording, and anything about a second monitor or a per-monitor DPI change. Timing
//! claims are measured on one machine against a public CDN — one zap came in at 1.91s
//! against the 1.5s budget in README §16.

#![cfg(windows)]

use std::ffi::c_void;

use aurora_core::markers::Chapter;
use aurora_core::timeshift::{Budget, Reading};
use libmpv2::mpv_node::MpvNode;
use libmpv2::{events::Event, Mpv};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SetWindowPos, ShowWindow, HWND_BOTTOM, SWP_NOACTIVATE, SW_SHOW,
    WINDOW_EX_STYLE, WS_CHILD, WS_VISIBLE,
};

use crate::backend::{LoadOptions, PlayerBackend};
use crate::error::{PlaybackError, PlayerError};
use crate::state::{Aspect, PlaybackStats, PlayerState, PlayerStatus, Track, TrackKind};

/// The runtime libmpv, by the name the import library was built against. `mpv.lib` is
/// generated from the DLL's own export table with `/name:` set to it, so this string and
/// the import table always agree.
const LIBMPV_DLL: &str = "libmpv-2.dll";

/// Whether libmpv can actually be loaded, asked before anything needs it.
fn libmpv_loadable() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::System::LibraryLoader::LoadLibraryW;

    let wide: Vec<u16> = LIBMPV_DLL
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // Safety: a null-terminated wide string that outlives the call. A handle that comes
    // back is deliberately not freed — the process is about to use the library.
    unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())).is_ok() }
}

/// mpv's end-file reason for a playback error.
const MPV_END_FILE_REASON_ERROR: u32 = 4;

/// Owned copies of the mpv events we care about, so the event-context borrow can be
/// released before handling them.
enum Drained {
    StartFile,
    FileLoaded,
    EndFile(u32),
    Property(String),
    /// mpv reported an error rather than an event. This is how a stream that will not
    /// open reaches us: libmpv2 turns an end-file carrying an error code into `Err`
    /// before `Event::EndFile` is ever constructed, so the most important thing the
    /// player can be told arrives down the arm that used to end the loop.
    Failed(String),
}

/// A live stream being kept on disk so it can be rewound (README §7.6).
struct Timeshift {
    budget: Budget,
    /// The first playhead reading after the tune. A live transport stream carries
    /// whatever timestamp the provider is up to, so this is not zero and is not known
    /// until playback reports it.
    tuned_at: Option<f64>,
}

pub struct MpvBackend {
    mpv: Mpv,
    /// The child window mpv draws into. Owned by us, parented to the Tauri window.
    video_hwnd: Option<HWND>,
    state: PlayerState,
    /// The buffer this load was given, or `None` when nothing is being kept.
    timeshift: Option<Timeshift>,
}

// SAFETY: `Mpv` is internally synchronized, and the backend lives behind a `Mutex` in
// the app layer, so only one thread is inside it at a time.
//
// The HWND is now touched from two: `attach` and `resize` run on the main thread (at
// setup, and from the window event loop), while `pump` runs on the player heartbeat.
// That is sound rather than merely tolerated. `CreateWindowExW` ties the child window
// to the creating thread's message queue — the main thread, which is the one with a
// message loop, and the only place that could be right. `SetWindowPos` on a window
// owned by another thread is explicitly permitted by Win32. And mpv renders into the
// handle from its own threads regardless of either, which is what passing `wid` means.
//
// What would *not* be sound is destroying the window from a thread other than its
// creator; `Drop` runs wherever the `Mutex` is dropped, which is the main thread at
// exit.
unsafe impl Send for MpvBackend {}

impl MpvBackend {
    pub fn new() -> Result<Self, PlayerError> {
        // Ask for the DLL before touching a single mpv symbol. With `/DELAYLOAD` set on
        // the binary (see `aurora-app/build.rs`), nothing has been resolved yet, so this
        // is the one place where "libmpv is not usable" can still be turned into a
        // value. Without it the process is already dead: a load-time import that cannot
        // be resolved ends it in the loader, before `main`, with nothing written
        // anywhere (F-34).
        if !libmpv_loadable() {
            return Err(PlayerError::Init(format!(
                "{LIBMPV_DLL} could not be loaded. It must sit beside aurora-app.exe; \
                 a scanner holding it open will do this too."
            )));
        }
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
            timeshift: None,
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

    /// Record a playback failure, in the one place that decides what one looks like.
    ///
    /// The text is whatever mpv said, which is coarse: `mpv_error_string` gives
    /// "loading failed" and not the HTTP status underneath it, so `classify` has little
    /// to go on and most failures land on the generic message. Telling a viewer the
    /// stream is dead is still the difference between that and a spinner that never
    /// stops. Getting "403" in front of `classify` means capturing mpv's log stream
    /// (`mpv_request_log_messages`), which is a larger change than this one.
    fn fail(&mut self, message: &str) {
        tracing::warn!("playback failed: {message}");
        self.state.error = Some(PlaybackError::classify(message));
        self.state.status = PlayerStatus::Error;
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
            // `while let Some(Ok(event))` ended the drain on the first `Err` and threw
            // away every event queued behind it — and an `Err` is exactly how a dead
            // stream arrives (see `Drained::Failed`). A channel whose URL refused every
            // connection therefore sat at `Loading` for as long as anyone was willing to
            // wait: no error, no status change, and no failover, because the event that
            // would have triggered one was the event that stopped the loop. Reproduced
            // in `tests-host/scenarios/stream_failure.py`.
            while let Some(event) = ctx.wait_event(timeout_secs) {
                out.push(match event {
                    Ok(Event::StartFile) => Drained::StartFile,
                    Ok(Event::FileLoaded) => Drained::FileLoaded,
                    Ok(Event::EndFile(reason)) => Drained::EndFile(reason),
                    Ok(Event::PropertyChange { name, .. }) => Drained::Property(name.to_string()),
                    Ok(_) => continue,
                    Err(e) => Drained::Failed(e.to_string()),
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
                    // MPV_END_FILE_REASON_ERROR. A file that ends *because* of an error
                    // normally reaches us as `Drained::Failed` instead, since libmpv2
                    // reads the error code off the event first; this arm remains for the
                    // case where mpv gives the reason without one.
                    //
                    // It used to read an `error-string` property to describe the
                    // failure. mpv has no such property — confirmed against libmpv
                    // v0.41, where it answers MPV_ERROR_PROPERTY_NOT_FOUND — so the read
                    // always failed and every playback error in the app's history was
                    // classified from the literal word "unknown".
                    if reason == MPV_END_FILE_REASON_ERROR {
                        self.fail("playback stopped with an error");
                    } else {
                        self.state.status = PlayerStatus::Idle;
                    }
                }
                Drained::Failed(message) => {
                    // Only when something was meant to be on. libmpv2 reports a refused
                    // command through the same `Err`, and a command that was refused is
                    // not a stream that died.
                    if matches!(
                        self.state.status,
                        PlayerStatus::Loading | PlayerStatus::Playing | PlayerStatus::Buffering
                    ) {
                        self.fail(&message);
                    } else {
                        tracing::debug!(
                            "mpv reported {message} while {:?}; not a playback failure",
                            self.state.status
                        );
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
        self.refresh_timeshift();
    }

    /// Work out what the viewer can currently rewind into.
    ///
    /// mpv is asked for the two things it knows — where the playhead is and how far the
    /// cache runs — and `aurora_core::timeshift` bounds the answer by the budget, since
    /// mpv enforces bytes and the setting is also written in minutes.
    fn refresh_timeshift(&mut self) {
        let Some(ts) = self.timeshift.as_mut() else {
            self.state.timeshift = None;
            return;
        };
        let position = self.state.position_secs;
        let tuned_at = *ts.tuned_at.get_or_insert(position);
        let budget = ts.budget;

        // `demuxer-cache-time` is the newest buffered moment. Before the first read it
        // is absent, and the playhead is then the only moment there is.
        let live = self
            .mpv
            .get_property::<f64>("demuxer-cache-time")
            .unwrap_or(position)
            .max(position);

        let mut window = budget.window(&Reading {
            position_secs: position,
            live_secs: live,
            tuned_at_secs: tuned_at,
            bitrate_bps: self.observed_bitrate(),
        });
        // mpv knows exactly where its retained data starts, and it is the authority when
        // it answers: the budget can only ever over-estimate, never under.
        if let Some(start) = self.cache_start() {
            window.start_secs = window.start_secs.max(start).min(window.position_secs);
        }
        self.state.timeshift = Some(window);
    }

    /// The oldest moment still held, from `demuxer-cache-state`.
    ///
    /// Read out of the property's documented shape — a map with a `seekable-ranges`
    /// array of `{start, end}` — and, like everything else in this file, never yet run
    /// against a real stream. `None` when mpv does not answer, which is why the caller
    /// has a bound of its own.
    fn cache_start(&self) -> Option<f64> {
        let state: MpvNode = self.mpv.get_property("demuxer-cache-state").ok()?;
        for (key, value) in state.map()? {
            if key != "seekable-ranges" {
                continue;
            }
            // The first range is the oldest. A live stream that has not been seeked has
            // exactly one.
            let first = value.array()?.next()?;
            for (field, number) in first.map()? {
                if field == "start" {
                    return number.f64();
                }
            }
        }
        None
    }

    /// Bits per second of everything being read, or 0 when mpv has not said yet.
    ///
    /// Both tracks, because the byte budget is spent on the whole stream and a 256 kb/s
    /// audio track is a fifth of a radio channel's bitrate.
    fn observed_bitrate(&self) -> u64 {
        let of = |name: &str| self.mpv.get_property::<i64>(name).unwrap_or(0).max(0) as u64;
        of("video-bitrate") + of("audio-bitrate")
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
        // `args[0]` on an empty slice panics, in Windows-only code CI cannot run. No
        // caller passes one today; this is one edit away from being reachable.
        let Some((name, rest)) = args.split_first() else {
            return Err(PlayerError::Command("an mpv command with no name".into()));
        };
        self.mpv
            .command(name, rest)
            .map_err(|e| PlayerError::Command(format!("{name}: {e}")))
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
            channel_id: options.playing.and_then(|p| p.channel_id),
            item_kind: options.playing.map(|p| p.kind),
            item_id: options.playing.map(|p| p.id),
            volume: self.state.volume,
            muted: self.state.muted,
            aspect: self.state.aspect,
            ..Default::default()
        };
        self.timeshift = options
            .timeshift
            .as_ref()
            .filter(|_| options.is_live)
            .map(|ts| Timeshift {
                budget: ts.budget,
                tuned_at: None,
            });
        self.cmd(&["loadfile", url, "replace"])
    }

    fn stop(&mut self) -> Result<(), PlayerError> {
        let r = self.cmd(&["stop"]);
        self.state.status = PlayerStatus::Idle;
        self.timeshift = None;
        self.state.timeshift = None;
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

    fn chapters(&self) -> Vec<Chapter> {
        let count: i64 = self.mpv.get_property("chapter-list/count").unwrap_or(0);
        (0..count)
            .filter_map(|i| {
                let start: f64 = self
                    .mpv
                    .get_property(&format!("chapter-list/{i}/time"))
                    .ok()?;
                Some(Chapter {
                    title: self
                        .mpv
                        .get_property(&format!("chapter-list/{i}/title"))
                        .ok(),
                    start_secs: start,
                })
            })
            .collect()
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

    /// The host hands over its `HWND` as an integer, because the trait it is coming
    /// through compiles on platforms where `HWND` does not exist.
    fn attach(&mut self, parent: isize, width: u32, height: u32) -> Result<(), PlayerError> {
        if parent == 0 {
            return Err(PlayerError::Init(
                "the host gave no window to attach to".into(),
            ));
        }
        MpvBackend::attach(self, HWND(parent as *mut c_void), width, height)
    }

    fn pump(&mut self, timeout_secs: f64) {
        MpvBackend::pump(self, timeout_secs)
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
