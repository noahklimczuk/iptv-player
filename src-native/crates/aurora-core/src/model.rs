//! Domain types shared across parsers, persistence, and the IPC boundary.

use serde::{Deserialize, Serialize};

/// What a playlist entry actually is. Providers rarely say, so this is inferred
/// by [`crate::classify`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaKind {
    Live,
    Movie,
    Episode,
}

/// Per-entry HTTP overrides. IPTV providers routinely require a specific
/// User-Agent or Referer; `#EXTVLCOPT` carries them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpOptions {
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
    pub origin: Option<String>,
}

impl HttpOptions {
    pub fn is_empty(&self) -> bool {
        self.user_agent.is_none() && self.referrer.is_none() && self.origin.is_none()
    }
}

/// Catch-up / archive capability advertised by the playlist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catchup {
    /// `default`, `append`, `shift`, `flussonic`, `xc`, …
    pub mode: String,
    pub source: Option<String>,
    pub days: u16,
}

/// One raw entry as it appeared in a playlist, before library reconciliation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    pub name: String,
    pub url: String,
    pub kind: MediaKind,
    pub tvg_id: Option<String>,
    pub tvg_name: Option<String>,
    pub logo: Option<String>,
    pub group: Option<String>,
    pub number: Option<u32>,
    /// Minutes to shift EPG data for this channel (`tvg-shift`).
    pub shift_minutes: i32,
    pub language: Option<String>,
    pub country: Option<String>,
    pub is_radio: bool,
    pub catchup: Option<Catchup>,
    pub http: HttpOptions,
    /// What the provider scores this out of ten, where it says so.
    ///
    /// Kept because it is the only rating most libraries will ever have. TMDB
    /// enrichment needs an API key a viewer may never set, and the panel in
    /// docs/ROADMAP.md sends one of these for 109,999 of its 122,499 films — all of
    /// which used to be parsed and then dropped on the floor.
    pub rating: Option<f32>,
    /// When the provider says this was added, in unix seconds.
    ///
    /// Without it every row imported in the same pass carries the same timestamp, and
    /// Browse's default "Recently added" sort is ordering by a constant.
    pub added_at: Option<i64>,
    /// Source line number, for diagnostics on malformed playlists.
    pub source_line: usize,
}

impl PlaylistEntry {
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            kind: MediaKind::Live,
            tvg_id: None,
            tvg_name: None,
            logo: None,
            group: None,
            number: None,
            shift_minutes: 0,
            language: None,
            country: None,
            is_radio: false,
            catchup: None,
            http: HttpOptions::default(),
            rating: None,
            added_at: None,
            source_line: 0,
        }
    }
}

/// A parse problem that did not justify discarding the whole playlist.
/// README §4.2: "Parse what you can, log what you can't, never crash."
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseWarning {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistParseResult {
    pub entries: Vec<PlaylistEntry>,
    pub warnings: Vec<ParseWarning>,
    /// Entries seen but skipped (duplicates, unusable URLs).
    pub skipped: usize,
}

/// An EPG programme, timezone-normalized to UTC at parse time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Programme {
    pub channel_id: String,
    /// Unix seconds, UTC.
    pub start: i64,
    pub stop: i64,
    pub title: String,
    pub sub_title: Option<String>,
    pub description: Option<String>,
    pub categories: Vec<String>,
    pub season: Option<u16>,
    pub episode: Option<u16>,
    pub icon: Option<String>,
    pub rating: Option<String>,
    pub star_rating: Option<f32>,
    pub is_new: bool,
    pub is_live: bool,
    pub is_premiere: bool,
    pub credits: Vec<Credit>,
}

impl Programme {
    pub fn duration_secs(&self) -> i64 {
        (self.stop - self.start).max(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credit {
    pub role: String,
    pub name: String,
}

/// A channel as declared by an XMLTV document (not by the playlist).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpgChannel {
    pub id: String,
    pub display_names: Vec<String>,
    pub icon: Option<String>,
}

/// A show assembled from flat playlist episode entries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesGroup {
    pub title: String,
    pub year: Option<i32>,
    /// The provider's category for this show, which is also where a language tag often
    /// hides (README §7.3).
    pub group: Option<String>,
    /// The best quality any of its episode entries advertised, so the same show listed
    /// twice at two qualities collapses onto the better one.
    pub quality: Option<String>,
    pub seasons: Vec<Season>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Season {
    pub number: u16,
    pub episodes: Vec<EpisodeRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeRef {
    pub number: u16,
    pub title: Option<String>,
    pub url: String,
    pub logo: Option<String>,
}
