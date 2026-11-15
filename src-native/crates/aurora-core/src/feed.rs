//! The home screen, worked out: which rails, in what order, holding what.
//!
//! This replaces a home screen that was six fixed shelves of 24 -- recently added, best
//! rated, the first 24 series alphabetically -- with one personal recommendation rail
//! among them. Everything here is about the viewer instead, and every rail holds the
//! whole of what belongs on it: the lists are computed in full and the screen pages
//! through them, so a rail ends where its subject ends rather than at a constant.
//!
//! Pure, like `recommend`: the rows come in as structs, nothing here touches SQLite,
//! the network or a clock, and the whole feed can be built in a unit test.
//!
//! What it knows about the viewer, and what each piece is for:
//!
//! * **What they watched, and how much.** Engagement-weighted and decayed, as in
//!   `recommend`: a film finished last night counts for more than one sampled in March.
//!   Abandoned titles count *against* their genres.
//! * **Who is in it.** The cast and directors of what they watched, from the credits
//!   enrichment already stores (1.75 million rows on the subscription this was built
//!   against, which nothing read). Somebody who has watched three Denis Villeneuve films
//!   has told you more than "likes sci-fi".
//! * **What other people liked alongside it.** TMDB's recommendations for each title
//!   they watched, cached by the host. Collaborative signal is the one thing a
//!   single-viewer app cannot compute, and it is what finds *Collateral* after *Heat*
//!   rather than every crime film from 1995.
//! * **What is popular right now.** TMDB's weekly trending list, resolved against the
//!   library -- the only "what everyone is watching" there is without a server.
//! * **What they said.** Likes and My List count for it; "Not for me" removes a title
//!   everywhere and counts against what it is made of.
//!
//! And the things a feed gets wrong, handled here rather than hoped about:
//!
//! * **The same film twice.** A subscription files one film as HD, FHD and 4K, and on
//!   the library measured 25,173 film rows were 21,656 titles. Every rail shows one
//!   copy -- the best -- and having watched any copy counts as having watched them all.
//! * **The same poster on every rail.** A title that is the top pick, trending and the
//!   first drama would open three rails. What is on screen when a rail first appears is
//!   claimed by the first rail to show it; later rails move it further along instead.
//! * **Stale sameness.** Close scores are perturbed by a per-session seed, so the home
//!   screen is a little different each time the app opens and stable while it is open.

use std::collections::{HashMap, HashSet};

pub use crate::recommend::Kind;

/// A title, as the feed refers to it.
pub type Key = (Kind, i64);

/// What one rail shows on screen before it is scrolled, for de-duplication: a title in
/// this many leading places of one rail is moved further back on the rails after it.
pub const FIRST_PAGE: usize = 20;

/// Below this many titles a rail is not worth its heading.
const MIN_RAIL: usize = 4;

/// Signals decay to half over this long. A little slower than `recommend`'s thirty days:
/// a home screen with two dozen rails has room for last month's tastes as well.
const HALF_LIFE_DAYS: f32 = 45.0;
const SECS_PER_DAY: f32 = 86_400.0;
/// Under this fraction watched, something says nothing in its favour.
const MIN_ENGAGEMENT: f32 = 0.05;
/// How long barely-started has to sit untouched to count as abandoned.
const ABANDONED_AFTER_DAYS: f32 = 14.0;
/// What liking a title multiplies its weight by.
const LIKED_BOOST: f32 = 1.6;
/// What being on My List counts for, unwatched: intent, not experience.
const LISTED_WEIGHT: f32 = 0.35;
/// What "Not for me" counts against, as a weight of evidence.
const DISMISSED_WEIGHT: f32 = 1.2;
/// What an abandonment counts against, relative to a finished title.
const ABANDONED_WEIGHT: f32 = 0.6;
/// How hard aversion pulls a genre or shelf down.
const AVERSION_PULL: f32 = 0.7;
/// How much a session's seed may move a score, either way.
const JITTER: f32 = 0.06;
/// What an unbelievable rating is pulled towards: an ordinary, watchable film.
const PRIOR_RATING: f32 = 6.3;
/// How many people the host should load the filmographies of.
pub const PEOPLE_TO_FOLLOW: usize = 40;
/// How many watched titles the host should fetch "also liked" lists for.
pub const ANCHORS_TO_FETCH: usize = 8;

/// One title in the library, with what the feed reads off it.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: Kind,
    pub id: i64,
    pub title: String,
    /// What makes two rows one title -- the duplicate group. The host builds it from
    /// `match_key` and year, the same definition "hide duplicates" uses.
    pub group: String,
    pub genres: Vec<String>,
    /// The provider's category, "EN ✪ NETFLIX".
    pub shelf: Option<String>,
    pub year: Option<i32>,
    /// Out of ten.
    pub rating: Option<f32>,
    pub added_at: Option<i64>,
    pub runtime_mins: Option<i32>,
    /// Higher is better: SD 1, HD 2, FHD 3, 4K 4.
    pub quality_rank: i32,
    pub has_poster: bool,
    pub has_backdrop: bool,
    pub tmdb_id: Option<i64>,
    pub lang: Option<String>,
    /// Passes the viewer's library filters and is not hidden. Rows that do not are still
    /// loaded, because something watched before a filter was switched on still says
    /// something about taste -- they are just never shown.
    pub listed: bool,
}

impl Item {
    pub fn key(&self) -> Key {
        (self.kind, self.id)
    }
}

/// A person in a title's credits.
#[derive(Debug, Clone, PartialEq)]
pub struct Credit {
    pub person: i64,
    pub name: String,
    pub director: bool,
    /// Billing order for cast; zero for a director.
    pub order: i32,
}

/// Something watched: a film, or a series with its episodes folded in.
#[derive(Debug, Clone, PartialEq)]
pub struct Watch {
    pub key: Key,
    /// 0..1, the furthest through it got; a completed one is 1.
    pub fraction: f32,
    pub updated_at: i64,
}

/// Everything the feed is built from.
#[derive(Debug, Default, Clone)]
pub struct Inputs {
    pub items: Vec<Item>,
    /// Newest first.
    pub history: Vec<Watch>,
    pub liked: HashSet<Key>,
    /// In the order the viewer added them, newest first.
    pub my_list: Vec<Key>,
    pub not_interested: HashSet<Key>,
    /// Continue Watching as the host built it, in order.
    pub continue_watching: Vec<Key>,
    /// Credits for anything watched, and for every title the followed people are in.
    pub credits: HashMap<Key, Vec<Credit>>,
    /// TMDB "also liked" lists, keyed by the TMDB id of the title they are about.
    pub also_liked: HashMap<(Kind, i64), Vec<(Kind, i64)>>,
    /// TMDB trending, most trending first, as TMDB ids.
    pub trending: Vec<(Kind, i64)>,
    pub now: i64,
    /// Changes each time the app opens, so close calls go a different way.
    pub seed: u64,
}

/// One rail, complete.
#[derive(Debug, Clone, PartialEq)]
pub struct Rail {
    /// Stable across builds of the same feed, so the screen can page it.
    pub id: String,
    /// One of the contract's `RailKind`s.
    pub kind: &'static str,
    pub title: String,
    /// Every title on it, in order. Not truncated.
    pub items: Vec<Key>,
    /// The line under a poster, where the rail has something to say about it.
    pub reasons: HashMap<Key, String>,
}

/// Which watched titles are worth fetching "also liked" lists for, best first.
pub fn anchors_to_fetch(inputs: &Inputs) -> Vec<Key> {
    let index = Index::new(&inputs.items);
    let evidence = evidence(inputs, &index);
    let mut keys: Vec<(Key, f32)> = evidence
        .iter()
        .filter(|e| e.weight > 0.0)
        .map(|e| (e.key, e.weight))
        .collect();
    keys.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    keys.into_iter()
        .map(|(k, _)| k)
        .take(ANCHORS_TO_FETCH)
        .collect()
}

/// Whose filmographies the host should load: the people who recur across what the
/// viewer watched, weighted the same way the taste is.
///
/// `inputs.credits` need only hold the watched titles' credits at this point.
pub fn people_to_follow(inputs: &Inputs) -> Vec<i64> {
    let index = Index::new(&inputs.items);
    let evidence = evidence(inputs, &index);
    let mut weights: HashMap<i64, f32> = HashMap::new();
    for e in evidence.iter().filter(|e| e.weight > 0.0) {
        for c in inputs.credits.get(&e.key).into_iter().flatten() {
            *weights.entry(c.person).or_insert(0.0) += e.weight * credit_weight(c);
        }
    }
    let mut people: Vec<(i64, f32)> = weights.into_iter().collect();
    people.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    people
        .into_iter()
        .map(|(p, _)| p)
        .take(PEOPLE_TO_FOLLOW)
        .collect()
}

