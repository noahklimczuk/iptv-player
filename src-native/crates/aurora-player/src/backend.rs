//! The playback seam.

use std::collections::HashMap;
use std::path::PathBuf;

use aurora_core::markers::Chapter;
use aurora_core::timeshift::{Budget, Reading, Window};

use crate::error::PlayerError;
use crate::state::{Aspect, MediaKind, PlayerState, PlayerStatus};

/// What is being played, so the state can say so.
///
/// One value rather than three loose fields: the kind, the row and the channel have to
/// agree, and the UI reads all three to decide whether this is an episode it can offer
/// Skip Intro for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Playing {
    pub kind: MediaKind,
    /// The library row — a movie, an episode, or the channel itself for live TV.
    pub id: i64,
    /// The channel, where one is involved: live and catch-up.
    pub channel_id: Option<i64>,
}

/// How much of a live stream mpv keeps on disk so it can be rewound (README §7.6).
///
/// Aurora does not fetch the stream twice. The buffer is the demuxer cache of the
/// connection already playing, written to `dir` — docs/DECISIONS.md D21.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeshiftCache {
    pub budget: Budget,
    /// Where mpv writes the cache file.
    pub dir: PathBuf,
}

/// Forward cache for a buffered live stream.
///
/// Small on purpose, and unrelated to the rewind budget: this is data ahead of the
/// playhead, which costs latency at the live edge. 64 MiB is half a minute of a 20 Mb/s
/// feed, well past the eight seconds `cache_secs` asks to keep smooth.
const FORWARD_CACHE_BYTES: u64 = 64 * 1024 * 1024;

/// Options that must travel with a stream URL. IPTV providers routinely require a
/// specific User-Agent or Referer, and live streams want a different cache profile
/// from VOD (README §6.1).
#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    pub headers: HashMap<String, String>,
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
    pub start_at_secs: Option<f64>,
    pub is_live: bool,
    /// Network buffer in seconds — README §6.1's Low latency / Balanced / Unstable presets.
    pub cache_secs: u32,
    /// Keep history for rewinding, when this is a live stream and the viewer asked for
    /// it (README §7.6). `None` buffers nothing.
    pub timeshift: Option<TimeshiftCache>,
    /// What the library calls this, when it is something the library knows about.
    pub playing: Option<Playing>,
    pub title: Option<String>,
}

impl LoadOptions {
    /// README §6.1: fast channel zapping needs small probe sizes on live TS, while VOD
    /// benefits from a deeper probe for correct track detection.
    pub fn mpv_options(&self) -> Vec<(String, String)> {
        let mut o: Vec<(String, String)> = Vec::new();
        if let Some(ua) = &self.user_agent {
            o.push(("user-agent".into(), ua.clone()));
        }
        if let Some(r) = &self.referrer {
            o.push(("referrer".into(), r.clone()));
        }
        if !self.headers.is_empty() {
            let joined = self
                .headers
                .iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join(",");
            o.push(("http-header-fields".into(), joined));
        }
        o.push(("cache-secs".into(), self.cache_secs.max(1).to_string()));

        // The timeshift buffer, in both states. Emitted even when off because these are
        // properties of a long-lived mpv handle rather than arguments to one file: left
        // alone, a channel tuned after the buffer was switched off would go on filling
        // the disk until the app restarted.
        match self.timeshift.as_ref().filter(|_| self.is_live) {
            Some(ts) => {
                o.push(("cache".into(), "yes".into()));
                o.push(("cache-on-disk".into(), "yes".into()));
                o.push(("cache-dir".into(), ts.dir.to_string_lossy().into_owned()));
                o.push(("demuxer-max-back-bytes".into(), ts.budget.bytes.to_string()));
                o.push(("demuxer-max-bytes".into(), FORWARD_CACHE_BYTES.to_string()));
                // An HTTP live stream announces itself unseekable, and mpv believes it.
                // The cache is what makes rewinding possible; this is what lets mpv
                // seek into it.
                o.push(("force-seekable".into(), "yes".into()));
            }
            None => {
                o.push(("cache-on-disk".into(), "no".into()));
                o.push(("demuxer-max-back-bytes".into(), "0".into()));
                o.push(("force-seekable".into(), "no".into()));
            }
        }

        if self.is_live {
            o.push(("demuxer-lavf-probesize".into(), "524288".into()));
            o.push(("demuxer-lavf-analyzeduration".into(), "0.6".into()));
            o.push(("cache-pause-initial".into(), "no".into()));
        } else {
            o.push(("demuxer-lavf-probesize".into(), "8000000".into()));
            if let Some(start) = self.start_at_secs {
                o.push(("start".into(), format!("{start}")));
            }
        }
        o
    }
}

