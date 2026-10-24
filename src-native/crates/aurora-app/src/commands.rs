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
    // The same filter the list behind each group is read through. Counting one way and
    // listing another is how a sidebar comes to disagree with its own screen.
    let filter = filtering::LibraryFilter::load(&db)?;
    Ok(channels::groups(&db, &filter)?
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
    // `get`, not `list(default)`. This is the call site `repo::channels::get` was
    // written for and the one that was missed: `ChannelFilter::default()` has
    // `radio_only: false`, which `list` turns into `AND is_radio = 0`, so a radio
    // station was found by its number and then thrown away -- a keypad entry for one
    // tuned nothing at all. A hidden channel went the same way. And it answered for one
    // channel by sorting and materialising every row, which on this subscription is
    // 6,698 of them, on every keypad entry (R-78).
    Ok(channels::get(&db, id)?)
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
        // A set, not `Vec::contains`. That was a linear scan of the requested ids for
        // every channel in the library, so asking about the whole list cost
        // `channels × ids` comparisons — 6,698 × 6,698 is 44.9 million on this line,
        // and 22,121 squared is 489 million on the one docs/ROADMAP.md describes. Same
        // shape as the prefilter R-131 was about.
        let asked: std::collections::HashSet<i64> = args.channel_ids.iter().copied().collect();
        all.into_iter().filter(|c| asked.contains(&c.id)).collect()
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

/// The most channels one `epg.nowNextMany` will answer about.
///
/// Generous against what the screen asks for — a virtualised list shows tens of rows,
/// and a fast scroll coalesces into a few hundred at the very most — and small against
/// what the library holds, which is what makes it a guard rather than a limit anyone
/// meets.
pub const MAX_NOW_NEXT_BATCH: usize = 512;

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
    // A screenful, bounded. Each id costs two queries — the channel, then its
    // programmes — all of them under the single writer mutex, so a caller asking about
    // the whole library would hold it for 44,000 round trips and freeze everything else
    // that needs the database. The virtualiser asks about rows on screen, which is tens;
    // this is the guard for when something else asks, since a command is reachable by
    // more than its caller.
    let ids: Vec<i64> = args
        .channel_ids
        .into_iter()
        .take(MAX_NOW_NEXT_BATCH)
        .collect();
    let db = services.db.lock();
    let mut out = std::collections::HashMap::with_capacity(ids.len());
    for id in ids {
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
    /// One letter, or `#` for everything that does not start with one.
    #[serde(default)]
    pub letter: Option<String>,
}

/// Everything a browse page can narrow by, before it becomes a query.
#[derive(Debug, Default)]
struct Browse {
    sort: String,
    genre: Option<String>,
    category: Option<String>,
    query: Option<String>,
    letter: Option<String>,
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
        // Exactly one character, upper-cased, or `#`. Normalised here rather than
        // trusted, because this reaches a SQL comparison: a UI that sent "Th" would
        // otherwise match nothing and look broken, and one that sent a long string would
        // be comparing a title's first letter against a sentence.
        letter: b.letter.and_then(|l| {
            let mut chars = l.trim().chars();
            let first = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            if first == '#' {
                return Some("#".to_string());
            }
            first
                .is_alphabetic()
                .then(|| first.to_uppercase().to_string())
        }),
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
            letter: args.letter,
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
    #[serde(default)]
    pub letter: Option<String>,
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
            letter: args.letter,
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
    #[serde(default)]
    pub letter: Option<String>,
}

/// The shelves, the genres, and how many rows the current filters actually match.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseFacets {
    /// The provider's own categories, biggest first.
    pub categories: Vec<library::Category>,
    /// Genres for *this* kind, commonest first — films and shows do not share a
    /// list, and offering one the other's is a filter that matches nothing.
    ///
    /// Counted, like the categories beside them: on a real panel this is 326 genres
    /// of which 201 are on two shows or fewer, and a count is how a viewer tells the
    /// one worth picking from the long tail.
    pub genres: Vec<library::Category>,
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
            // The facets' count has to be the count of what is on screen: without the
            // letter the heading said 117,508 while the grid showed the 4,312 films
            // beginning with S.
            letter: args.letter,
            limit: 1,
            offset: 0,
        },
    )?;
    Ok(BrowseFacets {
        // `q.library` rather than a second load: the count below is computed through that
        // one, and two reads of the settings table could in principle straddle a change.
        categories: library::categories(&db, kind, &q.library)?,
        // Scoped by kind as the categories are: Movies offering shows' genres is a
        // filter that matches nothing (docs/DECISIONS.md D26).
        genres: library::genres(&db, kind, &q.library)?,
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
    let filter = filtering::LibraryFilter::load(&db)?;
    // Both halves: this one is "every genre in the library". A browse screen wants
    // only its own kind's and asks `library_browse_facets`, which is where offering
    // the other half's became a filter that matched nothing.
    let mut all: Vec<String> = library::genres(&db, filtering::Kind::Movies, &filter)?
        .into_iter()
        .chain(library::genres(&db, filtering::Kind::Series, &filter)?)
        .map(|g| g.name)
        .collect();
    all.sort();
    all.dedup();
    Ok(all)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodesArgs {
    pub series_id: i64,
    pub season: Option<u16>,
}

/// A show's episodes — every copy of that show's, merged.
///
/// "Hide duplicates" keeps the best copy of a title and hides the rest, and for a film
/// that costs nothing but a choice of quality. For a show it was costing *content*: the
/// copies are separate listings on the panel and need not carry the same seasons, so a
/// season present only in the hidden copy could not be reached at all. Merging here
/// rather than in the collapser keeps one row in the grid, which is what the viewer asked
/// for, while the show itself holds everything any copy of it has.
///
/// Where the merge goes is decided by what the screen reads: `DetailModal`'s season tabs
/// come from the episodes this returns, not from `SeriesRow.seasons`, so a season added
/// here becomes reachable rather than merely counted.
#[tauri::command(async)]
pub fn library_episodes(
    services: State<'_, Services>,
    args: EpisodesArgs,
) -> Result<Vec<library::EpisodeRow>> {
    // Which rows this show is standing for, and which of them have nothing stored yet.
    // Both asked under one lock, and the lock dropped before the network below.
    let (group, missing) = {
        let db = services.db.lock();
        let filter = filtering::LibraryFilter::load(&db)?;
        let group = filter.group_of(&db, filtering::Kind::Series, args.series_id)?;
        let mut missing = Vec::new();
        for id in &group {
            // Whether to go and ask the panel is a question about the *show*, not about
            // the season being displayed. Asking per season looks the same for a show
            // with nothing stored and a show whose season 2 is simply empty — and for a
            // show whose episodes are all in season 2, the fetch would land them and this
            // would then filter them all away, every time, for good.
            if library::episodes_for(&db, *id, None)?.is_empty() {
                missing.push(*id);
            }
        }
        (group, missing)
    };

    // An import writes the series row but not its episodes — one request per show would
    // mean 28,693 of them before the library was usable — so the listing is fetched the
    // first time somebody opens the show. That is what the refresh's own warning has
    // always claimed happens; until now nothing did it, and every series on every panel
    // showed "0 seasons" forever.
    //
    // The lock is released above, deliberately: this goes to the network, and holding it
    // across a request is what used to freeze the DVR scheduler during a refresh.
    for id in &missing {
        if let Err(e) = crate::series::fetch_episodes(&services, *id) {
            // Fatal only for the show that was actually asked for. A sibling copy that
            // will not list is a reason to show less, not a reason to show nothing:
            // before this merge existed the viewer got the asked-for copy's episodes and
            // that must remain the floor.
            if *id == args.series_id {
                tracing::warn!(series = id, "could not fetch episodes: {e}");
                return Err(e);
            }
            tracing::warn!(
                series = id,
                "a duplicate copy of this show would not list its episodes ({e});                  merging what the others have"
            );
        }
    }

    let db = services.db.lock();
    if group.len() == 1 {
        return Ok(library::episodes_for(&db, args.series_id, args.season)?);
    }
    let mut merged: Vec<library::EpisodeRow> = Vec::new();
    for id in &group {
        for episode in library::episodes_for(&db, *id, args.season)? {
            // Keyed on which episode it is, not on its id: every copy has its own row
            // for season 1 episode 1, and the group is in best-copy-first order, so the
            // first one seen is the one the viewer should get. Without this, a show with
            // two complete copies would list all 62 of its episodes twice.
            if !merged
                .iter()
                .any(|kept| kept.season == episode.season && kept.episode == episode.episode)
            {
                merged.push(episode);
            }
        }
    }
    merged.sort_by_key(|e| (e.season, e.episode));
    Ok(merged)
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
        // Named, rather than falling through. `ItemKind::Recording` has existed since
        // the schema did, and the catch-all below filed a recording's position as a
        // *movie's* -- against the same rowid, so it collided with whatever film held
        // it. Nothing sent this kind until recordings became playable (R-118).
        "recording" => progress::ItemKind::Recording,
        "movie" => progress::ItemKind::Movie,
        // Refused rather than filed as a movie.
        //
        // `watch_progress`'s primary key is `(profile_id, item_kind, item_id)`, so an
        // unrecognised kind written as `Movie` lands on whatever film holds that rowid
        // and silently overwrites its position. The UI only ever sends the four above --
        // `useWatchProgress` guards on them -- so this is the guard for the first caller
        // that does not, and saying no is the only answer that cannot corrupt a row
        // belonging to something else (R-81).
        other => {
            return Err(crate::error::AppError::Other(format!(
                "Aurora does not know how to remember a position for a {other:?}."
            )))
        }
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

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FullscreenArgs {
    /// Leave it out to toggle.
    pub fullscreen: Option<bool>,
}

/// Put the window in or out of fullscreen. Returns the state it ended up in.
///
/// **This has to be the window, not the document.** `requestFullscreen()` makes the
/// *WebView* fullscreen and leaves the window the size it was — and mpv does not draw
/// into the WebView. It draws into a child HWND behind it (README §2.1), sized from the
/// window's own resize events. So the DOM call produced a full-screen sheet of UI with
/// the video still letterboxed into the old window behind it, which is a stranger result
/// than nothing happening.
///
/// Going through the window instead means the existing `WindowEvent::Resized` handler
/// resizes the video surface, which is the same path an ordinary drag already takes.
#[tauri::command(async)]
pub fn window_fullscreen(window: tauri::Window, args: Option<FullscreenArgs>) -> Result<bool> {
    let wanted = match args.unwrap_or_default().fullscreen {
        Some(want) => want,
        None => !window
            .is_fullscreen()
            .map_err(|e| crate::AppError::Other(e.to_string()))?,
    };
    window
        .set_fullscreen(wanted)
        .map_err(|e| crate::AppError::Other(e.to_string()))?;
    Ok(wanted)
}

/// Show the window, now that there is something on it worth looking at.
///
/// The window is created hidden (`"visible": false`). It has to be, because it is also
/// `"transparent": true` — the mpv surface composites behind the WebView2 and that is
/// what makes it possible — and a transparent window with nothing painted in it yet is a
/// hole straight through to the desktop. Aurora launched as a floating title bar over
/// whatever happened to be behind it, for as long as WebView2 took to start, which is
/// the slowest part of the launch and not a short time on a cold disk.
///
/// `index.html` paints a boot screen before any script runs, so by the time the UI calls
/// this there is already something to reveal. The reveal is deliberately not waited on
/// by anything: `main` shows the window regardless after a few seconds, because a UI
/// that fails to boot must still produce a window somebody can close.
///
/// Idempotent. The fallback and this may both run, and in either order.
#[tauri::command(async)]
pub fn window_ready(window: tauri::Window) -> Result<()> {
    let already = window.is_visible().unwrap_or(false);
    window
        .show()
        .map_err(|e| crate::AppError::Other(e.to_string()))?;
    if !already {
        // Only on the transition. Stealing focus from whatever the person moved on to
        // while waiting would be its own rudeness, but the first appearance of a window
        // somebody launched should be the thing they are looking at.
        let _ = window.set_focus();
        tracing::info!("window revealed by the UI");
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemArgs {
    pub kind: String,
    pub id: i64,
}

/// One library row, tagged the way the interface's `CatalogItem` is.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Item {
    Movie(library::MovieRow),
    Series(library::SeriesRow),
}

/// One film or show by id.
///
/// Search needs this and had nothing to use: picking a film in the command palette ran
/// `navigate('/movies')` and threw the id away, so the answer to "open this" was "here is
/// a page of everything". A search that finds the right title and then does not open it is
/// worse than one that finds nothing, because it looks like the title is not there.
///
/// Deliberately ignores the library filter. Search can surface a title that "English only"
/// or duplicate collapsing keeps off the browse pages, and refusing to open what was just
/// found would be the same bug in a different place.
#[tauri::command(async)]
pub fn library_item(services: State<'_, Services>, args: ItemArgs) -> Result<Option<Item>> {
    let db = services.db.lock();
    Ok(match args.kind.as_str() {
        "movie" => library::movie(&db, args.id)?.map(Item::Movie),
        "series" => library::series(&db, args.id)?.map(Item::Series),
        other => {
            return Err(crate::AppError::Other(format!(
                "library.item does not know the kind {other:?}"
            )))
        }
    })
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

    // How long this episode is, which decides whether Up Next and Skip Credits exist at
    // all.
    //
    // The player's own number first, because it is the file in front of us. But a
    // provider's VOD stream very often reports no duration — mpv has nothing to read it
    // from until the whole thing is buffered, and on some streams never — and the
    // consequence was that an episode with no chapters and no learned markers had no Up
    // Next either, so autoplay never fired on an ordinary library. The episode's own
    // runtime, which enrichment or the provider supplied, is the fallback.
    let duration = if args.duration_secs > 0.0 {
        Some(args.duration_secs)
    } else {
        library::episode_runtime_secs(&db, args.episode_id)?
    };

    let resolved = markers::resolve(&db, args.episode_id, markers::DEFAULT_MIN_SAMPLES, duration)?;
    let series = markers::series_id_of(&db, args.episode_id)?;
    let prefs = match series {
        Some(series_id) => markers::prefs(&db, args.profile_id, series_id)?,
        None => markers::SeriesPrefs::default(),
    };

    // Across every copy of this show, for the same reason its episode list is: a season
    // that came from one copy runs out, and the one that continues it belongs to another
    // row. Bounded to the copy alone, Up Next vanished at that boundary and autoplay
    // stopped halfway through a show the app was listing in full.
    let group = match series {
        Some(series_id) => {
            let filter = filtering::LibraryFilter::load(&db)?;
            filter.group_of(&db, filtering::Kind::Series, series_id)?
        }
        None => Vec::new(),
    };

    Ok(EpisodePlaybackAids {
        up_next_at_secs: aurora_core::markers::up_next_at(
            &resolved,
            duration.unwrap_or(0.0),
            UP_NEXT_TAIL_SECS,
        ),
        markers: resolved,
        next_episode: library::following_episode_in(&db, args.episode_id, &group)?,
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
    /// 0 when the stream has not told us, which is ordinary on a provider's VOD.
    #[serde(default)]
    pub duration_secs: f64,
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
    // And a region has to sit where what it claims to be sits. Only the length was
    // checked here, so an intro could be recorded an hour in while a chapter saying the
    // same thing was refused — the same table, reached two ways, with two ideas of what
    // an intro is. `library_observe_seek` has always gone through this.
    if !aurora_core::markers::position_fits_kind(&marker, args.duration_secs) {
        return Ok(());
    }
    let db = services.db.lock();
    markers::record(&db, args.episode_id, &marker, now_unix())?;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObserveSeekArgs {
    pub episode_id: i64,
    pub from_secs: f64,
    pub to_secs: f64,
    /// 0 when the stream has not told us, which is ordinary on a provider's VOD.
    pub duration_secs: f64,
}

/// Learn from a forward seek the viewer made by hand.
///
/// `library_record_skip` above is reached only from the Skip button, and that button is
/// only on screen when a marker already exists — so on a library whose streams carry no
/// chapters there was no way for an intro to come into being, and no way for a viewer to
/// teach one. Dragging past the titles is the manual skip the design's second tier is
/// about, and this is where it gets recorded.
///
/// What it is recorded *as* is the part that was wrong. Every jump was called an intro
/// and stored as `User` — the source a deliberate press has — so a viewer skipping the
/// recap taught the show an intro in the recap's place, and a single impatient nudge
/// outranked everything the rest of the series had to say. `seek_evidence` decides the
/// kind from what this episode already knows, and `MarkerSource::Seek` ranks the claim
/// honestly: a button, never a silent jump, and nothing another episode inherits until a
/// second observation agrees with it.
///
/// Returns whether the jump was taken as evidence, so the caller can refresh its markers
/// rather than guess.
#[tauri::command(async)]
pub fn library_observe_seek(services: State<'_, Services>, args: ObserveSeekArgs) -> Result<bool> {
    let db = services.db.lock();
    // What the episode already has, so a jump out of a known recap is not filed as an
    // intro. Read before the write, under the one lock -- the db mutex is not reentrant.
    let known = markers::resolve(
        &db,
        args.episode_id,
        markers::DEFAULT_MIN_SAMPLES,
        (args.duration_secs > 0.0).then_some(args.duration_secs),
    )?;
    let Some(marker) = aurora_core::markers::seek_evidence(
        args.from_secs,
        args.to_secs,
        args.duration_secs,
        &known,
    ) else {
        return Ok(false);
    };
    markers::record(&db, args.episode_id, &marker, now_unix())?;
    Ok(true)
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
