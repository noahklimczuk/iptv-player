//! What the engine reports: the values the controls draw from.
//!
//! Field names serialise to what `shared/ipc.ts` declares, so the host can hand a
//! [`Report`] to the page without translating it.

use aurora_core::neterr::NetFailure;
use aurora_core::timeshift::Window;
use serde::{Deserialize, Serialize};

/// Where playback is. `docs/PLAYER.md` E13.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// Nothing loaded.
    #[default]
    Idle,
    /// A file was asked for and has not started.
    Loading,
    /// Started, and the playhead is waiting for data.
    Buffering,
    Playing,
    Paused,
    /// It stopped and could not go on. [`Report::error`] says why.
    Error,
}

impl Status {
    /// Whether something is loaded, in any state short of failing.
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Loading | Self::Buffering | Self::Playing | Self::Paused
        )
    }
}

/// How the picture fills the window. E17.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Aspect {
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "16:9")]
    Wide,
    #[serde(rename = "4:3")]
    Standard,
    #[serde(rename = "21:9")]
    Cinema,
    #[serde(rename = "stretch")]
    Stretch,
    #[serde(rename = "zoom")]
    Zoom,
}

impl Aspect {
    /// The mpv properties that produce this shape, all of them every time, so changing
    /// from Zoom to 4:3 does not leave the zoom in force.
    ///
    /// `video-aspect-override` of `-1` is mpv's "the file's own aspect"; `keepaspect` off
    /// is what stretches; `panscan` of 1 crops to fill.
    pub fn mpv(self) -> [(&'static str, &'static str); 3] {
        let (aspect, keep, panscan) = match self {
            Self::Auto => ("-1", "yes", "0"),
            Self::Wide => ("16:9", "yes", "0"),
            Self::Standard => ("4:3", "yes", "0"),
            Self::Cinema => ("2.33", "yes", "0"),
            Self::Stretch => ("-1", "no", "0"),
            Self::Zoom => ("-1", "yes", "1"),
        };
        [
            ("video-aspect-override", aspect),
            ("keepaspect", keep),
            ("panscan", panscan),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrackKind {
    Audio,
    Subtitle,
}

/// An audio or subtitle track. E16: it carries the language and the title, so the menu
/// can say "English (Director's commentary)" rather than "Track 2".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    /// mpv's id for it, which is what selecting it takes.
    pub id: i64,
    pub kind: TrackKind,
    pub title: Option<String>,
    pub language: Option<String>,
    pub codec: Option<String>,
    /// Audio only: "5.1", "stereo".
    pub channels: Option<String>,
    pub default: bool,
}

/// The stats overlay's figures. E18.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub resolution: Option<String>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub fps: Option<f64>,
    pub bitrate_kbps: Option<u64>,
    pub dropped_frames: u64,
    /// Seconds of data held ahead of the playhead.
    pub buffer_secs: f64,
    pub hw_decoder: Option<String>,
    /// Everything mpv's demuxer cache holds: the rewind history and the forward buffer.
    pub cache_bytes: u64,
    /// The size of the on-disk cache file, when there is one.
    pub cache_file_bytes: Option<u64>,
}

/// Everything the engine knows about what is playing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub status: Status,
    /// Seconds into the file; for live, in the stream's own timebase.
    pub position_secs: f64,
    /// Zero for live, which has no duration (E15).
    pub duration_secs: f64,
    pub is_live: bool,
    /// 0–200.
    pub volume: u32,
    pub muted: bool,
    pub speed: f64,
    pub audio_tracks: Vec<Track>,
    pub subtitle_tracks: Vec<Track>,
    pub active_audio_track: Option<i64>,
    pub active_subtitle_track: Option<i64>,
    pub aspect: Aspect,
    /// What can be rewound, on a buffered live stream (E21–E24). `None` otherwise.
    pub timeshift: Option<Window>,
    pub error: Option<NetFailure>,
    pub stats: Option<Stats>,
    /// mpv has drawn a frame of the current file (E14). Until then the window must stay
    /// opaque, or the viewer sees the desktop through it.
    pub has_picture: bool,
}