/// Build the whole home screen.
pub fn build(inputs: &Inputs) -> Vec<Rail> {
    let index = Index::new(&inputs.items);
    let evidence = evidence(inputs, &index);
    let taste = Taste::new(inputs, &index, &evidence);
    let ctx = Ctx::new(inputs, &index, &taste);

    let mut rails: Vec<Rail> = Vec::new();
    let mut later: Vec<Rail> = Vec::new();

    // Continue Watching and My List are the viewer's own, and come as they are.
    let resumed: Vec<Key> = inputs
        .continue_watching
        .iter()
        .filter(|k| !inputs.not_interested.contains(k))
        .copied()
        .collect();
    push(
        &mut rails,
        "continue",
        "continueWatching",
        "Continue Watching",
        resumed,
        1,
    );

    let for_you = ctx.for_you();
    let top10 = ctx.top10();
    let trending = ctx.trending(&top10);
    let because: Vec<Rail> = ctx.because_you_watched();
    let genres_for_you = ctx.taste_genres();
    let people = ctx.people_rails();
    let shelves = ctx.shelves(&genres_for_you);

    // Interleaved: personal, popular, personal, new. A screen that front-loads every
    // personal rail reads as a wall of the same guess; one that front-loads the library
    // reads as a catalogue.
    let mut because = because.into_iter();
    let mut genre_rails = genres_for_you.iter().map(|g| ctx.genre_rail(g, true));
    let mut people = people.into_iter();
    let mut shelves = shelves.into_iter();

    rails.extend(for_you);
    rails.extend(top10);
    rails.extend(because.next());
    let mine: Vec<Key> = inputs
        .my_list
        .iter()
        .filter_map(|k| ctx.show_as(*k))
        .collect();
    push(&mut rails, "my-list", "myList", "My List", mine, 1);
    rails.extend(trending);
    rails.extend(genre_rails.next().flatten());
    rails.extend(because.next());
    rails.extend(ctx.recently_added());
    rails.extend(people.next());
    rails.extend(genre_rails.next().flatten());
    rails.extend(ctx.series_for_you());
    rails.extend(because.next());
    rails.extend(ctx.new_releases());
    rails.extend(genre_rails.next().flatten());
    rails.extend(ctx.highly_rated());
    rails.extend(shelves.next());
    rails.extend(people.next());
    rails.extend(genre_rails.next().flatten());
    rails.extend(ctx.modern_classics());
    rails.extend(ctx.decade());
    rails.extend(shelves.next());
    rails.extend(ctx.four_k());
    rails.extend(ctx.short_and_sweet());
    rails.extend(ctx.watch_again());
    for rail in genre_rails.flatten() {
        later.push(rail);
    }
    for rail in because {
        later.push(rail);
    }
    rails.extend(later);
    // The rest of the library's big genres, so the bottom of the screen is somewhere to
    // browse rather than an end.
    let shown: HashSet<String> = rails
        .iter()
        .filter_map(|r| r.id.strip_prefix("genre:").map(str::to_string))
        .collect();
    for genre in ctx.library_genres() {
        if shown.contains(&genre) {
            continue;
        }
        rails.extend(ctx.genre_rail(&genre, false));
    }

    claim_first_pages(&mut rails);
    rails
}

/* ── Indexing ─────────────────────────────────────────────────────────────── */

struct Index<'a> {
    by_key: HashMap<Key, &'a Item>,
    /// Each duplicate group's best listed copy.
    best: HashMap<&'a str, Key>,
    by_tmdb: HashMap<(Kind, i64), Key>,
}

impl<'a> Index<'a> {
    fn new(items: &'a [Item]) -> Self {
        let mut by_key = HashMap::with_capacity(items.len());
        let mut best: HashMap<&str, &Item> = HashMap::new();
        for item in items {
            by_key.insert(item.key(), item);
            if !item.listed {
                continue;
            }
            let slot = best.entry(item.group.as_str()).or_insert(item);
            if better_copy(item, slot) {
                *slot = item;
            }
        }
        let mut by_tmdb = HashMap::new();
        for item in best.values() {
            if let Some(t) = item.tmdb_id {
                by_tmdb.entry((item.kind, t)).or_insert(item.key());
            }
        }
        Self {
            by_key,
            best: best.into_iter().map(|(g, i)| (g, i.key())).collect(),
            by_tmdb,
        }
    }

    fn get(&self, key: Key) -> Option<&'a Item> {
        self.by_key.get(&key).copied()
    }

    /// The copy to show for this title, if any copy of it may be shown.
    fn canonical(&self, key: Key) -> Option<Key> {
        let item = self.get(key)?;
        self.best.get(item.group.as_str()).copied()
    }
}

/// Which of two copies of one title to show: the best picture, then one with artwork,
/// then the oldest row, so the answer is stable.
fn better_copy(a: &Item, b: &Item) -> bool {
    (a.quality_rank, a.has_poster, std::cmp::Reverse(a.id))
        > (b.quality_rank, b.has_poster, std::cmp::Reverse(b.id))
}

/* ── Evidence: what each thing the viewer did says ────────────────────────── */

struct Evidence {
    key: Key,
    /// Positive for a liking, negative for an aversion.
    weight: f32,
}

fn decay(now: i64, at: i64) -> f32 {
    let days = ((now - at).max(0) as f32) / SECS_PER_DAY;
    0.5f32.powf(days / HALF_LIFE_DAYS)
}

fn evidence(inputs: &Inputs, index: &Index) -> Vec<Evidence> {
    let mut out = Vec::new();
    let mut said: HashSet<Key> = HashSet::new();
    for w in &inputs.history {
        if index.get(w.key).is_none() {
            continue;
        }
        let liked = inputs.liked.contains(&w.key);
        let recency = decay(inputs.now, w.updated_at);
        let age = ((inputs.now - w.updated_at).max(0) as f32) / SECS_PER_DAY;
        let fraction = w.fraction.clamp(0.0, 1.0);
        let weight = if fraction < MIN_ENGAGEMENT && !liked {
            if age >= ABANDONED_AFTER_DAYS {
                -ABANDONED_WEIGHT * recency
            } else {
                // Just started: on Continue Watching, and no opinion yet.
                0.0
            }
        } else {
            // Engagement, but a series is credited for commitment: five episodes in is
            // a stronger statement than the fraction of the fifth.
            let engagement = fraction.max(if liked { 1.0 } else { 0.0 }).max(0.25);
            engagement * recency * if liked { LIKED_BOOST } else { 1.0 }
        };
        if weight != 0.0 {
            out.push(Evidence { key: w.key, weight });
        }
        said.insert(w.key);
    }
    for key in &inputs.liked {
        if !said.contains(key) && index.get(*key).is_some() {
            out.push(Evidence {
                key: *key,
                weight: LIKED_BOOST * 0.8,
            });
            said.insert(*key);
        }
    }
    for key in &inputs.my_list {
        if !said.contains(key) && index.get(*key).is_some() {
            out.push(Evidence {
                key: *key,
                weight: LISTED_WEIGHT,
            });
            said.insert(*key);
        }
    }
    for key in &inputs.not_interested {
        if index.get(*key).is_some() {
            out.push(Evidence {
                key: *key,
                weight: -DISMISSED_WEIGHT,
            });
        }
    }
    out
}

/// How much one credit says about a title: the lead more than the eighth name, and the
/// director most of all.
fn credit_weight(c: &Credit) -> f32 {
    if c.director {
        1.3
    } else {
        (1.0 - 0.11 * c.order.clamp(0, 7) as f32).max(0.2)
    }
}

/* ── Taste ────────────────────────────────────────────────────────────────── */

#[derive(Default)]
struct Taste {
    genres: HashMap<String, f32>,
    genre_aversion: HashMap<String, f32>,
    shelves: HashMap<String, f32>,
    shelf_aversion: HashMap<String, f32>,
    eras: HashMap<i32, f32>,
    kinds: HashMap<Kind, f32>,
    langs: HashMap<String, f32>,
    /// Person to weight, relative to the strongest.
    people: HashMap<i64, f32>,
    /// How many liked titles each person is in, so a rail about them needs two.
    people_count: HashMap<i64, usize>,
    names: HashMap<i64, (String, bool)>,
    mean_rating: Option<f32>,
    /// Watched titles, heaviest first: what "Because you watched" can be about.
    anchors: Vec<(Key, f32)>,
    /// TMDB key of a recommended title to how strongly, and which anchor said so.
    links: HashMap<(Kind, i64), (f32, Key)>,
    known: bool,
}

