//! The commands the contract declared and nobody registered.
//!
//! Every one of these was reachable from the UI and answered with "command not found"
//! on Windows: the home rails, the Settings counts, the provider list the first-run
//! check reads, My List, favourites and resume positions. The mock transport answered
//! all of them, so the browser preview looked complete while the shipped app had no
//! home screen.
//!
//! `crates/aurora-app/tests/contract.rs` now fails when the contract declares a
//! command the host does not register, which is what should have caught this.

use aurora_db::repo::{library, lists, progress, providers};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::Result;
use crate::now_unix;
use crate::services::Services;

/* ── Providers ────────────────────────────────────────────────────────────── */

#[tauri::command(async)]
pub fn providers_list(services: State<'_, Services>) -> Result<Vec<providers::ProviderRow>> {
    let db = services.db.lock();
    Ok(providers::list(&db)?)
}

/* ── Library ──────────────────────────────────────────────────────────────── */

#[tauri::command(async)]
pub fn library_stats(services: State<'_, Services>) -> Result<library::LibraryStats> {
    let db = services.db.lock();
    Ok(library::stats(&db)?)
}

/// One item on a home rail.
///
/// Serialized with the kind alongside the fields rather than wrapping them, because
/// that is what the contract's `{ kind: 'movie' } & Movie` describes and the rail
/// renderer reads `item.kind` off the same object as `item.title`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CatalogItem {
    Movie(library::MovieRow),
    Series(library::SeriesRow),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rail {
    pub id: String,
    pub kind: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// How far into each item, keyed `movie:12` / `series:7`.
    ///
    /// Only Continue Watching fills this. It rides on the rail rather than on the
    /// items because a `CatalogItem` is a library row — the same film is the same row
    /// whether it is in My List or half-watched, and giving it a position would make
    /// the row mean different things in different places.
    #[serde(skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub progress: std::collections::HashMap<String, RailProgress>,
    pub items: Vec<CatalogItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RailsArgs {
    pub profile_id: i64,
}

/// Where a half-watched item was left.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RailProgress {
    pub position_secs: i64,
    pub duration_secs: i64,
}

/// How many titles a rail carries. More than fits on screen, so sideways scrolling has
/// somewhere to go; far short of the library, so the home screen is not a full import.
const RAIL_SIZE: u32 = 24;

/// The home screen (README §11).
///
/// Empty rails are dropped rather than rendered empty: a row with a heading and no
/// posters reads as a loading failure, and on a fresh library most of these have
/// nothing in them yet.
#[tauri::command(async)]
pub fn library_rails(services: State<'_, Services>, args: RailsArgs) -> Result<Vec<Rail>> {
    let db = services.db.lock();
    let filters = aurora_db::repo::filtering::LibraryFilter::load(&db)?;

    let browse = |sort: library::MovieSort| library::BrowseQuery {
        sort,
        genre: None,
        limit: RAIL_SIZE,
        offset: 0,
        library: filters,
    };

    let mut rails = Vec::new();

    // Continue Watching first: it is the only rail about what the viewer was already
    // doing, and the one thing they are most likely to have come back for.
    //
    // Progress is stored against the episode but shown against the *show* — one card
    // saying "you are partway through this", not one per episode of it. Two episodes
    // of the same series collapse to the furthest-recent, which is the first seen
    // because the query is newest-first.
    let mut resumed: Vec<CatalogItem> = Vec::new();
    let mut resumed_progress = std::collections::HashMap::new();
    for p in progress::continue_watching(&db, args.profile_id, RAIL_SIZE)? {
        let item = match p.item_kind {
            progress::ItemKind::Movie => library::movie(&db, p.item_id)?.map(CatalogItem::Movie),
            progress::ItemKind::Episode => match library::series_of_episode(&db, p.item_id)? {
                Some(series_id) => library::series(&db, series_id)?.map(CatalogItem::Series),
                // The episode was removed by a refresh; its progress row outlives it
                // by design, and a card for a show that is gone is worse than no card.
                None => None,
            },
            // A channel or a recording is not a library row, so there is nothing here
            // to put on a poster rail.
            _ => None,
        };
        let Some(item) = item else { continue };
        let key = catalog_key(&item);
        if resumed_progress.contains_key(&key) {
            continue;
        }
        resumed_progress.insert(
            key,
            RailProgress {
                position_secs: p.position_secs,
                duration_secs: p.duration_secs,
            },
        );
        resumed.push(item);
    }
    if !resumed.is_empty() {
        rails.push(Rail {
            id: "continue".into(),
            kind: "continueWatching".into(),
            title: "Continue Watching".into(),
            reason: None,
            progress: resumed_progress,
            items: resumed,
        });
    }

    // My List next: it is the one rail whose contents somebody chose.
    let mut mine = Vec::new();
    for (kind, id) in lists::my_list(&db, args.profile_id)? {
        match kind {
            lists::ItemKind::Movie => {
                if let Some(m) = library::movie(&db, id)? {
                    mine.push(CatalogItem::Movie(m));
                }
            }
            lists::ItemKind::Series => {
                if let Some(s) = library::series(&db, id)? {
                    mine.push(CatalogItem::Series(s));
                }
            }
        }
    }
    push_if_any(&mut rails, "my-list", "myList", "My List", mine);

    let recent = library::list_movies(&db, &browse(library::MovieSort::RecentlyAdded))?;
    push_if_any(
        &mut rails,
        "recently-added",
        "recentlyAdded",
        "Recently added",
        recent.into_iter().map(CatalogItem::Movie).collect(),
    );

    let acclaimed = library::list_movies(&db, &browse(library::MovieSort::Rating))?;
    push_if_any(
        &mut rails,
        "acclaimed",
        "acclaimed",
        "Highly rated",
        acclaimed.into_iter().map(CatalogItem::Movie).collect(),
    );

    let shows = library::list_series(
        &db,
        &library::BrowseQuery {
            sort: library::MovieSort::Title,
            genre: None,
            limit: RAIL_SIZE,
            offset: 0,
            library: filters,
        },
    )?;
    push_if_any(
        &mut rails,
        "series",
        "recentlyAdded",
        "Series",
        shows.into_iter().map(CatalogItem::Series).collect(),
    );

    Ok(rails)
}

