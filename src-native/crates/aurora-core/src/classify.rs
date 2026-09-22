//! Live / movie / episode classification.
//!
//! README §4.2: providers do not tell you what an entry is. Infer from URL shape, group title,
//! and name pattern, in that order of reliability.

use crate::model::MediaKind;
use crate::series;

const MOVIE_GROUP_HINTS: &[&str] = &[
    "vod",
    "movie",
    "movies",
    "film",
    "films",
    "cinema",
    "peliculas",
    "filme",
    "kino",
];
const SERIES_GROUP_HINTS: &[&str] = &[
    "series",
    "serie",
    "tv show",
    "tvshow",
    "shows",
    "staffel",
    "temporada",
    "saison",
];
const VIDEO_EXTS: &[&str] = &[
    ".mkv", ".mp4", ".avi", ".mov", ".m4v", ".flv", ".wmv", ".mpg",
];

pub fn classify(name: &str, url: &str, group: Option<&str>) -> MediaKind {
    let lower_url = url.to_ascii_lowercase();

    // 1. Xtream path segments are authoritative when present.
    if lower_url.contains("/series/") {
        return MediaKind::Episode;
    }
    if lower_url.contains("/movie/") || lower_url.contains("/movies/") {
        return MediaKind::Movie;
    }
    if lower_url.contains("/live/") {
        return MediaKind::Live;
    }

    // 2. An SxxEyy in the name means episode regardless of anything else.
    if series::parse_episode_marker(name).is_some() {
        return MediaKind::Episode;
    }

    // 3. Group hints.
    if let Some(g) = group {
        let g = g.to_ascii_lowercase();
        if SERIES_GROUP_HINTS.iter().any(|h| g.contains(h)) {
            return MediaKind::Episode;
        }
        if MOVIE_GROUP_HINTS.iter().any(|h| g.contains(h)) {
            return MediaKind::Movie;
        }
    }

    // 4. A file extension implies on-demand content, not a live feed.
    let path = lower_url.split(['?', '#']).next().unwrap_or(&lower_url);
    if VIDEO_EXTS.iter().any(|e| path.ends_with(e)) {
        return MediaKind::Movie;
    }

    MediaKind::Live
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_segments_win() {
        assert_eq!(
            classify("Anything", "https://example.com/series/u/p/1.mkv", None),
            MediaKind::Episode
        );
        assert_eq!(
            classify("Anything", "https://example.com/movie/u/p/1.mkv", None),
            MediaKind::Movie
        );
        assert_eq!(
            classify("Anything", "https://example.com/live/u/p/1.ts", Some("VOD")),
            MediaKind::Live
        );
    }

    #[test]
    fn episode_marker_beats_group() {
        assert_eq!(
            classify(
                "Breaking Bad S01E02",
                "https://example.com/x.mkv",
                Some("Movies")
            ),
            MediaKind::Episode
        );
    }

    #[test]
    fn group_hints_apply() {
        assert_eq!(
            classify("Some Film", "https://example.com/x", Some("VOD | Action")),
            MediaKind::Movie
        );
        assert_eq!(
            classify("Some Show", "https://example.com/x", Some("Series | Drama")),
            MediaKind::Episode
        );
    }

    #[test]
    fn extension_implies_vod() {
        assert_eq!(
            classify("Movie", "https://example.com/a/b.mkv", None),
            MediaKind::Movie
        );
    }

    #[test]
    fn ts_streams_default_to_live() {
        assert_eq!(
            classify("BBC One", "https://example.com/a/b.ts", None),
            MediaKind::Live
        );
        assert_eq!(
            classify("BBC One", "https://example.com/a/b.m3u8", None),
            MediaKind::Live
        );
    }

    #[test]
    fn query_strings_do_not_defeat_extension_matching() {
        assert_eq!(
            classify("Film", "https://example.com/a/b.mp4?token=abc", None),
            MediaKind::Movie
        );
    }
}
