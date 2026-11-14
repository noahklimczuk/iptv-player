//! TheIntroDB: community-verified intro, recap and credits timestamps.
//!
//! Why it exists. Skip Intro and Skip Recap need evidence, and a provider's files rarely
//! carry it: chapters are the exception (MobLand has them; Riverdale, on the same panel,
//! does not), and learning from the viewer's own skips needs the viewer to have skipped
//! by hand first -- which they cannot do with a button that is not there. So on most of
//! a library the buttons never appeared at all.
//!
//! TheIntroDB answers by TMDB id, season and episode, which the library already holds
//! for nearly every show (8,091 of 8,487 on the library this was measured against).
//! Reads need no key. Only accepted submissions are returned without one.
//!
//! API: `GET https://api.theintrodb.org/v4/media?tmdb_id=&season=&episode=`, answering
//! `{"intro":[{"start_ms","end_ms"}], "recap":[…], "credits":[…], "preview":[…]}` in
//! milliseconds, each key omitted when there is nothing. `start_ms: null` means the
//! start of the file and `end_ms: null` the end. No data is a 404 with a `code`.
//! Spec: https://theintrodb.org/openapi.yaml.

use std::time::Duration;

use aurora_core::markers::{MarkerKind, MarkerSource, SkipMarker};
use serde::Deserialize;

use crate::http::{HttpClient, HttpConfig};

pub const BASE_URL: &str = "https://api.theintrodb.org/v4/media";

/// A client that will not hold up playback.
///
/// The shared client allows twelve seconds to connect and two minutes to read, with
/// retries, which is right for a 2 GB EPG and wrong for something asked as an episode
/// starts. One attempt, a few seconds, and the button simply does not appear if the
/// service is slow -- which is what happened before it existed.
pub fn client() -> Result<HttpClient, aurora_core::neterr::NetFailure> {
    HttpClient::new(HttpConfig {
        connect_timeout: Duration::from_secs(4),
        read_timeout: Duration::from_secs(6),
        max_attempts: 1,
        max_bytes: 256 * 1024,
        ..HttpConfig::default()
    })
}

pub fn url(tmdb_id: i64, season: u16, episode: u16) -> String {
    format!("{BASE_URL}?tmdb_id={tmdb_id}&season={season}&episode={episode}")
}

