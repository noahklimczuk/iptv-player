//! The typed IPC surface. Mirrors `shared/ipc.ts`; the names here are what the UI's
//! `invoke()` maps onto (dots become underscores).

use aurora_core::markers::{MarkerKind, MarkerSource, SkipMarker};
use aurora_db::repo::{channels, epg, filtering, library, markers, progress, search};
use aurora_player::{Aspect, PlayerState};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::Result;
use crate::now_unix;
use crate::services::Services;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListChannelsArgs {
    pub group: Option<String>,
    pub favorites_only: Option<bool>,
    /// Whose favourites. Optional so a caller that does not care — the zapper's own
    /// channel list, say — does not have to pass one.
    pub profile_id: Option<i64>,
}

#[tauri::command(async)]
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
            profile_id: args.profile_id,
            // `favorites_only` was accepted and then ignored, so the Favorites button
            // on Live TV re-fetched the same list and changed nothing on screen. The
            // mock transport did filter, which is why every browser journey looked
            // right.
            favorites_only: args.favorites_only.unwrap_or(false),
        },
    )?)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupRow {
    pub name: String,
    pub count: i64,
}

#[tauri::command(async)]
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

#[tauri::command(async)]
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

#[tauri::command(async)]
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NowNextManyArgs {
    pub channel_ids: Vec<i64>,
}

/// Now and next for a screenful of channels, in one call.
///
/// Live TV used to mount a `useCommand('epg.nowNext')` per row. With no virtualiser
/// that was one IPC round trip per channel in the library — 22,121 of them on the
/// subscription in docs/ROADMAP.md, each taking the single writer mutex, each on the
/// window's own thread. The list is virtualised now, so this is asked about the rows
/// actually on screen; the batching is what stops a fast scroll turning into a
/// thousand round trips anyway.
#[tauri::command(async)]
pub fn epg_now_next_many(
    services: State<'_, Services>,
    args: NowNextManyArgs,
) -> Result<std::collections::HashMap<i64, NowNext>> {
    let now = now_unix();
    let db = services.db.lock();
    let mut out = std::collections::HashMap::with_capacity(args.channel_ids.len());
    for id in args.channel_ids {
        let Some(epg_id) = channels::get(&db, id)?.and_then(|c| c.epg_channel_id) else {
            continue;
        };
        let (current, next) = epg::now_next(&db, &epg_id, now)?;
        // A channel with a guide id but nothing in the guide is the common case on a
        // real panel: 9,476 channels carry an id and 2,949 have any programmes. Left
        // out rather than sent as a pair of nulls.
        if current.is_some() || next.is_some() {
            out.insert(id, NowNext { now: current, next });
        }
    }
    Ok(out)
}

