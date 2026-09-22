//! EPG channel matching.
//!
//! README §4.4: "Channel matching is the hard part." Exact `tvg-id` first, then a normalized
//! fuzzy name match, then whatever the user mapped by hand. Produces a coverage report so the
//! UI can say "2,847 of 3,102 channels have EPG data" and list the rest.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::EpgChannel;
use crate::title;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchMethod {
    /// User pinned this mapping by hand; never overridden by a refresh.
    Manual,
    /// `tvg-id` equals an XMLTV channel id.
    ExactId,
    /// Normalized channel name equals a normalized XMLTV display name.
    NormalizedName,
    /// Normalized names differ only by a short edit distance.
    Fuzzy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Match {
    pub epg_channel_id: String,
    pub method: MatchMethod,
    /// 0-100. Exact matches are 100.
    pub confidence: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub total: usize,
    pub matched: usize,
    /// Names of channels with no EPG, for the "fix these" list in Settings.
    pub unmatched: Vec<String>,
}

impl Coverage {
    pub fn percent(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.matched as f32 / self.total as f32) * 100.0
        }
    }
}

/// Index of EPG channels, built once per import and reused for every playlist channel.
pub struct EpgIndex {
    by_id: HashMap<String, String>,
    by_key: HashMap<String, String>,
    /// Keys kept in a flat list for the fuzzy pass.
    keys: Vec<(String, String)>,
}

impl EpgIndex {
    pub fn build(channels: &[EpgChannel]) -> Self {
        let mut by_id = HashMap::with_capacity(channels.len());
        let mut by_key = HashMap::with_capacity(channels.len() * 2);
        let mut keys = Vec::with_capacity(channels.len());

        for c in channels {
            by_id.insert(c.id.to_ascii_lowercase(), c.id.clone());

            // The id itself is often a usable name ("cnn.us").
            let id_key = title::match_key(&c.id);
            if !id_key.is_empty() {
                by_key.entry(id_key).or_insert_with(|| c.id.clone());
            }
            for name in &c.display_names {
                let k = title::match_key(name);
                if k.is_empty() {
                    continue;
                }
                by_key.entry(k.clone()).or_insert_with(|| c.id.clone());
                keys.push((k, c.id.clone()));
            }
        }
        Self {
            by_id,
            by_key,
            keys,
        }
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Resolve one playlist channel to an EPG channel.
    pub fn resolve(
        &self,
        tvg_id: Option<&str>,
        channel_name: &str,
        manual: Option<&str>,
    ) -> Option<Match> {
        if let Some(m) = manual {
            return Some(Match {
                epg_channel_id: m.to_string(),
                method: MatchMethod::Manual,
                confidence: 100,
            });
        }

        if let Some(id) = tvg_id.map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(hit) = self.by_id.get(&id.to_ascii_lowercase()) {
                return Some(Match {
                    epg_channel_id: hit.clone(),
                    method: MatchMethod::ExactId,
                    confidence: 100,
                });
            }
            // A tvg-id that is not a literal id is often still a name.
            let k = title::match_key(id);
            if let Some(hit) = self.by_key.get(&k) {
                return Some(Match {
                    epg_channel_id: hit.clone(),
                    method: MatchMethod::NormalizedName,
                    confidence: 90,
                });
            }
        }

        let key = title::match_key(channel_name);
        if key.is_empty() {
            return None;
        }
        if let Some(hit) = self.by_key.get(&key) {
            return Some(Match {
                epg_channel_id: hit.clone(),
                method: MatchMethod::NormalizedName,
                confidence: 85,
            });
        }

        // Fuzzy pass: only for keys of a similar length, and only for a distance of 1.
        // Anything looser produces confident nonsense on channel names like "Sky 1" / "Sky 2".
        if key.len() >= 5 {
            let mut best: Option<(usize, &str)> = None;
            for (k, id) in &self.keys {
                if k.len().abs_diff(key.len()) > 1 {
                    continue;
                }
                let d = edit_distance_at_most(&key, k, 1);
                if let Some(d) = d {
                    if best.as_ref().map(|(bd, _)| d < *bd).unwrap_or(true) {
                        best = Some((d, id));
                    }
                }
            }
            if let Some((_, id)) = best {
                return Some(Match {
                    epg_channel_id: id.to_string(),
                    method: MatchMethod::Fuzzy,
                    confidence: 60,
                });
            }
        }
        None
    }
}

