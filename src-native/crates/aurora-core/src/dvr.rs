//! Recording scheduling logic (README §7.7).
//!
//! Pure: padding, conflict detection against the provider's connection limit, and
//! matching series rules onto guide entries. Nothing here touches a disk or a stream.

use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, UtcOffset, Weekday};

use crate::model::Programme;
use crate::title;

/// Default padding either side of a scheduled programme (README §7.7). Broadcasters
/// overrun; five minutes at the end is the difference between catching the ending and
/// not.
pub const DEFAULT_PRE_PADDING_SECS: i64 = 60;
pub const DEFAULT_POST_PADDING_SECS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordingState {
    Scheduled,
    Recording,
    Completed,
    /// Did not record, or recorded incompletely. `reason` lives on the row.
    Failed,
    /// Dropped because a higher-priority recording needed the connection.
    Skipped,
}

/// A recording as scheduled, in absolute time with padding already applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub id: i64,
    pub channel_id: i64,
    /// Unix seconds, padding included.
    pub start: i64,
    pub stop: i64,
    /// Higher wins a conflict. A manual recording outranks a series rule.
    pub priority: i32,
}

impl Slot {
    pub fn overlaps(&self, other: &Slot) -> bool {
        self.start < other.stop && other.start < self.stop
    }

    pub fn duration_secs(&self) -> i64 {
        (self.stop - self.start).max(0)
    }
}

/// Apply padding to a programme's airtime.
///
/// Never lets padding invert the window, and never pads a recording to start before
/// the epoch.
pub fn with_padding(start: i64, stop: i64, pre_secs: i64, post_secs: i64) -> (i64, i64) {
    let padded_start = (start - pre_secs.max(0)).max(0);
    let padded_stop = stop + post_secs.max(0);
    if padded_stop <= padded_start {
        (start, stop.max(start + 1))
    } else {
        (padded_start, padded_stop)
    }
}

/// A period where more recordings overlap than the subscription can carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    pub start: i64,
    pub stop: i64,
    /// Everything scheduled across that window, most important first.
    pub slot_ids: Vec<i64>,
    /// How many would have to be dropped.
    pub over_by: usize,
}

/// Find windows where concurrent recordings exceed `max_concurrent`.
///
/// This is the provider's connection limit made concrete: README §4.1 requires Aurora
/// to respect it rather than getting the line throttled, and §7.7 requires conflicts
/// to be surfaced before the recording silently fails at 8pm.
pub fn find_conflicts(slots: &[Slot], max_concurrent: usize) -> Vec<Conflict> {
    if max_concurrent == 0 {
        // Nothing may record at all; every slot is in conflict.
        if slots.is_empty() {
            return Vec::new();
        }
        let start = slots.iter().map(|s| s.start).min().unwrap_or(0);
        let stop = slots.iter().map(|s| s.stop).max().unwrap_or(0);
        return vec![Conflict {
            start,
            stop,
            slot_ids: ranked_ids(slots),
            over_by: slots.len(),
        }];
    }

    // Sweep the boundaries; between two consecutive boundaries the active set is
    // constant, so the count only has to be computed once per interval.
    let mut boundaries: Vec<i64> = Vec::with_capacity(slots.len() * 2);
    for s in slots {
        boundaries.push(s.start);
        boundaries.push(s.stop);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut conflicts: Vec<Conflict> = Vec::new();
    for pair in boundaries.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        if to <= from {
            continue;
        }
        let active: Vec<&Slot> = slots
            .iter()
            .filter(|s| s.start < to && from < s.stop)
            .collect();
        if active.len() <= max_concurrent {
            continue;
        }

        let over_by = active.len() - max_concurrent;
        let ids = ranked_ids(&active.into_iter().cloned().collect::<Vec<_>>());

        // Merge with the previous window when it is contiguous and identical, so a
        // long clash is reported once rather than per boundary.
        match conflicts.last_mut() {
            Some(prev) if prev.stop == from && prev.slot_ids == ids => prev.stop = to,
            _ => conflicts.push(Conflict {
                start: from,
                stop: to,
                slot_ids: ids,
                over_by,
            }),
        }
    }
    conflicts
}

