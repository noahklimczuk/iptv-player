//! What to watch next, worked out from what has been watched.
//!
//! Content-based, because that is the only kind that can work here. A collaborative
//! filter needs other people's viewing, and Aurora has exactly one viewer and no
//! server to pool anything on — so the signal has to come from the library itself:
//! genres, era, rating, and how much of each thing the viewer actually sat through.
//!
//! Deliberately pure. Nothing in this module touches SQLite, the network or a clock —
//! `now` is a parameter — so the whole algorithm is exercisable from a unit test with
//! a handful of structs, which is how the weighting below was arrived at rather than
//! guessed. `aurora-db` loads the rows; `aurora-app` turns the result into a rail.
//!
//! Three things it is careful about, because each is a way recommendations go wrong:
//!
//! * **Half-watched is not watched.** Something abandoned after four minutes says
//!   almost nothing, and treating it as a preference is how one bad Tuesday evening
//!   poisons a rail for a month. Engagement scales with the fraction actually seen,
//!   and anything under [`MIN_ENGAGEMENT`] is ignored outright.
//! * **Taste moves.** A film watched last night should count for more than one
//!   watched in March, so every signal decays with a half-life of
//!   [`HALF_LIFE_DAYS`].
//! * **Twenty of the same thing is not a recommendation.** A viewer who likes one
//!   genre will score every title in it highly, and a rail of twenty action films is
//!   useless. Selection penalises a genre each time it is picked, so the list stays
//!   recognisably theirs without being one note.

use std::collections::HashMap;

/// Below this fraction of an item, watching it says nothing about taste.
///
/// Five per cent of a two-hour film is six minutes: enough to decide it is not for
/// you, not enough to be evidence that it is.
pub const MIN_ENGAGEMENT: f32 = 0.05;

/// How long a signal takes to count half as much. Thirty days is roughly "last month
/// still matters, last year mostly does not".
pub const HALF_LIFE_DAYS: f32 = 30.0;

/// What an explicit favourite is worth against something merely watched.
pub const FAVOURITE_BOOST: f32 = 1.5;

/// How much each ingredient contributes to a score. They sum to 1, so a score is
/// always 0..1 and the numbers below can be read as percentages of the decision.
///
/// Genre leads where there is one, but a freshly imported library has none: genres
/// arrive from TMDB enrichment, which needs an API key that a viewer may never set.
/// What a panel does give, always and for free, is the category it filed each title
/// under — "EN ✪ BOX OFFICE", "FR ✪ ACTION", "DE ✪ FILME". Noisier than a genre,
/// because it mixes language, provider and genre together, and for exactly that
/// reason a real signal about what somebody watches. Without it this recommender had
/// nothing to say until an API key was configured.
const GENRE_WEIGHT: f32 = 0.45;
const CATEGORY_WEIGHT: f32 = 0.20;
const RATING_WEIGHT: f32 = 0.18;
const ERA_WEIGHT: f32 = 0.10;
const FRESH_WEIGHT: f32 = 0.07;

/// The most a title that matches nothing they watch can score.
///
/// Below the floor of anything that did match, so discovery fills the slots the
/// matches left rather than competing for them.
const DISCOVERY_CEILING: f32 = 0.20;

/// The most of one rail any single genre or shelf may take, as a fraction.
///
/// Somebody who has watched three films off one shelf has told us one thing, and
/// twenty more from that shelf is not twenty times as useful — it is a wall. The
/// diversity decay alone cannot help when their taste has only one bucket in it and
/// every match comes from there, which is exactly the state a first evening leaves.
const MAX_SHARE_PER_BUCKET: f32 = 0.35;

/// What a genre is multiplied by each further time it is picked.
///
/// 0.55 means the third film in a genre scores about a third of what the first did,
/// which in practice lets a strong preference take two or three of the top slots and
/// no more.
const DIVERSITY_DECAY: f32 = 0.55;

/// How far back an "added to your library" boost reaches.
const FRESH_WINDOW_DAYS: f32 = 30.0;

const SECS_PER_DAY: f32 = 86_400.0;

/// The two things that can be recommended. Channels cannot: live TV is a schedule
/// rather than a catalogue, and "you might like BBC One at 9pm" is not a thing anyone
/// wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Movie,
    Series,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Movie => "movie",
            Kind::Series => "series",
        }
    }
}

/// Something in the library that could be recommended.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Candidate {
    pub kind: Option<KindTag>,
    pub id: i64,
    pub title: String,
    pub genres: Vec<String>,
    /// The provider's own category for it, which exists from the first import where
    /// genres do not.
    pub category: Option<String>,
    pub year: Option<i32>,
    /// Out of ten, as TMDB and most panels report it.
    pub rating: Option<f32>,
    /// When it appeared in the library, for a mild nudge towards new arrivals.
    pub added_at: Option<i64>,
}

/// `Kind` is not `Default`, and `Candidate` is much nicer to build in a test if it is.
pub type KindTag = Kind;

impl Candidate {
    pub fn kind(&self) -> Kind {
        self.kind.unwrap_or(Kind::Movie)
    }
}

/// Something the viewer has actually watched, with how much of it they saw.
#[derive(Debug, Clone, PartialEq)]
pub struct Watched {
    pub kind: Kind,
    pub id: i64,
    pub title: String,
    pub genres: Vec<String>,
    /// The provider's own category, as on [`Candidate`].
    pub category: Option<String>,
    pub year: Option<i32>,
    pub rating: Option<f32>,
    /// 0..1. An episode contributes its series' fraction; a completed item is 1.
    pub fraction: f32,
    /// When it was last watched.
    pub updated_at: i64,
    /// On a list. Counts for more than merely having been watched.
    pub favourite: bool,
}

