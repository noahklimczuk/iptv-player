//! The home screen's commands: build the feed, page its rails, learn from "not for me",
//! and keep TMDB's lists fresh.
//!
//! The feed is computed whole by `aurora_core::feed` and kept here, per profile, so a
//! rail can be paged to its end without rebuilding it: the screen asks for the first
//! page of every rail at once, and for more of one rail as the viewer scrolls it. A
//! rebuild between pages would reorder what the next page continues from, so pages are
//! always read from the build the first page came from, and only `library.rails`
//! builds.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use aurora_core::feed::{self, Inputs, Key, Kind};
use aurora_db::repo::{feed as store, filtering::LibraryFilter, library, lists, progress};
use aurora_db::rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::Result;
use crate::library::{catalog_key, CatalogItem, Rail, RailProgress};
use crate::now_unix;
use crate::services::Services;

/// What each rail carries on the first answer. More than fits across a screen, so the
/// first scroll has somewhere to go before the next page arrives.
pub const FIRST_PAGE: usize = 30;
/// A page after that, unless the screen asks for another size.
const PAGE: usize = 40;
/// The most one page request may ask for, so a UI bug cannot hydrate the library.
const MAX_PAGE: usize = 200;
/// How many titles Continue Watching reads. Not a display limit -- the rail pages like
/// the rest -- but somebody with five hundred things half-watched has a different
/// problem.
const CONTINUE_LIMIT: u32 = 500;
/// How old the trending list may be before it is not shown at all. "Trending now" from
/// a fortnight ago is a claim that is no longer true.
const TRENDING_SHOWN_FOR: i64 = 4 * 86_400;
/// How often each list is fetched again.
const TRENDING_REFRESH: i64 = 86_400;
const ALSO_LIKED_REFRESH: i64 = 14 * 86_400;
/// How many lists one refresh fetches, so the first one on a new install does not sit
/// on the network for a minute.
const FETCHES_PER_REFRESH: usize = 10;

/// One profile's built feed.
pub struct HomeCache {
    profile_id: i64,
    seed: u64,
    /// `store::stamp` when it was built.
    stamp: String,
    /// The first pages as they were sent, to send again while the stamp holds.
    response: Vec<Rail>,
    rails: Vec<feed::Rail>,
    progress: HashMap<String, RailProgress>,
    /// What it was built from, for the signals refresh to read anchors off.
    inputs: Arc<Inputs>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RailsArgs {
    pub profile_id: i64,
    /// Changes each time the app opens; close calls go a different way each session.
    #[serde(default)]
    pub seed: Option<u64>,
}

fn key_of(kind: lists::ItemKind, id: i64) -> Key {
    match kind {
        lists::ItemKind::Movie => (Kind::Movie, id),
        lists::ItemKind::Series => (Kind::Series, id),
    }
}

fn string_key(key: Key) -> String {
    format!("{}:{}", key.0.as_str(), key.1)
}

fn parse_kind(raw: &str) -> Option<Kind> {
    match raw {
        "movie" => Some(Kind::Movie),
        "series" => Some(Kind::Series),
        _ => None,
    }
}

/// Everything the feed reads, from the database and the TMDB cache.
fn gather(
    db: &Connection,
    profile_id: i64,
    seed: u64,
) -> Result<(Inputs, HashMap<String, RailProgress>)> {
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

    // Continue Watching as before: one card per show, the query collapses episodes
    // before its limit so the rail is not one show long.
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
        let k = string_key(key);
        if progress_map.contains_key(&k) {
            continue;
        }
        progress_map.insert(
            k,
            RailProgress {
                position_secs: p.position_secs,
                duration_secs: p.duration_secs,
            },
        );
        inputs.continue_watching.push(key);
    }

    // People: credits of what was watched first, then the filmographies of whoever
    // recurs in them -- a person rail needs every title they are in, not a sample.
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