/// Most important first: higher priority, then the one that started earlier, then id
/// — so the ordering is stable and a UI can show a deterministic list.
fn ranked_ids(slots: &[Slot]) -> Vec<i64> {
    let mut ranked: Vec<&Slot> = slots.iter().collect();
    ranked.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then(a.start.cmp(&b.start))
            .then(a.id.cmp(&b.id))
    });
    ranked.into_iter().map(|s| s.id).collect()
}

/// Which slots to drop so nothing exceeds the limit. Lowest priority goes first.
pub fn resolve_conflicts(slots: &[Slot], max_concurrent: usize) -> Vec<i64> {
    let mut dropped: Vec<i64> = Vec::new();
    let mut kept: Vec<Slot> = slots.to_vec();

    loop {
        let conflicts = find_conflicts(&kept, max_concurrent);
        let Some(worst) = conflicts.first() else {
            break;
        };
        // The last id in a ranked list is the least important one in that window.
        let Some(&victim) = worst.slot_ids.last() else {
            break;
        };
        dropped.push(victim);
        kept.retain(|s| s.id != victim);
        if kept.is_empty() {
            break;
        }
    }
    dropped
}

/// A standing rule that turns guide entries into recordings (README §7.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesRule {
    pub id: i64,
    /// Normalized show title, compared with `title::match_key`.
    pub title_key: String,
    /// Restrict to one channel, or `None` for any.
    pub channel_id: Option<i64>,
    /// Only episodes the guide flags as new.
    pub new_only: bool,
    /// Restrict to particular weekdays (0 = Monday), in the viewer's zone.
    pub weekdays: Option<Vec<u8>>,
    /// Restrict to programmes starting near this local time, within `time_slack_secs`.
    pub around_local_minute: Option<u16>,
    pub time_slack_secs: i64,
    pub priority: i32,
}

impl SeriesRule {
    pub fn new(id: i64, title: &str) -> Self {
        Self {
            id,
            title_key: title::match_key(title),
            channel_id: None,
            new_only: false,
            weekdays: None,
            around_local_minute: None,
            time_slack_secs: 900,
            priority: 0,
        }
    }

    /// Whether this rule should record `programme` airing on `channel_id`.
    pub fn matches(&self, programme: &Programme, channel_id: i64, utc_offset_secs: i32) -> bool {
        if let Some(only) = self.channel_id {
            if only != channel_id {
                return false;
            }
        }
        if self.new_only && !programme.is_new {
            return false;
        }
        if title::match_key(&programme.title) != self.title_key {
            return false;
        }

        let Ok(offset) = UtcOffset::from_whole_seconds(utc_offset_secs) else {
            return false;
        };
        let Ok(local) = OffsetDateTime::from_unix_timestamp(programme.start) else {
            return false;
        };
        let local = local.to_offset(offset);

        if let Some(days) = &self.weekdays {
            let weekday = match local.weekday() {
                Weekday::Monday => 0,
                Weekday::Tuesday => 1,
                Weekday::Wednesday => 2,
                Weekday::Thursday => 3,
                Weekday::Friday => 4,
                Weekday::Saturday => 5,
                Weekday::Sunday => 6,
            };
            if !days.contains(&weekday) {
                return false;
            }
        }

        if let Some(target) = self.around_local_minute {
            let minute_of_day = local.hour() as i64 * 60 + local.minute() as i64;
            let diff = (minute_of_day - target as i64).abs().min(
                // Wrap around midnight: 23:55 and 00:05 are ten minutes apart.
                1440 - (minute_of_day - target as i64).abs(),
            );
            if diff * 60 > self.time_slack_secs {
                return false;
            }
        }
        true
    }
}

