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
    let db = services.read();
    Ok(providers::list(&db)?)
}

/* ── Library ──────────────────────────────────────────────────────────────── */

#[tauri::command(async)]
pub fn library_stats(services: State<'_, Services>) -> Result<library::LibraryStats> {
    let db = services.read();
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
    /// Why each item is here, keyed the same way as `progress`. Only the recommended
    /// rail fills it.
    ///
    /// Per item rather than per rail, because that is the only version anybody
    /// believes: "Because you watched Blade Runner" under one poster and "More Crime"
    /// under the next is the recommender showing its working, where a single heading
    /// for twenty titles is a claim about all of them that is true of none.
    #[serde(skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub reasons: std::collections::HashMap<String, String>,
    /// The first page of the rail; `library.railPage` has the rest.
    pub items: Vec<CatalogItem>,
    /// How many titles the whole rail holds, which is what its heading counts.
    pub total: usize,
    /// Whether there is more than `items` to page in.
    pub has_more: bool,
}

/// Where a half-watched item was left.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RailProgress {
    pub position_secs: i64,
    pub duration_secs: i64,
}

/// How many titles a rail of suggestions resolves to, where something still has to
/// pick a number: the Gemini rail asks a model for a list and keeps this many of the
/// matches. The home feed's own rails are not limited -- see `crate::home`.
pub(crate) const RAIL_SIZE: u32 = 24;