/// Everything the app needs from a video engine. Implemented by `MpvBackend` on Windows
/// and `NullBackend` everywhere else.
pub trait PlayerBackend: Send {
    fn load(&mut self, url: &str, options: &LoadOptions) -> Result<(), PlayerError>;
    fn stop(&mut self) -> Result<(), PlayerError>;
    fn set_paused(&mut self, paused: bool) -> Result<(), PlayerError>;
    fn seek(&mut self, position_secs: f64, relative: bool) -> Result<(), PlayerError>;
    fn set_volume(&mut self, volume: u32) -> Result<(), PlayerError>;
    fn set_muted(&mut self, muted: bool) -> Result<(), PlayerError>;
    fn set_speed(&mut self, speed: f64) -> Result<(), PlayerError>;
    fn set_audio_track(&mut self, track_id: Option<i64>) -> Result<(), PlayerError>;
    fn set_subtitle_track(&mut self, track_id: Option<i64>) -> Result<(), PlayerError>;
    fn set_aspect(&mut self, aspect: Aspect) -> Result<(), PlayerError>;
    fn state(&self) -> PlayerState;
    /// Chapters in the loaded file, if it has any. Empty is normal — most IPTV VOD
    /// carries none, which is why skip markers also learn from the viewer (README §9).
    fn chapters(&self) -> Vec<Chapter>;
    /// Re-attach the video surface after the window is resized or moved.
    fn resize(&mut self, width: u32, height: u32) -> Result<(), PlayerError>;

    /// Give the backend a window to render into.
    ///
    /// `parent` is the host window's native handle as an integer — on Windows, the
    /// `HWND`. An integer rather than a typed handle because this trait compiles on
    /// every platform and `HWND` does not; the backend that cares is the one that
    /// knows how to interpret it.
    ///
    /// This is on the trait, rather than being an inherent method on `MpvBackend`,
    /// because that is the whole reason it was never called: the app layer holds a
    /// `Box<dyn PlayerBackend>` and could not reach it. The default does nothing,
    /// which is correct for a backend that renders nothing.
    fn attach(&mut self, parent: isize, width: u32, height: u32) -> Result<(), PlayerError> {
        let _ = (parent, width, height);
        Ok(())
    }

    /// Drain whatever the player has to say and fold it into its state.
    ///
    /// The other half of the same gap. Position, tracks, buffering, errors and the
    /// timeshift window only change when this runs, so the 250 ms heartbeat that
    /// *reads* the state has to call it first or the OSD never moves after a load.
    ///
    /// `timeout_secs` is how long to wait for an event that has not arrived yet; the
    /// heartbeat passes zero, because it has somewhere else to be.
    fn pump(&mut self, timeout_secs: f64) {
        let _ = timeout_secs;
    }
}