impl Taste {
    fn new(inputs: &Inputs, index: &Index, evidence: &[Evidence]) -> Self {
        let mut t = Taste::default();
        let mut rating = (0.0f32, 0.0f32);
        let mut attention = 0.0f32;
        for e in evidence {
            let Some(item) = index.get(e.key) else {
                continue;
            };
            let genres = norm_all(&item.genres);
            let shelf = item.shelf.as_deref().map(norm).filter(|s| !s.is_empty());
            if e.weight < 0.0 {
                let w = -e.weight;
                for g in &genres {
                    *t.genre_aversion.entry(g.clone()).or_default() += w / genres.len() as f32;
                }
                if let Some(s) = shelf {
                    *t.shelf_aversion.entry(s).or_default() += w;
                }
                continue;
            }
            let w = e.weight;
            attention += w;
            for g in &genres {
                *t.genres.entry(g.clone()).or_default() += w / genres.len() as f32;
            }
            if let Some(s) = shelf {
                *t.shelves.entry(s).or_default() += w;
            }
            if let Some(y) = item.year {
                *t.eras.entry(decade(y)).or_default() += w;
            }
            *t.kinds.entry(item.kind).or_default() += w;
            if let Some(l) = item.lang.as_deref() {
                *t.langs.entry(l.to_string()).or_default() += w;
            }
            if let Some(r) = item.rating.filter(|r| *r > 0.0) {
                rating.0 += r * w;
                rating.1 += w;
            }
            for c in inputs.credits.get(&e.key).into_iter().flatten() {
                *t.people.entry(c.person).or_default() += w * credit_weight(c);
                *t.people_count.entry(c.person).or_default() += 1;
                t.names
                    .entry(c.person)
                    .or_insert((c.name.clone(), c.director));
            }
            // Only titles really watched can head a rail: "Because you watched" about
            // something merely listed would be untrue.
            if inputs.history.iter().any(|h| h.key == e.key) {
                t.anchors.push((e.key, w));
            }
        }
        t.known = attention > 0.0;
        if rating.1 > 0.0 {
            t.mean_rating = Some(rating.0 / rating.1);
        }
        // Aversion is relative to how much they watch, as in `recommend` (R-37): one
        // film walked out of must not erase a year of watching a genre.
        let scale = attention.max(1.0);
        for v in t.genre_aversion.values_mut() {
            *v /= scale;
        }
        for v in t.shelf_aversion.values_mut() {
            *v /= scale;
        }
        shares(&mut t.genres);
        shares(&mut t.shelves);
        shares(&mut t.eras);
        shares(&mut t.kinds);
        shares(&mut t.langs);
        let top = t.people.values().copied().fold(0.0f32, f32::max);
        if top > 0.0 {
            for v in t.people.values_mut() {
                *v /= top;
            }
        }
        t.anchors
            .sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        // Collapse duplicates among the anchors: two copies of one show are one show.
        let mut groups = HashSet::new();
        t.anchors.retain(|(k, _)| {
            index
                .get(*k)
                .is_some_and(|i| groups.insert(i.group.clone()))
        });

        // "Also liked" lists, weighted by how much the anchor counts and by rank.
        let heaviest = t.anchors.first().map(|a| a.1).unwrap_or(1.0).max(1e-6);
        for (key, w) in &t.anchors {
            let Some(item) = index.get(*key) else {
                continue;
            };
            let Some(tmdb) = item.tmdb_id else { continue };
            let Some(list) = inputs.also_liked.get(&(item.kind, tmdb)) else {
                continue;
            };
            for (rank, rec) in list.iter().enumerate() {
                let strength = (w / heaviest) * (1.0 - rank as f32 / 30.0).max(0.25);
                let slot = t.links.entry(*rec).or_insert((0.0, *key));
                if strength > slot.0 {
                    *slot = (strength.max(slot.0), *key);
                }
            }
        }
        t
    }
}

/* ── Scoring ──────────────────────────────────────────────────────────────── */

struct Ctx<'a> {
    inputs: &'a Inputs,
    index: &'a Index<'a>,
    taste: &'a Taste,
    /// Duplicate groups never to show: watched, part watched, or dismissed.
    excluded: HashSet<&'a str>,
    /// Groups on My List, kept off the rails that recommend.
    listed: HashSet<&'a str>,
    /// Who is in each title, from the credits the host loaded.
    people_in: HashMap<Key, Vec<&'a Credit>>,
    /// Trending rank by library key.
    trend_rank: HashMap<Key, usize>,
    /// Every title that may be shown at all, best copy only.
    pool: Vec<&'a Item>,
    /// `score` for each of them, memoised: most rails sort by it.
    scores: HashMap<Key, f32>,
    /// What every rail would otherwise work out again per title: its genres folded,
    /// and the words its franchise is recognised by.
    prep: HashMap<Key, Prep>,
    year_now: i32,
}

/// Per-title text the rails read, prepared once. See `Ctx::prep`.
struct Prep {
    genres: Vec<String>,
    shelf: Option<String>,
    lead: Option<String>,
    stem: Option<String>,
}

impl Prep {
    fn of(item: &Item) -> Self {
        let words = title_words(&item.title);
        Self {
            genres: norm_all(&item.genres),
            shelf: item.shelf.as_deref().map(norm).filter(|s| !s.is_empty()),
            lead: lead_of(&words),
            stem: stem_of(&words),
        }
    }
}

const NO_GENRES: &[String] = &[];

impl<'a> Ctx<'a> {
    fn genres_of(&self, item: &Item) -> &[String] {
        self.prep
            .get(&item.key())
            .map(|p| p.genres.as_slice())
            .unwrap_or(NO_GENRES)
    }