/// Why something was recommended, in the viewer's terms.
///
/// Carried rather than derived in the UI because the scoring is the only thing that
/// knows which ingredient actually decided it — and because a rail that cannot say
/// why is one nobody trusts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// Shares its strongest genre with something they watched.
    Because { title: String },
    /// Matches their taste in genre, without one title standing out.
    Genre { genre: String },
    /// Rated highly, in a library they have not told us much about yet.
    HighlyRated,
    /// Recently added.
    JustAdded,
}

impl Reason {
    /// The line a rail puts under a poster.
    pub fn label(&self) -> String {
        match self {
            Reason::Because { title } => format!("Because you watched {title}"),
            Reason::Genre { genre } => format!("More {genre}"),
            Reason::HighlyRated => "Highly rated".to_string(),
            Reason::JustAdded => "Just added".to_string(),
        }
    }
}

/// One recommendation.
#[derive(Debug, Clone, PartialEq)]
pub struct Recommendation {
    pub kind: Kind,
    pub id: i64,
    /// 0..1 before the diversity penalty; useful for a test and for a debug view.
    pub score: f32,
    pub reason: Reason,
}

/// What the viewer's history adds up to.
///
/// Built once and scored against every candidate, because a library of 140,000 rows
/// means this is walked a lot and rebuilding it per candidate would be the difference
/// between instant and not.
#[derive(Debug, Clone, Default)]
pub struct Taste {
    /// Genre to share of attention, summing to 1 when anything is known.
    genres: HashMap<String, f32>,
    /// The provider's categories, the same way. Kept separate from genres because
    /// they are a different kind of thing and must not be shown as one.
    categories: HashMap<String, f32>,
    /// Decade to share of attention.
    eras: HashMap<i32, f32>,
    /// What they tend to watch, out of ten — `None` when nothing watched had a rating.
    mean_rating: Option<f32>,
    /// Total decayed weight, which is how "do we know anything at all" is decided.
    weight: f32,
    /// The heaviest few titles, so a recommendation can name one.
    anchors: Vec<Anchor>,
}

#[derive(Debug, Clone)]
struct Anchor {
    title: String,
    genres: Vec<String>,
    category: Option<String>,
    weight: f32,
}

impl Taste {
    /// Whether there is enough history to say anything at all.
    pub fn is_known(&self) -> bool {
        self.weight > 0.0 && !(self.genres.is_empty() && self.categories.is_empty())
    }

    /// The genres they watch most, strongest first. For a rail heading.
    pub fn top_genres(&self, n: usize) -> Vec<String> {
        top(&self.genres, n)
    }

    /// The provider categories they watch most, strongest first.
    ///
    /// Used to *fetch* candidates, not to describe them. A library of 146,000 rows
    /// cannot be loaded to find twenty, and an arbitrary slice of it — the best-rated
    /// N, say — need not contain a single title from the shelves this viewer actually
    /// watches. On a panel that publishes no ratings, which is most of them until
    /// TMDB enrichment runs, that slice is effectively random.
    pub fn top_categories(&self, n: usize) -> Vec<String> {
        top(&self.categories, n)
    }

    /// Total decayed weight behind this profile.
    pub fn weight(&self) -> f32 {
        self.weight
    }
}

/// Build a taste profile from a watch history.
///
/// `now` is a Unix timestamp; everything is aged against it.
pub fn profile(history: &[Watched], now: i64) -> Taste {
    let mut taste = Taste::default();
    let mut rating_total = 0.0f32;
    let mut rating_weight = 0.0f32;

    for item in history {
        let Some(weight) = signal_weight(item, now) else {
            continue;
        };
        taste.weight += weight;

        // An item's vote is split across its genres, so a film tagged with five of
        // them does not count five times as much as one tagged with a single genre.
        let usable: Vec<&String> = item
            .genres
            .iter()
            .filter(|g| !g.trim().is_empty())
            .collect();
        if !usable.is_empty() {
            let share = weight / usable.len() as f32;
            for genre in &usable {
                *taste.genres.entry(normalise(genre)).or_insert(0.0) += share;
            }
        }

        if let Some(category) = item.category.as_deref().filter(|c| !c.trim().is_empty()) {
            *taste.categories.entry(normalise(category)).or_insert(0.0) += weight;
        }

        if let Some(year) = item.year {
            *taste.eras.entry(decade(year)).or_insert(0.0) += weight;
        }
        if let Some(rating) = item.rating.filter(|r| *r > 0.0) {
            rating_total += rating * weight;
            rating_weight += weight;
        }

        taste.anchors.push(Anchor {
            title: item.title.clone(),
            genres: usable.iter().map(|g| normalise(g)).collect(),
            category: item
                .category
                .as_deref()
                .filter(|c| !c.trim().is_empty())
                .map(normalise),
            weight,
        });
    }

    if rating_weight > 0.0 {
        taste.mean_rating = Some(rating_total / rating_weight);
    }
    normalise_shares(&mut taste.genres);
    normalise_shares(&mut taste.categories);
    normalise_shares(&mut taste.eras);
    // Heaviest first, so "because you watched…" names the thing they watched most
    // rather than whichever row came back first.
    taste.anchors.sort_by(|a, b| {
        b.weight
            .total_cmp(&a.weight)
            .then_with(|| a.title.cmp(&b.title))
    });
    taste.anchors.truncate(24);
    taste
}