#[tauri::command(async)]
pub fn epg_now_next(services: State<'_, Services>, args: NowNextArgs) -> Result<NowNext> {
    let db = services.db.lock();
    let Some(ch) = channels::get(&db, args.channel_id)? else {
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
    /// The provider's own shelf. On a library with no TMDB key this is the only
    /// structure there is, so browsing without it means browsing 117,508 rows in one
    /// undifferentiated list.
    #[serde(default)]
    pub category: Option<String>,
    /// Narrow by title, within whatever else is selected.
    #[serde(default)]
    pub query: Option<String>,
}

/// Everything a browse page can narrow by, before it becomes a query.
#[derive(Debug, Default)]
struct Browse {
    sort: String,
    genre: Option<String>,
    category: Option<String>,
    query: Option<String>,
    limit: u32,
    offset: u32,
}

fn browse_query(db: &aurora_db::rusqlite::Connection, b: Browse) -> Result<library::BrowseQuery> {
    Ok(library::BrowseQuery {
        sort: match b.sort.as_str() {
            "title" => library::MovieSort::Title,
            "year" => library::MovieSort::Year,
            "rating" => library::MovieSort::Rating,
            _ => library::MovieSort::RecentlyAdded,
        },
        genre: b.genre,
        category: b.category,
        // An empty box is not a filter. Without this, clearing the search field would
        // match every title containing "", which is all of them — the same answer,
        // reached the slow way.
        query: b.query.filter(|q| !q.trim().is_empty()),
        limit: b.limit.clamp(1, 500),
        offset: b.offset,
        library: filtering::LibraryFilter::load(db)?,
    })
}

#[tauri::command(async)]
pub fn library_movies(
    services: State<'_, Services>,
    args: MoviesArgs,
) -> Result<Vec<library::MovieRow>> {
    let db = services.db.lock();
    let q = browse_query(
        &db,
        Browse {
            sort: args.sort,
            genre: args.genre,
            category: args.category,
            query: args.query,
            limit: args.limit,
            offset: args.offset,
        },
    )?;
    Ok(library::list_movies(&db, &q)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesArgs {
    pub limit: u32,
    pub offset: u32,
    pub genre: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    /// How the list is ordered. Series used to be locked to A–Z while films had four
    /// sorts, for no reason anybody could name.
    #[serde(default)]
    pub sort: Option<String>,
}

#[tauri::command(async)]
pub fn library_series(
    services: State<'_, Services>,
    args: SeriesArgs,
) -> Result<Vec<library::SeriesRow>> {
    let db = services.db.lock();
    let q = browse_query(
        &db,
        Browse {
            sort: args.sort.unwrap_or_else(|| "title".into()),
            genre: args.genre,
            category: args.category,
            query: args.query,
            limit: args.limit,
            offset: args.offset,
        },
    )?;
    Ok(library::list_series(&db, &q)?)
}

/// What a browse page is showing, before it has fetched any of it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseFacetsArgs {
    /// `"movies"` or `"series"`.
    pub kind: String,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
}

/// The shelves, the genres, and how many rows the current filters actually match.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseFacets {
    /// The provider's own categories, biggest first.
    pub categories: Vec<library::Category>,
    /// Genres, where TMDB enrichment has produced any. Usually empty.
    pub genres: Vec<String>,
    /// The real total for these filters, so the heading is a number rather than
    /// "120+" — which was the page size wearing a library's clothes.
    pub total: u32,
}

#[tauri::command(async)]
pub fn library_browse_facets(
    services: State<'_, Services>,
    args: BrowseFacetsArgs,
) -> Result<BrowseFacets> {
    let series = args.kind == "series";
    let kind = if series {
        filtering::Kind::Series
    } else {
        filtering::Kind::Movies
    };
    let db = services.db.lock();
    let q = browse_query(
        &db,
        Browse {
            sort: String::new(),
            genre: args.genre,
            category: args.category,
            query: args.query,
            limit: 1,
            offset: 0,
        },
    )?;
    Ok(BrowseFacets {
        categories: library::categories(&db, kind)?,
        genres: library::genres(&db)?,
        total: if series {
            library::count_series(&db, &q)?
        } else {
            library::count_movies(&db, &q)?
        },
    })
}

#[tauri::command(async)]
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

#[tauri::command(async)]
pub fn library_episodes(
    services: State<'_, Services>,
    args: EpisodesArgs,
) -> Result<Vec<library::EpisodeRow>> {
    {
        let db = services.db.lock();
        // Whether to go and ask the panel is a question about the *show*, not about
        // the season being displayed. Asking per season looks the same for a show with
        // nothing stored and a show whose season 2 is simply empty — and for a show
        // whose episodes are all in season 2, the fetch would land them and this would
        // then filter them all away, every time, for good.
        if !library::episodes_for(&db, args.series_id, None)?.is_empty() {
            return Ok(library::episodes_for(&db, args.series_id, args.season)?);
        }
    }

    // Nothing stored. An import writes the series row but not its episodes — one
    // request per show would mean 28,693 of them before the library was usable — so
    // the listing is fetched the first time somebody opens the show. That is what the
    // refresh's own warning has always claimed happens; until now nothing did it, and
    // every series on every panel showed "0 seasons" forever.
    //
    // The lock is released above, deliberately: this goes to the network, and holding
    // it across a request is what used to freeze the DVR scheduler during a refresh.
    if let Err(e) = crate::series::fetch_episodes(&services, args.series_id) {
        tracing::warn!(series = args.series_id, "could not fetch episodes: {e}");
        return Err(e);
    }

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
#[tauri::command(async)]
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

#[tauri::command(async)]
pub fn player_play(services: State<'_, Services>, args: PlayArgs) -> Result<PlayerState> {
    // Live goes through the failover path: a channel has several URLs and the first is
    // only a guess. Everything else has exactly one.
    if args.kind == "live" {
        return logged("play", services.playback.play_live(args.id, now_unix()));
    }
    logged(
        "play",
        services
            .playback
            .play_item(&args.kind, args.id, args.position_secs),
    )
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
#[tauri::command(async)]
pub fn player_play_catchup(
    services: State<'_, Services>,
    args: CatchupArgs,
) -> Result<PlayerState> {
    services
        .playback
        .play_catchup(args.channel_id, args.start, args.stop, now_unix())
}

/// Record a control the viewer pressed, and what came back.
///
/// "None of the player controls work" cannot be answered from a log that never
/// mentions the attempt. Three things produce that same sentence and they have
/// opposite fixes: the click never reached the host at all, the host ran the command
/// and the backend refused it, or the command succeeded and the OSD simply never
/// redrew. From the sofa they are indistinguishable; in the log they are three
/// different lines, or the absence of one.
///
/// The *name* and the outcome, never the arguments. `player.play` carries a resolved
/// stream URL and those carry credentials on an Xtream line (README C10), so nothing
/// here formats an argument.
fn logged(control: &'static str, outcome: Result<PlayerState>) -> Result<PlayerState> {
    match &outcome {
        Ok(state) => tracing::info!(
            control,
            status = ?state.status,
            position = state.position_secs,
            "player control"
        ),
        Err(e) => tracing::warn!(control, "player control refused: {e}"),
    }
    outcome
}

#[tauri::command(async)]
pub fn player_pause(services: State<'_, Services>) -> Result<PlayerState> {
    logged("pause", {
        let mut p = services.player.lock();
        p.set_paused(true)?;
        Ok(p.state())
    })
}

#[tauri::command(async)]
pub fn player_resume(services: State<'_, Services>) -> Result<PlayerState> {
    logged("resume", {
        let mut p = services.player.lock();
        p.set_paused(false)?;
        Ok(p.state())
    })
}

#[tauri::command(async)]
pub fn player_stop(services: State<'_, Services>) -> Result<PlayerState> {
    // Through the service, so stopping also ends the failover session — otherwise the
    // next dead-stream tick would reconnect a channel the viewer had closed.
    logged("stop", services.playback.stop())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeekArgs {
    pub position_secs: f64,
    pub relative: Option<bool>,
}

/// Through the service, not the backend: a seek on a buffered live stream has to be
/// held inside what the buffer holds, and that bound belongs in one place (README §7.6).
#[tauri::command(async)]
pub fn player_seek(services: State<'_, Services>, args: SeekArgs) -> Result<PlayerState> {
    logged(
        "seek",
        services
            .playback
            .seek(args.position_secs, args.relative.unwrap_or(false)),
    )
}

/// Jump back to the live edge after pausing or rewinding live TV (README §7.6).
#[tauri::command(async)]
pub fn player_back_to_live(services: State<'_, Services>) -> Result<PlayerState> {
    logged("backToLive", services.playback.back_to_live())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeArgs {
    pub volume: u32,
}

#[tauri::command(async)]
pub fn player_set_volume(services: State<'_, Services>, args: VolumeArgs) -> Result<PlayerState> {
    logged("setVolume", {
        let mut p = services.player.lock();
        p.set_volume(args.volume)?;
        Ok(p.state())
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutedArgs {
    pub muted: bool,
}

#[tauri::command(async)]
pub fn player_set_muted(services: State<'_, Services>, args: MutedArgs) -> Result<PlayerState> {
    logged("setMuted", {
        let mut p = services.player.lock();
        p.set_muted(args.muted)?;
        Ok(p.state())
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackArgs {
    pub track_id: Option<i64>,
}

#[tauri::command(async)]
pub fn player_set_audio_track(
    services: State<'_, Services>,
    args: TrackArgs,
) -> Result<PlayerState> {
    let mut p = services.player.lock();
    p.set_audio_track(args.track_id)?;
    Ok(p.state())
}

#[tauri::command(async)]
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

#[tauri::command(async)]
pub fn player_set_aspect(services: State<'_, Services>, args: AspectArgs) -> Result<PlayerState> {
    let mut p = services.player.lock();
    p.set_aspect(args.aspect)?;
    Ok(p.state())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedArgs {
    pub speed: f64,
}

/// Playback speed. The backend clamps to 0.25x–4x; past that mpv drops audio entirely
/// and the result is indistinguishable from a broken stream.
#[tauri::command(async)]
pub fn player_set_speed(services: State<'_, Services>, args: SpeedArgs) -> Result<PlayerState> {
    let mut p = services.player.lock();
    p.set_speed(args.speed)?;
    Ok(p.state())
}

#[tauri::command(async)]
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

#[tauri::command(async)]
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

#[tauri::command(async)]
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
#[tauri::command(async)]
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
#[tauri::command(async)]
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

#[tauri::command(async)]
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

#[tauri::command(async)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_player::{NullBackend, PlayerBackend};

    /// Logging must be observation only.
    ///
    /// The whole point of `logged` is that it changes nothing about what the viewer
    /// gets — a wrapper that swallowed an error, or returned a stale state, would turn
    /// a diagnostic aid into the very class of bug it was added to find.
    #[test]
    fn logging_a_control_does_not_change_what_it_answered() {
        let state = NullBackend::default().state();

        let ok = logged("pause", Ok(state.clone()));
        assert_eq!(ok.unwrap(), state, "the state came back altered");

        let err = logged(
            "pause",
            Err(crate::error::AppError::Other("mpv said no".into())),
        );
        assert!(err.is_err(), "a refusal was swallowed");
        assert_eq!(err.unwrap_err().to_string(), "mpv said no");
    }
}