    fn new(inputs: &'a Inputs, index: &'a Index<'a>, taste: &'a Taste) -> Self {
        let mut excluded: HashSet<&str> = HashSet::new();
        for k in inputs
            .history
            .iter()
            .map(|h| &h.key)
            .chain(inputs.not_interested.iter())
            .chain(inputs.continue_watching.iter())
        {
            if let Some(i) = index.get(*k) {
                excluded.insert(i.group.as_str());
            }
        }
        let listed = inputs
            .my_list
            .iter()
            .filter_map(|k| index.get(*k).map(|i| i.group.as_str()))
            .collect();
        let mut people_in: HashMap<Key, Vec<&Credit>> = HashMap::new();
        for (k, credits) in &inputs.credits {
            people_in.entry(*k).or_default().extend(credits.iter());
        }
        let mut trend_rank = HashMap::new();
        for (rank, t) in inputs.trending.iter().enumerate() {
            if let Some(k) = index.by_tmdb.get(t) {
                trend_rank.entry(*k).or_insert(rank);
            }
        }
        let pool: Vec<&Item> = index
            .best
            .values()
            .filter_map(|k| index.get(*k))
            .filter(|i| !excluded.contains(i.group.as_str()))
            .collect();
        let prep = index
            .by_key
            .values()
            .map(|i| (i.key(), Prep::of(i)))
            .collect();
        let mut ctx = Self {
            inputs,
            index,
            taste,
            excluded,
            listed,
            people_in,
            trend_rank,
            pool,
            scores: HashMap::new(),
            prep,
            year_now: year_of(inputs.now),
        };
        // Scored once, and the pool kept in that order: nearly every rail is "the pool,
        // filtered, best first", and filtering an ordered list keeps it ordered. Sorting
        // 29,000 titles again for each of two dozen rails was most of the build.
        let mut scored: Vec<(f32, &Item)> = ctx.pool.iter().map(|i| (ctx.score(i).0, *i)).collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.key().cmp(&b.1.key())));
        ctx.scores = scored.iter().map(|(s, i)| (i.key(), *s)).collect();
        ctx.pool = scored.into_iter().map(|(_, i)| i).collect();
        ctx
    }

    /// The copy of this title to put on a rail, unless it may not be shown.
    fn show_as(&self, key: Key) -> Option<Key> {
        let k = self.index.canonical(key)?;
        let item = self.index.get(k)?;
        (!self.inputs.not_interested.contains(&key)
            && !self.inputs.not_interested.contains(&k)
            && !self.excluded_by_dismissal(item))
        .then_some(k)
    }

    fn excluded_by_dismissal(&self, item: &Item) -> bool {
        self.inputs
            .not_interested
            .iter()
            .any(|k| self.index.get(*k).is_some_and(|d| d.group == item.group))
    }

    fn jitter(&self, key: Key) -> f32 {
        let u = unit(self.inputs.seed, key);
        1.0 + JITTER * (u - 0.5)
    }

    /// A rating as far as it can be believed.
    ///
    /// There is no vote count to weigh it by, and the library shows why one is needed:
    /// 211 films rated a perfect 10, nearly all of them this year's TV films with a
    /// handful of votes, which put "Clean Hands (2026)" at the top of Highly Rated over
    /// everything anyone has heard of. So a rating is pulled towards an ordinary one by
    /// how little it can be trusted: a near-perfect score is almost always a small
    /// sample, and so is anything released in the last year or two.
    fn believed_rating(&self, item: &Item) -> Option<f32> {
        let r = item.rating.filter(|r| *r > 0.0 && *r <= 10.0)?;
        let mut confidence = 1.0f32;
        if r >= 9.5 {
            confidence *= 0.25;
        } else if r >= 9.0 {
            confidence *= 0.6;
        }
        if item.year.is_some_and(|y| y >= self.year_now - 1) {
            confidence *= 0.6;
        }
        Some(PRIOR_RATING + confidence * (r - PRIOR_RATING))
    }

    fn quality(&self, item: &Item) -> f32 {
        let Some(r) = self.believed_rating(item) else {
            return 0.25;
        };
        let absolute = ((r - 5.0) / 4.0).clamp(0.0, 1.0);
        match self.taste.mean_rating {
            // A little above what they usually watch is the sweet spot; far above is
            // the recommender substituting its taste for theirs.
            Some(mean) => {
                let fit = (1.0 - ((r - (mean + 0.5)).abs() / 4.0)).clamp(0.0, 1.0);
                0.6 * absolute + 0.4 * fit
            }
            None => absolute,
        }
    }

    fn freshness(&self, item: &Item) -> f32 {
        let Some(at) = item.added_at else { return 0.0 };
        let days = ((self.inputs.now - at).max(0) as f32) / SECS_PER_DAY;
        (1.0 - days / 45.0).clamp(0.0, 1.0)
    }

    fn trend(&self, item: &Item) -> f32 {
        self.trend_rank
            .get(&item.key())
            .map(|r| (1.0 - *r as f32 / 60.0).max(0.2))
            .unwrap_or(0.0)
    }

    fn link(&self, item: &Item) -> Option<(f32, Key)> {
        let tmdb = item.tmdb_id?;
        self.taste.links.get(&(item.kind, tmdb)).copied()
    }

    /// People in this title the viewer follows, strongest first, with their weight.
    fn followed_people(&self, item: &Item) -> Vec<(&'a Credit, f32)> {
        let mut out: Vec<(&Credit, f32)> = self
            .people_in
            .get(&item.key())
            .into_iter()
            .flatten()
            .filter_map(|c| {
                self.taste
                    .people
                    .get(&c.person)
                    .map(|w| (*c, *w * credit_weight(c)))
            })
            .collect();
        out.sort_by(|a, b| {
            b.1.total_cmp(&a.1)
                .then_with(|| a.0.person.cmp(&b.0.person))
        });
        out
    }

    /// How well a title fits this viewer, 0..1.
    fn score(&self, item: &Item) -> (f32, Option<String>) {
        self.score_explained(item, false)
    }

    /// The score, and when `explain` is set, the line to put under the poster. Asked for
    /// only for the titles a rail will show it on: building a sentence for each of
    /// 29,000 titles to keep four hundred of them was most of what scoring cost.
    fn score_explained(&self, item: &Item, explain: bool) -> (f32, Option<String>) {
        let t = self.taste;
        let art = if item.has_poster { 1.0 } else { 0.35 };
        let jitter = self.jitter(item.key());
        let quality = self.quality(item);
        let fresh = self.freshness(item);
        let trend = self.trend(item);

        if !t.known {
            // Nothing watched yet: what is good, what is popular, what is new -- and the
            // reason says which rather than pretending to know them.
            let score = 0.55 * quality
                + 0.25 * trend
                + 0.12 * fresh
                + 0.08 * f32::from(u8::from(item.has_backdrop));
            let reason = if trend > 0.0 {
                "Trending now"
            } else if quality >= 0.6 {
                "Highly rated"
            } else if fresh > 0.0 {
                "Just added"
            } else {
                ""
            };
            return (
                score * art * jitter,
                Some(reason.to_string()).filter(|r| explain && !r.is_empty()),
            );
        }

        let genres = self.genres_of(item);
        let genre = (affinity(&t.genres, genres)
            - AVERSION_PULL * affinity(&t.genre_aversion, genres))
        .max(0.0);
        let shelf = self
            .prep
            .get(&item.key())
            .and_then(|p| p.shelf.as_ref())
            .map(|s| {
                (t.shelves.get(s).copied().unwrap_or(0.0) * 2.0
                    - AVERSION_PULL * t.shelf_aversion.get(s).copied().unwrap_or(0.0))
                .clamp(0.0, 1.0)
            })
            .unwrap_or(0.0);
        let people = self.followed_people(item);
        let people_score = people.iter().map(|p| p.1).sum::<f32>().min(1.0);
        let link = self.link(item);
        let link_score = link.map(|l| l.0).unwrap_or(0.0);
        let era = item
            .year
            .map(|y| (t.eras.get(&decade(y)).copied().unwrap_or(0.0) * 2.0).min(1.0))
            .unwrap_or(0.0);
        let kind = t.kinds.get(&item.kind).copied().unwrap_or(0.0);

        // Sums to one, so a score reads as a share of the decision. Genre still leads,
        // but "people who liked what you watched liked this" is close behind it: it is
        // the only collaborative signal there is, and it is the one that finds the film
        // nobody would have filed next to the one they watched.
        let mut score = 0.27 * genre
            + 0.07 * shelf
            + 0.14 * people_score
            + 0.20 * link_score
            + 0.13 * quality
            + 0.05 * era
            + 0.04 * fresh
            + 0.04 * kind
            + 0.06 * trend;

        let matched = genre > 0.0 || shelf > 0.0 || people_score > 0.0 || link_score > 0.0;
        if !matched {
            // Discovery: allowed, and placed below anything that matched.
            score *= 0.45;
        }
        // Another language they never watch is a poster they cannot use. Untagged rows
        // are left alone, as the library filters leave them.
        if let Some(lang) = item.lang.as_deref() {
            if !t.langs.is_empty() && t.langs.get(lang).copied().unwrap_or(0.0) < 0.05 {
                score *= 0.4;
            }
        }

        let reason = if !explain {
            None
        } else if link_score >= 0.35 && link_score >= people_score {
            link.and_then(|(_, anchor)| self.index.get(anchor))
                .map(|a| format!("Fans of {} loved this", a.title))
        } else if let Some((c, _)) = people.first().filter(|_| people_score >= 0.3) {
            Some(if c.director {
                format!("From director {}", c.name)
            } else {
                format!("With {}", c.name)
            })
        } else if genre > 0.0 {
            self.anchor_sharing(genres)
                .map(|a| format!("Because you watched {a}"))
                .or_else(|| strongest_genre(t, &item.genres).map(|g| format!("More {g}")))
        } else if trend > 0.0 {
            Some("Trending now".into())
        } else if quality >= 0.7 {
            Some("Highly rated".into())
        } else if fresh > 0.0 {
            Some("Just added".into())
        } else {
            None
        };
        (score * art * jitter, reason)
    }

    /// A watched title sharing a genre with these, rotated so a rail names several.
    fn anchor_sharing(&self, genres: &[String]) -> Option<String> {
        self.taste
            .anchors
            .iter()
            .filter_map(|(k, _)| self.index.get(*k))
            .find(|a| self.genres_of(a).iter().any(|g| genres.contains(g)))
            .map(|a| a.title.clone())
    }

    fn scored(&self, item: &Item) -> f32 {
        self.scores.get(&item.key()).copied().unwrap_or(0.0)
    }

    /// The pool filtered, best first by this viewer's score -- which is the order the
    /// pool is already in.
    fn ranked(&self, keep: impl Fn(&Item) -> bool) -> Vec<&'a Item> {
        self.pool.iter().copied().filter(|i| keep(i)).collect()
    }

    fn reasons_for(&self, items: &[Key], cap: usize) -> HashMap<Key, String> {
        items
            .iter()
            .take(cap)
            .filter_map(|k| {
                let item = self.index.get(*k)?;
                self.score_explained(item, true).1.map(|r| (*k, r))
            })
            .collect()
    }

    /* ── The rails ── */

    fn for_you(&self) -> Option<Rail> {
        let ranked = self.ranked(|i| !self.listed.contains(i.group.as_str()));
        let items = diversify(ranked, 60);
        let title = if self.taste.known {
            "Top Picks For You"
        } else {
            "Top Picks"
        };
        let reasons = self.reasons_for(&items, 400);
        rail("for-you", "forYou", title, items, reasons)
    }

    fn top10(&self) -> Option<Rail> {
        let mut ranked: Vec<(usize, Key)> = self
            .trend_rank
            .iter()
            .filter(|(k, _)| self.show_as(**k) == Some(**k))
            .filter(|(k, _)| {
                self.index
                    .get(**k)
                    .is_some_and(|i| !self.excluded.contains(i.group.as_str()))
            })
            .map(|(k, r)| (*r, *k))
            .collect();
        ranked.sort();
        let items: Vec<Key> = ranked.into_iter().map(|(_, k)| k).take(10).collect();
        // A top ten of three is not a top ten.
        (items.len() >= 10).then(|| Rail {
            id: "top10".into(),
            kind: "top10",
            title: "Top 10 This Week".into(),
            items,
            reasons: HashMap::new(),
        })
    }

    fn trending(&self, top10: &Option<Rail>) -> Option<Rail> {
        let skip: HashSet<Key> = top10.iter().flat_map(|r| r.items.iter().copied()).collect();
        let mut ranked: Vec<(usize, Key)> = self
            .trend_rank
            .iter()
            .filter(|(k, _)| !skip.contains(k))
            .filter(|(k, _)| {
                self.index
                    .get(**k)
                    .is_some_and(|i| !self.excluded.contains(i.group.as_str()))
            })
            .map(|(k, r)| (*r, *k))
            .collect();
        ranked.sort();
        let items = ranked.into_iter().map(|(_, k)| k).collect();
        rail(
            "trending",
            "trending",
            "Trending Now",
            items,
            HashMap::new(),
        )
    }

    fn because_you_watched(&self) -> Vec<Rail> {
        let mut out = Vec::new();
        for (anchor_key, _) in self.taste.anchors.iter().take(5) {
            let Some(anchor) = self.index.get(*anchor_key) else {
                continue;
            };
            if let Some(r) = self.similar_to(anchor) {
                out.push(r);
            }
            if out.len() == 3 {
                break;
            }
        }
        out
    }

    /// Titles like one the viewer watched: shared people, a shared franchise, what fans
    /// of it also liked, and the same genres -- in that order of how convincing each is.
    fn similar_to(&self, anchor: &Item) -> Option<Rail> {
        let anchor_genres = self.genres_of(anchor);
        let anchor_people: HashMap<i64, &Credit> = self
            .people_in
            .get(&anchor.key())
            .into_iter()
            .flatten()
            .map(|c| (c.person, *c))
            .collect();
        let anchor_prep = self.prep.get(&anchor.key());
        let anchor_lead = anchor_prep.and_then(|p| p.lead.clone());
        let anchor_stem = anchor_prep.and_then(|p| p.stem.clone());
        let links: HashMap<(Kind, i64), usize> = anchor
            .tmdb_id
            .and_then(|t| self.inputs.also_liked.get(&(anchor.kind, t)))
            .map(|l| l.iter().enumerate().map(|(i, k)| (*k, i)).collect())
            .unwrap_or_default();

        let mut scored: Vec<(f32, &Item, String)> = Vec::new();
        for c in &self.pool {
            if c.group == anchor.group {
                continue;
            }
            let jaccard = jaccard(anchor_genres, self.genres_of(c));
            let mut shared: Vec<&Credit> = self
                .people_in
                .get(&c.key())
                .into_iter()
                .flatten()
                .filter(|p| anchor_people.contains_key(&p.person))
                .copied()
                .collect();
            shared.sort_by_key(|p| (!p.director, p.order));
            let directed = shared.iter().any(|p| p.director);
            let people =
                (shared.len() as f32 / 3.0).min(1.0) * 0.25 + if directed { 0.2 } else { 0.0 };
            let link = c
                .tmdb_id
                .and_then(|t| links.get(&(c.kind, t)))
                .map(|r| (1.0 - *r as f32 / 25.0).max(0.3))
                .unwrap_or(0.0);
            // "John Wick" and "John Wick: Chapter 2" either way round: the sequel has the
            // sequel's shape, and its stem is the original's first two words.
            let prep = self.prep.get(&c.key());
            let stem = prep
                .and_then(|p| p.stem.clone())
                .filter(|s| anchor_lead.as_ref() == Some(s) || anchor_stem.as_ref() == Some(s))
                .or_else(|| {
                    anchor_stem
                        .clone()
                        .filter(|s| prep.and_then(|p| p.lead.as_ref()) == Some(s))
                });
            let franchise = stem.is_some();
            if jaccard <= 0.0 && people <= 0.0 && link <= 0.0 && !franchise {
                continue;
            }
            let era = match (anchor.year, c.year) {
                (Some(a), Some(b)) => (1.0 - ((a - b).abs() as f32 / 15.0)).max(0.0),
                _ => 0.0,
            };
            let same_shelf = anchor.shelf.is_some() && anchor.shelf == c.shelf;
            let same_kind = if anchor.kind == c.kind { 1.0 } else { 0.85 };
            let sim = 0.32 * jaccard
                + people
                + 0.5 * link
                + if franchise { 0.6 } else { 0.0 }
                + 0.06 * era
                + if same_shelf { 0.04 } else { 0.0 };
            // Genre alone is a weak claim: two dramas are barely related. It has to be
            // a close genre match, or come with something else.
            if people <= 0.0 && link <= 0.0 && !franchise && jaccard < 0.5 {
                continue;
            }
            // Half the usual jitter: this rail is about one title, and its order should be
            // the similarity, nudged rather than reshuffled.
            let jitter = 1.0 + 0.5 * (self.jitter(c.key()) - 1.0);
            let fit = sim * (0.7 + 0.3 * self.quality(c)) * same_kind * jitter;
            let reason = if franchise {
                format!("More {}", display(stem.as_deref().unwrap_or_default()))
            } else if directed {
                let d = shared.iter().find(|p| p.director).expect("directed");
                format!("Also from {}", d.name)
            } else if let Some(p) = shared.first() {
                format!("Also starring {}", p.name)
            } else if link > 0.0 {
                "Fans also watched".to_string()
            } else {
                c.genres
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" · ")
            };
            scored.push((fit, c, reason));
        }
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.key().cmp(&b.1.key())));
        let mut reasons = HashMap::new();
        let items: Vec<Key> = scored
            .into_iter()
            .map(|(_, i, r)| {
                if !r.is_empty() {
                    reasons.insert(i.key(), r);
                }
                i.key()
            })
            .collect();
        rail(
            &format!("because:{}:{}", anchor.kind.as_str(), anchor.id),
            "becauseYouWatched",
            &format!("Because You Watched {}", anchor.title),
            items,
            reasons,
        )
    }

    /// The viewer's strongest genres, as normalised keys.
    fn taste_genres(&self) -> Vec<String> {
        if !self.taste.known {
            return Vec::new();
        }
        let mut g: Vec<(&String, &f32)> = self
            .taste
            .genres
            .iter()
            .filter(|(_, s)| **s >= 0.07)
            .collect();
        g.sort_by(|a, b| b.1.total_cmp(a.1).then_with(|| a.0.cmp(b.0)));
        g.into_iter().take(4).map(|(k, _)| k.clone()).collect()
    }

    /// The library's genres by size, for the browsable rails at the bottom.
    fn library_genres(&self) -> Vec<String> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for i in &self.pool {
            for g in self.genres_of(i) {
                *counts.entry(g.clone()).or_default() += 1;
            }
        }
        let mut g: Vec<(String, usize)> = counts
            .into_iter()
            .filter(|(_, n)| *n >= MIN_RAIL * 3)
            .collect();
        g.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        g.into_iter().map(|(k, _)| k).take(8).collect()
    }

    fn genre_rail(&self, genre: &str, personal: bool) -> Option<Rail> {
        let items: Vec<Key> = self
            .ranked(|i| self.genres_of(i).iter().any(|g| g == genre))
            .into_iter()
            .map(Item::key)
            .collect();
        let name = display(genre);
        let title = if personal {
            format!("{name} For You")
        } else {
            name
        };
        rail(
            &format!("genre:{genre}"),
            "genre",
            &title,
            items,
            HashMap::new(),
        )
    }

    fn people_rails(&self) -> Vec<Rail> {
        let mut people: Vec<(i64, f32)> = self
            .taste
            .people
            .iter()
            .filter(|(p, _)| self.taste.people_count.get(p).copied().unwrap_or(0) >= 2)
            .map(|(p, w)| (*p, *w))
            .collect();
        people.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let mut out = Vec::new();
        for (person, _) in people {
            let Some((name, director)) = self.taste.names.get(&person) else {
                continue;
            };
            let items: Vec<Key> = self
                .ranked(|i| {
                    self.people_in
                        .get(&i.key())
                        .is_some_and(|cs| cs.iter().any(|c| c.person == person))
                })
                .into_iter()
                .map(Item::key)
                .collect();
            let title = if *director {
                format!("Directed By {name}")
            } else {
                format!("More With {name}")
            };
            if let Some(r) = rail(
                &format!("person:{person}"),
                "person",
                &title,
                items,
                HashMap::new(),
            ) {
                out.push(r);
            }
            if out.len() == 2 {
                break;
            }
        }
        out
    }

    /// Shelves the viewer keeps watching from that a genre rail does not already cover:
    /// "Netflix", "Box Office", "4K [2024/2025]".
    fn shelves(&self, genres: &[String]) -> Vec<Rail> {
        let mut shelves: Vec<(&String, &f32)> = self
            .taste
            .shelves
            .iter()
            .filter(|(_, s)| **s >= 0.1)
            .collect();
        shelves.sort_by(|a, b| b.1.total_cmp(a.1).then_with(|| a.0.cmp(b.0)));
        let mut out = Vec::new();
        for (shelf, _) in shelves {
            let name = shelf_name(shelf);
            if name.is_empty()
                || is_generic_shelf(&name)
                || genres.iter().any(|g| norm(&name) == *g)
            {
                continue;
            }
            let items: Vec<Key> = self
                .ranked(|i| i.shelf.as_deref().map(norm).as_ref() == Some(shelf))
                .into_iter()
                .map(Item::key)
                .collect();
            if let Some(r) = rail(
                &format!("shelf:{shelf}"),
                "providerCategory",
                &display(&name),
                items,
                HashMap::new(),
            ) {
                out.push(r);
            }
            if out.len() == 2 {
                break;
            }
        }
        out
    }

    fn recently_added(&self) -> Option<Rail> {
        let mut items: Vec<&Item> = self
            .pool
            .iter()
            .copied()
            .filter(|i| i.added_at.is_some())
            .collect();
        items.sort_by(|a, b| {
            b.added_at
                .cmp(&a.added_at)
                .then_with(|| self.scored(b).total_cmp(&self.scored(a)))
                .then_with(|| a.key().cmp(&b.key()))
        });
        rail(
            "recently-added",
            "recentlyAdded",
            "Recently Added",
            keys(items),
            HashMap::new(),
        )
    }

    fn new_releases(&self) -> Option<Rail> {
        let floor = self.year_now - 1;
        let items = self.ranked(|i| i.year.is_some_and(|y| y >= floor && y <= self.year_now));
        rail(
            "new-releases",
            "newReleases",
            "New Releases",
            keys(items),
            HashMap::new(),
        )
    }

    fn series_for_you(&self) -> Option<Rail> {
        let items =
            self.ranked(|i| i.kind == Kind::Series && !self.listed.contains(i.group.as_str()));
        let title = if self.taste.known {
            "Series For You"
        } else {
            "Popular Series"
        };
        rail("series", "seriesForYou", title, keys(items), HashMap::new())
    }

    fn highly_rated(&self) -> Option<Rail> {
        let mut items: Vec<&Item> = self
            .pool
            .iter()
            .copied()
            .filter(|i| self.believed_rating(i).is_some_and(|r| r >= 7.4))
            .collect();
        items.sort_by(|a, b| {
            let ra = self.believed_rating(a).unwrap_or(0.0) + 1.5 * self.scored(a);
            let rb = self.believed_rating(b).unwrap_or(0.0) + 1.5 * self.scored(b);
            rb.total_cmp(&ra).then_with(|| a.key().cmp(&b.key()))
        });
        rail(
            "acclaimed",
            "acclaimed",
            "Highly Rated",
            keys(items),
            HashMap::new(),
        )
    }

    /// Well-regarded, from ten to thirty years ago. Not "hidden gems": without vote
    /// counts there is no way to tell a gem nobody found from a hit everybody did, and
    /// a rail that called NCIS hidden would be caught out on its first poster.
    fn modern_classics(&self) -> Option<Rail> {
        let (from, to) = (self.year_now - 30, self.year_now - 10);
        let items = self.ranked(|i| {
            self.believed_rating(i).is_some_and(|r| r >= 7.4)
                && i.year.is_some_and(|y| (from..=to).contains(&y))
        });
        rail(
            "classics",
            "hiddenGems",
            "Modern Classics",
            keys(items),
            HashMap::new(),
        )
    }

    fn decade(&self) -> Option<Rail> {
        let (era, share) = self
            .taste
            .eras
            .iter()
            .max_by(|a, b| a.1.total_cmp(b.1).then_with(|| b.0.cmp(a.0)))?;
        if *share < 0.25 || *era > self.year_now - 15 {
            return None;
        }
        let items = self.ranked(|i| i.year.is_some_and(|y| decade(y) == *era));
        rail(
            &format!("decade:{era}"),
            "decade",
            &format!("Back To The {era}s"),
            keys(items),
            HashMap::new(),
        )
    }

    fn four_k(&self) -> Option<Rail> {
        let items = self.ranked(|i| i.quality_rank >= 4);
        rail("4k", "fourK", "In 4K", keys(items), HashMap::new())
    }

    fn short_and_sweet(&self) -> Option<Rail> {
        let items = self.ranked(|i| {
            i.kind == Kind::Movie
                && i.runtime_mins.is_some_and(|m| (60..=90).contains(&m))
                && self.believed_rating(i).is_some_and(|r| r >= 6.5)
        });
        rail(
            "short",
            "shortAndSweet",
            "Under 90 Minutes",
            keys(items),
            HashMap::new(),
        )
    }

    fn watch_again(&self) -> Option<Rail> {
        let mut seen = HashSet::new();
        let mut items: Vec<(f32, Key)> = Vec::new();
        // Not what is still being watched: a show with one finished episode is on
        // Continue Watching, and offering to start it "again" is offering it twice.
        let resuming: HashSet<&str> = self
            .inputs
            .continue_watching
            .iter()
            .filter_map(|k| self.index.get(*k).map(|i| i.group.as_str()))
            .collect();
        for h in &self.inputs.history {
            if h.fraction < 0.9 {
                continue;
            }
            if self
                .index
                .get(h.key)
                .is_some_and(|i| resuming.contains(i.group.as_str()))
            {
                continue;
            }
            let Some(k) = self.show_as(h.key) else {
                continue;
            };
            let Some(item) = self.index.get(k) else {
                continue;
            };
            if !seen.insert(item.group.clone()) {
                continue;
            }
            let liked = if self.inputs.liked.contains(&h.key) {
                2.0
            } else {
                1.0
            };
            items.push((liked * decay(self.inputs.now, h.updated_at).max(0.05), k));
        }
        items.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let items: Vec<Key> = items.into_iter().map(|(_, k)| k).collect();
        (items.len() >= 3).then(|| Rail {
            id: "watch-again".into(),
            kind: "watchAgain",
            title: "Watch It Again".into(),
            items,
            reasons: HashMap::new(),
        })
    }
}

