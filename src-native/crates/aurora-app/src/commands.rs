//! The typed IPC surface. Mirrors `shared/ipc.ts`; the names here are what the UI's
//! `invoke()` maps onto (dots become underscores).

use aurora_db::repo::{channels, epg, library, progress, search};
use aurora_player::{Aspect, PlayerState};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::Result;
use crate::services::Services;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListChannelsArgs {
    pub group: Option<String>,
    pub favorites_only: Option<bool>,
}

#[tauri::command]
pub fn channels_list(
    services: State<'_, Services>,
    args: ListChannelsArgs,
) -> Result<Vec<channels::ChannelRow>> {
    let db = services.db.lock();
    Ok(channels::list(
        &db,
        &channels::ChannelFilter {
            group: args.group,
            include_hidden: false,
            radio_only: false,
            limit: None,
            offset: 0,
        },
    )?)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupRow {
    pub name: String,
    pub count: i64,
}

#[tauri::command]
pub fn channels_groups(services: State<'_, Services>) -> Result<Vec<GroupRow>> {
    let db = services.db.lock();
    Ok(channels::groups(&db)?
        .into_iter()
        .map(|(name, count)| GroupRow { name, count })
        .collect())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ByNumberArgs {
    pub number: u32,
}

#[tauri::command]
pub fn channels_by_number(
    services: State<'_, Services>,
    args: ByNumberArgs,
) -> Result<Option<channels::ChannelRow>> {
    let db = services.db.lock();
    let Some(id) = channels::find_by_number(&db, args.number)? else {
        return Ok(None);
    };
    Ok(channels::list(&db, &channels::ChannelFilter::default())?
        .into_iter()
        .find(|c| c.id == id))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridSliceArgs {
    pub from: i64,
    pub to: i64,
    pub channel_ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideSlice {
    pub channels: Vec<channels::ChannelRow>,
    pub programmes: std::collections::HashMap<String, Vec<epg::ProgrammeRow>>,
    pub from: i64,
    pub to: i64,
}

#[tauri::command]
pub fn epg_grid_slice(services: State<'_, Services>, args: GridSliceArgs) -> Result<GuideSlice> {
    let db = services.db.lock();
    let all = channels::list(&db, &channels::ChannelFilter::default())?;
    let wanted: Vec<channels::ChannelRow> = if args.channel_ids.is_empty() {
        all
    } else {
        all.into_iter()
            .filter(|c| args.channel_ids.contains(&c.id))
            .collect()
    };

    let epg_ids: Vec<String> = wanted
        .iter()
        .filter_map(|c| c.epg_channel_id.clone())
        .collect();
    let rows = epg::grid_slice(&db, &epg_ids, args.from, args.to)?;

    let mut programmes: std::collections::HashMap<String, Vec<epg::ProgrammeRow>> =
        std::collections::HashMap::new();
    for row in rows {
        programmes
            .entry(row.channel_id.clone())
            .or_default()
            .push(row);
    }

    Ok(GuideSlice {
        channels: wanted,
        programmes,
        from: args.from,
        to: args.to,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NowNextArgs {
    pub channel_id: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowNext {
    pub now: Option<epg::ProgrammeRow>,
    pub next: Option<epg::ProgrammeRow>,
}

#[tauri::command]
pub fn epg_now_next(services: State<'_, Services>, args: NowNextArgs) -> Result<NowNext> {
    let db = services.db.lock();
    let Some(ch) = channels::list(&db, &channels::ChannelFilter::default())?
        .into_iter()
        .find(|c| c.id == args.channel_id)
    else {
        return Ok(NowNext {
            now: None,
            next: None,
        });
    };
    let Some(epg_id) = ch.epg_channel_id else {
        return Ok(NowNext {
            now: None,
            next: None,
        });
    };
    let (now, next) = epg::now_next(&db, &epg_id, now_unix())?;
    Ok(NowNext { now, next })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoviesArgs {
    pub sort: String,
    pub limit: u32,
    pub offset: u32,
    pub genre: Option<String>,
}

#[tauri::command]
pub fn library_movies(
    services: State<'_, Services>,
    args: MoviesArgs,
) -> Result<Vec<library::MovieRow>> {
    let sort = match args.sort.as_str() {
        "title" => library::MovieSort::Title,
        "year" => library::MovieSort::Year,
        "rating" => library::MovieSort::Rating,
        _ => library::MovieSort::RecentlyAdded,
    };
    let db = services.db.lock();
    Ok(library::list_movies(&db, sort, args.limit, args.offset)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodesArgs {
    pub series_id: i64,
    pub season: Option<u16>,
}

#[tauri::command]
pub fn library_episodes(
    services: State<'_, Services>,
    args: EpisodesArgs,
) -> Result<Vec<library::EpisodeRow>> {
    let db = services.db.lock();
    Ok(library::episodes_for(&db, args.series_id, args.season)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchArgs {
    pub text: String,
}

#[tauri::command]
pub fn search_query(
    services: State<'_, Services>,
    args: SearchArgs,
) -> Result<Vec<search::SearchHit>> {
    let db = services.db.lock();
    Ok(search::query(&db, &args.text, 40)?)
}

/* ── Player ───────────────────────────────────────────────────────────────── */

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayArgs {
    pub kind: String,
    pub id: i64,
    pub position_secs: Option<f64>,
}

#[tauri::command]
pub fn player_play(services: State<'_, Services>, args: PlayArgs) -> Result<PlayerState> {
    let (url, options) = {
        let db = services.db.lock();
        crate::window::resolve_playback(&db, &args.kind, args.id, args.position_secs)?
    };
    let mut player = services.player.lock();
    player.load(&url, &options)?;
    Ok(player.state())
}

#[tauri::command]
pub fn player_pause(services: State<'_, Services>) -> Result<PlayerState> {
    let mut p = services.player.lock();
    p.set_paused(true)?;
    Ok(p.state())
}

#[tauri::command]
pub fn player_resume(services: State<'_, Services>) -> Result<PlayerState> {
    let mut p = services.player.lock();
    p.set_paused(false)?;
    Ok(p.state())
}

#[tauri::command]
pub fn player_stop(services: State<'_, Services>) -> Result<PlayerState> {
    let mut p = services.player.lock();
    p.stop()?;
    Ok(p.state())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeekArgs {
    pub position_secs: f64,
    pub relative: Option<bool>,
}

#[tauri::command]
pub fn player_seek(services: State<'_, Services>, args: SeekArgs) -> Result<PlayerState> {
    let mut p = services.player.lock();
    p.seek(args.position_secs, args.relative.unwrap_or(false))?;
    Ok(p.state())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeArgs {
    pub volume: u32,
}

#[tauri::command]
pub fn player_set_volume(services: State<'_, Services>, args: VolumeArgs) -> Result<PlayerState> {
    let mut p = services.player.lock();
    p.set_volume(args.volume)?;
    Ok(p.state())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutedArgs {
    pub muted: bool,
}

#[tauri::command]
pub fn player_set_muted(services: State<'_, Services>, args: MutedArgs) -> Result<PlayerState> {
    let mut p = services.player.lock();
    p.set_muted(args.muted)?;
    Ok(p.state())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackArgs {
    pub track_id: Option<i64>,
}

#[tauri::command]
pub fn player_set_audio_track(
    services: State<'_, Services>,
    args: TrackArgs,
) -> Result<PlayerState> {
    let mut p = services.player.lock();
    p.set_audio_track(args.track_id)?;
    Ok(p.state())
}

#[tauri::command]
pub fn player_set_subtitle_track(
    services: State<'_, Services>,
    args: TrackArgs,
) -> Result<PlayerState> {
    let mut p = services.player.lock();
    p.set_subtitle_track(args.track_id)?;
    Ok(p.state())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AspectArgs {
    pub aspect: Aspect,
}

#[tauri::command]
pub fn player_set_aspect(services: State<'_, Services>, args: AspectArgs) -> Result<PlayerState> {
    let mut p = services.player.lock();
    p.set_aspect(args.aspect)?;
    Ok(p.state())
}

#[tauri::command]
pub fn player_state(services: State<'_, Services>) -> Result<PlayerState> {
    Ok(services.player.lock().state())
}

/* ── Progress ─────────────────────────────────────────────────────────────── */

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProgressArgs {
    pub profile_id: i64,
    pub kind: String,
    pub id: i64,
    pub position_secs: i64,
    pub duration_secs: i64,
}

#[tauri::command]
pub fn progress_save(services: State<'_, Services>, args: SaveProgressArgs) -> Result<bool> {
    let kind = match args.kind.as_str() {
        "episode" => progress::ItemKind::Episode,
        "channel" => progress::ItemKind::Channel,
        _ => progress::ItemKind::Movie,
    };
    let db = services.db.lock();
    Ok(progress::save(
        &db,
        args.profile_id,
        kind,
        args.id,
        args.position_secs,
        args.duration_secs,
        now_unix(),
    )?)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