/// Rank `candidates` for this taste, best first, excluding anything already seen.
///
/// `seen` is every `(kind, id)` the viewer has any history with — watched, part
/// watched, or sitting in Continue Watching. All of them are excluded: a rail of
/// things you have already watched is not a recommendation, and the half-watched ones
/// have a rail of their own.
pub fn rank(
    taste: &Taste,
    candidates: &[Candidate],
    seen: &dyn Fn(Kind, i64) -> bool,
    now: i64,
    limit: usize,
) -> Vec<Recommendation> {
    if limit == 0 {
        return Vec::new();
    }

    let mut scored: Vec<(f32, Recommendation, Vec<String>)> = candidates
        .iter()
        .filter(|c| !seen(c.kind(), c.id))
        .filter_map(|c| {
            let genres: Vec<String> = c
                .genres
                .iter()
                .filter(|g| !g.trim().is_empty())
                .map(|g| normalise(g))
                .collect();
            let category = c
                .category
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .map(normalise);
            let (score, reason) = score_one(taste, c, &genres, category.as_deref(), now);
            // The diversity pass works on whatever describes this title. On a library
            // with no genres that is the provider's category, and without it every
            // pick would come from one category.
            let buckets: Vec<String> = if genres.is_empty() {
                category.clone().into_iter().collect()
            } else {
                genres.clone()
            };
            // Zero means nothing about this candidate matched anything known. Padding
            // a rail with those is worse than a shorter rail.
            (score > 0.0).then(|| {
                (
                    score,
                    Recommendation {
                        kind: c.kind(),
                        id: c.id,
                        score,
                        reason,
                    },
                    buckets,
                )
            })
        })
        .collect();

    // Ties broken by id so the same library always produces the same rail: a list that
    // reshuffles on every open looks broken even when every entry is defensible.
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));

    // Anything that matched their taste outranks anything that did not, whatever the
    // arithmetic says, so the split is by kind rather than by score.
    let (matches, discoveries): (Vec<_>, Vec<_>) = scored
        .into_iter()
        .partition(|(score, _, _)| *score > DISCOVERY_CEILING);

    let cap = ((limit as f32) * MAX_SHARE_PER_BUCKET).ceil().max(1.0) as usize;
    let mut state = Selection::default();
    let mut out = Vec::with_capacity(limit);

    // Their own taste first, under a per-bucket cap so it cannot take the whole rail.
    select_into(matches, Some(cap), limit, &mut state, &mut out);
    // Then discovery, for whatever is left. No cap here: the cap exists to stop a
    // viewer's own history walling them in, and this is the thing doing the opposite.
    // The diversity decay still applies, so twenty documentaries do not replace
    // twenty science-fiction films.
    select_into(discoveries, None, limit, &mut state, &mut out);
    out
}

/// What the greedy selection has spent so far, shared across both passes.
#[derive(Default)]
struct Selection {
    /// Multiplier per bucket, decayed each time one is picked.
    used: HashMap<String, f32>,
    /// How many picks each bucket has had, for the cap.
    taken: HashMap<String, usize>,
}

/// Greedy selection with a per-bucket penalty — the cheap half of MMR, and enough:
/// the thing being avoided is twenty of one genre, not subtle redundancy.
fn select_into(
    mut pool: Vec<(f32, Recommendation, Vec<String>)>,
    cap: Option<usize>,
    limit: usize,
    state: &mut Selection,
    out: &mut Vec<Recommendation>,
) {
    while out.len() < limit && !pool.is_empty() {
        let mut best: Option<usize> = None;
        let mut best_adjusted = f32::MIN;
        for (i, (score, _, buckets)) in pool.iter().enumerate() {
            if let Some(cap) = cap {
                // A bucket that has had its share sits out the rest of this pass.
                if !buckets.is_empty()
                    && buckets
                        .iter()
                        .all(|b| state.taken.get(b).copied().unwrap_or(0) >= cap)
                {
                    continue;
                }
            }
            let penalty = buckets
                .iter()
                .map(|b| state.used.get(b).copied().unwrap_or(1.0))
                .fold(1.0f32, f32::min);
            let adjusted = score * penalty;
            if adjusted > best_adjusted {
                best_adjusted = adjusted;
                best = Some(i);
            }
        }
        let Some(index) = best else { break };
        let (_, pick, buckets) = pool.remove(index);
        for bucket in buckets {
            *state.used.entry(bucket.clone()).or_insert(1.0) *= DIVERSITY_DECAY;
            *state.taken.entry(bucket).or_insert(0) += 1;
        }
        out.push(pick);
    }
}

/// How much one watched item counts, or `None` if it should not count at all.
fn signal_weight(item: &Watched, now: i64) -> Option<f32> {
    let engagement = item.fraction.clamp(0.0, 1.0);
    // A favourite is a statement in itself; it does not need to have been finished.
    if engagement < MIN_ENGAGEMENT && !item.favourite {
        return None;
    }
    let age_days = ((now - item.updated_at).max(0) as f32) / SECS_PER_DAY;
    let recency = 0.5f32.powf(age_days / HALF_LIFE_DAYS);
    let base = engagement.max(if item.favourite { 1.0 } else { 0.0 });
    let weight = base * recency * if item.favourite { FAVOURITE_BOOST } else { 1.0 };
    (weight > 0.0).then_some(weight)
}

