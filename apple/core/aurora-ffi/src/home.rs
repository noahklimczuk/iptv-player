//! The home screen: Continue Watching, then the personal feed, as Windows builds it
//! (`aurora-app/src/home.rs`).
//!
//! The rails themselves are scored by the shared `aurora_core::feed`, from inputs read
//! by the shared `aurora_db::repo::feed`; this module only gathers those inputs, keeps
//! the built feed so a rail can be paged without scoring it again, and turns keys into
//! cards. The constants are Windows' (checked by `mirrors.rs`).
//!
//! The one difference is the shape handed across: Windows sends each rail's reasons and
//! progress as maps keyed `movie:12`, for a web screen to look up per card. Swift gets
//! each card with its own reason and progress attached, which is the same information
//! without a string key to rebuild.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use aurora_core::feed::{self, Inputs, Key, Kind};
use aurora_db::repo::{feed as store, filtering::LibraryFilter, library, lists, progress};
use aurora_db::rusqlite::Connection;

use crate::mylist::ListKind;
use crate::vod::{Movie, Series};
use crate::{now_unix, AuroraCore, Result};

/// Cards on each rail in the first answer; the rest come a page at a time.
pub(crate) const FIRST_PAGE: usize = 30;
pub(crate) const PAGE: usize = 40;
pub(crate) const MAX_PAGE: usize = 200;
/// How far back Continue Watching reads before it is folded to one card per title.
pub(crate) const CONTINUE_LIMIT: u32 = 500;
/// A trending list older than this is not shown at all.
pub(crate) const TRENDING_SHOWN_FOR: i64 = 4 * 86_400;
pub(crate) const TRENDING_REFRESH: i64 = 86_400;
pub(crate) const ALSO_LIKED_REFRESH: i64 = 14 * 86_400;
/// TMDB calls one refresh may make.
pub(crate) const FETCHES_PER_REFRESH: usize = 10;

/// The feed last built, kept so paging a rail does not score the library again. Thrown
/// away when the profile, the seed or anything the feed reads (the stamp) changes.
pub(crate) struct HomeCache {
    profile_id: i64,
    seed: u64,
    stamp: String,
    response: Vec<Rail>,
    rails: Vec<feed::Rail>,
    progress: HashMap<Key, RailProgress>,
    inputs: Arc<Inputs>,
}

/// A film or a show, as a card.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum CatalogItem {
    Movie { movie: Movie },
    Series { series: Series },
}

/// How far into a title, for the bar under a Continue Watching card.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct RailProgress {
    pub position_secs: i64,
    pub duration_secs: i64,
}