/// Levenshtein distance, bailing out as soon as it exceeds `max`.
fn edit_distance_at_most(a: &str, b: &str, max: usize) -> Option<usize> {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];

    for i in 1..=a.len() {
        cur[0] = i;
        let mut row_min = cur[0];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            row_min = row_min.min(cur[j]);
        }
        if row_min > max {
            return None;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let d = prev[b.len()];
    (d <= max).then_some(d)
}

/// Build a coverage report over a whole channel list.
pub fn coverage<'a, I>(index: &EpgIndex, channels: I) -> Coverage
where
    I: IntoIterator<Item = (&'a str, Option<&'a str>)>,
{
    let mut report = Coverage::default();
    for (name, tvg_id) in channels {
        report.total += 1;
        if index.resolve(tvg_id, name, None).is_some() {
            report.matched += 1;
        } else {
            report.unmatched.push(name.to_string());
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(id: &str, names: &[&str]) -> EpgChannel {
        EpgChannel {
            id: id.into(),
            display_names: names.iter().map(|s| s.to_string()).collect(),
            icon: None,
        }
    }

    fn index() -> EpgIndex {
        EpgIndex::build(&[
            ch("cnn.us", &["CNN"]),
            ch("bbcone.uk", &["BBC One", "BBC 1"]),
            ch("discovery.uk", &["Discovery Channel"]),
        ])
    }

    #[test]
    fn exact_tvg_id_wins() {
        let m = index().resolve(Some("cnn.us"), "Whatever", None).unwrap();
        assert_eq!(m.method, MatchMethod::ExactId);
        assert_eq!(m.epg_channel_id, "cnn.us");
        assert_eq!(m.confidence, 100);
    }

    #[test]
    fn tvg_id_match_is_case_insensitive() {
        assert!(index().resolve(Some("CNN.US"), "x", None).is_some());
    }

    #[test]
    fn falls_back_to_normalized_name() {
        let m = index().resolve(None, "US| CNN HD", None).unwrap();
        assert_eq!(m.method, MatchMethod::NormalizedName);
        assert_eq!(m.epg_channel_id, "cnn.us");
    }

    #[test]
    fn matches_any_display_name_alias() {
        assert_eq!(
            index()
                .resolve(None, "BBC 1 FHD", None)
                .unwrap()
                .epg_channel_id,
            "bbcone.uk"
        );
    }

    #[test]
    fn manual_mapping_overrides_everything() {
        let m = index()
            .resolve(Some("cnn.us"), "CNN", Some("my.custom.id"))
            .unwrap();
        assert_eq!(m.method, MatchMethod::Manual);
        assert_eq!(m.epg_channel_id, "my.custom.id");
    }

    #[test]
    fn fuzzy_catches_a_single_typo() {
        let m = index().resolve(None, "Discovry Channel", None).unwrap();
        assert_eq!(m.method, MatchMethod::Fuzzy);
        assert_eq!(m.epg_channel_id, "discovery.uk");
    }

    #[test]
    fn fuzzy_does_not_confuse_numbered_siblings() {
        // "Sky 1" vs "Sky 2" differ by one edit but must never cross-match:
        // short keys are excluded from the fuzzy pass entirely.
        let idx = EpgIndex::build(&[ch("sky1.uk", &["Sky 1"]), ch("sky2.uk", &["Sky 2"])]);
        let m = idx.resolve(None, "Sky 3", None);
        assert!(m.is_none(), "unexpectedly matched: {m:?}");
    }

    #[test]
    fn unknown_channels_return_none() {
        assert!(index()
            .resolve(None, "Totally Unknown Network", None)
            .is_none());
    }

    #[test]
    fn coverage_counts_and_lists_the_gaps() {
        let idx = index();
        let report = coverage(
            &idx,
            vec![
                ("CNN", Some("cnn.us")),
                ("BBC One", None),
                ("Some Local Channel", None),
            ],
        );
        assert_eq!(report.total, 3);
        assert_eq!(report.matched, 2);
        assert_eq!(report.unmatched, vec!["Some Local Channel"]);
        assert!((report.percent() - 66.67).abs() < 0.1);
    }

    #[test]
    fn edit_distance_bails_out_early() {
        assert_eq!(edit_distance_at_most("abc", "abc", 1), Some(0));
        assert_eq!(edit_distance_at_most("abc", "abd", 1), Some(1));
        assert_eq!(edit_distance_at_most("abc", "xyz", 1), None);
    }
}
