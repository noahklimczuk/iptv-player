//! Skip markers: intro, recap, and credits regions within an episode.
//!
//! README §9 asks for "Skip Intro / Skip Recap / Skip Credits buttons where chapters
//! exist or where a black-frame/silence heuristic can detect them".
//!
//! Aurora derives markers from four sources, in descending order of trust:
//!
//! 1. **Chapters** embedded in the file — exact, when the release has them.
//! 2. **The user** — when someone skips manually, where they skipped is recorded.
//! 3. **Learned from the series** — the median of the evidence from *other* episodes of
//!    the same show, applied to episodes that have no marker yet. Chapters on a sibling
//!    count here as much as a manual skip does, which is what makes a show whose first
//!    file has chapters give every other episode its buttons for free.
//! 4. **Convention** — the credits, from the clock alone.
//!
//! Frame/audio fingerprinting is deliberately *not* implemented: it needs decoding
//! passes this crate cannot do, and a wrong "Skip Intro" button is worse than none.
//!
//! ## Why the credits can be guessed and an intro cannot
//!
//! The credits are *defined* by being at the end, so a clock locates them: `duration -
//! CREDITS_TAIL_SECS` is within a few seconds of right on almost every episode ever
//! made, and the action it offers — go to the next episode — costs nothing when it is
//! early, because what it skips is the credits.
//!
//! An intro has no such definition. It is usually near the front and usually about a
//! minute, but "usually" is doing load-bearing work there: a cold open before the titles
//! is common, and a Skip Intro button that cuts the first ninety seconds of a cold open
//! has destroyed the one part of the episode that cannot be inferred. So an intro is
//! offered only where there is evidence for it — a chapter, or a skip somebody made —
//! and tier 4 stays silent about it. [`conventional`] says so in code rather than
//! leaving it to be discovered.

use serde::{Deserialize, Serialize};

/// The shortest region worth offering a button for. Below this the button would
/// appear and vanish before anyone could hit it.
pub const MIN_MARKER_SECS: f64 = 5.0;
/// Longer than this and we have almost certainly mis-parsed a chapter.
pub const MAX_MARKER_SECS: f64 = 300.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarkerKind {
    Intro,
    Recap,
    Credits,
}

impl MarkerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MarkerKind::Intro => "intro",
            MarkerKind::Recap => "recap",
            MarkerKind::Credits => "credits",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "intro" => Some(MarkerKind::Intro),
            "recap" => Some(MarkerKind::Recap),
            "credits" => Some(MarkerKind::Credits),
            _ => None,
        }
    }

    /// The button label the UI shows.
    pub fn label(self) -> &'static str {
        match self {
            MarkerKind::Intro => "Skip Intro",
            MarkerKind::Recap => "Skip Recap",
            MarkerKind::Credits => "Skip Credits",
        }
    }
}

/// Where a marker came from. Surfaced so the UI can say "learned from this show"
/// and so a chapter-derived marker is never overwritten by a fuzzier source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarkerSource {
    Chapters,
    User,
    Learned,
    /// From the episode's own duration and nothing else. Only ever the credits.
    Convention,
}

impl MarkerSource {
    pub fn as_str(self) -> &'static str {
        match self {
            MarkerSource::Chapters => "chapters",
            MarkerSource::User => "user",
            MarkerSource::Learned => "learned",
            MarkerSource::Convention => "convention",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "chapters" => Some(MarkerSource::Chapters),
            "user" => Some(MarkerSource::User),
            "learned" => Some(MarkerSource::Learned),
            "convention" => Some(MarkerSource::Convention),
            _ => None,
        }
    }

    /// Higher wins when two sources describe the same region.
    pub fn trust(self) -> u8 {
        match self {
            MarkerSource::Chapters => 4,
            MarkerSource::User => 3,
            MarkerSource::Learned => 2,
            // Last, so anything with evidence behind it wins. A conventional credits
            // marker exists to fill a hole, not to argue with a chapter.
            MarkerSource::Convention => 1,
        }
    }

    /// Whether a marker from this source may be skipped *without being asked*.
    ///
    /// The per-show "always skip" preferences act on evidence, not on guesses. A
    /// conventional credits marker is a button, and pressing it is the viewer agreeing
    /// with the guess — auto-skipping one would mean a clock silently cutting the last
    /// minute of an episode whose runtime was wrong, which is how somebody loses the
    /// final scene of a finale.
    pub fn may_auto_skip(self) -> bool {
        !matches!(self, MarkerSource::Convention)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkipMarker {
    pub kind: MarkerKind,
    pub start_secs: f64,
    pub end_secs: f64,
    pub source: MarkerSource,
}

impl SkipMarker {
    pub fn new(kind: MarkerKind, start_secs: f64, end_secs: f64, source: MarkerSource) -> Self {
        Self {
            kind,
            start_secs,
            end_secs,
            source,
        }
    }

    pub fn duration_secs(&self) -> f64 {
        (self.end_secs - self.start_secs).max(0.0)
    }

    /// Whether a marker is worth showing a button for at all.
    pub fn is_plausible(&self) -> bool {
        self.start_secs >= 0.0
            && self.duration_secs() >= MIN_MARKER_SECS
            && self.duration_secs() <= MAX_MARKER_SECS
    }

    /// Is the playhead inside this marker?
    ///
    /// The button disappears a moment before the region ends, so it never lingers
    /// into content the viewer wanted to see.
    pub fn contains(&self, position_secs: f64) -> bool {
        position_secs >= self.start_secs && position_secs < self.end_secs - 0.5
    }
}

/// One chapter as the player reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    pub title: Option<String>,
    pub start_secs: f64,
}