/// One card on a rail.
#[derive(Debug, Clone, uniffi::Record)]
pub struct RailItem {
    pub item: CatalogItem,
    /// The line under the poster ("Because you watched Heat"), where the rail has one.
    pub reason: Option<String>,
    /// Only on Continue Watching.
    pub progress: Option<RailProgress>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Rail {
    /// Stable for this feed: hand it back to `home_rail_page`.
    pub id: String,
    /// Windows' `RailKind`: `continueWatching`, `myList`, `forYou`, `top10`, …
    pub kind: String,
    pub title: String,
    /// The first page.
    pub items: Vec<RailItem>,
    /// Everything on the rail, not just the first page.
    pub total: u32,
    pub has_more: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RailPage {
    pub items: Vec<RailItem>,
    pub has_more: bool,
    pub total: u32,
}

fn key_of(kind: lists::ItemKind, id: i64) -> Key {
    match kind {
        lists::ItemKind::Movie => (Kind::Movie, id),
        lists::ItemKind::Series => (Kind::Series, id),
    }
}

fn feed_kind(kind: ListKind) -> Kind {
    match kind {
        ListKind::Movie => Kind::Movie,
        ListKind::Series => Kind::Series,
    }
}

fn to_tmdb(kind: Kind) -> aurora_ingest::tmdb::Kind {
    match kind {
        Kind::Movie => aurora_ingest::tmdb::Kind::Movie,
        Kind::Series => aurora_ingest::tmdb::Kind::Series,
    }
}

fn from_tmdb(kind: aurora_ingest::tmdb::Kind) -> Kind {
    match kind {
        aurora_ingest::tmdb::Kind::Movie => Kind::Movie,
        aurora_ingest::tmdb::Kind::Series => Kind::Series,
    }
}

/// Everything the feed is scored from, and where each Continue Watching title got to.
fn gather(
    db: &Connection,
    profile_id: i64,
    seed: u64,
) -> Result<(Inputs, HashMap<Key, RailProgress>)> {
    let now = now_unix();
    let filter = LibraryFilter::load(db)?;
    let mut inputs = Inputs {
        items: store::items(db, &filter)?,
        history: store::watches(db, profile_id)?,
        liked: lists::liked(db, profile_id)?
            .into_iter()
            .map(|(k, i)| key_of(k, i))
            .collect(),
        my_list: lists::my_list(db, profile_id)?
            .into_iter()
            .map(|(k, i)| key_of(k, i))
            .collect(),
        not_interested: store::not_interested(db, profile_id)?,
        now,
        seed,
        ..Default::default()
    };

    // Continue Watching: newest first, one card per title. A show's card stands for
    // whichever of its episodes was played last.
    let mut progress_map = HashMap::new();
    for p in progress::continue_watching(db, profile_id, CONTINUE_LIMIT)? {
        let key = match p.item_kind {
            progress::ItemKind::Movie => (Kind::Movie, p.item_id),
            progress::ItemKind::Episode => match library::series_of_episode(db, p.item_id)? {
                Some(series) => (Kind::Series, series),
                None => continue,
            },
            _ => continue,
        };
        if progress_map.contains_key(&key) {
            continue;
        }
        progress_map.insert(
            key,
            RailProgress {
                position_secs: p.position_secs,
                duration_secs: p.duration_secs,
            },
        );
        inputs.continue_watching.push(key);
    }

    // Credits for what was watched or liked, then the whole filmography of the people
    // the feed follows, so "More with Al Pacino" can find what they are in.
    let watched: Vec<Key> = inputs
        .history
        .iter()
        .map(|w| w.key)
        .chain(inputs.liked.iter().copied())
        .collect();
    inputs.credits = store::credits_for(db, &watched)?;
    let people = feed::people_to_follow(&inputs);
    for (key, credits) in store::filmographies(db, &people)? {
        let list = inputs.credits.entry(key).or_default();
        for c in credits {
            if !list
                .iter()
                .any(|x| x.person == c.person && x.director == c.director)
            {
                list.push(c);
            }
        }
    }

    // TMDB's lists, from the cache only: fetching is `refresh_home_signals`' job.
    let by_key: HashMap<Key, Option<i64>> =
        inputs.items.iter().map(|i| (i.key(), i.tmdb_id)).collect();
    for anchor in feed::anchors_to_fetch(&inputs) {
        let Some(Some(tmdb)) = by_key.get(&anchor) else {
            continue;
        };
        if let Some((list, _)) = store::tmdb_list(db, &store::also_liked_key(anchor.0, *tmdb))? {
            inputs.also_liked.insert((anchor.0, *tmdb), list);
        }
    }
    if let Some((list, at)) = store::tmdb_list(db, store::TRENDING_KEY)? {
        if now - at <= TRENDING_SHOWN_FOR {
            inputs.trending = list;
        }
    }
    Ok((inputs, progress_map))
}

/// One page of a built rail, as cards. A key whose title has gone since the feed was
/// built is skipped rather than failing the page.
fn page_of(
    db: &Connection,
    rail: &feed::Rail,
    progress: &HashMap<Key, RailProgress>,
    offset: usize,
    limit: usize,
) -> Result<RailPage> {
    let end = (offset + limit).min(rail.items.len());
    let keys = if offset < end { &rail.items[offset..end] } else { &[][..] };
    let continuing = rail.kind == "continueWatching";
    let mut items = Vec::with_capacity(keys.len());
    for key in keys {
        let item = match key.0 {
            Kind::Movie => library::movie(db, key.1)?.map(|m| CatalogItem::Movie { movie: m.into() }),
            Kind::Series => {
                library::series(db, key.1)?.map(|s| CatalogItem::Series { series: s.into() })
            }
        };
        let Some(item) = item else { continue };
        items.push(RailItem {
            item,
            reason: rail.reasons.get(key).cloned(),
            progress: if continuing { progress.get(key).copied() } else { None },
        });
    }
    Ok(RailPage {
        items,
        has_more: end < rail.items.len(),
        total: rail.items.len() as u32,
    })
}

fn empty_page() -> RailPage {
    RailPage {
        items: vec![],
        has_more: false,
        total: 0,
    }
}

/// The seed Windows uses when the screen gives none: it changes every hour.
fn default_seed() -> u64 {
    now_unix() as u64 / 3_600
}

#[uniffi::export]
impl AuroraCore {
    /// The home screen's rails, each with its first page. Rails with nothing on them are
    /// left out. `seed` keeps the shuffle the same for one visit: pass the same value
    /// until the app is opened again.
    pub fn home_rails(&self, profile_id: i64, seed: Option<u64>) -> Result<Vec<Rail>> {
        let seed = seed.unwrap_or_else(default_seed);
        let db = self.db.lock();
        let stamp = store::stamp(&db, profile_id, &LibraryFilter::load(&db)?)?;
        if let Some(home) = self.home.lock().as_ref() {
            if home.profile_id == profile_id && home.seed == seed && home.stamp == stamp {
                return Ok(home.response.clone());
            }
        }
        let (inputs, progress_map) = gather(&db, profile_id, seed)?;
        drop(db);

        // Scoring touches no database, so nothing waits on it.
        let built = feed::build(&inputs);

        let db = self.db.lock();
        let mut out = Vec::with_capacity(built.len());
        for rail in &built {
            let page = page_of(&db, rail, &progress_map, 0, FIRST_PAGE)?;
            if page.items.is_empty() {
                continue;
            }
            out.push(Rail {
                id: rail.id.clone(),
                kind: rail.kind.to_string(),
                title: rail.title.clone(),
                items: page.items,
                total: page.total,
                has_more: page.has_more,
            });
        }
        drop(db);
        *self.home.lock() = Some(HomeCache {
            profile_id,
            seed,
            stamp,
            response: out.clone(),
            rails: built,
            progress: progress_map,
            inputs: Arc::new(inputs),
        });
        Ok(out)
    }

    /// More of one rail, from `offset`. Builds the feed first if it isn't kept (another
    /// profile was shown since); a rail that no longer exists is an empty page.
    pub fn home_rail_page(
        &self,
        profile_id: i64,
        rail_id: String,
        offset: u32,
        limit: Option<u32>,
        seed: Option<u64>,
    ) -> Result<RailPage> {
        let limit = (limit.unwrap_or(PAGE as u32) as usize).clamp(1, MAX_PAGE);
        let find = |core: &Self| {
            core.home
                .lock()
                .as_ref()
                .filter(|h| h.profile_id == profile_id)
                .and_then(|h| {
                    h.rails
                        .iter()
                        .find(|r| r.id == rail_id)
                        .map(|r| (r.clone(), h.progress.clone()))
                })
        };
        let found = match find(self) {
            Some(found) => Some(found),
            None => {
                self.home_rails(profile_id, seed)?;
                find(self)
            }
        };
        let Some((rail, progress)) = found else {
            return Ok(empty_page());
        };
        let db = self.db.lock();
        page_of(&db, &rail, &progress, offset as usize, limit)
    }

    /// "Not interested": the title stops being suggested to this profile. `on: false`
    /// takes it back. Returns whether it is now hidden. The kept feed drops it at once,
    /// so a rail paged after this doesn't bring it back.
    pub fn set_not_interested(
        &self,
        profile_id: i64,
        kind: ListKind,
        id: i64,
        on: bool,
    ) -> Result<bool> {
        let key = (feed_kind(kind), id);
        let on = {
            let db = self.db.lock();
            store::set_not_interested(&db, profile_id, key, on, now_unix())?
        };
        if on {
            if let Some(home) = self.home.lock().as_mut() {
                if home.profile_id == profile_id {
                    for rail in &mut home.rails {
                        rail.items.retain(|k| *k != key);
                    }
                }
            }
        }
        Ok(on)
    }

    /// Take a title off Continue Watching. A show is addressed by its series id, and
    /// every episode position it holds goes, as on Windows (`progress_forget`): one
    /// episode going would only promote the next. Returns whether anything was removed.
    pub fn forget_progress(&self, profile_id: i64, kind: ListKind, id: i64) -> Result<bool> {
        let db = self.db.lock();
        let removed = match kind {
            ListKind::Series => progress::forget_series(&db, profile_id, id)?,
            ListKind::Movie => progress::forget_movie(&db, profile_id, id)?,
        };
        Ok(removed > 0)
    }

    /// Fetch the TMDB lists the feed reads (trending, and "also liked" for the titles it
    /// is anchored on) where they are missing or old, at most `FETCHES_PER_REFRESH` at a
    /// time. Returns how many were stored: when it is more than none, ask for the rails
    /// again. Does nothing without a metadata key, or before the rails have been built.
    /// Makes network calls: run it off the main thread.
    pub fn refresh_home_signals(&self, profile_id: i64) -> Result<u32> {
        let Some(client) = self.tmdb_client() else {
            return Ok(0);
        };
        let inputs = {
            let home = self.home.lock();
            home.as_ref()
                .filter(|h| h.profile_id == profile_id)
                .map(|h| Arc::clone(&h.inputs))
        };
        let Some(inputs) = inputs else {
            return Ok(0);
        };
        let now = now_unix();
        let wanted = {
            let db = self.db.lock();
            wanted_lists(&db, &inputs, now)?
        };

        let mut fetched: Vec<(String, Vec<(Kind, i64)>)> = Vec::new();
        for (key, about) in wanted {
            let result = match about {
                None => client.trending(),
                Some((kind, id)) => client.recommendations(to_tmdb(kind), id),
            };
            match result {
                Ok(list) => fetched.push((
                    key,
                    list.into_iter().map(|r| (from_tmdb(r.kind), r.id)).collect(),
                )),
                Err(e) => tracing::warn!(list = %key, error = %e.message, "home signal fetch failed"),
            }
        }
        let updated = fetched.len() as u32;
        if updated > 0 {
            let db = self.db.lock();
            for (key, list) in fetched {
                store::store_tmdb_list(&db, &key, &list, now)?;
            }
        }
        Ok(updated)
    }
}

/// Which TMDB lists are due, trending first, then one per anchor; `None` is trending.
fn wanted_lists(
    db: &Connection,
    inputs: &Inputs,
    now: i64,
) -> Result<Vec<(String, Option<(Kind, i64)>)>> {
    let stale = |key: &str, max_age: i64| -> Result<bool> {
        Ok(match store::tmdb_list(db, key)? {
            Some((_, at)) => now - at > max_age,
            None => true,
        })
    };
    let mut wanted = Vec::new();
    if stale(store::TRENDING_KEY, TRENDING_REFRESH)? {
        wanted.push((store::TRENDING_KEY.to_string(), None));
    }
    let tmdb_of: HashMap<Key, i64> = inputs
        .items
        .iter()
        .filter_map(|i| i.tmdb_id.map(|t| (i.key(), t)))
        .collect();
    let mut asked = HashSet::new();
    for anchor in feed::anchors_to_fetch(inputs) {
        let Some(tmdb) = tmdb_of.get(&anchor) else {
            continue;
        };
        let key = store::also_liked_key(anchor.0, *tmdb);
        if asked.insert(key.clone()) && stale(&key, ALSO_LIKED_REFRESH)? {
            wanted.push((key, Some((anchor.0, *tmdb))));
        }
    }
    wanted.truncate(FETCHES_PER_REFRESH);
    Ok(wanted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::core;
    use crate::vod::VodKind;

    /// `n` films, newest added last, all on one shelf.
    fn films(core: &AuroraCore, n: usize) {
        let db = core.db.lock();
        db.execute(
            "INSERT INTO providers (id, name, kind, base_url, created_at) VALUES (1, 'P', 'm3u', 'http://p', 0)",
            [],
        )
        .unwrap();
        for i in 0..n {
            db.execute(
                "INSERT INTO movies (provider_id, provider_key, title, match_key, group_title,
                                     url, genres, added_at, last_seen_at)
                 VALUES (1, ?1, ?2, lower(?2), 'Films', 'http://p/m.mp4', '[\"crime\"]', ?3, 0)",
                aurora_db::rusqlite::params![format!("k{i}"), format!("Film {i}"), i as i64],
            )
            .unwrap();
        }
    }

    fn movie_id(item: &RailItem) -> i64 {
        match &item.item {
            CatalogItem::Movie { movie } => movie.id,
            CatalogItem::Series { series } => panic!("expected a film, got {}", series.title),
        }
    }

    fn all_items(core: &AuroraCore, rail: &Rail) -> Vec<i64> {
        let mut ids: Vec<i64> = rail.items.iter().map(movie_id).collect();
        while (ids.len() as u32) < rail.total {
            let page = core
                .home_rail_page(1, rail.id.clone(), ids.len() as u32, None, Some(7))
                .unwrap();
            assert!(!page.items.is_empty(), "a rail with more must page");
            ids.extend(page.items.iter().map(movie_id));
        }
        ids
    }

    #[test]
    fn an_empty_library_has_no_rails() {
        let (core, _dir) = core();
        assert!(core.home_rails(1, Some(7)).unwrap().is_empty());
    }

    #[test]
    fn continue_watching_carries_where_the_viewer_got_to() {
        let (core, _dir) = core();
        films(&core, 5);
        core.save_progress(1, VodKind::Movie, 3, 600, 6000).unwrap();
        let rails = core.home_rails(1, Some(7)).unwrap();
        let cw = rails
            .iter()
            .find(|r| r.kind == "continueWatching")
            .expect("a started film is on Continue Watching");
        assert_eq!(cw.items.len(), 1);
        assert_eq!(movie_id(&cw.items[0]), 3);
        assert_eq!(
            cw.items[0].progress,
            Some(RailProgress { position_secs: 600, duration_secs: 6000 })
        );
        // Only Continue Watching carries progress.
        for rail in rails.iter().filter(|r| r.kind != "continueWatching") {
            assert!(rail.items.iter().all(|i| i.progress.is_none()), "{}", rail.kind);
        }

        assert!(core.forget_progress(1, ListKind::Movie, 3).unwrap());
        let rails = core.home_rails(1, Some(7)).unwrap();
        assert!(rails.iter().all(|r| r.kind != "continueWatching"));
        assert!(!core.forget_progress(1, ListKind::Movie, 3).unwrap());
    }

    #[test]
    fn a_long_rail_pages_to_its_total_without_repeats() {
        let (core, _dir) = core();
        films(&core, FIRST_PAGE + 25);
        let rails = core.home_rails(1, Some(7)).unwrap();
        let long = rails
            .iter()
            .find(|r| r.has_more)
            .expect("some rail is longer than a first page");
        assert_eq!(long.items.len(), FIRST_PAGE);
        let ids = all_items(&core, long);
        assert_eq!(ids.len() as u32, long.total);
        let unique: HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len());

        let past_end = core
            .home_rail_page(1, long.id.clone(), long.total + 5, None, Some(7))
            .unwrap();
        assert!(past_end.items.is_empty() && !past_end.has_more);
        let gone = core
            .home_rail_page(1, "no-such-rail".into(), 0, None, Some(7))
            .unwrap();
        assert!(gone.items.is_empty());
    }

    #[test]
    fn the_same_seed_and_library_answer_from_the_kept_feed() {
        let (core, _dir) = core();
        films(&core, 10);
        let first = core.home_rails(1, Some(7)).unwrap();
        let again = core.home_rails(1, Some(7)).unwrap();
        let ids = |r: &[Rail]| -> Vec<(String, Vec<i64>)> {
            r.iter().map(|r| (r.id.clone(), r.items.iter().map(movie_id).collect())).collect()
        };
        assert_eq!(ids(&first), ids(&again));
    }

    #[test]
    fn not_interested_takes_a_title_off_every_suggestion() {
        let (core, _dir) = core();
        films(&core, 8);
        let rails = core.home_rails(1, Some(7)).unwrap();
        let victim = movie_id(&rails[0].items[0]);
        assert!(core.set_not_interested(1, ListKind::Movie, victim, true).unwrap());

        // The kept feed forgets it at once, for paging…
        for rail in &rails {
            let page = core.home_rail_page(1, rail.id.clone(), 0, Some(200), Some(7)).unwrap();
            assert!(page.items.iter().all(|i| movie_id(i) != victim), "{}", rail.kind);
        }
        // …and a rebuilt feed leaves it out.
        for rail in core.home_rails(1, Some(7)).unwrap() {
            assert!(all_items(&core, &rail).iter().all(|id| *id != victim), "{}", rail.kind);
        }

        assert!(!core.set_not_interested(1, ListKind::Movie, victim, false).unwrap());
        let back = core
            .home_rails(1, Some(7))
            .unwrap()
            .iter()
            .any(|r| all_items(&core, r).contains(&victim));
        assert!(back, "taking it back brings it back");
    }

    #[test]
    fn signals_need_a_key_and_a_built_feed() {
        let (core, _dir) = core();
        films(&core, 3);
        // No key in a test build unless the environment set one.
        if core.tmdb_client().is_none() {
            core.home_rails(1, Some(7)).unwrap();
            assert_eq!(core.refresh_home_signals(1).unwrap(), 0);
        }
        // With no feed built for this profile there is nothing to anchor on.
        let (fresh, _d2) = crate::testing::core();
        assert_eq!(fresh.refresh_home_signals(1).unwrap(), 0);
    }

    #[test]
    fn trending_and_one_list_per_anchor_are_asked_for_when_stale() {
        let (core, _dir) = core();
        films(&core, 3);
        let db = core.db.lock();
        let (inputs, _) = gather(&db, 1, 7).unwrap();
        let now = now_unix();
        let wanted = wanted_lists(&db, &inputs, now).unwrap();
        assert_eq!(wanted.first().map(|w| w.0.as_str()), Some(store::TRENDING_KEY));
        assert!(wanted.len() <= FETCHES_PER_REFRESH);

        store::store_tmdb_list(&db, store::TRENDING_KEY, &[(Kind::Movie, 1)], now).unwrap();
        let wanted = wanted_lists(&db, &inputs, now).unwrap();
        assert!(wanted.iter().all(|w| w.0 != store::TRENDING_KEY), "fresh trending is kept");
        let later = now + TRENDING_REFRESH + 1;
        let wanted = wanted_lists(&db, &inputs, later).unwrap();
        assert_eq!(wanted.first().map(|w| w.0.as_str()), Some(store::TRENDING_KEY));
    }
}