/// Everything a rule set would record from a slice of guide entries.
pub fn expand_rules(
    rules: &[SeriesRule],
    programmes: &[(Programme, i64)],
    utc_offset_secs: i32,
) -> Vec<(i64, usize)> {
    let mut out = Vec::new();
    for (index, (programme, channel_id)) in programmes.iter().enumerate() {
        for rule in rules {
            if rule.matches(programme, *channel_id, utc_offset_secs) {
                out.push((rule.id, index));
                break; // one recording per programme, however many rules match
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(id: i64, start: i64, stop: i64, priority: i32) -> Slot {
        Slot {
            id,
            channel_id: id,
            start,
            stop,
            priority,
        }
    }

    fn programme(title: &str, start: i64, is_new: bool) -> Programme {
        Programme {
            channel_id: "c".into(),
            start,
            stop: start + 3600,
            title: title.into(),
            sub_title: None,
            description: None,
            categories: vec![],
            season: None,
            episode: None,
            icon: None,
            rating: None,
            star_rating: None,
            is_new,
            is_live: false,
            is_premiere: false,
            credits: vec![],
        }
    }

    #[test]
    fn padding_extends_both_ends() {
        assert_eq!(with_padding(1000, 4600, 60, 300), (940, 4900));
    }

    #[test]
    fn padding_never_goes_before_the_epoch() {
        let (start, _) = with_padding(30, 3600, 300, 0);
        assert_eq!(start, 0);
    }

    #[test]
    fn padding_never_inverts_a_window() {
        // Absurd negative-length input should still yield a usable window.
        let (start, stop) = with_padding(1000, 900, 0, 0);
        assert!(stop > start, "{start}..{stop}");
    }

    #[test]
    fn zero_padding_is_a_no_op() {
        assert_eq!(with_padding(1000, 4600, 0, 0), (1000, 4600));
    }

    #[test]
    fn overlap_is_half_open() {
        let a = slot(1, 0, 100, 0);
        // Touching at the boundary is not an overlap — one ends as the other starts.
        assert!(!a.overlaps(&slot(2, 100, 200, 0)));
        assert!(a.overlaps(&slot(3, 99, 200, 0)));
    }

    #[test]
    fn no_conflict_within_the_limit() {
        let slots = [slot(1, 0, 100, 0), slot(2, 50, 150, 0)];
        assert!(find_conflicts(&slots, 2).is_empty());
    }

    #[test]
    fn reports_a_conflict_over_the_limit() {
        let slots = [slot(1, 0, 100, 0), slot(2, 50, 150, 0), slot(3, 60, 80, 0)];
        let conflicts = find_conflicts(&slots, 2);
        assert_eq!(conflicts.len(), 1);
        assert_eq!((conflicts[0].start, conflicts[0].stop), (60, 80));
        assert_eq!(conflicts[0].over_by, 1);
        assert_eq!(conflicts[0].slot_ids.len(), 3);
    }

    #[test]
    fn a_long_clash_is_reported_as_one_window() {
        // Three identical slots would otherwise produce a conflict per boundary.
        let slots = [slot(1, 0, 300, 0), slot(2, 0, 300, 0), slot(3, 0, 300, 0)];
        let conflicts = find_conflicts(&slots, 2);
        assert_eq!(conflicts.len(), 1);
        assert_eq!((conflicts[0].start, conflicts[0].stop), (0, 300));
    }

    #[test]
    fn conflicts_rank_by_priority_then_time() {
        let slots = [
            slot(1, 0, 100, 0),
            slot(2, 0, 100, 5), // manual recording, outranks the rest
            slot(3, 0, 100, 0),
        ];
        let conflicts = find_conflicts(&slots, 1);
        assert_eq!(conflicts[0].slot_ids[0], 2, "highest priority first");
        assert_eq!(
            conflicts[0].slot_ids.last(),
            Some(&3),
            "least important last"
        );
    }

    #[test]
    fn a_zero_connection_limit_conflicts_with_everything() {
        let slots = [slot(1, 0, 100, 0)];
        let conflicts = find_conflicts(&slots, 0);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].over_by, 1);
        assert!(find_conflicts(&[], 0).is_empty());
    }

    #[test]
    fn resolution_drops_the_least_important_until_it_fits() {
        let slots = [
            slot(1, 0, 100, 10), // keep
            slot(2, 0, 100, 5),
            slot(3, 0, 100, 0), // drop first
        ];
        let dropped = resolve_conflicts(&slots, 1);
        assert_eq!(dropped, vec![3, 2]);
    }

    #[test]
    fn resolution_is_a_no_op_when_everything_fits() {
        let slots = [slot(1, 0, 100, 0), slot(2, 200, 300, 0)];
        assert!(resolve_conflicts(&slots, 1).is_empty());
    }

    #[test]
    fn a_rule_matches_its_show_by_normalized_title() {
        let rule = SeriesRule::new(1, "Breaking Bad");
        assert!(rule.matches(&programme("BREAKING BAD", 0, false), 1, 0));
        assert!(rule.matches(&programme("Breaking  Bad", 0, false), 1, 0));
        assert!(!rule.matches(&programme("Breaking Good", 0, false), 1, 0));
    }

    #[test]
    fn a_channel_restriction_is_honoured() {
        let mut rule = SeriesRule::new(1, "The News");
        rule.channel_id = Some(7);
        assert!(rule.matches(&programme("The News", 0, false), 7, 0));
        assert!(!rule.matches(&programme("The News", 0, false), 8, 0));
    }

    #[test]
    fn new_only_skips_repeats() {
        let mut rule = SeriesRule::new(1, "The News");
        rule.new_only = true;
        assert!(rule.matches(&programme("The News", 0, true), 1, 0));
        assert!(!rule.matches(&programme("The News", 0, false), 1, 0));
    }

    #[test]
    fn weekday_restrictions_use_local_time() {
        // 2024-01-15T12:00:00Z is a Monday.
        let monday_noon = 1_705_320_000;
        let mut rule = SeriesRule::new(1, "Show");
        rule.weekdays = Some(vec![0]); // Monday
        assert!(rule.matches(&programme("Show", monday_noon, false), 1, 0));

        rule.weekdays = Some(vec![1]); // Tuesday
        assert!(!rule.matches(&programme("Show", monday_noon, false), 1, 0));

        // Far enough east and that Monday noon is still Monday, but Monday 23:00 UTC
        // is Tuesday locally.
        let monday_late = monday_noon + 11 * 3600;
        rule.weekdays = Some(vec![1]);
        assert!(
            rule.matches(&programme("Show", monday_late, false), 1, 4 * 3600),
            "the local day is what a viewer means by 'Tuesday'"
        );
    }

    #[test]
    fn time_of_day_restrictions_allow_slack() {
        let monday_noon = 1_705_320_000; // 12:00 UTC
        let mut rule = SeriesRule::new(1, "Show");
        rule.around_local_minute = Some(12 * 60);
        rule.time_slack_secs = 900;

        assert!(rule.matches(&programme("Show", monday_noon, false), 1, 0));
        // Ten minutes late is within slack.
        assert!(rule.matches(&programme("Show", monday_noon + 600, false), 1, 0));
        // An hour late is not.
        assert!(!rule.matches(&programme("Show", monday_noon + 3600, false), 1, 0));
    }

    #[test]
    fn time_matching_wraps_around_midnight() {
        let mut rule = SeriesRule::new(1, "Show");
        rule.around_local_minute = Some(0); // midnight
        rule.time_slack_secs = 900;

        // Five minutes before midnight is five minutes from it, not 23h55m.
        let base = 1_705_276_800; // 2024-01-15T00:00:00Z
        assert!(rule.matches(&programme("Show", base + 23 * 3600 + 55 * 60, false), 1, 0));
        assert!(rule.matches(&programme("Show", base + 5 * 60, false), 1, 0));
        assert!(!rule.matches(&programme("Show", base + 12 * 3600, false), 1, 0));
    }

    #[test]
    fn expanding_rules_records_each_programme_once() {
        let rules = [SeriesRule::new(1, "Show"), SeriesRule::new(2, "Show")];
        let programmes = vec![
            (programme("Show", 0, false), 1),
            (programme("Other", 0, false), 1),
        ];
        let out = expand_rules(&rules, &programmes, 0);
        assert_eq!(out.len(), 1, "two matching rules must not double-record");
        assert_eq!(out[0], (1, 0), "the first rule wins");
    }
}