/// Whole-word keyword sets. Matching on substrings would classify "Introduction to
/// the Case" — a real episode title — as an intro.
const INTRO_WORDS: &[&str] = &["intro", "opening", "op", "titles", "theme", "maintitles"];
const RECAP_WORDS: &[&str] = &["recap", "previously", "previouslyon", "resume"];
const CREDITS_WORDS: &[&str] = &["credits", "endcredits", "ending", "ed", "outro", "closing"];

fn normalize(title: &str) -> Vec<String> {
    title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect()
}

fn classify_title(title: &str) -> Option<MarkerKind> {
    let words = normalize(title);
    if words.is_empty() {
        return None;
    }
    // Join too, so "Previously On" and "End Credits" match their compound forms.
    let joined = words.concat();

    let hit = |set: &[&str]| -> bool {
        words.iter().any(|w| set.contains(&w.as_str())) || set.contains(&joined.as_str())
    };

    // Recap before intro: "Previously / Opening" style titles should read as a recap.
    if hit(RECAP_WORDS) {
        Some(MarkerKind::Recap)
    } else if hit(INTRO_WORDS) {
        Some(MarkerKind::Intro)
    } else if hit(CREDITS_WORDS) {
        Some(MarkerKind::Credits)
    } else {
        None
    }
}

/// Derive markers from a chapter list.
///
/// A chapter runs until the next one starts, or until the end of the file. Positional
/// sanity checks reject nonsense: an "intro" two thirds of the way in is a mis-titled
/// chapter, not an intro.
pub fn from_chapters(chapters: &[Chapter], duration_secs: f64) -> Vec<SkipMarker> {
    if duration_secs <= 0.0 {
        return Vec::new();
    }
    let mut sorted: Vec<&Chapter> = chapters.iter().collect();
    sorted.sort_by(|a, b| {
        a.start_secs
            .partial_cmp(&b.start_secs)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut out: Vec<SkipMarker> = Vec::new();
    for (i, ch) in sorted.iter().enumerate() {
        let Some(title) = ch.title.as_deref() else {
            continue;
        };
        let Some(kind) = classify_title(title) else {
            continue;
        };

        let end = sorted
            .get(i + 1)
            .map(|next| next.start_secs)
            .unwrap_or(duration_secs)
            .min(duration_secs);

        let marker = SkipMarker::new(kind, ch.start_secs.max(0.0), end, MarkerSource::Chapters);
        if !marker.is_plausible() || !position_is_sane(kind, &marker, duration_secs) {
            continue;
        }
        // Keep only the first of each kind; some releases chapter an intro twice.
        if out.iter().any(|m| m.kind == kind) {
            continue;
        }
        out.push(marker);
    }
    out
}

/// Reject markers whose position contradicts their label.
fn position_is_sane(kind: MarkerKind, marker: &SkipMarker, duration_secs: f64) -> bool {
    match kind {
        // Intros and recaps live near the front: first 10 minutes, or first third of
        // a short episode, whichever is more generous.
        MarkerKind::Intro | MarkerKind::Recap => {
            marker.start_secs <= (duration_secs / 3.0).max(600.0)
        }
        // Credits live in the last quarter.
        MarkerKind::Credits => marker.start_secs >= duration_secs * 0.75,
    }
}

/// Median of a set of samples. Returns `None` for an empty slice.
fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    })
}