#[derive(Debug, Default, Deserialize)]
struct Media {
    #[serde(default)]
    intro: Vec<Segment>,
    #[serde(default)]
    recap: Vec<Segment>,
    #[serde(default)]
    credits: Vec<Segment>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct Segment {
    start_ms: Option<f64>,
    end_ms: Option<f64>,
}

impl Segment {
    /// In seconds, with the open ends filled in. `None` when an end is open and the
    /// runtime is not known to close it.
    fn bounds(self, duration_secs: Option<f64>) -> Option<(f64, f64)> {
        let start = self.start_ms.unwrap_or(0.0) / 1000.0;
        let end = match self.end_ms {
            Some(ms) => ms / 1000.0,
            None => duration_secs?,
        };
        (start.is_finite() && end.is_finite()).then_some((start, end))
    }
}

/// The markers one answer describes.
///
/// One per kind, because that is what the rest of the pipeline holds. Where an episode
/// has two intro segments and no recap -- MobLand S2E2 lists 0:11-1:06 and 4:14-5:12 --
/// the first, when it sits in the opening two minutes, is the "previously on" and is
/// offered as Skip Recap; the second is the titles. Anything implausible (TheIntroDB
/// carries `credits: 0-0` on some Riverdale episodes) is dropped by the same rules a
/// chapter is held to.
pub fn parse(body: &str, duration_secs: Option<f64>) -> Vec<SkipMarker> {
    let Ok(media) = serde_json::from_str::<Media>(body) else {
        return Vec::new();
    };
    let make = |kind, seg: &Segment| {
        seg.bounds(duration_secs)
            .map(|(s, e)| SkipMarker::new(kind, s, e, MarkerSource::Community))
            .filter(|m| m.is_plausible())
            .filter(|m| aurora_core::markers::position_fits_kind(m, duration_secs.unwrap_or(0.0)))
    };

    let mut out = Vec::new();
    let mut intros: Vec<SkipMarker> = media
        .intro
        .iter()
        .filter_map(|s| make(MarkerKind::Intro, s))
        .collect();
    intros.sort_by(|a, b| a.start_secs.total_cmp(&b.start_secs));

    let recap = media.recap.iter().find_map(|s| make(MarkerKind::Recap, s));
    match recap {
        Some(r) => out.push(r),
        None if intros.len() > 1 && intros[0].start_secs < 120.0 => {
            let first = intros.remove(0);
            out.push(SkipMarker {
                kind: MarkerKind::Recap,
                ..first
            });
        }
        None => {}
    }
    if let Some(intro) = intros.into_iter().next() {
        out.push(intro);
    }
    if let Some(credits) = media
        .credits
        .iter()
        .find_map(|s| make(MarkerKind::Credits, s))
    {
        out.push(credits);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MobLand S1E9, as the service answered it on 2026-11-14.
    const MOBLAND_S1E9: &str = r#"{"tvdb_id":446831,"type":"tv","season":1,"episode":9,
        "intro":[{"start_ms":438912,"end_ms":489451}],
        "recap":[{"start_ms":0,"end_ms":65000}],
        "credits":[{"start_ms":2815476,"end_ms":null}]}"#;

    #[test]
    fn an_answer_becomes_markers_in_seconds() {
        let m = parse(MOBLAND_S1E9, Some(2858.0));
        let get = |k| m.iter().find(|x| x.kind == k).copied().unwrap();
        let intro = get(MarkerKind::Intro);
        assert!((intro.start_secs - 438.912).abs() < 0.01);
        assert!((intro.end_secs - 489.451).abs() < 0.01);
        assert_eq!(get(MarkerKind::Recap).start_secs, 0.0);
        // An open end is the end of the file.
        assert_eq!(get(MarkerKind::Credits).end_secs, 2858.0);
        assert!(m.iter().all(|x| x.source == MarkerSource::Community));
    }

    #[test]
    fn an_open_ended_credits_segment_needs_a_runtime() {
        let m = parse(MOBLAND_S1E9, None);
        assert!(m.iter().all(|x| x.kind != MarkerKind::Credits));
        assert!(m.iter().any(|x| x.kind == MarkerKind::Intro));
    }

    /// Riverdale S2E5: a real recap beside a junk `credits: 0-0`.
    #[test]
    fn junk_segments_are_dropped() {
        let body = r#"{"recap":[{"start_ms":0,"end_ms":35995}],
                       "credits":[{"start_ms":0,"end_ms":0}]}"#;
        let m = parse(body, Some(2550.0));
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].kind, MarkerKind::Recap);
    }

    /// MobLand S2E2: two intro segments and no recap.
    #[test]
    fn an_opening_segment_before_the_titles_is_the_recap() {
        let body = r#"{"intro":[{"start_ms":11623,"end_ms":66480},
                                {"start_ms":254466,"end_ms":312266}]}"#;
        let m = parse(body, Some(2736.0));
        let recap = m.iter().find(|x| x.kind == MarkerKind::Recap).unwrap();
        let intro = m.iter().find(|x| x.kind == MarkerKind::Intro).unwrap();
        assert!((recap.start_secs - 11.623).abs() < 0.01);
        assert!((intro.start_secs - 254.466).abs() < 0.01);
    }

    #[test]
    fn an_unreadable_body_is_no_markers() {
        assert!(parse("<html>", Some(100.0)).is_empty());
        assert!(parse(r#"{"error":"x","code":"no_submissions"}"#, Some(100.0)).is_empty());
    }
}
