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

/// Is this the bare Xtream live address, `http://host/<user>/<pass>/<id>`?
///
/// Three or more path segments ending in a segment that is nothing but digits, with no
/// extension on it. That shape is how a panel addresses a *live* stream: on-demand
/// content carries the container extension the provider declared, and the catch-up and
/// VOD forms carry a `/live/`, `/movie/` or `/series/` segment that is checked first.
fn is_bare_stream_id(path: &str) -> bool {
    let after_host = match path.find("://") {
        Some(i) => match path[i + 3..].find('/') {
            Some(j) => &path[i + 3 + j + 1..],
            // A host and nothing else addresses no stream at all.
            None => return false,
        },
        None => path,
    };
    let segments: Vec<&str> = after_host.split('/').filter(|s| !s.is_empty()).collect();
    segments.len() >= 3
        && segments
            .last()
            .is_some_and(|last| !last.is_empty() && last.bytes().all(|b| b.is_ascii_digit()))
}

pub fn classify(name: &str, url: &str, group: Option<&str>) -> MediaKind {
    let lower_url = url.to_ascii_lowercase();
    let path = lower_url
        .split(['?', '#'])
        .next()
        .unwrap_or(&lower_url)
        .to_string();

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

    // 3. A bare stream id is a live stream, and the URL outranks a word in a group name.
    //
    // This rule used to be absent and the group title decided instead, which filed 659
    // of this subscription's 6,698 live channels as films and episodes -- 520 from a
    // group called "CINEMANIA", 103 from "MOVIES", the rest from "PEACOCK SERIES",
    // "DISNEY+ SERIES" and "APPLE TV+ SERIES". They are 24/7 channels: live streams
    // that happen to play films. Filed as movies they turned up in the Movies library,
    // where the player offers a scrubber and a resume position for something that has
    // neither, and they were missing from Live TV where they belong. Every live URL on
    // this panel has this shape, because none of them carries `/live/` or an extension
    // -- so rule 1 never fires here and this is the one that has to (R-01).
    if is_bare_stream_id(&path) {
        return MediaKind::Live;
    }

    // 4. Group hints, now that the URL has been given its say.
    if let Some(g) = group {
        let g = g.to_ascii_lowercase();
        if SERIES_GROUP_HINTS.iter().any(|h| g.contains(h)) {
            return MediaKind::Episode;
        }
        if MOVIE_GROUP_HINTS.iter().any(|h| g.contains(h)) {
            return MediaKind::Movie;
        }
    }

    // 5. A file extension implies on-demand content, not a live feed.
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

    /// The 24/7 channels this subscription files under film and show groups.
    ///
    /// 660 of its 6,699 live streams, measured by replaying the classifier over the real
    /// 50.2 MB playlist: 521 in "CINEMANIA", 103 in "MOVIES", and the rest in "PEACOCK
    /// SERIES", "DISNEY+ SERIES" and "APPLE TV+ SERIES". They are live streams that
    /// happen to play films, and as movies they appeared in the Movies library -- where
    /// the player offers a scrubber and a resume position for something with neither --
    /// and were missing from Live TV (R-01).
    #[test]
    fn a_bare_stream_id_is_live_whatever_its_group_is_called() {
        for group in [
            "US CINEMANIA",
            "US MOVIES",
            "US PEACOCK SERIES",
            "US DISNEY+ SERIES",
            "US APPLE TV+ SERIES",
        ] {
            assert_eq!(
                classify("A 24/7 Channel", "http://host/user/pass/12345", Some(group)),
                MediaKind::Live,
                "{group}"
            );
        }
    }

    /// The group hint still decides when the URL says nothing either way, which is the
    /// only job it was ever meant to do.
    #[test]
    fn a_group_hint_still_decides_when_the_url_is_silent() {
        assert_eq!(
            classify(
                "Some Film",
                "http://host/stream.php?id=900",
                Some("VOD Movies")
            ),
            MediaKind::Movie
        );
        assert_eq!(
            classify("Some Show", "http://host/play/abc", Some("Series")),
            MediaKind::Episode
        );
    }

    /// On-demand content carries the extension the provider declared, so it is not the
    /// bare form and nothing here changes for it.
    #[test]
    fn on_demand_urls_are_not_mistaken_for_live() {
        assert_eq!(
            classify(
                "Inception (2010)",
                "http://host/user/pass/900.mkv",
                Some("MOVIES")
            ),
            MediaKind::Movie
        );
        // A path with too few segments to be `<user>/<pass>/<id>`.
        assert_eq!(
            classify("Something", "http://host/12345", Some("Movies")),
            MediaKind::Movie
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