/* ── Assembly ─────────────────────────────────────────────────────────────── */

fn keys(items: Vec<&Item>) -> Vec<Key> {
    items.into_iter().map(Item::key).collect()
}

fn rail(
    id: &str,
    kind: &'static str,
    title: &str,
    items: Vec<Key>,
    reasons: HashMap<Key, String>,
) -> Option<Rail> {
    (items.len() >= MIN_RAIL).then(|| Rail {
        id: id.to_string(),
        kind,
        title: title.to_string(),
        items,
        reasons,
    })
}

fn push(
    rails: &mut Vec<Rail>,
    id: &str,
    kind: &'static str,
    title: &str,
    items: Vec<Key>,
    min: usize,
) {
    if items.len() >= min {
        rails.push(Rail {
            id: id.into(),
            kind,
            title: title.into(),
            items,
            reasons: HashMap::new(),
        });
    }
}

/// Greedy diversity over the head of a ranked list: each genre picked makes the next
/// title of that genre worth a little less, so the first screen of "Top Picks" is not
/// twelve crime dramas. Past the head, plain score order.
fn diversify(ranked: Vec<&Item>, head: usize) -> Vec<Key> {
    let window = ranked.len().min(head * 4);
    let (mut pool, rest) = {
        let mut r = ranked;
        let rest = r.split_off(window);
        (r, rest)
    };
    let mut used: HashMap<String, f32> = HashMap::new();
    let mut out = Vec::with_capacity(pool.len() + rest.len());
    // Position in the original order stands in for score: it is what the list was sorted
    // by, and it keeps this pure arithmetic on integers.
    let n = pool.len().max(1) as f32;
    let mut ranks: HashMap<Key, f32> = HashMap::new();
    for (i, item) in pool.iter().enumerate() {
        ranks.insert(item.key(), 1.0 - i as f32 / n);
    }
    while out.len() < head && !pool.is_empty() {
        let (best, _) = pool
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let penalty = norm_all(&item.genres)
                    .iter()
                    .map(|g| used.get(g).copied().unwrap_or(1.0))
                    .fold(1.0f32, f32::min);
                (i, ranks[&item.key()] * penalty)
            })
            .max_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
            .expect("pool is not empty");
        let item = pool.remove(best);
        for g in norm_all(&item.genres) {
            *used.entry(g).or_insert(1.0) *= 0.7;
        }
        out.push(item.key());
    }
    out.extend(pool.into_iter().map(Item::key));
    out.extend(rest.into_iter().map(Item::key));
    out
}

