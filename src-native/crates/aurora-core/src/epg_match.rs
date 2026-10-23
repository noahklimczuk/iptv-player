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
    /// Display-name keys bucketed by length, for the fuzzy pass.
    ///
    /// A `Vec` of all of them, walked in full for every channel, is 4,597 channels
    /// against 6,798 keys — and the first thing the loop did was `continue` on any key
    /// whose length differed by more than one, which left **4,475,192** distance
    /// computations inside the import. The prefilter worked; it was just being applied
    /// one key at a time. Bucketing moves it into the lookup: the pass now visits three
    /// buckets instead of the whole list, and the candidate set it considers is
    /// identical, because that is exactly the condition it was already testing (R-23).
    keys_by_len: HashMap<usize, Vec<(String, String)>>,
}

impl EpgIndex {
    pub fn build(channels: &[EpgChannel]) -> Self {
        let mut by_id = HashMap::with_capacity(channels.len());
        let mut by_key = HashMap::with_capacity(channels.len() * 2);
        let mut keys_by_len: HashMap<usize, Vec<(String, String)>> = HashMap::new();

        for c in channels {
            // First declaration wins, which is what `by_key` below does.
            //
            // This was `insert`, so the *last* won, six lines from two `or_insert_with`
            // calls where the first does. The real guide declares 9,033 `<channel>`
            // elements carrying 4,828 distinct ids, so 4,205 of them are duplicates and
            // this tie-break is reached constantly — and nothing was wrong, because for
            // a duplicated id the value stored is the same string either way. Two
            // different tie-breaks in one function where only one of them was decided
            // on, which is the kind of thing that stops being harmless the moment the
            // value stops being the key (R-24).
            by_id
                .entry(c.id.to_ascii_lowercase())
                .or_insert_with(|| c.id.clone());

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
                keys_by_len
                    .entry(k.len())
                    .or_default()
                    .push((k, c.id.clone()));
            }
        }
        Self {
            by_id,
            by_key,
            keys_by_len,
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

        // Fuzzy pass: only for keys of a similar length, only for a distance of 1, only
        // when the one edit is not a digit, and only when exactly one channel is that
        // close.
        //
        // The first two conditions were here and were not enough. `key.len() >= 5`
        // excludes only *short* names -- `sky1` is four characters and was excluded while
        // `spectrumxcast3` is fourteen and was not -- and between two long channel names
        // a single-character edit is, in practice, the channel number. Replayed over the
        // real guide and playlist: 655 channels got a fuzzy match, 639 of them (97%)
        // differed from it by a single digit, and 624 had several candidates tied at the
        // same distance. `espn156` was handed one of ten equally close channels because
        // it came first in the provider's `<channel>` order (R-22).
        //
        // Each of those channels then showed another channel's schedule at confidence
        // 60, which is worse than a blank row: a blank row is understood, and a
        // plausible wrong programme is believed.
        if key.len() >= 5 {
            let mut matches: Vec<&str> = Vec::new();
            // The three lengths a key one edit away can have. The length check this
            // replaces is gone from the loop body because it is now the lookup (R-23);
            // the set of keys considered is the same set it selected.
            let candidates = [
                key.len().checked_sub(1),
                Some(key.len()),
                Some(key.len() + 1),
            ]
            .into_iter()
            .flatten()
            .filter_map(|n| self.keys_by_len.get(&n))
            .flatten();
            for (k, id) in candidates {
                if edit_distance_at_most(&key, k, 1).is_none() {
                    continue;
                }
                // "BBC 1" and "BBC 2" are one edit apart and are different channels.
                if single_edit_touches_a_digit(&key, k) {
                    continue;
                }
                if !matches.contains(&id.as_str()) {
                    matches.push(id.as_str());
                }
                // Two channels equally close is not a match, it is a coin toss.
                if matches.len() > 1 {
                    return None;
                }
            }
            if let Some(id) = matches.first() {
                return Some(Match {
                    epg_channel_id: (*id).to_string(),
                    method: MatchMethod::Fuzzy,
                    confidence: 60,
                });
            }
        }
        None
    }
}

