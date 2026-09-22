//! Episode-marker detection and flat-list → show/season/episode folding.
//!
//! README §4.2: "detect `Show Name S01E02` patterns across hundreds of flat entries and fold
//! them into proper show → season → episode hierarchies."

use std::collections::BTreeMap;
use std::sync::OnceLock;

use regex::Regex;

use crate::model::{EpisodeRef, PlaylistEntry, Season, SeriesGroup};
use crate::title;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpisodeMarker {
    pub season: u16,
    pub episode: u16,
    /// Byte range of the marker within the input, so callers can split show title from
    /// episode title.
    pub start: usize,
    pub end: usize,
}

fn re_sxxexx() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // S01E02 / s1e2 / S01 E02 / S01.E02
    R.get_or_init(|| Regex::new(r"(?i)\bs(\d{1,3})[\s\._\-]?e(\d{1,4})\b").unwrap())
}

fn re_nxnn() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // 1x02
    R.get_or_init(|| Regex::new(r"(?i)\b(\d{1,3})x(\d{1,4})\b").unwrap())
}

fn re_worded() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // Season 1 Episode 2 / Saison 1 Episode 2 / Temporada 1 Capitulo 2
    R.get_or_init(|| {
        Regex::new(
            r"(?i)\b(?:season|saison|temporada|staffel)\s*(\d{1,3})\s*[\-\.,:]?\s*(?:episode|episodio|cap[ií]tulo|folge|ep)\s*(\d{1,4})\b",
        )
        .unwrap()
    })
}

/// Find an episode marker anywhere in a title. Returns the first match by pattern priority.
pub fn parse_episode_marker(name: &str) -> Option<EpisodeMarker> {
    for re in [re_sxxexx(), re_worded(), re_nxnn()] {
        if let Some(c) = re.captures(name) {
            let whole = c.get(0)?;
            let season = c.get(1)?.as_str().parse().ok()?;
            let episode = c.get(2)?.as_str().parse().ok()?;
            return Some(EpisodeMarker {
                season,
                episode,
                start: whole.start(),
                end: whole.end(),
            });
        }
    }
    None
}

/// Split `"Breaking Bad S01E02 Cat's in the Bag"` into
/// `("Breaking Bad", Some("Cat's in the Bag"), marker)`.
pub fn split_show_and_episode(name: &str) -> Option<(String, Option<String>, EpisodeMarker)> {
    let m = parse_episode_marker(name)?;
    let show = tidy(&name[..m.start]);
    let ep_title = tidy(&name[m.end..]);
    if show.is_empty() {
        return None;
    }
    Some((
        show,
        if ep_title.is_empty() {
            None
        } else {
            Some(ep_title)
        },
        m,
    ))
}

fn tidy(s: &str) -> String {
    s.trim()
        .trim_matches(|c: char| c == '-' || c == '|' || c == ':' || c == '.' || c == '_')
        .trim()
        .to_string()
}

