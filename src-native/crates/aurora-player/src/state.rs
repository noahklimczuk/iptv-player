use serde::{Deserialize, Serialize};

use crate::error::PlaybackError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlayerStatus {
    #[default]
    Idle,
    Loading,
    Buffering,
    Playing,
    Paused,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Aspect {
    #[default]
    Auto,
    #[serde(rename = "16:9")]
    Wide,
    #[serde(rename = "4:3")]
    Classic,
    #[serde(rename = "21:9")]
    Ultra,
    Stretch,
    Zoom,
}

impl Aspect {
    /// The value mpv's `video-aspect-override` expects. `None` means "leave alone".
    pub fn mpv_value(self) -> Option<&'static str> {
        match self {
            Aspect::Auto => Some("-1"),
            Aspect::Wide => Some("16:9"),
            Aspect::Classic => Some("4:3"),
            Aspect::Ultra => Some("21:9"),
            Aspect::Stretch | Aspect::Zoom => None, // handled via panscan
        }
    }

    pub fn panscan(self) -> f64 {
        match self {
            Aspect::Zoom | Aspect::Stretch => 1.0,
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrackKind {
    Audio,
    Subtitle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: i64,
    pub kind: TrackKind,
    pub title: Option<String>,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub channels: Option<String>,
    pub default: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackStats {
    pub resolution: Option<String>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub fps: Option<f64>,
    pub bitrate_kbps: Option<u32>,
    pub dropped_frames: u64,
    pub buffer_secs: f64,
    pub hw_decoder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub status: PlayerStatus,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub channel_id: Option<i64>,
    pub item_id: Option<i64>,
    pub position_secs: f64,
    pub duration_secs: f64,
    pub is_live: bool,
    pub volume: u32,
    pub muted: bool,
    pub speed: f64,
    pub audio_tracks: Vec<Track>,
    pub subtitle_tracks: Vec<Track>,
    pub active_audio_track: Option<i64>,
    pub active_subtitle_track: Option<i64>,
    pub aspect: Aspect,
    pub error: Option<PlaybackError>,
    pub stats: Option<PlaybackStats>,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            status: PlayerStatus::Idle,
            title: None,
            subtitle: None,
            channel_id: None,
            item_id: None,
            position_secs: 0.0,
            duration_secs: 0.0,
            is_live: false,
            volume: 70,
            muted: false,
            speed: 1.0,
            audio_tracks: Vec::new(),
            subtitle_tracks: Vec::new(),
            active_audio_track: None,
            active_subtitle_track: None,
            aspect: Aspect::Auto,
            error: None,
            stats: None,
        }
    }
}

impl PlayerState {
    /// README §6.3: movies count as watched at 92%, episodes at 95%.
    pub fn progress_ratio(&self) -> f64 {
        if self.duration_secs <= 0.0 {
            0.0
        } else {
            (self.position_secs / self.duration_secs).clamp(0.0, 1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_streams_never_divide_by_zero() {
        let s = PlayerState {
            is_live: true,
            ..Default::default()
        };
        assert_eq!(s.progress_ratio(), 0.0);
    }

    #[test]
    fn progress_is_clamped() {
        let s = PlayerState {
            position_secs: 500.0,
            duration_secs: 100.0,
            ..Default::default()
        };
        assert_eq!(s.progress_ratio(), 1.0);
    }

    #[test]
    fn aspect_maps_to_mpv_values() {
        assert_eq!(Aspect::Wide.mpv_value(), Some("16:9"));
        assert_eq!(Aspect::Auto.mpv_value(), Some("-1"));
        assert_eq!(Aspect::Zoom.mpv_value(), None);
        assert_eq!(Aspect::Zoom.panscan(), 1.0);
        assert_eq!(Aspect::Wide.panscan(), 0.0);
    }

    #[test]
    fn aspect_serializes_with_the_names_the_ui_uses() {
        assert_eq!(serde_json::to_string(&Aspect::Wide).unwrap(), "\"16:9\"");
        assert_eq!(serde_json::to_string(&Aspect::Auto).unwrap(), "\"auto\"");
    }
}