/// Infer a marker for an episode that has none, from the user's own skips elsewhere
/// in the same series.
///
/// Uses the median rather than the mean so one mis-drag does not poison the show, and
/// requires at least `min_samples` observations so a single accident is not treated as
/// a pattern.
pub fn learn_from_series(
    observations: &[(f64, f64)],
    kind: MarkerKind,
    min_samples: usize,
) -> Option<SkipMarker> {
    if observations.len() < min_samples.max(1) {
        return None;
    }
    let mut starts: Vec<f64> = observations.iter().map(|(s, _)| *s).collect();
    let mut ends: Vec<f64> = observations.iter().map(|(_, e)| *e).collect();
    let marker = SkipMarker::new(
        kind,
        median(&mut starts)?,
        median(&mut ends)?,
        MarkerSource::Learned,
    );
    marker.is_plausible().then_some(marker)
}

/// Merge markers from several sources, keeping the most trusted per kind.
pub fn merge(sources: &[Vec<SkipMarker>]) -> Vec<SkipMarker> {
    let mut best: Vec<SkipMarker> = Vec::new();
    for list in sources {
        for m in list {
            if !m.is_plausible() {
                continue;
            }
            match best.iter_mut().find(|b| b.kind == m.kind) {
                Some(existing) => {
                    if m.source.trust() > existing.source.trust() {
                        *existing = *m;
                    }
                }
                None => best.push(*m),
            }
        }
    }
    best.sort_by(|a, b| {
        a.start_secs
            .partial_cmp(&b.start_secs)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    best
}

/// The marker the Skip button should offer at this playhead position, if any.
pub fn active_marker(markers: &[SkipMarker], position_secs: f64) -> Option<&SkipMarker> {
    markers.iter().find(|m| m.contains(position_secs))
}

/// How long before the end the conventional credits marker starts.
///
/// Sixty seconds, which is a touch generous for broadcast television and about right for
/// streaming. Being early costs the viewer the credits; being late costs them the button,
/// so the error is asymmetric and this leans early.
pub const CREDITS_TAIL_SECS: f64 = 60.0;

/// Below this, an episode is too short to assume anything about.
///
/// A six-minute file is a trailer, a recap reel, or an episode the provider has truncated
/// — and `duration - 60` on any of those is a "Skip Credits" button over the middle of it.
pub const MIN_EPISODE_FOR_CONVENTION_SECS: f64 = 600.0;

/// The markers a clock alone can justify: the credits, and nothing else.
///
/// This is tier 4, and it is why "Skip Credits" and Up Next work on a library whose
/// provider ships no chapters and whose viewer has never skipped anything — which is the
/// ordinary case, not the exotic one.
///
/// It deliberately returns no intro and no recap. See the module header: an intro has no
/// definition a clock can use, and a Skip Intro button that cuts a cold open has removed
/// the one part of an episode nobody can infer. Those two stay evidence-only.
pub fn conventional(duration_secs: f64) -> Vec<SkipMarker> {
    if !duration_secs.is_finite() || duration_secs < MIN_EPISODE_FOR_CONVENTION_SECS {
        return Vec::new();
    }
    let start = duration_secs - CREDITS_TAIL_SECS;
    // Belt and braces: `MIN_EPISODE_FOR_CONVENTION_SECS` already guarantees this, and a
    // future smaller minimum should not silently produce a marker over the second act.
    if start <= duration_secs * 0.5 {
        return Vec::new();
    }
    let marker = SkipMarker::new(
        MarkerKind::Credits,
        start,
        duration_secs,
        MarkerSource::Convention,
    );
    // Through the same gate every other marker goes through, so a tail longer than
    // `MAX_MARKER_SECS` cannot arrive here by the back door.
    if marker.is_plausible() {
        vec![marker]
    } else {
        Vec::new()
    }
}

/// When the "Up Next" card should appear.
///
/// Prefers the start of the credits; otherwise falls back to a fixed tail. README §9
/// specifies a 10-second countdown "over the tail of the current one".
pub fn up_next_at(
    markers: &[SkipMarker],
    duration_secs: f64,
    fallback_tail_secs: f64,
) -> Option<f64> {
    if duration_secs <= 0.0 {
        return None;
    }
    if let Some(credits) = markers.iter().find(|m| m.kind == MarkerKind::Credits) {
        return Some(credits.start_secs);
    }
    Some((duration_secs - fallback_tail_secs).max(duration_secs * 0.5))
}

#[cfg(test)]
mod convention_tests {
    use super::*;

    /// The case this tier exists for: a provider that ships no chapters, a viewer who
    /// has never skipped anything, and a Skip Credits button anyway.
    #[test]
    fn an_ordinary_episode_gets_credits_from_the_clock_alone() {
        let markers = conventional(45.0 * 60.0);
        assert_eq!(markers.len(), 1);
        let credits = markers[0];
        assert_eq!(credits.kind, MarkerKind::Credits);
        assert_eq!(credits.source, MarkerSource::Convention);
        assert_eq!(credits.start_secs, 45.0 * 60.0 - CREDITS_TAIL_SECS);
        assert_eq!(credits.end_secs, 45.0 * 60.0);
        assert!(credits.is_plausible());
    }

    /// No intro and no recap, ever. A clock cannot find either, and a Skip Intro button
    /// over a cold open removes the part of an episode nobody can infer.
    #[test]
    fn convention_never_invents_an_intro_or_a_recap() {
        for minutes in [10.0, 22.0, 45.0, 90.0, 180.0] {
            let markers = conventional(minutes * 60.0);
            assert!(
                markers.iter().all(|m| m.kind == MarkerKind::Credits),
                "{minutes} minutes produced {markers:?}"
            );
        }
    }

    /// A short file is a trailer or a truncated episode, and `duration - 60` on one of
    /// those is a button over the middle of it.
    #[test]
    fn something_too_short_to_be_an_episode_gets_nothing() {
        for secs in [0.0, 30.0, 120.0, MIN_EPISODE_FOR_CONVENTION_SECS - 1.0] {
            assert!(conventional(secs).is_empty(), "{secs}s produced a marker");
        }
        // And exactly at the threshold it does appear, so the boundary is not a cliff
        // nobody tested.
        assert_eq!(conventional(MIN_EPISODE_FOR_CONVENTION_SECS).len(), 1);
    }

    /// A stream that reports no duration — which an IPTV provider often does — must not
    /// produce a marker at zero.
    #[test]
    fn an_unknown_duration_is_not_a_marker_at_the_start() {
        assert!(conventional(0.0).is_empty());
        assert!(conventional(-1.0).is_empty());
        assert!(conventional(f64::NAN).is_empty());
        assert!(conventional(f64::INFINITY).is_empty());
    }

    /// Evidence always wins: a chapter saying the credits start at 38 minutes beats a
    /// clock saying 44.
    #[test]
    fn a_real_credits_marker_outranks_the_conventional_one() {
        let duration = 45.0 * 60.0;
        // Two minutes, not seven: `MAX_MARKER_SECS` rejects a seven-minute credits
        // region as a mis-parsed chapter, which is the existing rule and the right one.
        let from_chapters = SkipMarker::new(
            MarkerKind::Credits,
            43.0 * 60.0,
            duration,
            MarkerSource::Chapters,
        );
        let merged = merge(&[vec![from_chapters], conventional(duration)]);
        let credits: Vec<&SkipMarker> = merged
            .iter()
            .filter(|m| m.kind == MarkerKind::Credits)
            .collect();
        assert_eq!(credits.len(), 1, "two credits markers survived: {merged:?}");
        assert_eq!(credits[0].source, MarkerSource::Chapters);
        assert_eq!(credits[0].start_secs, 43.0 * 60.0);
    }

    /// The whole point of Up Next working without anyone teaching it anything.
    #[test]
    fn up_next_follows_the_conventional_credits() {
        let duration = 45.0 * 60.0;
        let at = up_next_at(&conventional(duration), duration, 45.0).unwrap();
        assert_eq!(at, duration - CREDITS_TAIL_SECS);
    }

    /// A guess may be pressed; it may not fire by itself.
    #[test]
    fn a_guess_is_never_auto_skipped_but_evidence_is() {
        assert!(!MarkerSource::Convention.may_auto_skip());
        for source in [
            MarkerSource::Chapters,
            MarkerSource::User,
            MarkerSource::Learned,
        ] {
            assert!(source.may_auto_skip(), "{source:?}");
        }
    }

    #[test]
    fn the_source_round_trips_through_its_name() {
        assert_eq!(
            MarkerSource::parse(MarkerSource::Convention.as_str()),
            Some(MarkerSource::Convention)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(title: &str, start: f64) -> Chapter {
        Chapter {
            title: Some(title.into()),
            start_secs: start,
        }
    }

    #[test]
    fn derives_an_intro_from_chapters() {
        let chapters = vec![
            ch("Recap", 0.0),
            ch("Opening Titles", 45.0),
            ch("Part 1", 135.0),
        ];
        let markers = from_chapters(&chapters, 2700.0);

        let intro = markers
            .iter()
            .find(|m| m.kind == MarkerKind::Intro)
            .unwrap();
        assert_eq!(intro.start_secs, 45.0);
        assert_eq!(intro.end_secs, 135.0);
        assert_eq!(intro.source, MarkerSource::Chapters);

        let recap = markers
            .iter()
            .find(|m| m.kind == MarkerKind::Recap)
            .unwrap();
        assert_eq!(recap.start_secs, 0.0);
        assert_eq!(recap.end_secs, 45.0);
    }

    #[test]
    fn last_chapter_runs_to_the_end_of_the_file() {
        let markers = from_chapters(&[ch("Episode", 0.0), ch("End Credits", 2600.0)], 2700.0);
        let credits = markers
            .iter()
            .find(|m| m.kind == MarkerKind::Credits)
            .unwrap();
        assert_eq!(credits.end_secs, 2700.0);
    }

    #[test]
    fn episode_titles_that_merely_contain_a_keyword_are_not_markers() {
        // "Introduction to the Case" must not become a Skip Intro button.
        let markers = from_chapters(
            &[ch("Introduction to the Case", 0.0), ch("Part 2", 600.0)],
            2700.0,
        );
        assert!(markers.is_empty(), "unexpected markers: {markers:?}");
    }

    #[test]
    fn a_misplaced_intro_chapter_is_rejected() {
        // An "Opening" two thirds in is a mis-titled chapter, not an intro.
        let markers = from_chapters(&[ch("Part 1", 0.0), ch("Opening", 1800.0)], 2700.0);
        assert!(markers.is_empty(), "unexpected markers: {markers:?}");
    }

    #[test]
    fn credits_early_in_the_file_are_rejected() {
        let markers = from_chapters(&[ch("Credits", 60.0), ch("Show", 200.0)], 2700.0);
        assert!(markers.is_empty());
    }

    #[test]
    fn very_short_and_very_long_regions_are_ignored() {
        let short = from_chapters(&[ch("Intro", 0.0), ch("Show", 2.0)], 2700.0);
        assert!(short.is_empty(), "2s intro should not get a button");

        let long = from_chapters(&[ch("Intro", 0.0), ch("Show", 600.0)], 2700.0);
        assert!(long.is_empty(), "10min 'intro' is a parse error");
    }

    #[test]
    fn chapters_out_of_order_are_sorted_first() {
        let markers = from_chapters(&[ch("Part 1", 135.0), ch("Opening", 45.0)], 2700.0);
        let intro = markers
            .iter()
            .find(|m| m.kind == MarkerKind::Intro)
            .unwrap();
        assert_eq!(intro.end_secs, 135.0);
    }

    #[test]
    fn untitled_chapters_and_zero_duration_are_handled() {
        let untitled = vec![Chapter {
            title: None,
            start_secs: 0.0,
        }];
        assert!(from_chapters(&untitled, 2700.0).is_empty());
        assert!(from_chapters(&[ch("Intro", 0.0)], 0.0).is_empty());
    }

    #[test]
    fn anime_conventions_are_recognised_as_whole_words() {
        let markers = from_chapters(&[ch("OP", 0.0), ch("Part A", 90.0)], 1440.0);
        assert_eq!(markers[0].kind, MarkerKind::Intro);
    }

    #[test]
    fn recap_wins_over_intro_when_a_title_mentions_both() {
        let markers = from_chapters(&[ch("Previously / Opening", 0.0), ch("Show", 60.0)], 2700.0);
        assert_eq!(markers[0].kind, MarkerKind::Recap);
    }

    #[test]
    fn duplicate_intro_chapters_yield_one_marker() {
        let markers = from_chapters(
            &[ch("Intro", 0.0), ch("Intro", 60.0), ch("Show", 120.0)],
            2700.0,
        );
        assert_eq!(
            markers
                .iter()
                .filter(|m| m.kind == MarkerKind::Intro)
                .count(),
            1
        );
    }

    #[test]
    fn learns_the_median_of_user_skips() {
        let obs = [(30.0, 120.0), (32.0, 118.0), (95.0, 200.0)];
        let learned = learn_from_series(&obs, MarkerKind::Intro, 2).unwrap();
        assert_eq!(learned.start_secs, 32.0, "median resists the outlier");
        assert_eq!(learned.end_secs, 120.0);
        assert_eq!(learned.source, MarkerSource::Learned);
    }

    #[test]
    fn a_single_skip_is_not_yet_a_pattern() {
        assert!(learn_from_series(&[(30.0, 120.0)], MarkerKind::Intro, 2).is_none());
        // ...but one is enough when the caller says so.
        assert!(learn_from_series(&[(30.0, 120.0)], MarkerKind::Intro, 1).is_some());
    }

    #[test]
    fn learning_from_implausible_skips_yields_nothing() {
        // Two accidental one-second drags must not create a button.
        let obs = [(30.0, 30.5), (31.0, 31.4)];
        assert!(learn_from_series(&obs, MarkerKind::Intro, 2).is_none());
    }

    #[test]
    fn merge_prefers_chapters_over_user_over_learned() {
        let learned = vec![SkipMarker::new(
            MarkerKind::Intro,
            10.0,
            80.0,
            MarkerSource::Learned,
        )];
        let user = vec![SkipMarker::new(
            MarkerKind::Intro,
            20.0,
            90.0,
            MarkerSource::User,
        )];
        let chapters = vec![SkipMarker::new(
            MarkerKind::Intro,
            30.0,
            100.0,
            MarkerSource::Chapters,
        )];

        let merged = merge(&[learned.clone(), user.clone(), chapters.clone()]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].source, MarkerSource::Chapters);

        // Order of the inputs must not matter.
        let merged2 = merge(&[chapters, user, learned]);
        assert_eq!(merged2[0].source, MarkerSource::Chapters);
    }

    #[test]
    fn merge_keeps_one_of_each_kind_sorted_by_start() {
        let merged = merge(&[vec![
            SkipMarker::new(MarkerKind::Credits, 2600.0, 2700.0, MarkerSource::Chapters),
            SkipMarker::new(MarkerKind::Intro, 45.0, 135.0, MarkerSource::Chapters),
        ]]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].kind, MarkerKind::Intro);
        assert_eq!(merged[1].kind, MarkerKind::Credits);
    }

    #[test]
    fn active_marker_tracks_the_playhead_and_releases_early() {
        let markers = vec![SkipMarker::new(
            MarkerKind::Intro,
            45.0,
            135.0,
            MarkerSource::Chapters,
        )];
        assert!(active_marker(&markers, 44.0).is_none());
        assert!(active_marker(&markers, 45.0).is_some());
        assert!(active_marker(&markers, 134.0).is_some());
        // Gone just before the region ends, so it never covers real content.
        assert!(active_marker(&markers, 134.6).is_none());
        assert!(active_marker(&markers, 200.0).is_none());
    }

    #[test]
    fn up_next_prefers_the_credits_marker() {
        let markers = vec![SkipMarker::new(
            MarkerKind::Credits,
            2550.0,
            2700.0,
            MarkerSource::Chapters,
        )];
        assert_eq!(up_next_at(&markers, 2700.0, 45.0), Some(2550.0));
    }

    #[test]
    fn up_next_falls_back_to_a_tail_offset() {
        assert_eq!(up_next_at(&[], 2700.0, 45.0), Some(2655.0));
    }

    #[test]
    fn up_next_never_lands_in_the_first_half_of_a_short_episode() {
        // A 60s clip with a 45s tail would otherwise prompt at 15s.
        assert_eq!(up_next_at(&[], 60.0, 45.0), Some(30.0));
    }

    #[test]
    fn up_next_is_none_without_a_duration() {
        assert_eq!(up_next_at(&[], 0.0, 45.0), None);
    }

    #[test]
    fn kind_and_source_round_trip_through_strings() {
        for k in [MarkerKind::Intro, MarkerKind::Recap, MarkerKind::Credits] {
            assert_eq!(MarkerKind::parse(k.as_str()), Some(k));
        }
        for s in [
            MarkerSource::Chapters,
            MarkerSource::User,
            MarkerSource::Learned,
        ] {
            assert_eq!(MarkerSource::parse(s.as_str()), Some(s));
        }
        assert_eq!(MarkerKind::parse("nonsense"), None);
    }

    #[test]
    fn kind_serializes_to_the_names_the_ui_uses() {
        assert_eq!(
            serde_json::to_string(&MarkerKind::Intro).unwrap(),
            "\"intro\""
        );
        assert_eq!(
            serde_json::to_string(&MarkerSource::Learned).unwrap(),
            "\"learned\""
        );
    }
}