/// Fold a flat list of episode entries into shows.
///
/// Entries without a recognizable marker are returned in the second tuple slot so the caller
/// can decide what to do with them rather than having them silently vanish.
pub fn group_series(entries: &[PlaylistEntry]) -> (Vec<SeriesGroup>, Vec<usize>) {
    /// What we accumulate per show while scanning the flat entry list.
    struct Accum {
        title: String,
        year: Option<i32>,
        group: Option<String>,
        quality: Option<String>,
        seasons: BTreeMap<u16, Vec<EpisodeRef>>,
    }

    let mut shows: BTreeMap<String, Accum> = BTreeMap::new();
    let mut ungrouped = Vec::new();

    for (idx, entry) in entries.iter().enumerate() {
        let Some((show_raw, ep_title, marker)) = split_show_and_episode(&entry.name) else {
            ungrouped.push(idx);
            continue;
        };

        let cleaned = title::clean_movie_title(&show_raw);
        let key = title::match_key(&cleaned.title);
        if key.is_empty() {
            ungrouped.push(idx);
            continue;
        }

        let slot = shows.entry(key).or_insert_with(|| Accum {
            title: cleaned.title.clone(),
            year: cleaned.year,
            group: entry.group.clone(),
            quality: None,
            seasons: BTreeMap::new(),
        });

        // Prefer the longest observed spelling of the show name — provider entries vary.
        if cleaned.title.len() > slot.title.len() {
            slot.title.clone_from(&cleaned.title);
        }
        if slot.year.is_none() {
            slot.year = cleaned.year;
        }
        if slot.group.is_none() {
            slot.group.clone_from(&entry.group);
        }
        // A show is only as good as its best episode stream.
        let found = cleaned
            .quality
            .clone()
            .or_else(|| title::detect_quality(&entry.name));
        if title::quality_rank(found.as_deref()) > title::quality_rank(slot.quality.as_deref()) {
            slot.quality = found;
        }

        let season = slot.seasons.entry(marker.season).or_default();
        if !season.iter().any(|e| e.number == marker.episode) {
            season.push(EpisodeRef {
                number: marker.episode,
                title: ep_title,
                url: entry.url.clone(),
                logo: entry.logo.clone(),
            });
        }
    }

    let groups = shows
        .into_values()
        .map(|accum| SeriesGroup {
            title: accum.title,
            year: accum.year,
            group: accum.group,
            quality: accum.quality,
            seasons: accum
                .seasons
                .into_iter()
                .map(|(number, mut episodes)| {
                    episodes.sort_by_key(|e| e.number);
                    Season { number, episodes }
                })
                .collect(),
        })
        .collect();

    (groups, ungrouped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(name: &str, url: &str) -> PlaylistEntry {
        PlaylistEntry::new(name, url)
    }

    #[test]
    fn detects_standard_markers() {
        assert_eq!(parse_episode_marker("Show S01E02").unwrap().season, 1);
        assert_eq!(parse_episode_marker("Show s1e2").unwrap().episode, 2);
        assert_eq!(parse_episode_marker("Show 1x02").unwrap().episode, 2);
        assert_eq!(
            parse_episode_marker("Show Season 2 Episode 10")
                .unwrap()
                .episode,
            10
        );
        assert_eq!(parse_episode_marker("Show S01 E05").unwrap().episode, 5);
    }

    #[test]
    fn ignores_names_without_markers() {
        assert!(parse_episode_marker("Inception 2010").is_none());
        assert!(parse_episode_marker("Channel 4 HD").is_none());
    }

    #[test]
    fn resolution_is_not_mistaken_for_an_episode() {
        // "1080" must not parse as 10x80 — the \b guard plus digit limits handle this.
        let m = parse_episode_marker("Movie 1080p");
        assert!(m.is_none(), "unexpected marker: {m:?}");
    }

    #[test]
    fn splits_show_from_episode_title() {
        let (show, ep_title, _) =
            split_show_and_episode("Breaking Bad S01E02 - Cat's in the Bag").unwrap();
        assert_eq!(show, "Breaking Bad");
        assert_eq!(ep_title.as_deref(), Some("Cat's in the Bag"));
    }

    #[test]
    fn groups_a_flat_list_into_shows() {
        let entries = vec![
            ep("Breaking Bad S01E01", "https://example.com/1.mkv"),
            ep("Breaking Bad S01E02", "https://example.com/2.mkv"),
            ep("Breaking Bad S02E01", "https://example.com/3.mkv"),
            ep("The Wire S01E01", "https://example.com/4.mkv"),
        ];
        let (groups, ungrouped) = group_series(&entries);
        assert!(ungrouped.is_empty());
        assert_eq!(groups.len(), 2);

        let bb = groups.iter().find(|g| g.title == "Breaking Bad").unwrap();
        assert_eq!(bb.seasons.len(), 2);
        assert_eq!(bb.seasons[0].number, 1);
        assert_eq!(bb.seasons[0].episodes.len(), 2);
        assert_eq!(bb.seasons[1].episodes.len(), 1);
    }

    #[test]
    fn collapses_naming_variants_of_the_same_show() {
        let entries = vec![
            ep("Breaking Bad S01E01 1080p", "https://example.com/1.mkv"),
            ep("BREAKING BAD s01e02", "https://example.com/2.mkv"),
            ep("Breaking.Bad.S01E03.WEB-DL", "https://example.com/3.mkv"),
        ];
        let (groups, _) = group_series(&entries);
        assert_eq!(groups.len(), 1, "variants should collapse into one show");
        assert_eq!(groups[0].seasons[0].episodes.len(), 3);
    }

    #[test]
    fn episodes_are_sorted_and_deduplicated() {
        let entries = vec![
            ep("Show S01E03", "https://example.com/3.mkv"),
            ep("Show S01E01", "https://example.com/1.mkv"),
            ep("Show S01E03", "https://example.com/3b.mkv"),
        ];
        let (groups, _) = group_series(&entries);
        let nums: Vec<u16> = groups[0].seasons[0]
            .episodes
            .iter()
            .map(|e| e.number)
            .collect();
        assert_eq!(nums, vec![1, 3]);
    }

    #[test]
    fn unmarked_entries_are_reported_not_dropped() {
        let entries = vec![
            ep("Show S01E01", "https://example.com/1.mkv"),
            ep("Just A Movie", "https://example.com/2.mkv"),
        ];
        let (groups, ungrouped) = group_series(&entries);
        assert_eq!(groups.len(), 1);
        assert_eq!(ungrouped, vec![1]);
    }

    #[test]
    fn handles_specials_as_season_zero() {
        let entries = vec![ep("Show S00E01 Special", "https://example.com/1.mkv")];
        let (groups, _) = group_series(&entries);
        assert_eq!(groups[0].seasons[0].number, 0);
    }
}