fn push_if_any(rails: &mut Vec<Rail>, id: &str, kind: &str, title: &str, items: Vec<CatalogItem>) {
    if items.is_empty() {
        return;
    }
    rails.push(Rail {
        id: id.into(),
        kind: kind.into(),
        title: title.into(),
        reason: None,
        progress: std::collections::HashMap::new(),
        items,
    });
}

/// The key a rail's progress map is read by, and the one the UI rebuilds per card.
pub fn catalog_key(item: &CatalogItem) -> String {
    match item {
        CatalogItem::Movie(m) => format!("movie:{}", m.id),
        CatalogItem::Series(s) => format!("series:{}", s.id),
    }
}

/* ── My List and favourites ───────────────────────────────────────────────── */

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MyListArgs {
    pub profile_id: i64,
    pub kind: String,
    pub id: i64,
}

/// Returns whether the title is on the list afterwards, so the UI never has to guess.
#[tauri::command(async)]
pub fn mylist_toggle(services: State<'_, Services>, args: MyListArgs) -> Result<bool> {
    let kind = match args.kind.as_str() {
        "series" => lists::ItemKind::Series,
        _ => lists::ItemKind::Movie,
    };
    let db = services.db.lock();
    Ok(lists::toggle_my_list(
        &db,
        args.profile_id,
        kind,
        args.id,
        now_unix(),
    )?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteArgs {
    pub profile_id: i64,
    pub channel_id: i64,
}

#[tauri::command(async)]
pub fn favorites_toggle(services: State<'_, Services>, args: FavoriteArgs) -> Result<bool> {
    let db = services.db.lock();
    Ok(lists::toggle_favorite(
        &db,
        args.profile_id,
        args.channel_id,
        now_unix(),
    )?)
}

/* ── Resume ───────────────────────────────────────────────────────────────── */

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetProgressArgs {
    pub profile_id: i64,
    pub kind: String,
    pub id: i64,
}

#[tauri::command(async)]
pub fn progress_get(
    services: State<'_, Services>,
    args: GetProgressArgs,
) -> Result<Option<progress::Progress>> {
    let kind = match args.kind.as_str() {
        "episode" => progress::ItemKind::Episode,
        "channel" => progress::ItemKind::Channel,
        _ => progress::ItemKind::Movie,
    };
    let db = services.db.lock();
    Ok(progress::get(&db, args.profile_id, kind, args.id)?)
}