/// A backend that models state without decoding anything. Used on non-Windows hosts
/// and in tests, so every command path is exercisable without a GPU or a network.
#[derive(Debug, Default)]
pub struct NullBackend {
    state: PlayerState,
    chapters: Vec<Chapter>,
    /// The budget the current load was given, so the window can be recomputed after a
    /// seek. `None` when this stream is not being buffered.
    timeshift: Option<Budget>,
    /// The newest buffered moment. On a real backend the stream moves this; here
    /// [`NullBackend::advance_live`] does.
    live_secs: f64,
    /// The playhead when the stream was loaded. Nothing older than this was buffered.
    tuned_at_secs: f64,
}

impl NullBackend {
    /// Stand in for a file's chapter list, so marker handling can be exercised
    /// without a decoder.
    pub fn set_chapters(&mut self, chapters: Vec<Chapter>) {
        self.chapters = chapters;
    }

    /// Let time pass on a modelled live stream.
    ///
    /// The live edge moves on; the playhead follows it only while something is playing,
    /// which is the whole of timeshift in one line — pausing is what puts the viewer
    /// behind, and staying paused is what keeps them there.
    pub fn advance_live(&mut self, secs: f64) {
        self.live_secs += secs;
        if self.state.status == PlayerStatus::Playing {
            self.state.position_secs = (self.state.position_secs + secs).min(self.live_secs);
        }
        self.refresh_window();
    }

    fn refresh_window(&mut self) {
        // Bitrate is left unknown: inventing one here would make the modelled window
        // disagree with the real one for no gain, so the minutes cap governs.
        self.state.timeshift = self.timeshift.map(|budget| {
            budget.window(&Reading {
                position_secs: self.state.position_secs,
                live_secs: self.live_secs,
                tuned_at_secs: self.tuned_at_secs,
                bitrate_bps: 0,
            })
        });
    }

    /// The window the viewer can reach, when this stream is being buffered.
    pub fn window(&self) -> Option<Window> {
        self.state.timeshift
    }
}

impl PlayerBackend for NullBackend {
    fn load(&mut self, url: &str, options: &LoadOptions) -> Result<(), PlayerError> {
        if url.trim().is_empty() {
            return Err(PlayerError::Command("empty URL".into()));
        }
        self.chapters.clear();
        let position = options.start_at_secs.unwrap_or(0.0);
        self.state = PlayerState {
            status: PlayerStatus::Playing,
            title: options.title.clone(),
            is_live: options.is_live,
            channel_id: options.playing.and_then(|p| p.channel_id),
            item_kind: options.playing.map(|p| p.kind),
            item_id: options.playing.map(|p| p.id),
            position_secs: position,
            duration_secs: if options.is_live { 0.0 } else { 5400.0 },
            volume: self.state.volume,
            muted: self.state.muted,
            ..Default::default()
        };
        self.timeshift = options
            .timeshift
            .as_ref()
            .filter(|_| options.is_live)
            .map(|ts| ts.budget);
        self.live_secs = position;
        self.tuned_at_secs = position;
        self.refresh_window();
        Ok(())
    }

    fn stop(&mut self) -> Result<(), PlayerError> {
        let (volume, muted) = (self.state.volume, self.state.muted);
        self.state = PlayerState {
            volume,
            muted,
            ..Default::default()
        };
        self.timeshift = None;
        self.live_secs = 0.0;
        self.tuned_at_secs = 0.0;
        Ok(())
    }

    fn set_paused(&mut self, paused: bool) -> Result<(), PlayerError> {
        if self.state.status == PlayerStatus::Idle {
            return Err(PlayerError::NoMedia);
        }
        self.state.status = if paused {
            PlayerStatus::Paused
        } else {
            PlayerStatus::Playing
        };
        Ok(())
    }