/// Score one candidate, and work out what to tell the viewer about it.
fn score_one(
    taste: &Taste,
    c: &Candidate,
    genres: &[String],
    category: Option<&str>,
    now: i64,
) -> (f32, Reason) {
    let fresh = freshness(c.added_at, now);

    if !taste.is_known() {
        // Nothing watched yet. Rating is the only honest signal, with new arrivals
        // nudged up — and the reason says so rather than pretending to know them.
        let rated = c.rating.filter(|r| *r > 0.0).map(|r| (r / 10.0).min(1.0));
        let score = rated.unwrap_or(0.0) * 0.85 + fresh * 0.15;
        let reason = if rated.is_some_and(|r| r >= fresh) {
            Reason::HighlyRated
        } else if fresh > 0.0 {
            Reason::JustAdded
        } else {
            Reason::HighlyRated
        };
        return (score, reason);
    }

    let genre_affinity = affinity(&taste.genres, genres);
    let category_affinity = category
        .map(|c| affinity(&taste.categories, std::slice::from_ref(&c.to_string())))
        .unwrap_or(0.0);
    let era_affinity = c
        .year
        .map(|y| taste.eras.get(&decade(y)).copied().unwrap_or(0.0))
        .map(|share| (share * 2.0).min(1.0))
        .unwrap_or(0.0);
    let rating_score = rating_fit(taste.mean_rating, c.rating);

    // Nothing about this candidate resembles anything they watch. It can still be
    // *shown* — a rail with no discovery in it walls somebody into one evening's
    // viewing — but it is scored as discovery, below anything that actually matched,
    // and `rank` fills only the slots the matches left.
    if genre_affinity <= 0.0 && category_affinity <= 0.0 {
        let discovery =
            DISCOVERY_CEILING * (0.70 * rating_score + 0.20 * era_affinity + 0.10 * fresh);
        let reason = if rating_score > 0.0 {
            Reason::HighlyRated
        } else {
            Reason::JustAdded
        };
        return (discovery, reason);
    }

    let score = GENRE_WEIGHT * genre_affinity
        + CATEGORY_WEIGHT * category_affinity
        + RATING_WEIGHT * rating_score
        + ERA_WEIGHT * era_affinity
        + FRESH_WEIGHT * fresh;

    (
        score,
        explain(
            taste,
            c.id,
            genres,
            category,
            Parts {
                genre: genre_affinity,
                category: category_affinity,
                rating: rating_score,
                fresh,
            },
        ),
    )
}

/// What each ingredient contributed, so the explanation can pick the one that
/// actually carried the score.
struct Parts {
    genre: f32,
    category: f32,
    rating: f32,
    fresh: f32,
}

/// The line to show, chosen by which ingredient actually carried the score.
fn explain(
    taste: &Taste,
    id: i64,
    genres: &[String],
    category: Option<&str>,
    parts: Parts,
) -> Reason {
    if parts.genre > 0.0 {
        // Name a title they watched that shares this candidate's strongest genre —
        // far more convincing than the genre alone, and true by construction.
        let strongest = genres
            .iter()
            .max_by(|a, b| {
                taste
                    .genres
                    .get(*a)
                    .copied()
                    .unwrap_or(0.0)
                    .total_cmp(&taste.genres.get(*b).copied().unwrap_or(0.0))
            })
            .cloned();
        if let Some(genre_name) = strongest {
            if let Some(title) = pick_anchor(taste, id, |a| a.genres.contains(&genre_name)) {
                return Reason::Because { title };
            }
            return Reason::Genre {
                genre: display_genre(&genre_name),
            };
        }
    }
    // No genres, but the provider filed it somewhere the viewer watches. Name a title
    // from the same shelf rather than the shelf itself: "EN ✪ BOX OFFICE" is not a
    // sentence anybody wants under a poster.
    if parts.category > 0.0 {
        if let Some(category) = category {
            if let Some(title) = pick_anchor(taste, id, |a| a.category.as_deref() == Some(category))
            {
                return Reason::Because { title };
            }
        }
    }
    if parts.rating * RATING_WEIGHT >= parts.fresh * FRESH_WEIGHT {
        Reason::HighlyRated
    } else {
        Reason::JustAdded
    }
}

/// Name one of the titles that put this candidate on the rail.
///
/// Rotated by the candidate's id rather than always taking the heaviest match. Six
/// posters in a row all saying "Because you watched NL Painkillers" is accurate and
/// reads as a stuck record; naming three different films the viewer actually watched
/// is equally true and tells them more about how the rail was built. Deterministic,
/// so the same library still produces the same rail twice.
fn pick_anchor(taste: &Taste, id: i64, matches: impl Fn(&Anchor) -> bool) -> Option<String> {
    let candidates: Vec<&Anchor> = taste.anchors.iter().filter(|a| matches(a)).collect();
    if candidates.is_empty() {
        return None;
    }
    // Only the heaviest few: the point is variety among the titles that actually
    // explain this, not a tour of everything ever watched.
    let pool = candidates.len().min(ANCHOR_CHOICES);
    let index = (id.rem_euclid(pool as i64)) as usize;
    Some(candidates[index].title.clone())
}

/// How many recently watched titles a rail may name.
const ANCHOR_CHOICES: usize = 3;

/// How much a candidate's genres overlap what the viewer watches.
///
/// The share of the viewer's attention this candidate's genres account for, capped at
/// one. Not cosine similarity: a candidate tagged with a single genre the viewer loves
/// should not be punished for lacking the other four, which is exactly what dividing
/// by its own magnitude would do.
fn affinity(taste: &HashMap<String, f32>, genres: &[String]) -> f32 {
    if genres.is_empty() {
        return 0.0;
    }
    let covered: f32 = genres
        .iter()
        .filter_map(|g| taste.get(g))
        .copied()
        .sum::<f32>();
    // Scaled so that matching a genre worth a third of their viewing already reads as
    // a strong match; shares are small once a viewer has several interests.
    (covered * 3.0).min(1.0)
}