/// What is on screen when a rail first appears belongs to the first rail to show it.
///
/// A later rail keeps the title -- it is still a drama, still new -- but moves it past
/// its own first page, so scrolling down the home screen keeps showing new posters.
///
/// Except at the head of a rail about one thing. "Because You Watched Heat" exists for
/// its best few matches, and claiming strictly in screen order took *Collateral* off it
/// because Top Picks, above it, had it too -- the rail's reason for being, gone to a
/// rail that could have found something else. Letting those rails claim first was
/// worse: after one crime film the rail about it took every crime title, and Top Picks
/// opened with none. So they keep their first `ANCHORED_HEAD`, and only that.
///
/// Continue Watching, My List, the Top 10, Trending and Watch It Again are never
/// reordered: their order is the point.
fn claim_first_pages(rails: &mut [Rail]) {
    let mut claimed: HashSet<Key> = HashSet::new();
    for rail in rails.iter_mut() {
        let fixed = matches!(
            rail.kind,
            "continueWatching" | "myList" | "top10" | "trending" | "watchAgain"
        );
        if !fixed {
            let keep = if matches!(rail.kind, "becauseYouWatched" | "person") {
                ANCHORED_HEAD
            } else {
                0
            };
            let (mut front, mut back): (Vec<Key>, Vec<Key>) = (Vec::new(), Vec::new());
            for (i, k) in rail.items.drain(..).enumerate() {
                if i < keep || (front.len() < FIRST_PAGE && !claimed.contains(&k)) {
                    front.push(k);
                } else {
                    back.push(k);
                }
            }
            front.extend(back);
            rail.items = front;
        }
        claimed.extend(rail.items.iter().take(FIRST_PAGE).copied());
    }
}

/// How many of its best matches a rail about one title keeps, shared or not.
const ANCHORED_HEAD: usize = 6;

/* ── Small helpers ────────────────────────────────────────────────────────── */

fn norm(s: &str) -> String {
    s.trim().to_lowercase()
}

fn norm_all(genres: &[String]) -> Vec<String> {
    let mut out: Vec<String> = genres
        .iter()
        .map(|g| norm(g))
        .filter(|g| !g.is_empty())
        .collect();
    out.dedup();
    out
}

fn shares<K: std::hash::Hash + Eq>(map: &mut HashMap<K, f32>) {
    let total: f32 = map.values().sum();
    if total > 0.0 {
        for v in map.values_mut() {
            *v /= total;
        }
    }
}

/// Share of the viewer's attention these genres account for, scaled so a genre worth a
/// third of their viewing is already a strong match.
fn affinity(taste: &HashMap<String, f32>, genres: &[String]) -> f32 {
    (genres.iter().filter_map(|g| taste.get(g)).sum::<f32>() * 2.5).min(1.0)
}

fn jaccard(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.iter().filter(|g| b.contains(g)).count() as f32;
    let union = (a.len() + b.len()) as f32 - inter;
    inter / union
}