    fn seek(&mut self, position_secs: f64, relative: bool) -> Result<(), PlayerError> {
        if self.state.status == PlayerStatus::Idle {
            return Err(PlayerError::NoMedia);
        }
        let target = if relative {
            self.state.position_secs + position_secs
        } else {
            position_secs
        };
        // A buffered live stream is bounded by what is held, not by a duration: there
        // is nothing past the live edge to seek to, and nothing before the oldest
        // retained moment either.
        if let Some(window) = self.state.timeshift {
            self.state.position_secs = window.clamp(target);
            self.refresh_window();
            return Ok(());
        }
        let max = if self.state.duration_secs > 0.0 {
            self.state.duration_secs
        } else {
            f64::MAX
        };
        self.state.position_secs = target.clamp(0.0, max);
        Ok(())
    }

    fn set_volume(&mut self, volume: u32) -> Result<(), PlayerError> {
        self.state.volume = volume.min(200);
        Ok(())
    }

    fn set_muted(&mut self, muted: bool) -> Result<(), PlayerError> {
        self.state.muted = muted;
        Ok(())
    }

    fn set_speed(&mut self, speed: f64) -> Result<(), PlayerError> {
        self.state.speed = speed.clamp(0.25, 4.0);
        Ok(())
    }

    fn set_audio_track(&mut self, track_id: Option<i64>) -> Result<(), PlayerError> {
        self.state.active_audio_track = track_id;
        Ok(())
    }

    fn set_subtitle_track(&mut self, track_id: Option<i64>) -> Result<(), PlayerError> {
        self.state.active_subtitle_track = track_id;
        Ok(())
    }

    fn set_aspect(&mut self, aspect: Aspect) -> Result<(), PlayerError> {
        self.state.aspect = aspect;
        Ok(())
    }

    fn state(&self) -> PlayerState {
        self.state.clone()
    }

    fn chapters(&self) -> Vec<Chapter> {
        self.chapters.clone()
    }

    fn resize(&mut self, _width: u32, _height: u32) -> Result<(), PlayerError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live() -> LoadOptions {
        LoadOptions {
            is_live: true,
            cache_secs: 8,
            ..Default::default()
        }
    }

    #[test]
    fn live_and_vod_get_different_probe_profiles() {
        let l = live().mpv_options();
        let probe = |o: &Vec<(String, String)>| {
            o.iter()
                .find(|(k, _)| k == "demuxer-lavf-probesize")
                .map(|(_, v)| v.clone())
        };
        assert_eq!(
            probe(&l).as_deref(),
            Some("524288"),
            "live must probe small to zap fast"
        );

        let vod = LoadOptions {
            is_live: false,
            cache_secs: 30,
            ..Default::default()
        };
        assert_eq!(probe(&vod.mpv_options()).as_deref(), Some("8000000"));
    }

    #[test]
    fn headers_and_user_agent_are_passed_through() {
        let mut o = live();
        o.user_agent = Some("AuroraTV/1.0".into());
        o.referrer = Some("https://example.com/".into());
        o.headers.insert("X-Token".into(), "abc".into());
        let opts = o.mpv_options();
        assert!(opts
            .iter()
            .any(|(k, v)| k == "user-agent" && v == "AuroraTV/1.0"));
        assert!(opts
            .iter()
            .any(|(k, v)| k == "referrer" && v == "https://example.com/"));
        assert!(opts
            .iter()
            .any(|(k, v)| k == "http-header-fields" && v.contains("X-Token: abc")));
    }

    #[test]
    fn a_vod_start_position_is_forwarded_but_a_live_one_is_not() {
        let vod = LoadOptions {
            start_at_secs: Some(120.0),
            ..Default::default()
        };
        assert!(vod.mpv_options().iter().any(|(k, _)| k == "start"));

        let mut l = live();
        l.start_at_secs = Some(120.0);
        assert!(!l.mpv_options().iter().any(|(k, _)| k == "start"));
    }

    #[test]
    fn cache_secs_is_never_zero() {
        let o = LoadOptions {
            cache_secs: 0,
            ..Default::default()
        };
        let v = o
            .mpv_options()
            .into_iter()
            .find(|(k, _)| k == "cache-secs")
            .unwrap()
            .1;
        assert_eq!(v, "1");
    }