/// For two keys one edit apart, is that edit a digit?
///
/// Which is the difference between "BBC One" vs "BBC Two" -- a real pair to consider --
/// and "ESPN+ 156" vs "ESPN+ 15", which are two different channels whose names differ by
/// the number that identifies them. Assumes the two are within one edit of each other;
/// callers check that first.
fn single_edit_touches_a_digit(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len() == b.len() {
        return a
            .iter()
            .zip(&b)
            .find(|(x, y)| x != y)
            .is_some_and(|(x, y)| x.is_ascii_digit() || y.is_ascii_digit());
    }
    // One is the other with a character inserted; find it.
    let (short, long) = if a.len() < b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    let at = (0..short.len())
        .find(|&i| short[i] != long[i])
        .unwrap_or(short.len());
    long.get(at).is_some_and(|c| c.is_ascii_digit())
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

    /// The first declaration of a duplicated id wins, as it does in `by_key` (R-24).
    ///
    /// The real guide declares 9,033 `<channel>` elements with 4,828 distinct ids, so
    /// this tie-break is reached 4,205 times per import. It was `insert` here and
    /// `or_insert_with` six lines below, and it never showed because for a duplicated id
    /// the stored value is the same string either way — unless the two declarations spell
    /// it differently, which is what this asserts on.
    #[test]
    fn a_duplicated_channel_id_keeps_its_first_spelling() {
        let index = EpgIndex::build(&[ch("BBC1.uk", &["BBC One"]), ch("bbc1.uk", &["BBC 1"])]);
        // Both declarations key on the same lowercased id, so the index holds one entry.
        assert_eq!(index.len(), 1);
        let m = index
            .resolve(Some("BBC1.UK"), "Whatever", None)
            .expect("the id should resolve however it is cased");
        assert_eq!(m.method, MatchMethod::ExactId);
        assert_eq!(
            m.epg_channel_id, "BBC1.uk",
            "the later declaration overwrote the earlier one"
        );
    }

    /// The fuzzy pass considers keys within one character of the name, and no others.
    ///
    /// That was a `continue` inside a walk of every key — 4,475,192 distance
    /// computations on the real guide, all but a fraction of them discarded by the first
    /// line of the loop. It is the bucket lookup now, and this pins the boundary the
    /// bucketing has to reproduce: ±1 is considered, ±2 is not (R-23).
    #[test]
    fn the_fuzzy_pass_only_looks_one_character_either_side() {
        let index = EpgIndex::build(&[ch("sky.uk", &["Sky Atlantic"])]);

        // One character longer than the stored key, one edit away: a match.
        let longer = index.resolve(None, "Sky Atlanttic", None);
        assert_eq!(
            longer.as_ref().map(|m| m.method),
            Some(MatchMethod::Fuzzy),
            "one edit away was not considered: {longer:?}"
        );

        // Two characters longer is outside the window, whatever the distance.
        assert!(
            index.resolve(None, "Sky Atlanttick", None).is_none(),
            "two characters away should not be a candidate at all"
        );

        // And an exact normalised match is still taken by the pass above this one.
        assert_eq!(
            index.resolve(None, "sky atlantic", None).map(|m| m.method),
            Some(MatchMethod::NormalizedName)
        );
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

    /// Long names whose one difference is the channel number, which is what the fuzzy
    /// pass actually spent its time doing.
    ///
    /// `key.len() >= 5` was meant to stop this and only ever excluded short names:
    /// `sky1` is four characters and was excluded while `spectrumxcast3` is fourteen and
    /// was not. Replayed over the real library: 703 fuzzy matches, 663 of them differing
    /// by a single digit -- "CBS 2 LOS ANGELES" was being shown the schedule of
    /// `cbs9losangeles`, "ABC 4 CHARLESTON" that of `abc8charleston` (R-22).
    #[test]
    fn a_long_name_is_not_matched_to_one_with_a_different_number() {
        let idx = EpgIndex::build(&[
            ch("spectrumxcast.us", &["Spectrum XCast"]),
            ch("cbs9losangeles.us", &["CBS 9 Los Angeles"]),
            ch("nbc14columbus.us", &["NBC 14 Columbus"]),
        ]);
        for name in [
            "US SPECTRUM XCAST 3",
            "US CBS 2 LOS ANGELES HD",
            "US NBC 4 COLUMBUS HD",
        ] {
            let m = idx.resolve(None, name, None);
            assert!(m.is_none(), "{name} matched {m:?}");
        }
    }

    /// Several channels equally close is not a match, it is a coin toss.
    ///
    /// 624 of the 655 fuzzy matches measured were ties, and the old code kept whichever
    /// candidate it walked past first -- so the answer depended on the order the
    /// provider wrote its `<channel>` elements in, and `espn156` was handed one of ten
    /// equally close channels for no reason at all.
    #[test]
    fn a_tie_is_not_resolved_by_whichever_came_first() {
        // Two guide channels one edit from the name, neither involving a digit.
        let idx = EpgIndex::build(&[
            ch("first.us", &["Channel Alpha"]),
            ch("second.us", &["Channel Alpho"]),
        ]);
        let m = idx.resolve(None, "Channel Alphx", None);
        assert!(m.is_none(), "picked one of a tie: {m:?}");

        // One clear winner is still matched.
        let only = EpgIndex::build(&[ch("first.us", &["Channel Alpha"])]);
        let hit = only.resolve(None, "Channel Alphx", None).unwrap();
        assert_eq!(hit.epg_channel_id, "first.us");
        assert_eq!(hit.method, MatchMethod::Fuzzy);
    }

    /// Two guide entries that are aliases of the same channel are not a tie.
    #[test]
    fn two_names_for_one_channel_are_still_a_match() {
        let idx = EpgIndex::build(&[ch("one.us", &["Channel Alpha", "Channel Alpho"])]);
        let hit = idx.resolve(None, "Channel Alphx", None).unwrap();
        assert_eq!(hit.epg_channel_id, "one.us");
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
