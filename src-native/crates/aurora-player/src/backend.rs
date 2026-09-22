//! The playback seam.

use std::collections::HashMap;

use aurora_core::markers::Chapter;

use crate::error::PlayerError;
use crate::state::{Aspect, PlayerState, PlayerStatus};

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
}

/// A backend that models state without decoding anything. Used on non-Windows hosts
/// and in tests, so every command path is exercisable without a GPU or a network.
#[derive(Debug, Default)]
pub struct NullBackend {
    state: PlayerState,
    chapters: Vec<Chapter>,
}

impl NullBackend {
    /// Stand in for a file's chapter list, so marker handling can be exercised
    /// without a decoder.
    pub fn set_chapters(&mut self, chapters: Vec<Chapter>) {
        self.chapters = chapters;
    }
}

impl PlayerBackend for NullBackend {
    fn load(&mut self, url: &str, options: &LoadOptions) -> Result<(), PlayerError> {
        if url.trim().is_empty() {
            return Err(PlayerError::Command("empty URL".into()));
        }
        self.chapters.clear();
        self.state = PlayerState {
            status: PlayerStatus::Playing,
            title: options.title.clone(),
            is_live: options.is_live,
            position_secs: options.start_at_secs.unwrap_or(0.0),
            duration_secs: if options.is_live { 0.0 } else { 5400.0 },
            volume: self.state.volume,
            muted: self.state.muted,
            ..Default::default()
        };
        Ok(())
    }

    fn stop(&mut self) -> Result<(), PlayerError> {
        let (volume, muted) = (self.state.volume, self.state.muted);
        self.state = PlayerState {
            volume,
            muted,
            ..Default::default()
        };
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