/// How well a rating fits what they tend to watch.
///
/// Not simply "higher is better". Somebody whose history averages 6.5 is telling you
/// what they enjoy, and handing them a 9.0 art film on that basis is the recommender
/// substituting its taste for theirs. A little above their average is the sweet spot.
fn rating_fit(mean: Option<f32>, rating: Option<f32>) -> f32 {
    let Some(rating) = rating.filter(|r| *r > 0.0) else {
        return 0.0;
    };
    let Some(mean) = mean else {
        return (rating / 10.0).min(1.0);
    };
    let target = (mean + 0.5).min(10.0);
    let distance = (rating - target).abs();
    (1.0 - distance / 5.0).clamp(0.0, 1.0)
}

/// A mild boost for things that arrived recently, decaying to nothing over a month.
fn freshness(added_at: Option<i64>, now: i64) -> f32 {
    let Some(added) = added_at else { return 0.0 };
    let days = ((now - added).max(0) as f32) / SECS_PER_DAY;
    (1.0 - days / FRESH_WINDOW_DAYS).clamp(0.0, 1.0)
}

fn decade(year: i32) -> i32 {
    year - year.rem_euclid(10)
}

/// Genres arrive from TMDB, from panels, and from M3U group titles, so the same one
/// turns up as "Sci-Fi", "sci-fi" and "SCI-FI ". Matching has to see one genre.
fn normalise(genre: &str) -> String {
    genre.trim().to_lowercase()
}