    // TMDB's lists, from the cache only. Fetching is `refresh_signals`, which the
    // screen calls after it has drawn, so the home screen never waits on the network.
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

fn hydrate(db: &Connection, keys: &[Key]) -> Result<Vec<CatalogItem>> {
    let mut out = Vec::with_capacity(keys.len());
    for (kind, id) in keys {
        // A row a refresh removed since the build is skipped rather than faked.
        let item = match kind {
            Kind::Movie => library::movie(db, *id)?.map(CatalogItem::Movie),
            Kind::Series => library::series(db, *id)?.map(CatalogItem::Series),
        };
        out.extend(item);
    }
    Ok(out)
}

fn page_of(
    db: &Connection,
    rail: &feed::Rail,
    progress: &HashMap<String, RailProgress>,
    offset: usize,
    limit: usize,
) -> Result<RailPage> {
    let end = (offset + limit).min(rail.items.len());
    let keys = if offset < end {
        &rail.items[offset..end]
    } else {
        &[][..]
    };
    let items = hydrate(db, keys)?;
    let mut reasons = HashMap::new();
    let mut prog = HashMap::new();
    for item in &items {
        let k = catalog_key(item);
        let key = match item {
            CatalogItem::Movie(m) => (Kind::Movie, m.id),
            CatalogItem::Series(s) => (Kind::Series, s.id),
        };
        if let Some(r) = rail.reasons.get(&key) {
            reasons.insert(k.clone(), r.clone());
        }
        if rail.kind == "continueWatching" {
            if let Some(p) = progress.get(&k) {
                prog.insert(k, *p);
            }
        }
    }
    Ok(RailPage {
        has_more: end < rail.items.len(),
        total: rail.items.len(),
        items,
        reasons,
        progress: prog,
    })
}

/// The home screen (README §11): every rail, each with its first page.
pub fn rails(services: &Services, args: RailsArgs) -> Result<Vec<Rail>> {
    let seed = args.seed.unwrap_or_else(|| now_unix() as u64 / 3_600);
    let db = services.db.lock();
    let stamp = store::stamp(&db, args.profile_id, &LibraryFilter::load(&db)?)?;
    // Nothing the feed reads has moved since the last build: the same screen, without
    // reading the library again. The common case by far -- Movies and back.
    if let Some(home) = services.home.lock().as_ref() {
        if home.profile_id == args.profile_id && home.seed == seed && home.stamp == stamp {
            return Ok(home.response.clone());
        }
    }
    let (inputs, progress_map) = gather(&db, args.profile_id, seed)?;
    let built = feed::build(&inputs);

    let mut out = Vec::with_capacity(built.len());
    for rail in &built {
        let RailPage {
            items,
            reasons,
            progress,
            has_more,
            total,
        } = page_of(&db, rail, &progress_map, 0, FIRST_PAGE)?;
        if items.is_empty() {
            continue;
        }
        out.push(Rail {
            id: rail.id.clone(),
            kind: rail.kind.to_string(),
            title: rail.title.clone(),
            reason: None,
            progress,
            reasons,
            items,
            total,
            has_more,
        });
    }
    drop(db);
    *services.home.lock() = Some(HomeCache {
        profile_id: args.profile_id,
        seed,
        stamp,
        response: out.clone(),
        rails: built,
        progress: progress_map,
        inputs: Arc::new(inputs),
    });
    Ok(out)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RailPageArgs {
    pub profile_id: i64,
    pub rail_id: String,
    pub offset: u32,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub seed: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RailPage {
    pub items: Vec<CatalogItem>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub reasons: HashMap<String, String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub progress: HashMap<String, RailProgress>,
    pub has_more: bool,
    pub total: usize,
}

#[tauri::command(async)]
pub fn library_rail_page(services: State<'_, Services>, args: RailPageArgs) -> Result<RailPage> {
    let limit = (args.limit.unwrap_or(PAGE as u32) as usize).clamp(1, MAX_PAGE);
    // The build the first page came from. Only if there is none for this profile -- the
    // app was restarted under an open screen, or another profile has built since -- is
    // one made, and then the screen may see a page that does not follow on exactly.
    let cached = {
        let home = services.home.lock();
        home.as_ref()
            .filter(|h| h.profile_id == args.profile_id)
            .and_then(|h| {
                h.rails
                    .iter()
                    .find(|r| r.id == args.rail_id)
                    .map(|r| (r.clone(), h.progress.clone()))
            })
    };
    let (rail, progress) = match cached {
        Some(found) => found,
        None => {
            rails(
                &services,
                RailsArgs {
                    profile_id: args.profile_id,
                    seed: args.seed,
                },
            )?;
            let home = services.home.lock();
            let Some(h) = home.as_ref() else {
                return Ok(RailPage {
                    items: vec![],
                    reasons: HashMap::new(),
                    progress: HashMap::new(),
                    has_more: false,
                    total: 0,
                });
            };
            match h.rails.iter().find(|r| r.id == args.rail_id) {
                Some(r) => (r.clone(), h.progress.clone()),
                None => {
                    return Ok(RailPage {
                        items: vec![],
                        reasons: HashMap::new(),
                        progress: HashMap::new(),
                        has_more: false,
                        total: 0,
                    })
                }
            }
        }
    };
    let db = services.db.lock();
    page_of(&db, &rail, &progress, args.offset as usize, limit)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotInterestedArgs {
    pub profile_id: i64,
    pub kind: String,
    pub id: i64,
    /// False takes it back.
    #[serde(default = "yes")]
    pub on: bool,
}

fn yes() -> bool {
    true
}

/// "Not for me": the title leaves every rail now, and counts against what it is made
/// of from the next build on. Returns whether it is set afterwards.
#[tauri::command(async)]
pub fn library_not_interested(
    services: State<'_, Services>,
    args: NotInterestedArgs,
) -> Result<bool> {
    let Some(kind) = parse_kind(&args.kind) else {
        return Err(crate::error::AppError::Other(format!(
            "Not a title kind: {}",
            args.kind
        )));
    };
    let key = (kind, args.id);
    let on = {
        let db = services.db.lock();
        store::set_not_interested(&db, args.profile_id, key, args.on, now_unix())?
    };
    // Out of the build the screen is paging, so the next page does not bring it back.
    // Positions after it shift down by one, which is what the screen did too when it
    // took the card away.
    if on {
        if let Some(home) = services.home.lock().as_mut() {
            if home.profile_id == args.profile_id {
                for rail in &mut home.rails {
                    rail.items.retain(|k| *k != key);
                }
            }
        }
    }
    Ok(on)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshArgs {
    pub profile_id: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Refreshed {
    /// How many lists were fetched. Non-zero means the screen should build again.
    pub updated: u32,
}

/// Fetch whichever TMDB lists the feed would read and are missing or old.
///
/// Called by the screen after it has drawn, never by the build: the home screen must
/// not wait on the network, and a viewer without a metadata key gets every rail but
/// "Trending" and a little less from "Because You Watched", not an error.
#[tauri::command(async)]
pub fn library_refresh_home_signals(
    services: State<'_, Services>,
    args: RefreshArgs,
) -> Result<Refreshed> {
    let Ok(client) = crate::metadata::client(&services) else {
        return Ok(Refreshed { updated: 0 });
    };
    let inputs = {
        let home = services.home.lock();
        home.as_ref()
            .filter(|h| h.profile_id == args.profile_id)
            .map(|h| Arc::clone(&h.inputs))
    };
    let Some(inputs) = inputs else {
        return Ok(Refreshed { updated: 0 });
    };
    let now = now_unix();

    // Decide what to fetch with the database held only for reads.
    let mut wanted: Vec<(String, Option<(Kind, i64)>)> = Vec::new();
    {
        let db = services.db.lock();
        let stale = |key: &str, max_age: i64| -> Result<bool> {
            Ok(match store::tmdb_list(&db, key)? {
                Some((_, at)) => now - at > max_age,
                None => true,
            })
        };
        if stale(store::TRENDING_KEY, TRENDING_REFRESH)? {
            wanted.push((store::TRENDING_KEY.to_string(), None));
        }
        let tmdb_of: HashMap<Key, i64> = inputs
            .items
            .iter()
            .filter_map(|i| i.tmdb_id.map(|t| (i.key(), t)))
            .collect();
        let mut asked = HashSet::new();
        for anchor in feed::anchors_to_fetch(&inputs) {
            let Some(tmdb) = tmdb_of.get(&anchor) else {
                continue;
            };
            let key = store::also_liked_key(anchor.0, *tmdb);
            if asked.insert(key.clone()) && stale(&key, ALSO_LIKED_REFRESH)? {
                wanted.push((key, Some((anchor.0, *tmdb))));
            }
        }
    }
    wanted.truncate(FETCHES_PER_REFRESH);

    // The network, with nothing held.
    let mut fetched: Vec<(String, Vec<(Kind, i64)>)> = Vec::new();
    for (key, about) in wanted {
        let result = match about {
            None => client.trending(),
            Some((kind, id)) => client.recommendations(to_tmdb(kind), id),
        };
        match result {
            Ok(list) => fetched.push((
                key,
                list.into_iter()
                    .map(|r| (from_tmdb(r.kind), r.id))
                    .collect(),
            )),
            // One list failing is no reason to drop the rest; the next open retries.
            Err(e) => tracing::warn!(list = %key, error = %e.message, "home signal fetch failed"),
        }
    }
    let updated = fetched.len() as u32;
    if updated > 0 {
        let db = services.db.lock();
        for (key, list) in fetched {
            store::store_tmdb_list(&db, &key, &list, now)?;
        }
    }
    Ok(Refreshed { updated })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_match_how_the_screen_keys_cards() {
        assert_eq!(string_key((Kind::Movie, 12)), "movie:12");
        assert_eq!(string_key((Kind::Series, 7)), "series:7");
        assert_eq!(parse_kind("series"), Some(Kind::Series));
        assert_eq!(parse_kind("channel"), None);
    }
}
