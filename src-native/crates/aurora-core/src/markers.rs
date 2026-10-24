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
    /// A forward jump the viewer made, which nothing has corroborated yet.
    ///
    /// Separate from `User` — which means the Skip button was *pressed*, an informed act
    /// on a region already on screen — because a bare seek is an inference about an
    /// intention. Dragging forward twenty seconds four minutes into an episode might be
    /// skipping the titles, or it might be impatience with a scene. One of those should
    /// teach the show something and the other should not, and the only honest reading of
    /// a single jump is "possibly".
    ///
    /// So it earns a button, which a viewer can confirm by pressing (and that press
    /// records `User`), and it never fires on its own and never spreads to other episodes
    /// on its own: `may_auto_skip` says no, and learning needs `MIN_AGREEING_SAMPLES` of
    /// these before any other episode inherits anything.
    Seek,
    /// From the episode's own duration and nothing else. Only ever the credits.
    Convention,
}

impl MarkerSource {
    pub fn as_str(self) -> &'static str {
        match self {
            MarkerSource::Chapters => "chapters",
            MarkerSource::User => "user",
            MarkerSource::Learned => "learned",
            MarkerSource::Seek => "seek",
            MarkerSource::Convention => "convention",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "chapters" => Some(MarkerSource::Chapters),
            "user" => Some(MarkerSource::User),
            "learned" => Some(MarkerSource::Learned),
            "seek" => Some(MarkerSource::Seek),
            "convention" => Some(MarkerSource::Convention),
            _ => None,
        }
    }

    /// Higher wins when two sources describe the same region.
    pub fn trust(self) -> u8 {
        match self {
            MarkerSource::Chapters => 5,
            MarkerSource::User => 4,
            MarkerSource::Learned => 3,
            // Below `Learned` deliberately. A median that several episodes agree on is
            // better evidence than one jump nothing has corroborated, and this used to be
            // the other way round by accident: a bare seek was recorded as `User`, so one
            // mis-drag outranked everything the rest of the show had to say.
            MarkerSource::Seek => 2,
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
        matches!(
            self,
            MarkerSource::Chapters | MarkerSource::User | MarkerSource::Learned
        )
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
const INTRO_WORDS: &[&str] = &["intro", "opening", "titles", "theme", "maintitles"];
const RECAP_WORDS: &[&str] = &["recap", "previously", "previouslyon", "resume"];
const CREDITS_WORDS: &[&str] = &["credits", "endcredits", "ending", "outro", "closing"];

/// Keywords that are also ordinary words, so they only count as the *whole* title.
///
/// "OP" and "ED" are how anime labels its opening and ending, and they are also a
/// preposition and a name. Matched word-wise, as they were, a chapter titled
/// `Ed Loses the Farm` read as the credits and `position_is_sane` only caught it when
/// the chapter happened to be in the wrong part of the file. The module header above is
/// explicit that whole-word matching was chosen so `Introduction to the Case` is not an
/// intro; these two came in through the same door (R-14).
///
/// A trailing number is allowed, because that is how they appear: `OP`, `ED`, `OP1`,
/// `ED2`.
const WHOLE_TITLE_INTRO: &[&str] = &["op"];
const WHOLE_TITLE_CREDITS: &[&str] = &["ed"];

/// Whether the whole title is one of `set`, optionally followed by a number.
fn is_whole_title(joined: &str, set: &[&str]) -> bool {
    set.iter().any(|w| {
        joined
            .strip_prefix(w)
            .is_some_and(|rest| rest.is_empty() || rest.chars().all(|c| c.is_ascii_digit()))
    })
}

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
    } else if hit(INTRO_WORDS) || is_whole_title(&joined, WHOLE_TITLE_INTRO) {
        Some(MarkerKind::Intro)
    } else if hit(CREDITS_WORDS) || is_whole_title(&joined, WHOLE_TITLE_CREDITS) {
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

/// Whether a marker's position agrees with what it claims to be.
///
/// The same rule chapters are held to, exposed because a marker arriving from the UI is
/// held to it now as well. It was not: `library_record_skip` checked only a region's
/// *length*, so an intro could be recorded an hour into a film while a chapter saying
/// the same thing was refused — two paths to one table disagreeing about what an intro
/// is. Unreachable from the button, which can only pass bounds that came from a marker
/// that had already passed this, and a command is reachable by more than its caller.
///
/// `duration_secs` of 0 means the runtime is unknown, which is ordinary on a provider's
/// stream: intros and recaps then get the first ten minutes, and credits cannot be
/// placed at all, so they are let through rather than guessed at.
pub fn position_fits_kind(marker: &SkipMarker, duration_secs: f64) -> bool {
    let duration = if duration_secs.is_finite() && duration_secs > 0.0 {
        duration_secs
    } else {
        0.0
    };
    position_is_sane(marker.kind, marker, duration)
}

/// Reject markers whose position contradicts their label.
fn position_is_sane(kind: MarkerKind, marker: &SkipMarker, duration_secs: f64) -> bool {
    match kind {
        // Intros and recaps live near the front: first 10 minutes, or first third of
        // a short episode, whichever is more generous -- and never more than 15, which
        // is the half the sentence above did not have.
        //
        // `max(600.0)` alone grows the window with the runtime, so on a three-hour film
        // the first third is an hour and a mis-titled chapter that far in was accepted
        // as an intro. The generosity exists for files too short for ten minutes to be
        // near the front; it has no reason to keep widening once they are long (R-15).
        MarkerKind::Intro | MarkerKind::Recap => {
            marker.start_secs <= (duration_secs / 3.0).clamp(600.0, 900.0)
        }
        // Credits live in the last quarter.
        MarkerKind::Credits => marker.start_secs >= duration_secs * 0.75,
    }
}

/// How far apart two observations may be and still be describing the same region.
///
/// Episodes of one show do not have identical intros: a cold open runs a different length
/// each week, and the viewer's own thumb lands somewhere different each time. Twenty
/// seconds is wide enough for that and narrow enough that an intro and a scene somebody
/// skipped elsewhere in the first few minutes do not read as the same thing.
pub const AGREEMENT_SECS: f64 = 20.0;

/// How many observations have to agree before a show teaches its other episodes.
///
/// Two, not one. The old value was one, with its own comment admitting the cost: "one
/// mis-drag colours the show". It did, and worse than the comment says — the median of a
/// single observation *is* that observation, so a single twenty-second nudge became a
/// marker on every episode of the show, and a `User` one, which the per-show "always
/// skip" preference is allowed to fire without asking. One accident, silently applied to
/// a whole series for ever.
///
/// Two agreeing observations is not a high bar for a real intro -- a viewer who skips the
/// titles does it every episode -- and it is a bar an accident does not clear, because an
/// accident does not repeat in the same place.
pub const MIN_AGREEING_SAMPLES: usize = 2;

/// The largest set of observations that all describe the same region.
///
/// Agreement is on both ends: two jumps that start together and end a minute apart are
/// not the same region. Quadratic, over a handful of rows per show per kind.
fn largest_agreeing(observations: &[(f64, f64)]) -> Vec<&(f64, f64)> {
    let mut best: Vec<&(f64, f64)> = Vec::new();
    for anchor in observations {
        let cluster: Vec<&(f64, f64)> = observations
            .iter()
            .filter(|(s, e)| {
                (s - anchor.0).abs() <= AGREEMENT_SECS && (e - anchor.1).abs() <= AGREEMENT_SECS
            })
            .collect();
        if cluster.len() > best.len() {
            best = cluster;
        }
    }
    best
}

/// Infer a marker for an episode that has none, from what the rest of the series shows.
///
/// Two deliberate choices, both about skipping *less* rather than guessing more:
///
/// **Agreement, not an average.** Only the observations that agree with each other count,
/// and if fewer than `min_samples` of them do, the show has not taught anything. The
/// medians this used to take were independent — median of the starts and median of the
/// ends — which can synthesise a region no episode ever had: observations of `0..90` and
/// `55..115` gave `27.5..102.5`, which is not either intro, is not the average of them in
/// any meaningful sense, and begins half a minute into content the viewer wanted.
///
/// **The intersection, not the union.** What comes back is the region every agreeing
/// observation covers — the latest start and the earliest end. Being short costs the
/// viewer a few seconds of titles they can sit through; being long costs them the opening
/// of the episode, which is the thing they cannot get back without rewinding. The error
/// is asymmetric, so this leans on the safe side of it.
pub fn learn_from_series(
    observations: &[(f64, f64)],
    kind: MarkerKind,
    min_samples: usize,
) -> Option<SkipMarker> {
    let cluster = largest_agreeing(observations);
    if cluster.len() < min_samples.max(1) {
        return None;
    }
    let start = cluster.iter().map(|(s, _)| *s).fold(f64::MIN, f64::max);
    let end = cluster.iter().map(|(_, e)| *e).fold(f64::MAX, f64::min);
    let marker = SkipMarker::new(kind, start, end, MarkerSource::Learned);
    marker.is_plausible().then_some(marker)
}

/// Trim overlaps so no two buttons claim the same second of video.
///
/// Two markers of different kinds can overlap honestly — a chapter list that says the
/// recap runs to 0:90 and a learned intro that starts at 0:75 are both doing their best —
/// and `active_marker` takes the first match in start order, so the overlap decided which
/// *label* the button wore by an accident of sorting. A viewer pressing "Skip Recap" then
/// landed in the middle of the titles, or pressed "Skip Intro" and lost the last of the
/// recap.
///
/// The less trusted marker yields: its start moves to the end of the better one. If that
/// leaves it too short to be worth a button it goes altogether, because half a region is
/// not a skip anybody wants.
fn separate(markers: &mut Vec<SkipMarker>) {
    markers.sort_by(|a, b| {
        a.start_secs
            .partial_cmp(&b.start_secs)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut out: Vec<SkipMarker> = Vec::new();
    for mut m in markers.drain(..) {
        if let Some(previous) = out.last_mut() {
            if m.start_secs < previous.end_secs {
                if m.source.trust() > previous.source.trust() {
                    // The better marker keeps its region; the one already placed gives up
                    // the tail they share.
                    previous.end_secs = m.start_secs;
                    if !previous.is_plausible() {
                        out.pop();
                    }
                } else {
                    m.start_secs = previous.end_secs;
                }
            }
        }
        if m.is_plausible() {
            out.push(m);
        }
    }
    *markers = out;
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
    separate(&mut best);
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

/// The shortest forward jump that is a statement about where the intro is.
///
/// Longer than [`MIN_MARKER_SECS`] on purpose. A marker only has to be five seconds to be
/// worth a button, but a *seek* that short is navigation rather than an opinion: the
/// player's own overlay has ±10-second buttons, and a viewer nudging forward twice near
/// the start of an episode would otherwise teach the show that its intro is the twenty
/// seconds they happened to cross. An intro is "usually about a minute", so twenty
/// seconds is comfortably below any real one and above every nudge the UI itself offers.
pub const MIN_SEEK_EVIDENCE_SECS: f64 = 20.0;

/// How far into an episode a *seek* may start and still be read as skipping the titles.
///
/// Much tighter than the window a chapter gets. A chapter titled "Intro" is a statement
/// about the file, so it is believed wherever it plausibly sits; a seek is an inference
/// about why somebody moved their thumb, and the further in it happens the worse that
/// inference gets. Intros — including after a cold open — are over well inside three
/// minutes; a jump at four is far more likely to be impatience with a scene.
///
/// This is the gate that was missing. `position_is_sane` gave a seek the same ten-minute
/// window a chapter gets, so a twenty-second nudge four minutes into an episode was
/// recorded as that show's intro and applied to every episode of it.
pub const SEEK_INTRO_WINDOW_SECS: f64 = 180.0;

/// Intro evidence from a forward jump the viewer made by hand.
///
/// Tier 2 is "the user — when someone skips manually, where they skipped is recorded",
/// and the only thing that ever recorded a skip was the Skip button. So the evidence for
/// an intro was reachable only through a button that evidence for an intro is what puts
/// on screen. A release with chapters got its buttons; a library whose streams carry
/// none — the ordinary IPTV case, and chapters are rarer in a provider's VOD than a
/// duration is — could never show Skip Intro, and a viewer had no way to teach it,
/// because tier 4 places no intro and tier 3 has nothing to take a median of.
///
/// Dragging past the titles *is* the manual skip tier 2 describes. This turns it into
/// the same evidence the button records, which is what closes the loop.
///
/// `None` unless the jump looks like an intro, through the same two gates a chapter goes
/// through — long enough to be worth a button and short enough not to be a scrub across
/// the episode, and beginning where an intro can be. Deliberately the same, so a seek
/// cannot produce a marker a chapter could not have.
pub fn intro_from_seek(from_secs: f64, to_secs: f64, duration_secs: f64) -> Option<SkipMarker> {
    seek_evidence(from_secs, to_secs, duration_secs, &[])
}

/// What a forward jump the viewer made is evidence *of*.
///
/// Three things this decides, and the old `intro_from_seek` got two of them wrong.
///
/// **Which kind.** It called every jump an intro. But the first thing a viewer skips is
/// very often the recap — "Previously on…" — and recording that as the intro put a button
/// labelled "Skip Intro" over the recap, left the real intro with no marker, and gave the
/// show a learned intro in the wrong place. `known` is what the episode already has from
/// better sources, so a jump that starts inside a known recap is recap evidence and a jump
/// that starts inside a known intro is intro evidence. With nothing known it is an intro,
/// which is the commoner case and the one a viewer can correct by pressing the button.
///
/// **How far in.** `SEEK_INTRO_WINDOW_SECS`, not the ten minutes a chapter gets. See that
/// constant.
///
/// **What it is worth.** `MarkerSource::Seek`, not `User`. A press of the Skip button is
/// `User`; a drag is a guess at the same intention and ranks below a median the rest of
/// the show agrees on.
///
/// `None` unless the jump passes every gate a chapter passes — long enough to be worth a
/// button, short enough not to be a scrub across the episode, and beginning where the kind
/// it is being read as can actually begin.
pub fn seek_evidence(
    from_secs: f64,
    to_secs: f64,
    duration_secs: f64,
    known: &[SkipMarker],
) -> Option<SkipMarker> {
    if !from_secs.is_finite() || !to_secs.is_finite() {
        return None;
    }
    // A stream that reports no duration is the common case here, and `position_is_sane`
    // reads 0 as "the first ten minutes", which is the right window when the runtime is
    // unknown. Anything not a real number is treated the same way rather than silently
    // rejecting every seek.
    let duration = if duration_secs.is_finite() && duration_secs > 0.0 {
        duration_secs
    } else {
        0.0
    };
    // What the viewer was sitting in when they jumped, where the episode already knows.
    // Credits are not a candidate: a jump out of the credits is going to the next episode,
    // not teaching anything, and `SEEK_INTRO_WINDOW_SECS` excludes it anyway.
    let kind = known
        .iter()
        .filter(|m| matches!(m.kind, MarkerKind::Recap | MarkerKind::Intro))
        .find(|m| from_secs >= m.start_secs && from_secs < m.end_secs)
        .map(|m| m.kind)
        .unwrap_or(MarkerKind::Intro);

    let marker = SkipMarker::new(kind, from_secs, to_secs, MarkerSource::Seek);
    if marker.duration_secs() < MIN_SEEK_EVIDENCE_SECS {
        return None;
    }
    if from_secs > SEEK_INTRO_WINDOW_SECS {
        return None;
    }
    if !marker.is_plausible() || !position_is_sane(kind, &marker, duration) {
        return None;
    }
    Some(marker)
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
    // The floor used to be half the runtime, which is not a floor on a short file: on a
    // two-minute one `duration - 60` *is* half of it, so the Up Next card came up over the
    // middle of the episode. A card offering the next episode half way through this one is
    // not an aid, it is a mistake the viewer has to dismiss.
    //
    // `MIN_UP_NEXT_RATIO` instead, so the card is always in the tail whatever the runtime,
    // and a file too short for a tail to mean anything gets no card at all.
    if duration_secs < MIN_EPISODE_FOR_CONVENTION_SECS {
        return None;
    }
    Some((duration_secs - fallback_tail_secs).max(duration_secs * MIN_UP_NEXT_RATIO))
}

/// The earliest point in an episode the Up Next card may appear, as a fraction of the
/// runtime. Anything earlier is over content rather than over the tail.
pub const MIN_UP_NEXT_RATIO: f64 = 0.8;

#[cfg(test)]
mod seek_tests {
    use super::*;

    /// The case the whole function exists for: a provider stream with no chapters, where
    /// the viewer drags past the titles. Before this, that taught the library nothing and
    /// Skip Intro could never appear on any episode of the show.
    #[test]
    fn dragging_past_the_titles_is_intro_evidence() {
        let marker = intro_from_seek(8.0, 98.0, 45.0 * 60.0).expect("a 90-second jump at 8s");
        assert_eq!(marker.kind, MarkerKind::Intro);
        assert_eq!(marker.start_secs, 8.0);
        assert_eq!(marker.end_secs, 98.0);
        assert!(marker.is_plausible());
        // `Seek`, not `User`: a drag is a guess at an intention and a press of the button
        // is that intention stated. The difference is what the viewer gets for free --
        // nothing happens on its own from one jump.
        assert_eq!(marker.source, MarkerSource::Seek);
        assert!(
            !marker.source.may_auto_skip(),
            "one drag must never become a silent jump on every later episode"
        );
        assert!(
            marker.source.trust() < MarkerSource::Learned.trust(),
            "a jump nothing corroborates must not outrank what the show agrees on"
        );
    }

    /// Which kind a jump is evidence of, when the episode already knows something.
    ///
    /// A viewer's first skip is very often the recap, and calling it the intro put a
    /// button labelled "Skip Intro" over "Previously on...", left the real intro with no
    /// marker at all, and taught the series an intro in the recap's place.
    #[test]
    fn a_jump_out_of_a_known_recap_is_recap_evidence() {
        let known = [SkipMarker::new(
            MarkerKind::Recap,
            0.0,
            70.0,
            MarkerSource::Chapters,
        )];
        let out_of_the_recap =
            seek_evidence(5.0, 68.0, 45.0 * 60.0, &known).expect("a jump inside the recap");
        assert_eq!(out_of_the_recap.kind, MarkerKind::Recap);

        // And a jump that starts after the recap is still the intro.
        let after = seek_evidence(72.0, 140.0, 45.0 * 60.0, &known).expect("a jump after it");
        assert_eq!(after.kind, MarkerKind::Intro);

        // With nothing known it is an intro, which is the commoner case and the one the
        // viewer can correct by pressing the button.
        assert_eq!(
            seek_evidence(5.0, 68.0, 45.0 * 60.0, &[]).unwrap().kind,
            MarkerKind::Intro
        );
    }

    /// The credits are never what a jump teaches.
    ///
    /// Jumping out of the credits is going to the next episode, not a statement about
    /// where they start.
    #[test]
    fn a_jump_is_never_credits_evidence() {
        let known = [SkipMarker::new(
            MarkerKind::Credits,
            60.0,
            150.0,
            MarkerSource::Chapters,
        )];
        let m = seek_evidence(70.0, 140.0, 45.0 * 60.0, &known).expect("a jump at 70s");
        assert_eq!(m.kind, MarkerKind::Intro, "not Credits: {m:?}");
    }

    /// A stream that reports no duration at all, which is the ordinary case on a
    /// provider's VOD and the thing that broke the chapter path in the first place.
    #[test]
    fn an_unknown_duration_still_allows_the_first_ten_minutes() {
        assert!(intro_from_seek(30.0, 120.0, 0.0).is_some());
        assert!(intro_from_seek(30.0, 120.0, f64::NAN).is_some());
        // Still bounded: 11 minutes in is not an intro even with no runtime to go on.
        assert!(intro_from_seek(660.0, 720.0, 0.0).is_none());
    }

    #[test]
    fn a_backward_or_stationary_seek_is_not_a_skip() {
        assert!(intro_from_seek(120.0, 30.0, 45.0 * 60.0).is_none());
        assert!(intro_from_seek(120.0, 120.0, 45.0 * 60.0).is_none());
    }

    /// The same length gates a chapter goes through. A nudge is not a statement about
    /// where the intro is, and a scrub across the episode is not one either.
    /// The overlay's own ±10-second buttons must not teach anything, which is the whole
    /// reason seek evidence has a higher floor than a marker does.
    #[test]
    fn a_nudge_and_a_scrub_are_both_refused() {
        assert!(
            intro_from_seek(10.0, 13.0, 45.0 * 60.0).is_none(),
            "3s nudge"
        );
        assert!(
            intro_from_seek(30.0, 40.0, 45.0 * 60.0).is_none(),
            "the overlay's own 10-second skip"
        );
        assert!(
            intro_from_seek(30.0, 49.0, 45.0 * 60.0).is_none(),
            "two nudges, still not an opinion"
        );
        assert!(
            intro_from_seek(10.0, 10.0 + MAX_MARKER_SECS + 1.0, 45.0 * 60.0).is_none(),
            "a scrub past the window"
        );
        // The boundaries themselves are accepted, so the gate is the documented one.
        assert!(intro_from_seek(10.0, 10.0 + MIN_SEEK_EVIDENCE_SECS, 45.0 * 60.0).is_some());
        assert!(intro_from_seek(10.0, 10.0 + MAX_MARKER_SECS, 45.0 * 60.0).is_some());
        // And a marker's own floor is lower than this one, so the two are not confused.
        // Checked at compile time, because both sides are constants.
        const { assert!(MIN_SEEK_EVIDENCE_SECS > MIN_MARKER_SECS) };
    }

    /// A jump in the middle of an episode is somebody skipping a dull scene, and calling
    /// it an intro would put a button over content on every other episode of the show.
    #[test]
    fn a_jump_in_the_middle_is_not_an_intro() {
        let hour = 60.0 * 60.0;
        assert!(intro_from_seek(hour, hour + 60.0, 2.0 * hour).is_none());
    }

    /// `position_fits_kind` is the same rule, reachable by the command that records what
    /// the Skip button was pressed on. That command checked length alone, so these are
    /// the cases it used to accept.
    #[test]
    fn a_recorded_region_has_to_sit_where_its_kind_belongs() {
        let film = 2.0 * 60.0 * 60.0;
        let hour_in = SkipMarker::new(MarkerKind::Intro, 3600.0, 3690.0, MarkerSource::User);
        assert!(
            hour_in.is_plausible(),
            "length alone was all that was checked"
        );
        assert!(
            !position_fits_kind(&hour_in, film),
            "an intro an hour into a film is not an intro"
        );

        let titles = SkipMarker::new(MarkerKind::Intro, 20.0, 110.0, MarkerSource::User);
        assert!(position_fits_kind(&titles, film));

        // Credits belong in the last quarter, and cannot be placed without a runtime —
        // so an unknown duration lets them through rather than guessing.
        let tail = SkipMarker::new(MarkerKind::Credits, film - 60.0, film, MarkerSource::User);
        assert!(position_fits_kind(&tail, film));
        let too_early = SkipMarker::new(MarkerKind::Credits, 600.0, 660.0, MarkerSource::User);
        assert!(!position_fits_kind(&too_early, film));
        assert!(
            position_fits_kind(&too_early, 0.0),
            "no runtime, no placing it"
        );
    }

    /// A seek gets a much tighter window than a chapter, and this is why.
    ///
    /// A chapter titled "Intro" is a statement about the file, believed wherever it
    /// plausibly sits -- `position_is_sane` gives it ten minutes, fifteen on a long one.
    /// A seek is an inference about why somebody moved their thumb, and it decays fast:
    /// at four minutes in, a twenty-second jump is far more likely to be impatience with
    /// a scene than the titles.
    ///
    /// Sharing the chapter window is the defect this closes. A nudge four minutes into
    /// one episode was recorded as the show's intro and, with `min_samples` of one,
    /// applied to every episode of it.
    #[test]
    fn a_seek_gets_a_tighter_window_than_a_chapter() {
        let three_hours = 3.0 * 60.0 * 60.0;
        // A chapter at 14:30 of a three-hour film is still allowed to call itself an
        // intro...
        let late_chapter = SkipMarker::new(MarkerKind::Intro, 870.0, 930.0, MarkerSource::Chapters);
        assert!(position_fits_kind(&late_chapter, three_hours));
        // ...and the identical jump is not evidence of one.
        assert!(intro_from_seek(870.0, 930.0, three_hours).is_none());

        // The boundary, stated: three minutes in is the last moment a drag can mean the
        // titles.
        assert!(intro_from_seek(175.0, 240.0, three_hours).is_some());
        assert!(intro_from_seek(185.0, 250.0, three_hours).is_none());

        // The case the window is generous enough for: a cold open, then the titles.
        assert!(
            intro_from_seek(95.0, 160.0, 45.0 * 60.0).is_some(),
            "a skip after a cold open is the case this has to keep"
        );
    }

    /// The accident this rework exists to stop, end to end.
    #[test]
    fn an_impatient_nudge_mid_episode_teaches_nothing() {
        let episode = 45.0 * 60.0;
        // Four minutes in, a twenty-five second jump. Under the old window this became
        // the show's intro.
        assert!(intro_from_seek(240.0, 265.0, episode).is_none());
        // And even inside the window, one jump cannot reach another episode: learning
        // needs agreement.
        let one = [(8.0, 98.0)];
        assert!(learn_from_series(&one, MarkerKind::Intro, MIN_AGREEING_SAMPLES).is_none());
    }
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

    /// The window for an intro stops widening once the file is long (R-15).
    #[test]
    fn an_intro_an_hour_into_a_long_film_is_rejected() {
        // Three hours, and a chapter titled "Intro" an hour in. A third of the runtime
        // is 3,600s, so the old rule accepted this as an intro and offered a button that
        // skips an hour of a film.
        let long = from_chapters(
            &[ch("Part 1", 0.0), ch("Intro", 3600.0), ch("Rest", 3700.0)],
            10800.0,
        );
        assert!(long.is_empty(), "unexpected markers: {long:?}");

        // The generosity it replaces is still there for the files it was for: a short
        // episode where ten minutes really is near the front.
        let short = from_chapters(&[ch("Intro", 540.0), ch("Show", 600.0)], 1200.0);
        assert_eq!(
            short.len(),
            1,
            "a 9min mark in a 20min episode is still an intro"
        );
        assert_eq!(short[0].kind, MarkerKind::Intro);

        // And the ceiling is where it says it is: 15 minutes in is in, 16 is out,
        // whatever the runtime.
        let inside = from_chapters(&[ch("Intro", 890.0), ch("Show", 950.0)], 10800.0);
        assert_eq!(inside.len(), 1, "unexpected: {inside:?}");
        let outside = from_chapters(&[ch("Intro", 960.0), ch("Show", 1020.0)], 10800.0);
        assert!(outside.is_empty(), "unexpected markers: {outside:?}");
    }

    /// `OP` and `ED` only count as the whole title (R-14).
    #[test]
    fn a_name_that_happens_to_be_a_keyword_is_not_a_marker() {
        // `Ed Loses the Farm` two thirds in used to read as the credits, and the
        // positional check only catches that when the chapter is in the wrong place --
        // so in the last quarter it was accepted.
        let ed = from_chapters(
            &[
                ch("Part 1", 0.0),
                ch("Ed Loses the Farm", 2200.0),
                ch("End", 2600.0),
            ],
            2700.0,
        );
        assert!(ed.is_empty(), "unexpected markers: {ed:?}");

        // `Op` as an ordinary word near the front, which is where an intro would be.
        let op = from_chapters(&[ch("Signed Op Orders", 0.0), ch("Show", 90.0)], 2700.0);
        assert!(op.is_empty(), "unexpected markers: {op:?}");

        // The anime convention still works, which is why the words were there at all.
        let anime = from_chapters(&[ch("OP", 0.0), ch("Episode", 90.0)], 1400.0);
        assert_eq!(anime.len(), 1, "unexpected: {anime:?}");
        assert_eq!(anime[0].kind, MarkerKind::Intro);

        let numbered = from_chapters(&[ch("Show", 0.0), ch("ED2", 1300.0)], 1400.0);
        assert_eq!(numbered.len(), 1, "unexpected: {numbered:?}");
        assert_eq!(numbered[0].kind, MarkerKind::Credits);
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

    /// What the agreeing observations all cover, and nothing beyond it.
    #[test]
    fn learns_the_region_the_observations_agree_on() {
        let obs = [(30.0, 120.0), (32.0, 118.0), (95.0, 200.0)];
        let learned = learn_from_series(&obs, MarkerKind::Intro, 2).unwrap();
        // The outlier at 95s agrees with nothing, so it is not in the cluster at all --
        // and the two that do agree are skipped only where both of them say to.
        assert_eq!(learned.start_secs, 32.0, "the latest start the cluster has");
        assert_eq!(learned.end_secs, 118.0, "the earliest end it has");
        assert_eq!(learned.source, MarkerSource::Learned);
    }

    /// The region the independent medians used to invent.
    ///
    /// `median(starts)` and `median(ends)` were taken separately, so two observations
    /// that do not describe the same region produced one that neither episode had:
    /// `0..90` and `55..115` gave `27.5..102.5`, which starts half a minute into content
    /// the viewer wanted and ends inside the episode. Disagreement has to mean "nothing
    /// learned", not "something averaged".
    #[test]
    fn observations_that_disagree_teach_nothing() {
        let obs = [(0.0, 90.0), (55.0, 115.0)];
        assert!(
            learn_from_series(&obs, MarkerKind::Intro, 2).is_none(),
            "two different regions must not be averaged into a third"
        );
        // Each on its own is a cluster of one, so a caller willing to act on one sample
        // gets a real observation rather than a synthesised one.
        let single = learn_from_series(&obs[..1], MarkerKind::Intro, 1).unwrap();
        assert_eq!((single.start_secs, single.end_secs), (0.0, 90.0));
    }

    /// Agreement is on both ends. Two jumps that start together and end a minute apart
    /// are not the same region.
    #[test]
    fn agreement_is_on_both_ends_of_a_region() {
        let same_start = [(10.0, 40.0), (12.0, 130.0)];
        assert!(learn_from_series(&same_start, MarkerKind::Intro, 2).is_none());
    }

    /// An intersection too short to be worth a button is no marker.
    #[test]
    fn an_intersection_too_short_to_press_is_not_offered() {
        // They agree within the tolerance, but what both cover is under
        // `MIN_MARKER_SECS`.
        let barely = [(10.0, 30.0), (28.0, 48.0)];
        let learned = learn_from_series(&barely, MarkerKind::Intro, 2);
        assert!(learned.is_none(), "{learned:?}");
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

    /// A file too short for a tail to mean anything gets no card.
    ///
    /// The floor used to be half the runtime, which is not a floor at all on a short
    /// file: on a 60-second clip with a 45-second tail, `duration - tail` *is* 15s and
    /// half the runtime is 30s, so the card came up at the half-way mark. An Up Next
    /// prompt over the middle of what you are watching is not an aid.
    #[test]
    fn a_clip_too_short_for_a_tail_gets_no_up_next_card() {
        assert_eq!(up_next_at(&[], 60.0, 45.0), None);
        assert_eq!(up_next_at(&[], 9.0 * 60.0, 45.0), None);
    }

    /// And where there is a card, it is always in the tail.
    #[test]
    fn up_next_is_always_in_the_last_fifth() {
        for duration in [600.0, 1320.0, 2700.0, 7200.0] {
            let at = up_next_at(&[], duration, 45.0).expect("a real episode gets a card");
            assert!(
                at >= duration * MIN_UP_NEXT_RATIO,
                "{duration}s episode prompts at {at}s, which is not its tail"
            );
            assert!(at < duration);
        }
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