fn strongest_genre(taste: &Taste, genres: &[String]) -> Option<String> {
    genres
        .iter()
        .max_by(|a, b| {
            let wa = taste.genres.get(&norm(a)).copied().unwrap_or(0.0);
            let wb = taste.genres.get(&norm(b)).copied().unwrap_or(0.0);
            wa.total_cmp(&wb).then_with(|| b.cmp(a))
        })
        .cloned()
}

fn decade(year: i32) -> i32 {
    year - year.rem_euclid(10)
}

/// The calendar year of a Unix timestamp. Close enough for "new releases": it is off
/// by at most a day either side of New Year.
fn year_of(unix: i64) -> i32 {
    1970 + (unix as f64 / 31_556_952.0).floor() as i32
}

/// A deterministic number in 0..1 for this seed and title (SplitMix64).
fn unit(seed: u64, key: Key) -> f32 {
    let kind = match key.0 {
        Kind::Movie => 1u64,
        Kind::Series => 2,
    };
    let mut z = seed
        .wrapping_add((key.1 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add(kind.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

/// The franchise a title belongs to, guessed from its name: "John Wick 2" and "John
/// Wick: Chapter 4" are both "john wick". The first two words, when the title has a
/// sequel's shape -- a number or "Chapter" after them -- so two films that merely start
/// with "The Man" are not one franchise.
///
/// From the title rather than the duplicate group, because `match_key` has had its
/// spaces taken out ("johnwick2") and a word boundary is the whole question here.
#[cfg(test)]
fn franchise_stem(title: &str) -> Option<String> {
    stem_of(&title_words(title))
}

fn stem_of(words: &[String]) -> Option<String> {
    if words.len() < 3 {
        return None;
    }
    let lead = lead_of(words)?;
    let rest = &words[2..];
    let sequel = rest.iter().any(|w| {
        w.chars().all(|c| c.is_ascii_digit())
            || matches!(
                w.as_str(),
                "ii" | "iii" | "iv" | "v" | "vi" | "part" | "chapter" | "returns" | "reloaded"
            )
    });
    sequel.then_some(lead)
}

/// A title as lowercase words, without punctuation or the provider's language tag:
/// "EN John Wick: Chapter 2" is `["john", "wick", "chapter", "2"]`.
fn title_words(title: &str) -> Vec<String> {
    let mut tokens: Vec<&str> = title.split_whitespace().collect();
    // "EN", "FR", "4K": the panel's prefix when enrichment has not named the film.
    if tokens.len() > 1
        && tokens[0].len() <= 3
        && tokens[0]
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        tokens.remove(0);
    }
    // One pass, splitting on anything that is not a letter or digit: this runs for
    // every title in the library, so it allocates the words and nothing else.
    let mut out = Vec::with_capacity(tokens.len());
    for token in tokens {
        let mut word = String::new();
        for c in token.chars() {
            if c.is_alphanumeric() {
                word.extend(c.to_lowercase());
            } else if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
        }
        if !word.is_empty() {
            out.push(word);
        }
    }
    out
}

/// A title's first two words, unless they are only articles: what a franchise is
/// recognised by.
#[cfg(test)]
fn lead_words(title: &str) -> Option<String> {
    lead_of(&title_words(title))
}

fn lead_of(words: &[String]) -> Option<String> {
    if words.len() < 2 {
        return None;
    }
    let stop = ["the", "a", "an", "of", "and"];
    if words[..2].iter().all(|w| stop.contains(&w.as_str())) {
        return None;
    }
    Some(words[..2].join(" "))
}

/// A normalised name back to something printable: "science fiction" to "Science Fiction".
fn display(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().chain(c).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A shelf that only says what kind of thing is on it: "Series", "TV Shows",
/// "Movies [Multi-Sub]". A rail by that name says nothing a rail is for.
fn is_generic_shelf(name: &str) -> bool {
    let bare = name.split('[').next().unwrap_or(name).trim();
    matches!(
        bare,
        "series"
            | "tv series"
            | "tv shows"
            | "shows"
            | "movies"
            | "films"
            | "vod"
            | "new"
            | "all"
            | "general"
            | "other"
            | "multi-sub"
            | "multi-audio"
            | "uk series"
            | "us series"
    )
}

/// A provider shelf without the provider's prefix: "EN ✪ BOX OFFICE" to "box office".
fn shelf_name(shelf: &str) -> String {
    let marks = ['✪', '◉', '|', '•', '★', ':'];
    let cut = shelf
        .char_indices()
        .find(|(_, c)| marks.contains(c))
        .filter(|(i, _)| *i <= 6)
        .map(|(i, c)| i + c.len_utf8());
    let rest = match cut {
        Some(at) => &shelf[at..],
        None => shelf,
    };
    norm(rest.trim_matches(|c: char| c == '-' || c.is_whitespace()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;
    const DAY: i64 = 86_400;

    fn film(id: i64, title: &str, genres: &[&str]) -> Item {
        Item {
            kind: Kind::Movie,
            id,
            title: title.to_string(),
            group: format!("{}|2000", title.to_lowercase()),
            genres: genres.iter().map(|g| g.to_string()).collect(),
            shelf: Some("EN ✪ MOVIES".into()),
            year: Some(2000),
            rating: Some(7.0),
            added_at: Some(NOW - 400 * DAY),
            runtime_mins: Some(110),
            quality_rank: 3,
            has_poster: true,
            has_backdrop: true,
            tmdb_id: Some(id + 10_000),
            lang: Some("en".into()),
            listed: true,
        }
    }

    fn show(id: i64, title: &str, genres: &[&str]) -> Item {
        Item {
            kind: Kind::Series,
            ..film(id, title, genres)
        }
    }

    fn watched(key: Key, days_ago: i64) -> Watch {
        Watch {
            key,
            fraction: 1.0,
            updated_at: NOW - days_ago * DAY,
        }
    }

    fn library(n: i64) -> Vec<Item> {
        let genres = [
            "Drama",
            "Comedy",
            "Crime",
            "Horror",
            "Action",
            "Documentary",
        ];
        (1..=n)
            .map(|i| Item {
                // Spread, so the order is decided by something other than the seed.
                rating: Some(5.0 + ((i * 37) % 50) as f32 / 10.0),
                ..film(
                    i,
                    &format!("Film {i}"),
                    &[genres[(i as usize) % genres.len()]],
                )
            })
            .collect()
    }

    fn find<'a>(rails: &'a [Rail], id: &str) -> Option<&'a Rail> {
        rails.iter().find(|r| r.id == id)
    }

    fn inputs(items: Vec<Item>) -> Inputs {
        Inputs {
            items,
            now: NOW,
            seed: 7,
            ..Default::default()
        }
    }

    /// The point of the whole change: a rail is as long as its subject.
    #[test]
    fn a_rail_holds_everything_that_belongs_on_it() {
        let mut i = inputs(library(600));
        i.history = vec![watched((Kind::Movie, 2), 1)];
        let rails = build(&i);
        let picks = find(&rails, "for-you").expect("top picks");
        // Everything but the one film watched.
        assert_eq!(picks.items.len(), 599);
        let crime = rails
            .iter()
            .find(|r| r.id == "genre:crime")
            .expect("crime rail");
        assert_eq!(crime.items.len(), 100 - 1, "every unseen crime film");
    }

    #[test]
    fn one_title_filed_three_times_is_shown_once_and_watching_any_copy_counts() {
        let mut items = library(40);
        let mut hd = film(100, "Heat", &["Crime"]);
        hd.quality_rank = 2;
        let mut uhd = film(101, "Heat", &["Crime"]);
        uhd.quality_rank = 4;
        let sd = Item {
            quality_rank: 1,
            ..film(102, "Heat", &["Crime"])
        };
        items.extend([hd, uhd, sd]);
        let rails = build(&inputs(items.clone()));
        let picks = &find(&rails, "for-you").unwrap().items;
        let heats: Vec<_> = picks
            .iter()
            .filter(|k| (100..=102).contains(&k.1))
            .collect();
        assert_eq!(heats, vec![&(Kind::Movie, 101)], "the 4K copy, once");

        let mut i = inputs(items);
        i.history = vec![watched((Kind::Movie, 100), 2)];
        let rails = build(&i);
        for rail in &rails {
            assert!(
                !rail.items.iter().any(|k| (100..=102).contains(&k.1)),
                "watched the HD copy, so no copy of Heat on {}",
                rail.id
            );
        }
    }

    #[test]
    fn not_for_me_removes_a_title_everywhere_and_counts_against_its_genre() {
        let mut items = library(120);
        items.push(film(500, "Saw", &["Horror"]));
        let mut i = inputs(items);
        i.history = vec![watched((Kind::Movie, 1), 1), watched((Kind::Movie, 3), 2)];
        let before = build(&i);
        let horror_rank = |rails: &[Rail]| {
            find(rails, "for-you").unwrap().items.iter().position(|k| {
                i.items
                    .iter()
                    .any(|it| it.key() == *k && it.genres == ["Horror"])
            })
        };
        let was = horror_rank(&before);

        i.not_interested.insert((Kind::Movie, 500));
        i.not_interested.insert((Kind::Movie, 3));
        let after = build(&i);
        for rail in &after {
            assert!(!rail.items.contains(&(Kind::Movie, 500)), "{}", rail.id);
        }
        assert!(horror_rank(&after) >= was, "horror moved down, not up");
    }

    #[test]
    fn because_you_watched_finds_the_same_director_and_the_sequel_first() {
        let mut items = library(300);
        let anchor = Item {
            group: "john wick|2014".into(),
            ..film(900, "John Wick", &["Action"])
        };
        let sequel = Item {
            group: "john wick chapter 2|2017".into(),
            year: Some(2017),
            ..film(901, "John Wick: Chapter 2", &["Action", "Thriller"])
        };
        let same_director = film(902, "Atomic Blonde", &["Action"]);
        items.extend([anchor, sequel, same_director]);
        let mut i = inputs(items);
        i.history = vec![watched((Kind::Movie, 900), 1)];
        let director = Credit {
            person: 77,
            name: "Chad Stahelski".into(),
            director: true,
            order: 0,
        };
        i.credits.insert((Kind::Movie, 900), vec![director.clone()]);
        i.credits.insert((Kind::Movie, 902), vec![director]);
        let rails = build(&i);
        let because = find(&rails, "because:movie:900").expect("because rail");
        assert_eq!(because.title, "Because You Watched John Wick");
        assert_eq!(
            &because.items[..2],
            &[(Kind::Movie, 901), (Kind::Movie, 902)]
        );
        assert_eq!(
            because.reasons[&(Kind::Movie, 902)],
            "Also from Chad Stahelski"
        );
        assert_eq!(because.reasons[&(Kind::Movie, 901)], "More John Wick");
    }

    #[test]
    fn fans_also_watched_lifts_what_tmdb_links_to_a_watched_title() {
        let mut items = library(200);
        let heat = film(800, "Heat", &["Crime"]);
        // Filed under a genre the viewer has never watched as well as one they have:
        // what TMDB links is usually near, rarely identical.
        let collateral = film(801, "Collateral", &["Thriller", "Crime"]);
        items.extend([heat, collateral]);
        let mut i = inputs(items);
        i.history = vec![watched((Kind::Movie, 800), 1)];
        i.also_liked
            .insert((Kind::Movie, 10_800), vec![(Kind::Movie, 10_801)]);
        let rails = build(&i);
        let because = find(&rails, "because:movie:800").unwrap();
        assert_eq!(because.items[0], (Kind::Movie, 801));
        let picks = &find(&rails, "for-you").unwrap();
        let at = picks
            .items
            .iter()
            .position(|k| *k == (Kind::Movie, 801))
            .unwrap();
        assert!(at < 5, "linked title near the top of Top Picks, was {at}");
        assert_eq!(
            picks.reasons[&(Kind::Movie, 801)],
            "Fans of Heat loved this"
        );
    }

    #[test]
    fn a_person_rail_needs_them_in_two_things_watched() {
        let mut items = library(100);
        for id in 700..710 {
            items.push(film(id, &format!("Nolan {id}"), &["Drama"]));
        }
        let nolan = Credit {
            person: 5,
            name: "Christopher Nolan".into(),
            director: true,
            order: 0,
        };
        let mut i = inputs(items);
        i.history = vec![watched((Kind::Movie, 700), 1)];
        for id in 700..710 {
            i.credits.insert((Kind::Movie, id), vec![nolan.clone()]);
        }
        assert!(
            find(&build(&i), "person:5").is_none(),
            "one film is not a pattern"
        );
        i.history.push(watched((Kind::Movie, 701), 3));
        let rails = build(&i);
        let rail = find(&rails, "person:5").expect("two is");
        assert_eq!(rail.title, "Directed By Christopher Nolan");
        assert_eq!(rail.items.len(), 8, "the eight not yet watched");
    }

    #[test]
    fn trending_resolves_against_the_library_and_skips_what_it_lacks() {
        let items = library(60);
        let mut i = inputs(items);
        // TMDB ids are library id + 10,000 in this fixture; 99,999 is not in it.
        i.trending = (1..=14).map(|n| (Kind::Movie, 10_000 + n)).collect();
        i.trending.insert(3, (Kind::Movie, 99_999));
        let rails = build(&i);
        let top10 = find(&rails, "top10").expect("top 10");
        assert_eq!(top10.items.len(), 10);
        assert_eq!(top10.items[0], (Kind::Movie, 1));
        let trending = find(&rails, "trending").expect("the rest");
        assert_eq!(
            trending.items,
            (11..=14).map(|n| (Kind::Movie, n)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_first_screen_of_each_rail_does_not_repeat_the_one_above() {
        let mut i = inputs(library(400));
        i.history = vec![watched((Kind::Movie, 2), 1), watched((Kind::Movie, 8), 2)];
        let rails = build(&i);
        let mut seen: HashSet<Key> = HashSet::new();
        for rail in &rails {
            let first: Vec<Key> = rail.items.iter().take(FIRST_PAGE).copied().collect();
            let fresh = first.iter().filter(|k| !seen.contains(k)).count();
            // A rail that has run out of titles nobody has seen may repeat; one of 60
            // comedies has not.
            if rail.items.len() >= FIRST_PAGE * 4 {
                assert_eq!(
                    fresh,
                    first.len(),
                    "{} repeats a poster from above",
                    rail.id
                );
            }
            seen.extend(first);
        }
    }

    #[test]
    fn the_same_inputs_build_the_same_screen_and_a_new_seed_changes_it_a_little() {
        let mut i = inputs(library(300));
        i.history = vec![watched((Kind::Movie, 2), 1)];
        let a = build(&i);
        assert_eq!(a, build(&i));
        i.seed = 8;
        let b = build(&i);
        let pa = &find(&a, "for-you").unwrap().items;
        let pb = &find(&b, "for-you").unwrap().items;
        assert_ne!(pa[..20], pb[..20], "a new session reads differently");
        // What moved is the discovery around the edges. The titles that are there because
        // they match the viewer -- crime, after a crime film -- are all still on the first few screens.
        let crime = |k: &Key| k.1 % 6 == 2;
        let matched: Vec<&Key> = pa[..20].iter().filter(|k| crime(k)).collect();
        assert!(!matched.is_empty());
        for k in matched {
            // Three screens of a rail: past the diversified head, never off a cliff.
            assert!(
                pb[..90].contains(k),
                "{k:?} fell out of the top 90 on a new seed"
            );
        }
    }

    #[test]
    fn a_language_they_never_watch_sinks_and_an_untagged_one_does_not() {
        let mut items = library(200);
        items.push(Item {
            lang: Some("fr".into()),
            ..film(600, "Amour", &["Drama"])
        });
        items.push(Item {
            lang: None,
            ..film(601, "Untagged", &["Drama"])
        });
        let mut i = inputs(items);
        i.history = vec![watched((Kind::Movie, 6), 1), watched((Kind::Movie, 12), 2)];
        let rails = build(&i);
        let picks = &find(&rails, "for-you").unwrap().items;
        let fr = picks.iter().position(|k| k.1 == 600).unwrap();
        let none = picks.iter().position(|k| k.1 == 601).unwrap();
        assert!(fr > none, "French {fr} below untagged {none}");
    }

    #[test]
    fn nothing_watched_still_builds_a_screen_and_does_not_claim_to_know_them() {
        let rails = build(&inputs(library(100)));
        let picks = find(&rails, "for-you").unwrap();
        assert_eq!(picks.title, "Top Picks");
        assert!(rails.iter().all(|r| !r.title.contains("For You")));
        assert!(rails.iter().all(|r| r.kind != "becauseYouWatched"));
    }

    #[test]
    fn series_are_recommended_and_episodes_count_for_their_show() {
        let mut items = library(80);
        for id in 300..320 {
            items.push(show(id, &format!("Show {id}"), &["Crime"]));
        }
        let mut i = inputs(items);
        i.history = vec![watched((Kind::Series, 300), 1)];
        let rails = build(&i);
        let series = find(&rails, "series").expect("series rail");
        assert_eq!(series.title, "Series For You");
        assert!(!series.items.contains(&(Kind::Series, 300)));
        assert!(series.items.iter().all(|k| k.0 == Kind::Series));
    }

    #[test]
    fn helpers_survive_the_shapes_data_arrives_in() {
        assert_eq!(shelf_name("EN ✪ BOX OFFICE"), "box office");
        assert_eq!(shelf_name("UK ◉ UK SERIES"), "uk series");
        assert_eq!(shelf_name("Movies"), "movies");
        assert!(is_generic_shelf("series [multi-sub]") && is_generic_shelf("tv shows"));
        assert!(!is_generic_shelf("netflix") && !is_generic_shelf("box office"));
        assert_eq!(
            franchise_stem("John Wick: Chapter 4").as_deref(),
            Some("john wick")
        );
        assert_eq!(
            franchise_stem("EN John Wick 2").as_deref(),
            Some("john wick")
        );
        assert_eq!(lead_words("EN John Wick").as_deref(), Some("john wick"));
        assert_eq!(franchise_stem("The Man Who Knew Too Much"), None);
        assert_eq!(franchise_stem("Heat"), None);
        assert_eq!(display("science fiction"), "Science Fiction");
        assert_eq!(year_of(1_800_000_000), 2027);
        let u = unit(1, (Kind::Movie, 5));
        assert!((0.0..1.0).contains(&u));
    }
}