/// Back to something worth printing. Only ever used for a label.
fn display_genre(normalised: &str) -> String {
    let mut out = String::with_capacity(normalised.len());
    for (i, part) in normalised.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

fn top(map: &HashMap<String, f32>, n: usize) -> Vec<String> {
    let mut pairs: Vec<_> = map.iter().collect();
    pairs.sort_by(|a, b| b.1.total_cmp(a.1).then_with(|| a.0.cmp(b.0)));
    pairs.into_iter().take(n).map(|(k, _)| k.clone()).collect()
}

fn normalise_shares(map: &mut HashMap<impl std::hash::Hash + Eq, f32>) {
    let total: f32 = map.values().sum();
    if total > 0.0 {
        for value in map.values_mut() {
            *value /= total;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;
    const DAY: i64 = 86_400;

    fn watched(title: &str, genres: &[&str], fraction: f32, days_ago: i64) -> Watched {
        Watched {
            kind: Kind::Movie,
            id: title.len() as i64,
            title: title.to_string(),
            genres: genres.iter().map(|g| g.to_string()).collect(),
            category: None,
            year: Some(2020),
            rating: Some(7.0),
            fraction,
            updated_at: NOW - days_ago * DAY,
            favourite: false,
        }
    }

    fn candidate(id: i64, genres: &[&str]) -> Candidate {
        Candidate {
            kind: Some(Kind::Movie),
            id,
            title: format!("Film {id}"),
            genres: genres.iter().map(|g| g.to_string()).collect(),
            category: None,
            year: Some(2020),
            rating: Some(7.5),
            added_at: None,
        }
    }

    fn nothing_seen(_: Kind, _: i64) -> bool {
        false
    }

    #[test]
    fn an_abandoned_film_says_nothing_about_taste() {
        // Four minutes of a two-hour film is a decision not to watch it. Counting that
        // as a preference is how one bad evening poisons a rail for a month.
        let taste = profile(&[watched("Dune", &["Sci-Fi"], 0.03, 1)], NOW);
        assert!(!taste.is_known(), "3% watched should not be a signal");

        let taste = profile(&[watched("Dune", &["Sci-Fi"], 0.9, 1)], NOW);
        assert!(taste.is_known());
    }

    #[test]
    fn a_favourite_counts_even_when_it_was_never_finished() {
        let mut item = watched("Heat", &["Crime"], 0.0, 2);
        item.favourite = true;
        let taste = profile(&[item], NOW);
        assert!(taste.is_known(), "putting it on a list is a statement");
        assert_eq!(taste.top_genres(1), vec!["crime"]);
    }

    #[test]
    fn last_night_counts_for_more_than_last_year() {
        let taste = profile(
            &[
                watched("Recent", &["Comedy"], 1.0, 1),
                watched("Ancient", &["Horror"], 1.0, 365),
            ],
            NOW,
        );
        let top = taste.top_genres(2);
        assert_eq!(top[0], "comedy", "the recent one should lead: {top:?}");

        // And the gap should be large, not marginal: a year is twelve half-lives.
        let comedy = taste.genres["comedy"];
        let horror = taste.genres["horror"];
        assert!(comedy > horror * 50.0, "comedy {comedy}, horror {horror}");
    }

    #[test]
    fn a_five_genre_film_does_not_outvote_a_one_genre_film() {
        // The bug this prevents: splitting nothing, so a title tagged with every genre
        // under the sun drags the whole profile towards itself.
        let broad = profile(&[watched("Broad", &["A", "B", "C", "D", "E"], 1.0, 0)], NOW);
        let narrow = profile(&[watched("Narrow", &["A"], 1.0, 0)], NOW);
        assert!(
            (broad.weight() - narrow.weight()).abs() < 0.001,
            "both watched one film in full"
        );
        assert!(
            broad.genres["a"] < narrow.genres["a"],
            "a genre shared with four others is worth less"
        );
    }

    #[test]
    fn genres_match_whatever_case_the_provider_wrote_them_in() {
        // Real panels and TMDB disagree about this constantly.
        let taste = profile(&[watched("A", &["Sci-Fi"], 1.0, 0)], NOW);
        let out = rank(
            &taste,
            &[candidate(1, &["sci-fi"]), candidate(2, &["  SCI-FI "])],
            &nothing_seen,
            NOW,
            5,
        );
        assert_eq!(out.len(), 2, "both spellings are the same genre");
        assert!(out.iter().all(|r| r.score > 0.0));
    }

    #[test]
    fn what_they_watch_outranks_what_they_do_not() {
        let taste = profile(&[watched("Alien", &["Sci-Fi", "Horror"], 1.0, 1)], NOW);
        let out = rank(
            &taste,
            &[candidate(1, &["Romance"]), candidate(2, &["Sci-Fi"])],
            &nothing_seen,
            NOW,
            5,
        );
        assert_eq!(out[0].id, 2, "sci-fi should lead: {out:?}");
    }

    #[test]
    fn a_rail_is_not_twenty_of_the_same_genre() {
        // A viewer with one strong preference scores every title in it highly. Without
        // the diversity penalty the whole rail is one genre, which is useless.
        let taste = profile(
            &[
                watched("Alien", &["Sci-Fi"], 1.0, 1),
                watched("Heat", &["Crime"], 1.0, 2),
                watched("Airplane", &["Comedy"], 1.0, 3),
            ],
            NOW,
        );
        let mut library = Vec::new();
        for i in 0..20 {
            library.push(candidate(i, &["Sci-Fi"]));
        }
        for i in 20..24 {
            library.push(candidate(i, &["Crime"]));
        }
        for i in 24..28 {
            library.push(candidate(i, &["Comedy"]));
        }

        let out = rank(&taste, &library, &nothing_seen, NOW, 8);
        assert_eq!(out.len(), 8);
        let scifi = out.iter().filter(|r| r.id < 20).count();
        assert!(
            scifi <= 4,
            "8 picks from a library of 20 sci-fi and 8 others gave {scifi} sci-fi"
        );
        assert!(scifi >= 2, "their favourite genre should still lead");
    }

    /// The state a first evening leaves: one shelf watched, and every title that
    /// matches it comes from that shelf. Measured on a real panel, the whole rail was
    /// twenty films off "NL ✪ FILMS [SUB]" — their taste reflected back at them and
    /// nothing else.
    #[test]
    fn a_one_note_history_does_not_produce_a_one_note_rail() {
        let taste = profile(&[watched("Seen", &["Sci-Fi"], 1.0, 1)], NOW);

        let mut library: Vec<Candidate> = (0..40).map(|i| candidate(i, &["Sci-Fi"])).collect();
        // Things they have said nothing about, but which are worth something.
        for i in 40..60 {
            let mut other = candidate(i, &["Documentary"]);
            other.rating = Some(8.0);
            library.push(other);
        }

        let out = rank(&taste, &library, &nothing_seen, NOW, 20);
        assert_eq!(out.len(), 20);
        let matched = out.iter().filter(|r| r.id < 40).count();
        assert!(
            matched < 20,
            "every pick came from the one genre they watch ({matched} of 20)"
        );
        assert!(
            matched >= 5,
            "their own taste should still dominate the top of the rail ({matched} of 20)"
        );
    }

    /// Discovery must never outrank a real match, however well rated it is.
    #[test]
    fn what_they_watch_comes_before_what_they_have_not() {
        let taste = profile(&[watched("Seen", &["Sci-Fi"], 1.0, 1)], NOW);
        let mut acclaimed = candidate(1, &["Documentary"]);
        acclaimed.rating = Some(9.9);
        let mut ordinary = candidate(2, &["Sci-Fi"]);
        ordinary.rating = Some(5.0);

        let out = rank(&taste, &[acclaimed, ordinary], &nothing_seen, NOW, 2);
        assert_eq!(out[0].id, 2, "the genre match leads: {out:?}");
        assert_eq!(out[1].id, 1);
    }

    /// And a title nobody can say anything about is still left out rather than used
    /// as padding.
    #[test]
    fn discovery_is_not_a_licence_to_pad() {
        let taste = profile(&[watched("Seen", &["Sci-Fi"], 1.0, 1)], NOW);
        let mut blank = candidate(9, &["Documentary"]);
        blank.rating = None;
        blank.year = None;
        blank.added_at = None;
        let out = rank(&taste, &[blank], &nothing_seen, NOW, 10);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn nothing_already_watched_comes_back() {
        let taste = profile(&[watched("Alien", &["Sci-Fi"], 1.0, 1)], NOW);
        let seen = |kind: Kind, id: i64| kind == Kind::Movie && id == 1;
        let out = rank(
            &taste,
            &[candidate(1, &["Sci-Fi"]), candidate(2, &["Sci-Fi"])],
            &seen,
            NOW,
            5,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, 2);
    }

    #[test]
    fn an_empty_history_still_produces_a_rail() {
        // A new install has nothing to go on, and an empty "Recommended" rail on the
        // first evening is worse than an honest "Highly rated" one.
        let taste = profile(&[], NOW);
        assert!(!taste.is_known());

        let mut good = candidate(1, &["Drama"]);
        good.rating = Some(9.0);
        let mut poor = candidate(2, &["Drama"]);
        poor.rating = Some(3.0);

        let out = rank(&taste, &[poor, good], &nothing_seen, NOW, 5);
        assert_eq!(out[0].id, 1, "the better-rated one should lead");
        assert_eq!(out[0].reason, Reason::HighlyRated);
    }

    #[test]
    fn a_recommendation_can_name_the_thing_it_came_from() {
        let taste = profile(&[watched("Blade Runner", &["Sci-Fi"], 1.0, 1)], NOW);
        let out = rank(&taste, &[candidate(1, &["Sci-Fi"])], &nothing_seen, NOW, 1);
        assert_eq!(
            out[0].reason,
            Reason::Because {
                title: "Blade Runner".into()
            }
        );
        assert_eq!(
            out[0].reason.label(),
            "Because you watched Blade Runner",
            "this is the line under the poster"
        );
    }

    #[test]
    fn the_title_it_names_is_one_that_actually_shares_the_genre() {
        // The failure this prevents: "Because you watched Airplane" under a horror
        // film, which reads as the recommender making things up.
        let taste = profile(
            &[
                watched("Airplane", &["Comedy"], 1.0, 1),
                watched("The Thing", &["Horror"], 1.0, 4),
            ],
            NOW,
        );
        let out = rank(&taste, &[candidate(1, &["Horror"])], &nothing_seen, NOW, 1);
        assert_eq!(
            out[0].reason,
            Reason::Because {
                title: "The Thing".into()
            }
        );
    }

    #[test]
    fn a_nine_out_of_ten_is_not_automatically_better_than_their_usual() {
        // Somebody averaging 6.5 is telling you what they enjoy. Handing them a 9.0 on
        // that basis is the recommender substituting its taste for theirs.
        let mut history = watched("Comfort", &["Action"], 1.0, 1);
        history.rating = Some(6.5);
        let taste = profile(&[history], NOW);

        let mut near = candidate(1, &["Action"]);
        near.rating = Some(7.0);
        let mut far = candidate(2, &["Action"]);
        far.rating = Some(9.8);

        let out = rank(&taste, &[far, near], &nothing_seen, NOW, 2);
        assert_eq!(out[0].id, 1, "7.0 fits a 6.5 viewer better than 9.8");
    }

    #[test]
    fn a_candidate_with_nothing_in_common_is_left_out_rather_than_padded_in() {
        let taste = profile(&[watched("Alien", &["Sci-Fi"], 1.0, 1)], NOW);
        let mut unrelated = candidate(9, &["Documentary"]);
        unrelated.rating = None;
        unrelated.year = None;
        unrelated.added_at = None;

        let out = rank(&taste, &[unrelated], &nothing_seen, NOW, 10);
        assert!(
            out.is_empty(),
            "a zero score should not fill a slot: {out:?}"
        );
    }

    #[test]
    fn the_same_library_gives_the_same_rail_twice() {
        // A list that reshuffles on every open looks broken even when every entry is
        // defensible.
        let taste = profile(&[watched("Alien", &["Sci-Fi"], 1.0, 1)], NOW);
        let library: Vec<_> = (0..30).map(|i| candidate(i, &["Sci-Fi"])).collect();
        let first = rank(&taste, &library, &nothing_seen, NOW, 10);
        let second = rank(&taste, &library, &nothing_seen, NOW, 10);
        assert_eq!(first, second);
    }

    #[test]
    fn asking_for_nothing_returns_nothing_rather_than_everything() {
        let taste = profile(&[watched("Alien", &["Sci-Fi"], 1.0, 1)], NOW);
        assert!(rank(&taste, &[candidate(1, &["Sci-Fi"])], &nothing_seen, NOW, 0).is_empty());
    }

    #[test]
    fn a_series_and_a_film_with_the_same_id_are_different_things() {
        // `seen` is keyed on both, and the two tables have independent id spaces — so
        // watching film 7 must not hide series 7.
        let taste = profile(&[watched("Alien", &["Sci-Fi"], 1.0, 1)], NOW);
        let film = Candidate {
            kind: Some(Kind::Movie),
            ..candidate(7, &["Sci-Fi"])
        };
        let show = Candidate {
            kind: Some(Kind::Series),
            ..candidate(7, &["Sci-Fi"])
        };
        let seen = |kind: Kind, id: i64| kind == Kind::Movie && id == 7;
        let out = rank(&taste, &[film, show], &seen, NOW, 5);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, Kind::Series);
    }

    #[test]
    fn scores_stay_inside_the_range_the_weights_promise() {
        let taste = profile(
            &[
                watched("A", &["Sci-Fi"], 1.0, 0),
                watched("B", &["Sci-Fi"], 1.0, 0),
            ],
            NOW,
        );
        let mut perfect = candidate(1, &["Sci-Fi"]);
        perfect.rating = Some(7.5);
        perfect.added_at = Some(NOW);
        let out = rank(&taste, &[perfect], &nothing_seen, NOW, 1);
        assert!(
            (0.0..=1.0).contains(&out[0].score),
            "score was {}",
            out[0].score
        );
    }

    #[test]
    fn an_era_they_watch_counts_for_something() {
        let mut eighties = watched("Aliens", &["Sci-Fi"], 1.0, 1);
        eighties.year = Some(1986);
        let taste = profile(&[eighties], NOW);

        let mut same_era = candidate(1, &["Sci-Fi"]);
        same_era.year = Some(1989);
        let mut other_era = candidate(2, &["Sci-Fi"]);
        other_era.year = Some(2021);

        let out = rank(&taste, &[other_era, same_era], &nothing_seen, NOW, 2);
        assert_eq!(out[0].id, 1, "same decade should edge it: {out:?}");
    }

    /// The case this exists for: a freshly imported library has no genres at all.
    /// They come from TMDB enrichment, which needs an API key a viewer may never set.
    /// What a panel always gives is the shelf it filed each title on.
    #[test]
    fn a_library_with_no_genres_is_still_recommendable() {
        let mut watched_item = watched("Heat", &[], 1.0, 1);
        watched_item.category = Some("EN ✪ BOX OFFICE".into());
        let taste = profile(&[watched_item], NOW);
        assert!(taste.is_known(), "a category alone is enough to go on");

        let same_shelf = Candidate {
            category: Some("EN ✪ BOX OFFICE".into()),
            ..candidate(1, &[])
        };
        let elsewhere = Candidate {
            category: Some("AL ✪ ALBANIA".into()),
            ..candidate(2, &[])
        };

        let out = rank(&taste, &[elsewhere, same_shelf], &nothing_seen, NOW, 2);
        assert_eq!(out[0].id, 1, "the shelf they watch should lead: {out:?}");
        assert_eq!(
            out[0].reason,
            Reason::Because {
                title: "Heat".into()
            },
            "name the title, not the shelf: \"EN ✪ BOX OFFICE\" is not a sentence"
        );
    }

    /// And a genre, where there is one, still outweighs the shelf — a category mixes
    /// language and provider in with the genre and is the noisier signal.
    #[test]
    fn a_genre_match_beats_a_category_match() {
        let mut item = watched("Alien", &["Sci-Fi"], 1.0, 1);
        item.category = Some("EN ✪ BOX OFFICE".into());
        let taste = profile(&[item], NOW);

        let by_genre = Candidate {
            category: Some("AL ✪ ALBANIA".into()),
            ..candidate(1, &["Sci-Fi"])
        };
        let by_shelf = Candidate {
            category: Some("EN ✪ BOX OFFICE".into()),
            ..candidate(2, &[])
        };

        let out = rank(&taste, &[by_shelf, by_genre], &nothing_seen, NOW, 2);
        assert_eq!(out[0].id, 1, "genre is the stronger signal: {out:?}");
    }

    /// A category-only library must not produce twenty titles off one shelf, the same
    /// way a genre-led one must not produce twenty of one genre.
    #[test]
    fn a_category_only_library_is_still_diversified() {
        let shelves = ["EN ✪ BOX OFFICE", "FR ✪ ACTION", "DE ✪ FILME"];
        let history: Vec<Watched> = shelves
            .iter()
            .enumerate()
            .map(|(i, shelf)| {
                let mut w = watched(&format!("Seen {i}"), &[], 1.0, 1 + i as i64);
                w.category = Some((*shelf).into());
                w
            })
            .collect();
        let taste = profile(&history, NOW);

        let mut library = Vec::new();
        for i in 0..20 {
            library.push(Candidate {
                category: Some(shelves[0].into()),
                ..candidate(i, &[])
            });
        }
        for i in 20..26 {
            library.push(Candidate {
                category: Some(shelves[1 + (i as usize % 2)].into()),
                ..candidate(i, &[])
            });
        }

        let out = rank(&taste, &library, &nothing_seen, NOW, 8);
        let first_shelf = out.iter().filter(|r| r.id < 20).count();
        assert!(
            first_shelf < 8,
            "every pick came off one shelf ({first_shelf} of 8)"
        );
    }

    /// Six posters in a row all naming the same film is accurate and reads as a stuck
    /// record, which is what a real panel produced.
    #[test]
    fn a_rail_names_more_than_one_of_the_things_they_watched() {
        let taste = profile(
            &[
                watched("Alien", &["Sci-Fi"], 1.0, 1),
                watched("Solaris", &["Sci-Fi"], 1.0, 2),
                watched("Arrival", &["Sci-Fi"], 1.0, 3),
            ],
            NOW,
        );
        let library: Vec<_> = (0..12).map(|i| candidate(i, &["Sci-Fi"])).collect();
        let out = rank(&taste, &library, &nothing_seen, NOW, 6);

        let named: std::collections::HashSet<_> = out
            .iter()
            .filter_map(|r| match &r.reason {
                Reason::Because { title } => Some(title.clone()),
                _ => None,
            })
            .collect();
        assert!(named.len() > 1, "every card named the same film: {named:?}");
        assert!(
            named
                .iter()
                .all(|t| ["Alien", "Solaris", "Arrival"].contains(&t.as_str())),
            "named something never watched: {named:?}"
        );
    }

    #[test]
    fn genre_labels_are_printable_whatever_case_they_arrived_in() {
        assert_eq!(display_genre("science fiction"), "Science Fiction");
        assert_eq!(display_genre("horror"), "Horror");
        assert_eq!(display_genre(""), "");
    }

    #[test]
    fn a_library_of_a_hundred_thousand_is_ranked_in_reasonable_time() {
        // The real panel this was built against holds 117,508 films and 28,528 shows.
        // This is the whole of it, and the check is that ranking stays a thing that
        // can happen while a rail renders.
        let taste = profile(
            &[
                watched("A", &["Sci-Fi"], 1.0, 1),
                watched("B", &["Crime"], 0.8, 5),
                watched("C", &["Comedy"], 1.0, 20),
            ],
            NOW,
        );
        let genres = ["Sci-Fi", "Crime", "Comedy", "Drama", "Horror", "Romance"];
        let library: Vec<Candidate> = (0..146_000)
            .map(|i| candidate(i, &[genres[(i as usize) % genres.len()]]))
            .collect();

        let started = std::time::Instant::now();
        let out = rank(&taste, &library, &nothing_seen, NOW, 20);
        let elapsed = started.elapsed();

        assert_eq!(out.len(), 20);
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "ranking the whole library took {elapsed:?}"
        );
    }
}