impl Default for Report {
    fn default() -> Self {
        Self {
            status: Status::Idle,
            position_secs: 0.0,
            duration_secs: 0.0,
            is_live: false,
            volume: 100,
            muted: false,
            speed: 1.0,
            audio_tracks: Vec::new(),
            subtitle_tracks: Vec::new(),
            active_audio_track: None,
            active_subtitle_track: None,
            aspect: Aspect::Auto,
            timeshift: None,
            error: None,
            stats: None,
            has_picture: false,
        }
    }
}

impl Report {
    /// How far through, 0–1. Never NaN, never outside the range, never a division by a
    /// live stream's zero duration (E15).
    pub fn progress(&self) -> f64 {
        if !self.duration_secs.is_finite() || self.duration_secs <= 0.0 {
            return 0.0;
        }
        let p = self.position_secs / self.duration_secs;
        if p.is_finite() {
            p.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// What survives from one file to the next: the viewer's own settings. Everything
    /// about the file itself starts again.
    pub fn carried_over(&self) -> Self {
        Self {
            volume: self.volume,
            muted: self.muted,
            aspect: self.aspect,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_live_stream_never_divides_by_zero() {
        let r = Report {
            position_secs: 1234.0,
            is_live: true,
            ..Report::default()
        };
        assert_eq!(r.progress(), 0.0);
    }

    #[test]
    fn progress_is_held_between_nothing_and_everything() {
        let mut r = Report {
            position_secs: 150.0,
            duration_secs: 100.0,
            ..Report::default()
        };
        assert_eq!(r.progress(), 1.0);
        r.position_secs = -5.0;
        assert_eq!(r.progress(), 0.0);
        r.position_secs = f64::NAN;
        assert_eq!(r.progress(), 0.0);
    }

    #[test]
    fn every_aspect_sets_all_three_properties() {
        for a in [
            Aspect::Auto,
            Aspect::Wide,
            Aspect::Standard,
            Aspect::Cinema,
            Aspect::Stretch,
            Aspect::Zoom,
        ] {
            let names: Vec<_> = a.mpv().iter().map(|(n, _)| *n).collect();
            assert_eq!(names, ["video-aspect-override", "keepaspect", "panscan"]);
        }
        assert_eq!(Aspect::Zoom.mpv()[2], ("panscan", "1"));
        assert_eq!(Aspect::Stretch.mpv()[1], ("keepaspect", "no"));
    }

    #[test]
    fn names_are_the_ones_the_page_reads() {
        let v = serde_json::to_value(Report::default()).unwrap();
        for key in [
            "status",
            "positionSecs",
            "durationSecs",
            "isLive",
            "volume",
            "muted",
            "speed",
            "audioTracks",
            "subtitleTracks",
            "activeAudioTrack",
            "activeSubtitleTrack",
            "aspect",
            "timeshift",
            "error",
            "stats",
            "hasPicture",
        ] {
            assert!(v.get(key).is_some(), "{key} missing");
        }
        assert_eq!(serde_json::to_value(Aspect::Wide).unwrap(), "16:9");
        assert_eq!(
            serde_json::to_value(Status::Buffering).unwrap(),
            "buffering"
        );
    }

    #[test]
    fn a_new_file_keeps_the_viewers_settings_and_nothing_else() {
        let r = Report {
            status: Status::Playing,
            volume: 40,
            muted: true,
            aspect: Aspect::Zoom,
            speed: 2.0,
            has_picture: true,
            position_secs: 99.0,
            ..Report::default()
        };
        let next = r.carried_over();
        assert_eq!(
            (next.volume, next.muted, next.aspect),
            (40, true, Aspect::Zoom)
        );
        assert_eq!(next.speed, 1.0);
        assert!(!next.has_picture);
        assert_eq!(next.position_secs, 0.0);
        assert_eq!(next.status, Status::Idle);
    }
}
