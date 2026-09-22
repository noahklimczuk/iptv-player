//! The typed IPC surface. Mirrors `shared/ipc.ts`; the names here are what the UI's
//! `invoke()` maps onto (dots become underscores).

use aurora_core::markers::{MarkerKind, MarkerSource, SkipMarker};
use aurora_db::repo::{channels, epg, filtering, library, markers, progress, search};
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
            library: filtering::LibraryFilter::load(&db)?,
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
    let all = channels::list(
        &db,
        &channels::ChannelFilter {
            library: filtering::LibraryFilter::load(&db)?,
            ..Default::default()
        },
    )?;
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

fn browse_query(
    db: &aurora_db::rusqlite::Connection,
    sort: &str,
    genre: Option<String>,
    limit: u32,
    offset: u32,
) -> Result<library::BrowseQuery> {
    Ok(library::BrowseQuery {
        sort: match sort {
            "title" => library::MovieSort::Title,
            "year" => library::MovieSort::Year,
            "rating" => library::MovieSort::Rating,
            _ => library::MovieSort::RecentlyAdded,
        },
        genre,
        limit: limit.clamp(1, 500),
        offset,
        library: filtering::LibraryFilter::load(db)?,
    })
}

#[tauri::command]
pub fn library_movies(
    services: State<'_, Services>,
    args: MoviesArgs,
) -> Result<Vec<library::MovieRow>> {
    let db = services.db.lock();
    let q = browse_query(&db, &args.sort, args.genre, args.limit, args.offset)?;
    Ok(library::list_movies(&db, &q)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesArgs {
    pub limit: u32,
    pub offset: u32,
    pub genre: Option<String>,
}

#[tauri::command]
pub fn library_series(
    services: State<'_, Services>,
    args: SeriesArgs,
) -> Result<Vec<library::SeriesRow>> {
    let db = services.db.lock();
    let q = browse_query(&db, "title", args.genre, args.limit, args.offset)?;
    Ok(library::list_series(&db, &q)?)
}

#[tauri::command]
pub fn library_genres(services: State<'_, Services>) -> Result<Vec<String>> {
    let db = services.db.lock();
    Ok(library::genres(&db)?)
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

/// Grouped so the palette can render its sections without re-sorting (README §10).
///
/// The flat `search::query` is the FTS half of this; the palette wants programmes and
/// people beside it, and those are not in the index.
#[tauri::command]
pub fn search_query(
    services: State<'_, Services>,
    args: SearchArgs,
) -> Result<search::SearchResults> {
    let db = services.db.lock();
    Ok(search::grouped(&db, &args.text, now_unix(), 40)?)
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatchupArgs {
    pub channel_id: i64,
    /// The programme's airtime, unix seconds.
    pub start: i64,
    pub stop: i64,
}

/// Play a past programme from its start (README §7.5).
///
/// Separate from `player_play` rather than another `kind`: catch-up is addressed by a
/// channel *and* a time window, which does not fit an item id.
#[tauri::command]
pub fn player_play_catchup(
    services: State<'_, Services>,
    args: CatchupArgs,
) -> Result<PlayerState> {
    let (url, options) = {
        let db = services.db.lock();
        crate::window::resolve_catchup(&db, args.channel_id, args.start, args.stop, now_unix())?
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

/* ── Skip markers & next episode (README §9) ──────────────────────────────── */

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeIdArgs {
    pub episode_id: i64,
}

/// The markers the Skip button should offer, plus when the Up Next card is due.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodePlaybackAids {
    pub markers: Vec<SkipMarker>,
    pub up_next_at_secs: Option<f64>,
    pub next_episode: Option<library::EpisodeRow>,
    pub prefs: markers::SeriesPrefs,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AidsArgs {
    pub profile_id: i64,
    pub episode_id: i64,
    /// Runtime of the loaded file, which the UI knows from player state.
    pub duration_secs: f64,
}

#[tauri::command]
pub fn library_playback_aids(
    services: State<'_, Services>,
    args: AidsArgs,
) -> Result<EpisodePlaybackAids> {
    let db = services.db.lock();

    let resolved = markers::resolve(&db, args.episode_id, markers::DEFAULT_MIN_SAMPLES)?;
    let prefs = match markers::series_id_of(&db, args.episode_id)? {
        Some(series_id) => markers::prefs(&db, args.profile_id, series_id)?,
        None => markers::SeriesPrefs::default(),
    };

    Ok(EpisodePlaybackAids {
        up_next_at_secs: aurora_core::markers::up_next_at(
            &resolved,
            args.duration_secs,
            UP_NEXT_TAIL_SECS,
        ),
        markers: resolved,
        next_episode: library::following_episode(&db, args.episode_id)?,
        prefs,
    })
}

/// How long before the end the Up Next card appears when there is no credits marker.
const UP_NEXT_TAIL_SECS: f64 = 45.0;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordSkipArgs {
    pub episode_id: i64,
    pub kind: String,
    pub start_secs: f64,
    pub end_secs: f64,
}

/// Remember that the viewer skipped a region, so later episodes of the show can offer
/// the button without being asked.
#[tauri::command]
pub fn library_record_skip(services: State<'_, Services>, args: RecordSkipArgs) -> Result<()> {
    let Some(kind) = MarkerKind::parse(&args.kind) else {
        return Err(crate::AppError::Other(format!(
            "unknown marker kind {:?}",
            args.kind
        )));
    };
    let marker = SkipMarker::new(kind, args.start_secs, args.end_secs, MarkerSource::User);
    if !marker.is_plausible() {
        // An accidental one-second drag must not teach the series anything.
        return Ok(());
    }
    let db = services.db.lock();
    markers::record(&db, args.episode_id, &marker, now_unix())?;
    Ok(())
}

/// Store the chapter-derived markers for the file the player just loaded.
#[tauri::command]
pub fn library_sync_chapters(services: State<'_, Services>, args: AidsArgs) -> Result<usize> {
    let chapters = services.player.lock().chapters();
    let derived = aurora_core::markers::from_chapters(&chapters, args.duration_secs);
    let mut db = services.db.lock();
    Ok(markers::set_chapter_markers(
        &mut db,
        args.episode_id,
        &derived,
        now_unix(),
    )?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPrefsArgs {
    pub profile_id: i64,
    pub series_id: i64,
}

#[tauri::command]
pub fn library_series_prefs(
    services: State<'_, Services>,
    args: SeriesPrefsArgs,
) -> Result<markers::SeriesPrefs> {
    let db = services.db.lock();
    Ok(markers::prefs(&db, args.profile_id, args.series_id)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSeriesPrefsArgs {
    pub profile_id: i64,
    pub series_id: i64,
    pub prefs: markers::SeriesPrefs,
}

#[tauri::command]
pub fn library_set_series_prefs(
    services: State<'_, Services>,
    args: SetSeriesPrefsArgs,
) -> Result<()> {
    let db = services.db.lock();
    markers::set_prefs(
        &db,
        args.profile_id,
        args.series_id,
        &args.prefs,
        now_unix(),
    )?;
    Ok(())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