    #[test]
    fn null_backend_models_the_full_command_surface() {
        let mut b = NullBackend::default();
        assert_eq!(b.state().status, PlayerStatus::Idle);

        // Commands before load are refused rather than silently ignored.
        assert!(b.set_paused(true).is_err());
        assert!(b.seek(10.0, true).is_err());

        b.load("https://example.com/s.ts", &live()).unwrap();
        assert_eq!(b.state().status, PlayerStatus::Playing);
        assert!(b.state().is_live);

        b.set_paused(true).unwrap();
        assert_eq!(b.state().status, PlayerStatus::Paused);

        b.set_volume(500).unwrap();
        assert_eq!(b.state().volume, 200, "volume clamps at 200%");

        b.set_speed(99.0).unwrap();
        assert_eq!(b.state().speed, 4.0, "speed clamps at 4x");

        b.stop().unwrap();
        assert_eq!(b.state().status, PlayerStatus::Idle);
        assert_eq!(b.state().volume, 200, "volume survives a stop");
    }

    #[test]
    fn chapters_are_reported_and_cleared_on_the_next_load() {
        let mut b = NullBackend::default();
        assert!(b.chapters().is_empty());

        b.load("https://example.com/e1.mkv", &LoadOptions::default())
            .unwrap();
        b.set_chapters(vec![Chapter {
            title: Some("Intro".into()),
            start_secs: 0.0,
        }]);
        assert_eq!(b.chapters().len(), 1);

        // A new file must not inherit the previous one's chapters.
        b.load("https://example.com/e2.mkv", &LoadOptions::default())
            .unwrap();
        assert!(b.chapters().is_empty());
    }

    #[test]
    fn empty_urls_are_rejected() {
        let mut b = NullBackend::default();
        assert!(b.load("   ", &live()).is_err());
    }

    #[test]
    fn seeking_a_live_stream_does_not_clamp_to_zero_duration() {
        let mut b = NullBackend::default();
        b.load("https://example.com/s.ts", &live()).unwrap();
        b.seek(30.0, true).unwrap();
        assert_eq!(b.state().position_secs, 30.0);
    }

    fn buffered_live() -> LoadOptions {
        LoadOptions {
            timeshift: Some(TimeshiftCache {
                budget: Budget::default(),
                dir: PathBuf::from("/tmp/aurora-timeshift"),
            }),
            ..live()
        }
    }