/// The home screen (README §11), built by `crate::home`.
///
/// It used to be built here, as six fixed shelves of `RAIL_SIZE`: recently added, best
/// rated, the first 24 series alphabetically, and one personal rail among them. Kept as
/// the registered command so the contract and every caller stay as they are.
#[tauri::command(async)]
pub fn library_rails(
    services: State<'_, Services>,
    args: crate::home::RailsArgs,
) -> Result<Vec<Rail>> {
    crate::home::rails(&services, args)
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

/// Like a film or show, or stop. Returns whether it is liked afterwards.
///
/// Stored in `favorites` beside the channel hearts, because that is the table
/// `repo::recommend::history` already reads to decide `FAVOURITE_BOOST` — a flag it has
/// always been asking about and nothing has ever set for a title.
#[tauri::command(async)]
pub fn likes_toggle(services: State<'_, Services>, args: MyListArgs) -> Result<bool> {
    let kind = match args.kind.as_str() {
        "series" => lists::ItemKind::Series,
        _ => lists::ItemKind::Movie,
    };
    let db = services.db.lock();
    Ok(lists::toggle_liked(
        &db,
        args.profile_id,
        kind,
        args.id,
        now_unix(),
    )?)
}

/// What this profile has marked, as `kind:id` keys.
///
/// One request for both sets rather than a question per poster: a grid draws a hundred
/// and twenty cards and each one needs to know whether its plus should be a tick. The
/// sets are small — tens of titles, not the library — so the interface holds them and
/// answers from memory.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Marks {
    pub my_list: Vec<String>,
    pub liked: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarksArgs {
    pub profile_id: i64,
}

#[tauri::command(async)]
pub fn lists_marks(services: State<'_, Services>, args: MarksArgs) -> Result<Marks> {
    let db = services.read();
    let key = |(kind, id): (lists::ItemKind, i64)| format!("{}:{id}", kind.as_str());
    Ok(Marks {
        my_list: lists::my_list(&db, args.profile_id)?
            .into_iter()
            .map(key)
            .collect(),
        liked: lists::liked(&db, args.profile_id)?
            .into_iter()
            .map(key)
            .collect(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteArgs {
    pub profile_id: i64,
    pub channel_id: i64,
    /// Which named list, or the default when absent (README §4).
    #[serde(default)]
    pub list: Option<String>,
}

#[tauri::command(async)]
pub fn favorites_toggle(services: State<'_, Services>, args: FavoriteArgs) -> Result<bool> {
    let db = services.db.lock();
    Ok(lists::toggle_favorite(
        &db,
        args.profile_id,
        args.channel_id,
        args.list.as_deref(),
        now_unix(),
    )?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorderFavoritesArgs {
    pub profile_id: i64,
    /// Which named list, or the default when absent.
    #[serde(default)]
    pub list: Option<String>,
    /// The whole list, in its new order.
    pub channel_ids: Vec<i64>,
}

/// Put the favourites in the order the viewer dragged them into (README §4).
///
/// The whole list rather than "move this one to position N": a positional patch has to
/// agree with the client about what the order *was*, and two of them racing leaves a
/// sequence neither party asked for. Sending the list the viewer is looking at makes the
/// operation idempotent and the result exactly what is on screen.
#[tauri::command(async)]
pub fn favorites_reorder(
    services: State<'_, Services>,
    args: ReorderFavoritesArgs,
) -> Result<usize> {
    let mut db = services.db.lock();
    Ok(lists::reorder_favorites(
        &mut db,
        args.profile_id,
        args.list.as_deref(),
        &args.channel_ids,
    )?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteListsArgs {
    pub profile_id: i64,
}

/// The named lists this profile has (README §4).
///
/// Derived from the rows rather than kept in a table: a list exists because something is
/// in it. The default is always offered even when empty, because it is where the heart
/// puts things and a picker that loses its only entry the moment you empty it has lost
/// the viewer their way back.
#[tauri::command(async)]
pub fn favorites_lists(
    services: State<'_, Services>,
    args: FavoriteListsArgs,
) -> Result<Vec<lists::FavoriteList>> {
    let db = services.read();
    Ok(lists::favorite_lists(&db, args.profile_id)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameFavoriteListArgs {
    pub profile_id: i64,
    pub from: String,
    pub to: String,
}

/// Rename a list, moving everything in it. Renaming onto an existing name merges them.
#[tauri::command(async)]
pub fn favorites_rename_list(
    services: State<'_, Services>,
    args: RenameFavoriteListArgs,
) -> Result<usize> {
    let db = services.db.lock();
    Ok(lists::rename_favorite_list(
        &db,
        args.profile_id,
        &args.from,
        &args.to,
    )?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteFavoriteListArgs {
    pub profile_id: i64,
    pub name: String,
}

/// Empty a list, returning how many channels were in it.
#[tauri::command(async)]
pub fn favorites_delete_list(
    services: State<'_, Services>,
    args: DeleteFavoriteListArgs,
) -> Result<usize> {
    let db = services.db.lock();
    Ok(lists::delete_favorite_list(
        &db,
        args.profile_id,
        &args.name,
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
        // Named, rather than falling through. `ItemKind::Recording` has existed since
        // the schema did, and the catch-all below filed a recording's position as a
        // *movie's* -- against the same rowid, so it collided with whatever film held
        // it. Nothing sent this kind until recordings became playable (R-118).
        "recording" => progress::ItemKind::Recording,
        _ => progress::ItemKind::Movie,
    };
    // Where another device stopped, before this PC answers from what it knows (§3.6).
    // Recordings are this PC's own files and never sync.
    let synced = match kind {
        progress::ItemKind::Movie => Some("movie"),
        progress::ItemKind::Episode => Some("episode"),
        _ => None,
    };
    if let Some(item_kind) = synced {
        let enabled = aurora_db::repo::sync::is_enabled(&services.read())?;
        services
            .sync
            .refresh_progress(enabled, args.profile_id, item_kind, args.id);
    }
    let db = services.read();
    Ok(progress::get(&db, args.profile_id, kind, args.id)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgetProgressArgs {
    pub profile_id: i64,
    /// `movie` or `series` — what the *card* is, which is not always what the progress
    /// is stored against. A show's position lives on its episodes.
    pub kind: String,
    pub id: i64,
}

/// Take something off Continue Watching.
///
/// Addressed the way the rail is addressed, by the card: a show is removed by its series
/// id even though the rows being deleted are its episodes'. Asking the UI to find the
/// episode the card happens to stand for would make it the only screen that has to know
/// how the rail is assembled, and would be wrong anyway — one episode's position going
/// just promotes the next.
#[tauri::command(async)]
pub fn progress_forget(services: State<'_, Services>, args: ForgetProgressArgs) -> Result<bool> {
    let db = services.db.lock();
    let removed = match args.kind.as_str() {
        "series" => progress::forget_series(&db, args.profile_id, args.id)?,
        "movie" => progress::forget_movie(&db, args.profile_id, args.id)?,
        other => {
            return Err(crate::AppError::Other(format!(
                "nothing on Continue Watching is a {other:?}"
            )))
        }
    };
    Ok(removed > 0)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesProgressArgs {
    pub profile_id: i64,
    pub series_id: i64,
}

/// Every position this profile holds for one show, for the episode list.
///
/// One call rather than one per episode: a season of a long-running show is forty rows,
/// and the modal would open with forty round trips before it could draw a single tick.
#[tauri::command(async)]
pub fn progress_for_series(
    services: State<'_, Services>,
    args: SeriesProgressArgs,
) -> Result<Vec<progress::EpisodeProgress>> {
    let db = services.read();
    Ok(progress::for_series(&db, args.profile_id, args.series_id)?)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumePoint {
    pub episode_id: i64,
    pub position_secs: i64,
}

/// Which episode Play should open for a show, and where in it to start.
///
/// Null when the show has never been touched or has been watched to the end, and the
/// caller opens the first episode — which is what it used to do unconditionally, and is
/// the bug: pressing Play on a show you are four episodes into started the pilot again.
#[tauri::command(async)]
pub fn progress_resume_point(
    services: State<'_, Services>,
    args: SeriesProgressArgs,
) -> Result<Option<ResumePoint>> {
    let db = services.read();
    Ok(
        progress::resume_point(&db, args.profile_id, args.series_id)?.map(
            |(episode_id, position_secs)| ResumePoint {
                episode_id,
                position_secs,
            },
        ),
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkWatchedArgs {
    pub profile_id: i64,
    /// `episode` or `movie`.
    pub kind: String,
    pub id: i64,
    pub watched: bool,
}

/// Mark one thing watched, or put it back to unwatched.
///
/// Marking sets the flag and leaves any position alone, so a show marked watched by
/// mistake still remembers where it got to when it is unmarked. Both directions matter:
/// a provider that re-lists a show, or an episode watched somewhere else, are the two
/// reasons anyone reaches for this.
#[tauri::command(async)]
pub fn progress_set_watched(services: State<'_, Services>, args: MarkWatchedArgs) -> Result<()> {
    let kind = match args.kind.as_str() {
        "episode" => progress::ItemKind::Episode,
        "movie" => progress::ItemKind::Movie,
        other => {
            return Err(crate::AppError::Other(format!(
                "{other:?} is not something that can be marked watched"
            )))
        }
    };
    let db = services.db.lock();
    progress::mark_watched(
        &db,
        args.profile_id,
        kind,
        args.id,
        args.watched,
        now_unix(),
    )?;
    Ok(())
}
