//! Playlist editing and the library-wide filters (README §7.3).
//!
//! The editor is the one screen that sees everything — hidden rows included — because
//! hiding something has to be reversible. Every other list in the app goes through the
//! filters instead.

use aurora_db::repo::filtering::{self, Alternate, FilterCounts, Kind, LibraryFilter};
use aurora_db::repo::playlist::{self, PlaylistRow, Query, Show};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::Result;
use crate::services::Services;
use crate::AppError;

fn kind_of(raw: &str) -> Result<Kind> {
    Kind::parse(raw).ok_or_else(|| AppError::Other(format!("unknown list {raw:?}")))
}

fn show_of(raw: Option<&str>) -> Show {
    match raw {
        Some("visible") => Show::Visible,
        Some("hidden") => Show::Hidden,
        _ => Show::All,
    }
}

/* ── Filters ──────────────────────────────────────────────────────────────── */

#[tauri::command(async)]
pub fn library_filters(services: State<'_, Services>) -> Result<LibraryFilter> {
    let db = services.db.lock();
    Ok(LibraryFilter::load(&db)?)
}

#[tauri::command(async)]
pub fn library_set_filters(
    services: State<'_, Services>,
    args: LibraryFilter,
) -> Result<LibraryFilter> {
    let db = services.db.lock();
    args.save(&db)?;
    Ok(LibraryFilter::load(&db)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KindArgs {
    pub kind: String,
}

#[tauri::command(async)]
pub fn library_filter_counts(
    services: State<'_, Services>,
    args: KindArgs,
) -> Result<FilterCounts> {
    let kind = kind_of(&args.kind)?;
    let db = services.db.lock();
    Ok(filtering::counts(&db, kind)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlternatesArgs {
    pub kind: String,
    pub id: i64,
}

#[tauri::command(async)]
pub fn library_alternates(
    services: State<'_, Services>,
    args: AlternatesArgs,
) -> Result<Vec<Alternate>> {
    let kind = kind_of(&args.kind)?;
    let db = services.db.lock();
    Ok(filtering::alternates(&db, kind, args.id)?)
}

/* ── The editor ───────────────────────────────────────────────────────────── */

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListArgs {
    pub kind: String,
    pub text: Option<String>,
    pub group: Option<String>,
    pub show: Option<String>,
    #[serde(default)]
    pub duplicates_only: bool,
    pub limit: u32,
    pub offset: u32,
}

impl ListArgs {
    fn query(&self) -> Result<Query> {
        Ok(Query {
            kind: kind_of(&self.kind)?,
            text: self.text.clone(),
            group: self.group.clone(),
            show: show_of(self.show.as_deref()),
            duplicates_only: self.duplicates_only,
            limit: self.limit.clamp(1, 1000),
            offset: self.offset,
        })
    }
}

/// A page of rows plus how many there are in total, so the editor can page without a
/// second round trip and can say "412 channels" in its header.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistPage {
    pub rows: Vec<PlaylistRow>,
    pub total: i64,
}

#[tauri::command(async)]
pub fn playlist_list(services: State<'_, Services>, args: ListArgs) -> Result<PlaylistPage> {
    let query = args.query()?;
    let db = services.db.lock();
    Ok(PlaylistPage {
        rows: playlist::list(&db, &query)?,
        total: playlist::count(&db, &query)?,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupRow {
    pub name: String,
    pub count: i64,
}

#[tauri::command(async)]
pub fn playlist_groups(services: State<'_, Services>, args: KindArgs) -> Result<Vec<GroupRow>> {
    let kind = kind_of(&args.kind)?;
    let db = services.db.lock();
    Ok(playlist::groups(&db, kind)?
        .into_iter()
        .map(|(name, count)| GroupRow { name, count })
        .collect())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateArgs {
    pub kind: String,
    pub id: i64,
    pub patch: playlist::Patch,
}

#[tauri::command(async)]
pub fn playlist_update(services: State<'_, Services>, args: UpdateArgs) -> Result<()> {
    let kind = kind_of(&args.kind)?;
    let db = services.db.lock();
    playlist::update(&db, kind, args.id, &args.patch)?;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetHiddenArgs {
    pub kind: String,
    pub ids: Vec<i64>,
    pub hidden: bool,
}

#[tauri::command(async)]
pub fn playlist_set_hidden(services: State<'_, Services>, args: SetHiddenArgs) -> Result<usize> {
    let kind = kind_of(&args.kind)?;
    let mut db = services.db.lock();
    Ok(playlist::set_hidden_many(
        &mut db,
        kind,
        &args.ids,
        args.hidden,
    )?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HideMatchingArgs {
    pub kind: String,
    pub text: Option<String>,
    pub group: Option<String>,
    pub show: Option<String>,
    #[serde(default)]
    pub duplicates_only: bool,
    pub hidden: bool,
}

#[tauri::command(async)]
pub fn playlist_hide_matching(
    services: State<'_, Services>,
    args: HideMatchingArgs,
) -> Result<usize> {
    let query = Query {
        kind: kind_of(&args.kind)?,
        text: args.text.clone(),
        group: args.group.clone(),
        show: show_of(args.show.as_deref()),
        duplicates_only: args.duplicates_only,
        ..Default::default()
    };
    let db = services.db.lock();
    Ok(playlist::hide_matching(&db, &query, args.hidden)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetArgs {
    pub kind: String,
    pub ids: Vec<i64>,
}

#[tauri::command(async)]
pub fn playlist_reset(services: State<'_, Services>, args: ResetArgs) -> Result<usize> {
    let kind = kind_of(&args.kind)?;
    let mut db = services.db.lock();
    Ok(playlist::reset(&mut db, kind, &args.ids)?)
}