    fn option<'a>(opts: &'a [(String, String)], key: &str) -> Option<&'a str> {
        opts.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    #[test]
    fn a_buffered_live_stream_asks_mpv_to_keep_history_on_disk() {
        let opts = buffered_live().mpv_options();
        assert_eq!(option(&opts, "cache-on-disk"), Some("yes"));
        assert_eq!(
            option(&opts, "demuxer-max-back-bytes"),
            Some(Budget::default().bytes.to_string().as_str())
        );
        assert_eq!(
            option(&opts, "force-seekable"),
            Some("yes"),
            "an HTTP live stream calls itself unseekable; the cache is what makes \
             rewinding possible"
        );
        assert!(option(&opts, "cache-dir").is_some_and(|d| d.contains("aurora-timeshift")));
    }

    #[test]
    fn turning_the_buffer_off_is_said_out_loud_rather_than_left_unsaid() {
        // These are properties of one long-lived mpv handle. If the off state emitted
        // nothing, a channel tuned after the buffer was switched off would inherit it
        // and go on filling the disk until the app restarted.
        let opts = live().mpv_options();
        assert_eq!(option(&opts, "cache-on-disk"), Some("no"));
        assert_eq!(option(&opts, "demuxer-max-back-bytes"), Some("0"));
        assert_eq!(option(&opts, "force-seekable"), Some("no"));
    }

    #[test]
    fn a_film_is_never_given_a_timeshift_buffer() {
        // Nothing needs buffering to rewind a file, and a 10 GB back-buffer on a movie
        // would be 10 GB of disk for a seek the server already supports.
        let vod = LoadOptions {
            is_live: false,
            ..buffered_live()
        };
        assert_eq!(option(&vod.mpv_options(), "cache-on-disk"), Some("no"));
    }

    #[test]
    fn pausing_a_buffered_live_stream_is_what_puts_the_viewer_behind_live() {
        let mut b = NullBackend::default();
        b.load("https://example.com/s.ts", &buffered_live())
            .unwrap();

        // Two minutes of watching: still live, with two minutes to rewind into.
        b.advance_live(120.0);
        let w = b.window().expect("a buffered stream has a window");
        assert!(w.is_at_live());
        assert_eq!(w.rewindable_secs(), 120.0);

        // Answer the door for five minutes.
        b.set_paused(true).unwrap();
        b.advance_live(300.0);
        let w = b.window().unwrap();
        assert_eq!(
            w.delay_secs(),
            300.0,
            "five minutes behind, where we left off"
        );
        assert_eq!(b.state().position_secs, 120.0, "the playhead stayed put");
        assert!(!w.is_at_live());

        // Resuming keeps that delay: this is watching from the buffer now.
        b.set_paused(false).unwrap();
        b.advance_live(60.0);
        assert_eq!(b.window().unwrap().delay_secs(), 300.0);
    }

    #[test]
    fn rewinding_stops_at_the_oldest_buffered_moment_and_never_past_live() {
        let mut b = NullBackend::default();
        b.load("https://example.com/s.ts", &buffered_live())
            .unwrap();
        b.advance_live(600.0);

        b.seek(-9_999.0, true).unwrap();
        let w = b.window().unwrap();
        assert_eq!(b.state().position_secs, w.start_secs);
        assert_eq!(w.start_secs, 0.0, "the tune is the oldest thing there is");

        b.seek(9_999.0, true).unwrap();
        assert_eq!(
            b.state().position_secs,
            600.0,
            "fast-forwarding lands on live, not beyond it"
        );
        assert!(b.window().unwrap().is_at_live());
    }

    #[test]
    fn the_window_never_offers_more_rewind_than_the_channel_has_been_on() {
        let mut b = NullBackend::default();
        b.load("https://example.com/s.ts", &buffered_live())
            .unwrap();
        b.advance_live(20.0);
        // The budget allows half an hour. Twenty seconds in, twenty seconds exist.
        assert_eq!(b.window().unwrap().span_secs(), 20.0);
    }

    #[test]
    fn a_live_stream_nobody_asked_to_buffer_has_no_window() {
        let mut b = NullBackend::default();
        b.load("https://example.com/s.ts", &live()).unwrap();
        assert!(b.window().is_none());
        assert!(b.state().timeshift.is_none());
    }

    #[test]
    fn stopping_forgets_the_buffer() {
        let mut b = NullBackend::default();
        b.load("https://example.com/s.ts", &buffered_live())
            .unwrap();
        b.advance_live(60.0);
        b.stop().unwrap();
        assert!(b.window().is_none());

        // …and a film loaded next does not inherit one.
        b.load("https://example.com/m.mkv", &LoadOptions::default())
            .unwrap();
        assert!(b.state().timeshift.is_none());
    }

    #[test]
    fn seeking_vod_clamps_to_the_duration() {
        let mut b = NullBackend::default();
        b.load("https://example.com/m.mkv", &LoadOptions::default())
            .unwrap();
        b.seek(999_999.0, false).unwrap();
        assert_eq!(b.state().position_secs, 5400.0, "clamps to the end");

        b.seek(-600.0, true).unwrap();
        assert_eq!(
            b.state().position_secs,
            4800.0,
            "relative seek is applied, not clamped"
        );

        b.seek(-999_999.0, true).unwrap();
        assert_eq!(b.state().position_secs, 0.0, "clamps to the start");
    }
}
