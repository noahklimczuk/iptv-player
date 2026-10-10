//! Multi-view (README §7.4): several channels at once, each in its own tile.
//!
//! One mpv instance per tile, each drawing into its own child surface positioned by
//! `aurora_core::mosaic::Layout`. README §6.1 asks for exactly this shape — "one
//! long-lived libmpv instance for the main surface, plus short-lived instances for PiP,
//! multi-view tiles" — and the instances here are short-lived in the strongest sense:
//! they exist while the mosaic is open and are dropped when it closes, which is what
//! destroys the surfaces and releases the provider's connections.
//!
//! ## The connection limit is the feature
//!
//! A 3×3 mosaic is nine simultaneous streams. The subscription this project was
//! measured against allows **one**. A provider does not queue the surplus, it refuses
//! it — or cuts a stream already running, including a recording in flight — so opening
//! optimistically and letting the tiles fail would spend somebody's DVR to draw eight
//! error messages. Every open is checked first, against the tightest declared limit
//! minus what the DVR is already using, and refused in words that name the number.
//!
//! Where no provider declares a limit — which is every M3U playlist, so the common case
//! rather than the odd one — the attempt is allowed and the UI says it may not work.
//! Guessing a limit would refuse layouts that are fine; assuming none would be the
//! optimistic open this module exists to avoid.
//!
//! ## Audio
//!
//! Exactly one tile is audible. Nine streams of sound at once is not a feature, and
//! muting is per-instance rather than per-track, so "audio focus" is simply which
//! backend is unmuted. Tile 0 starts focused: it is the large tile in the `1+n`
//! layouts and the top-left one in the grids, so in every layout it is where the eye
//! already is.

use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;

use aurora_core::mosaic::{Budget, Demand, Layout, Rect};
use aurora_db::repo::{channels, mosaic as repo};
use aurora_db::rusqlite::Connection;
use aurora_player::{PlayerBackend, PlayerStatus};
use parking_lot::Mutex;
use serde::Serialize;

use crate::error::{AppError, Result};

/// How a tile's backend is made.
///
/// Injected rather than calling `create_backend` directly so the service can be driven
/// by `NullBackend`s in a test: everything here except the decoding — the budget, the
/// geometry, which tile has audio, what happens to a channel with no sources — is
/// logic that would otherwise only ever run on Windows with a subscription.
pub type TileFactory = Arc<dyn Fn() -> Box<dyn PlayerBackend> + Send + Sync>;

/// One tile, as the UI draws it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TileView {
    pub index: usize,
    pub rect: Rect,
    pub channel_id: Option<i64>,
    pub name: Option<String>,
    /// Whether this is the tile with audio.
    pub focused: bool,
    pub status: PlayerStatus,
    /// Why this tile has no picture, in words a viewer can act on. A tile failing is
    /// ordinary — it is one stream of several — so it is reported per tile rather than
    /// failing the whole mosaic.
    pub error: Option<String>,
}

/// The mosaic as a whole, for one `mosaic.state` reply.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MosaicView {
    pub open: bool,
    pub layout: Option<Layout>,
    pub tiles: Vec<TileView>,
    pub focused: usize,
}

impl MosaicView {
    fn closed() -> Self {
        Self {
            open: false,
            layout: None,
            tiles: Vec::new(),
            focused: 0,
        }
    }
}

/// What the picker needs to know before offering a layout.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetView {
    pub layout: Layout,
    pub tiles: usize,
    #[serde(flatten)]
    pub budget: Budget,
    /// The largest layout that would fit right now, or `null` when none does.
    pub largest_fitting: Option<Layout>,
    /// Recordings in flight, so the UI can say *why* the budget is tighter than the
    /// provider's own number.
    pub recordings: usize,
}

struct Tile {
    channel_id: Option<i64>,
    name: Option<String>,
    player: Option<Box<dyn PlayerBackend>>,
    error: Option<String>,
}

struct Session {
    layout: Layout,
    tiles: Vec<Tile>,
    focused: usize,
}

pub struct Mosaic {
    db: Arc<Mutex<Connection>>,
    dvr: Arc<crate::dvr::Dvr>,
    factory: TileFactory,
    session: Mutex<Option<Session>>,
    /// The window tiles are children of, recorded when the main video surface attached.
    ///
    /// Zero until then, and zero for ever off Windows, where there is no surface to be
    /// a child of. A tile whose backend renders nothing does not need one, which is why
    /// this is not an error — `NullBackend::attach` is never called at all.
    parent: AtomicIsize,
    /// The client area to divide. Updated on resize.
    size: Mutex<(u32, u32)>,
    /// Unused today, kept because a tile that gains timeshift will need it where the
    /// main player does.
    #[allow(dead_code)]
    data_dir: PathBuf,
    /// The main player, so its surface can be got out of the way.
    ///
    /// Not for playing anything — the tiles have their own backends. It is here because
    /// opening and closing a mosaic is exactly when the main surface has to be hidden and
    /// shown again, and holding it means those two cannot drift apart. Done from the
    /// commands instead, `open`, `open_saved`, `close` and `promote` would each have to
    /// remember, and the one that forgot would be a mosaic nobody can see.
    main: Arc<Mutex<Box<dyn PlayerBackend>>>,
}

impl Mosaic {
    /// Put the main player's surface underneath the tiles.
    ///
    /// Every `place` sends the surface it moves to the very bottom of the z-order --
    /// right for the only surface there is, since it has to stay under the WebView, and
    /// wrong once there are tiles, because they go to the bottom too and the main
    /// player's full-window surface is already down there covering them. That is a
    /// mosaic you can hear and cannot see.
    ///
    /// So the tiles are placed first and the main surface is sent to the bottom *after*
    /// them, which leaves the one order that works: WebView, tiles, main surface.
    ///
    /// Done this way rather than by naming a sibling to sit above, because Win32 cannot
    /// say that: `SetWindowPos`'s `hWndInsertAfter` names the window the moved one goes
    /// *behind*, so asking for "above the main surface" with the only handle to hand puts
    /// the tile behind it again. Measured, not assumed -- the first attempt did exactly
    /// that and the tile stayed invisible.
    ///
    /// And not by hiding the main surface, which was the attempt before that: hiding
    /// could not be undone, because `SetWindowPos` on a window whose sibling belongs to a
    /// thread with no message loop does not return (AUDIT/findings.md F-36), so the
    /// window stayed black after closing. Nothing needs restoring if nothing was hidden.
    fn sink_main_surface(&self) {
        let (w, h) = *self.size.lock();
        let rect = Rect {
            x: 0,
            y: 0,
            width: w.max(1),
            height: h.max(1),
        };
        if let Err(e) = self.main.lock().place(rect) {
            tracing::warn!("could not put the main video surface under the tiles: {e}");
        }
    }

    pub fn new(
        db: Arc<Mutex<Connection>>,
        dvr: Arc<crate::dvr::Dvr>,
        factory: TileFactory,
        data_dir: PathBuf,
        main: Arc<Mutex<Box<dyn PlayerBackend>>>,
    ) -> Self {
        Self {
            db,
            dvr,
            factory,
            session: Mutex::new(None),
            parent: AtomicIsize::new(0),
            size: Mutex::new((1280, 720)),
            data_dir,
            main,
        }
    }

    /// Record the window tiles will be children of, and its size.
    pub fn set_window(&self, parent: isize, width: u32, height: u32) {
        self.parent.store(parent, Ordering::SeqCst);
        *self.size.lock() = (width.max(1), height.max(1));
    }

    /// Recordings currently holding a connection.
    fn recordings(&self) -> usize {
        self.dvr.active_ids().len()
    }

    /// The tightest `max_connections` across enabled providers, or `None`.
    ///
    /// The same query `Services` uses to cap simultaneous recordings, asked again here
    /// rather than cached: a provider's limit is rewritten by every refresh that reads
    /// `player_api.php`, and a mosaic opened after one should use the new number.
    fn limit(&self) -> Option<usize> {
        let db = self.db.lock();
        db.query_row(
            "SELECT MIN(max_connections) FROM providers
             WHERE enabled = 1 AND max_connections > 0",
            [],
            |r| r.get::<_, Option<i64>>(0),
        )
        .ok()
        .flatten()
        .map(|n| n.max(1) as usize)
    }

    /// What the picker should say about a layout, without opening it.
    pub fn check(&self, layout: Layout) -> BudgetView {
        let recordings = self.recordings();
        let limit = self.limit();
        BudgetView {
            layout,
            tiles: layout.tiles(),
            budget: Budget::check(
                Demand {
                    tiles: layout.tiles(),
                    recordings,
                },
                limit,
            ),
            largest_fitting: Budget::largest_fitting(recordings, limit),
            recordings,
        }
    }

    /// Open a mosaic.
    ///
    /// `channels` is positional: index 2 is tile 2, and `None` leaves that tile empty.
    /// A list shorter than the layout is padded with empty tiles, so "2×2 with the two
    /// channels I picked" is a thing a viewer can have.
    ///
    /// The budget is checked against the tiles that will actually carry a stream rather
    /// than against the layout's capacity — an empty tile opens no connection, and
    /// refusing a half-filled 3×3 on a four-connection line would be arithmetic nobody
    /// could argue with and everybody would be annoyed by.
    ///
    /// **The caller must stop the main player first.** It holds a connection of its own,
    /// and this counts tiles and recordings only. `mosaic_open` does it in one line
    /// before calling here; doing it inside would make this service depend on
    /// `Playback`, which is the thing that would then have to depend on it to promote a
    /// tile back.
    pub fn open(&self, layout: Layout, channels: &[Option<i64>], now: i64) -> Result<MosaicView> {
        let mut wanted: Vec<Option<i64>> = channels.to_vec();
        wanted.truncate(layout.tiles());
        wanted.resize(layout.tiles(), None);

        let filled = wanted.iter().filter(|c| c.is_some()).count();
        if filled == 0 {
            return Err(AppError::Other(
                "Pick at least one channel to show in the mosaic.".into(),
            ));
        }

        let recordings = self.recordings();
        let budget = Budget::check(
            Demand {
                tiles: filled,
                recordings,
            },
            self.limit(),
        );
        if let Budget::Exceeds {
            needed,
            limit,
            over,
            recordings,
        } = budget
        {
            return Err(AppError::Other(refusal(needed, limit, over, recordings)));
        }

        // Close whatever was open first. Opening a second mosaic over the first would
        // leave its instances alive and its connections held, which is the failure this
        // whole module is trying to avoid.
        self.close();

        let rects = layout.rects_now(*self.size.lock());
        let parent = self.parent.load(Ordering::SeqCst);

        let mut tiles = Vec::with_capacity(layout.tiles());
        for (index, channel_id) in wanted.into_iter().enumerate() {
            tiles.push(self.build_tile(index, channel_id, rects[index], parent, now));
        }

        // Every tile has gone to the bottom of the z-order, so the main surface goes
        // under them. See `sink_main_surface` -- this is the line that makes a mosaic
        // visible rather than merely audible.
        self.sink_main_surface();

        let mut session = Session {
            layout,
            tiles,
            focused: 0,
        };
        // Audio on the first tile that actually has a stream: focusing an empty tile
        // would mean a mosaic with a picture and no sound.
        session.focused = session
            .tiles
            .iter()
            .position(|t| t.player.is_some())
            .unwrap_or(0);
        apply_audio(&mut session);

        let view = view_of(&session, &rects);
        *self.session.lock() = Some(session);
        Ok(view)
    }

    /// Make one tile: its backend, its surface, its stream.
    ///
    /// Every failure stays inside the tile. A channel that was deleted by a refresh, a
    /// channel with no sources, and a stream the provider refuses are three ordinary
    /// things, and one of them must not take the other eight tiles down with it.
    fn build_tile(
        &self,
        index: usize,
        channel_id: Option<i64>,
        rect: Rect,
        parent: isize,
        now: i64,
    ) -> Tile {
        let Some(channel_id) = channel_id else {
            return Tile {
                channel_id: None,
                name: None,
                player: None,
                error: None,
            };
        };

        let resolved = {
            let db = self.db.lock();
            crate::window::live_sources(&db, channel_id, now)
        };
        let (sources, mut options) = match resolved {
            Ok(pair) => pair,
            Err(e) => {
                return Tile {
                    channel_id: Some(channel_id),
                    name: channel_name(&self.db.lock(), channel_id),
                    player: None,
                    error: Some(e.to_string()),
                }
            }
        };
        let name = options.title.clone();

        let Some(source) = sources.into_iter().next() else {
            return Tile {
                channel_id: Some(channel_id),
                name,
                player: None,
                error: Some("Your provider listed this channel without a stream.".into()),
            };
        };

        // Tiles are not the main player: no timeshift buffer, because nine on-disk ring
        // buffers is a disk fire and nobody rewinds a tile; and muted by default, with
        // `apply_audio` turning exactly one back on.
        options.timeshift = None;

        let mut player = (self.factory)();
        if parent != 0 {
            if let Err(e) = player.attach(parent, rect.width.max(1), rect.height.max(1)) {
                return Tile {
                    channel_id: Some(channel_id),
                    name,
                    player: None,
                    error: Some(format!("This tile has no video surface: {e}")),
                };
            }
        }
        if let Err(e) = player.place(rect) {
            tracing::warn!("tile {index} could not be placed: {e}");
        }
        let _ = player.set_muted(true);

        match player.load(&source.url, &options) {
            Ok(()) => Tile {
                channel_id: Some(channel_id),
                name,
                player: Some(player),
                error: None,
            },
            Err(e) => {
                // The instance is dropped with the `Tile` that does not hold it, which
                // is the point: a tile that could not load must not keep a connection
                // open on the way to saying so.
                //
                // Classified rather than printed, because the commonest failure here is
                // the one this module is about: a provider at its connection limit
                // answers 403, and `NetFailure` already turns that into "This often
                // means the line has hit its connection limit".
                let failure = aurora_core::neterr::NetFailure::classify(&e.to_string());
                Tile {
                    channel_id: Some(channel_id),
                    name,
                    player: None,
                    error: Some(format!("{}. {}", failure.message, failure.cause)),
                }
            }
        }
    }

    /// Close the mosaic, dropping every instance and with them every connection.
    pub fn close(&self) -> MosaicView {
        if let Some(mut session) = self.session.lock().take() {
            for tile in &mut session.tiles {
                if let Some(player) = tile.player.as_mut() {
                    let _ = player.stop();
                }
                // Dropped here rather than at the end of the function so a nine-tile
                // mosaic releases its connections one at a time as it goes, rather than
                // all at once after the last `stop` has been waited for.
                tile.player = None;
            }
        }
        MosaicView::closed()
    }

    pub fn is_open(&self) -> bool {
        self.session.lock().is_some()
    }

    /// Move audio to a tile. `1`–`9` in the UI, and a click.
    pub fn focus(&self, index: usize) -> Result<MosaicView> {
        let mut guard = self.session.lock();
        let session = guard.as_mut().ok_or_else(not_open)?;
        if index >= session.tiles.len() {
            return Err(AppError::Other(format!(
                "There is no tile {} in this layout.",
                index + 1
            )));
        }
        if session.tiles[index].player.is_none() {
            return Err(AppError::Other(
                "That tile has no picture, so there is no sound to move to it.".into(),
            ));
        }
        session.focused = index;
        apply_audio(session);
        let rects = session.layout.rects_now(*self.size.lock());
        Ok(view_of(session, &rects))
    }

    /// Put a different channel in one tile, leaving the rest alone.
    ///
    /// The budget does not change when a tile is swapped — one stream out, one in — but
    /// filling a tile that was empty does, so that case is checked.
    pub fn set_tile(&self, index: usize, channel_id: Option<i64>, now: i64) -> Result<MosaicView> {
        let parent = self.parent.load(Ordering::SeqCst);
        let size = *self.size.lock();

        let (layout, was_empty) = {
            let guard = self.session.lock();
            let session = guard.as_ref().ok_or_else(not_open)?;
            if index >= session.tiles.len() {
                return Err(AppError::Other(format!(
                    "There is no tile {} in this layout.",
                    index + 1
                )));
            }
            (session.layout, session.tiles[index].player.is_none())
        };

        if was_empty && channel_id.is_some() {
            let recordings = self.recordings();
            let filled = {
                let guard = self.session.lock();
                let session = guard.as_ref().ok_or_else(not_open)?;
                session.tiles.iter().filter(|t| t.player.is_some()).count()
            };
            if let Budget::Exceeds {
                needed,
                limit,
                over,
                recordings,
            } = Budget::check(
                Demand {
                    tiles: filled + 1,
                    recordings,
                },
                self.limit(),
            ) {
                return Err(AppError::Other(refusal(needed, limit, over, recordings)));
            }
        }

        let rects = layout.rects_now(size);

        // The old instance goes before the new one is built, so the two never hold a
        // connection at the same time — on a line with exactly enough for the layout,
        // building first would be one over the limit for as long as the load takes.
        {
            let mut guard = self.session.lock();
            let session = guard.as_mut().ok_or_else(not_open)?;
            if let Some(player) = session.tiles[index].player.as_mut() {
                let _ = player.stop();
            }
            session.tiles[index].player = None;
        }

        let tile = self.build_tile(index, channel_id, rects[index], parent, now);
        // The replacement tile went to the bottom like every other, so the main surface
        // goes under it too.
        self.sink_main_surface();

        let mut guard = self.session.lock();
        let session = guard.as_mut().ok_or_else(not_open)?;
        session.tiles[index] = tile;
        if session.tiles[session.focused].player.is_none() {
            session.focused = session
                .tiles
                .iter()
                .position(|t| t.player.is_some())
                .unwrap_or(0);
        }
        apply_audio(session);
        Ok(view_of(session, &rects))
    }

    /// Which channel a tile is showing, for promoting it to the main player.
    ///
    /// The mosaic is *not* closed here. The caller tunes the main player first and
    /// closes on success, so a promotion that fails leaves the mosaic as it was rather
    /// than taking away the picture the viewer had.
    pub fn channel_at(&self, index: usize) -> Result<i64> {
        let guard = self.session.lock();
        let session = guard.as_ref().ok_or_else(not_open)?;
        session
            .tiles
            .get(index)
            .and_then(|t| t.channel_id)
            .ok_or_else(|| AppError::Other("That tile has no channel in it.".into()))
    }

    /// Re-divide the window after a resize, and move every surface.
    pub fn relayout(&self, width: u32, height: u32) {
        *self.size.lock() = (width.max(1), height.max(1));
        let mut guard = self.session.lock();
        let Some(session) = guard.as_mut() else {
            return;
        };
        let rects = session.layout.rects_now((width.max(1), height.max(1)));
        for (tile, rect) in session.tiles.iter_mut().zip(rects) {
            if let Some(player) = tile.player.as_mut() {
                let _ = player.place(rect);
            }
        }
        // Every tile has just gone to the bottom of the z-order, so the main surface has
        // to go under them again -- otherwise the first drag of a window edge re-buries
        // the whole mosaic. Dropped first: `sink_main_surface` takes the player mutex,
        // and holding the session lock across it is how a deadlock gets built.
        drop(guard);
        self.sink_main_surface();
    }

    /// Drain every tile's events, the way the main player's heartbeat does.
    ///
    /// Without this a tile's status never changes after its load: `pump` is the only
    /// thing that turns a stream dying into something the UI can be told about, and a
    /// mosaic of nine frozen "playing" labels over nine black squares is precisely the
    /// bug F-30 was.
    pub fn tick(&self) -> Option<MosaicView> {
        let mut guard = self.session.lock();
        let session = guard.as_mut()?;
        for tile in session.tiles.iter_mut() {
            if let Some(player) = tile.player.as_mut() {
                player.pump(0.0);
                let state = player.state();
                if state.status == PlayerStatus::Error && tile.error.is_none() {
                    tile.error = Some(match state.error {
                        Some(failure) => format!("{}. {}", failure.message, failure.cause),
                        None => "This stream stopped.".into(),
                    });
                }
            }
        }
        let rects = session.layout.rects_now(*self.size.lock());
        Some(view_of(session, &rects))
    }

    pub fn state(&self) -> MosaicView {
        let guard = self.session.lock();
        match guard.as_ref() {
            None => MosaicView::closed(),
            Some(session) => {
                let rects = session.layout.rects_now(*self.size.lock());
                view_of(session, &rects)
            }
        }
    }

    /// Save the mosaic as it stands, under a name.
    pub fn save(&self, name: &str, now: i64) -> Result<i64> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::Other("Give the layout a name.".into()));
        }
        let guard = self.session.lock();
        let session = guard.as_ref().ok_or_else(not_open)?;
        let channels: Vec<Option<i64>> = session.tiles.iter().map(|t| t.channel_id).collect();
        let db = self.db.lock();
        repo::upsert_layout(&db, name, session.layout.as_str(), &channels, now)
            .map_err(AppError::from)
    }
}

/// Exactly one tile audible, and only if it has a stream.
fn apply_audio(session: &mut Session) {
    let focused = session.focused;
    for (index, tile) in session.tiles.iter_mut().enumerate() {
        if let Some(player) = tile.player.as_mut() {
            let _ = player.set_muted(index != focused);
        }
    }
}

fn view_of(session: &Session, rects: &[Rect]) -> MosaicView {
    MosaicView {
        open: true,
        layout: Some(session.layout),
        focused: session.focused,
        tiles: session
            .tiles
            .iter()
            .enumerate()
            .map(|(index, tile)| TileView {
                index,
                rect: rects.get(index).copied().unwrap_or(Rect {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                }),
                channel_id: tile.channel_id,
                name: tile.name.clone(),
                focused: index == session.focused,
                status: tile.player.as_ref().map(|p| p.state().status).unwrap_or(
                    if tile.error.is_some() {
                        PlayerStatus::Error
                    } else {
                        PlayerStatus::Idle
                    },
                ),
                error: tile.error.clone(),
            })
            .collect(),
    }
}

fn not_open() -> AppError {
    AppError::Other("The mosaic is not open.".into())
}

/// The refusal, in one place, because it is the sentence this feature is judged on.
///
/// It says the number, what is using it, and what to do — a viewer whose line allows
/// one stream needs to know that no layout will ever work, not that this one did not.
fn refusal(needed: usize, limit: usize, over: usize, recordings: usize) -> String {
    let streams = if needed == 1 { "stream" } else { "streams" };
    let mut msg = format!(
        "That needs {needed} {streams} at once and your provider allows {limit}. \
         Remove {over} channel{} and try again",
        if over == 1 { "" } else { "s" }
    );
    if recordings > 0 {
        let r = if recordings == 1 {
            "1 recording is".to_string()
        } else {
            format!("{recordings} recordings are")
        };
        msg.push_str(&format!(" — {r} using the line as well"));
    }
    msg.push('.');
    msg
}

fn channel_name(db: &Connection, channel_id: i64) -> Option<String> {
    channels::get(db, channel_id).ok().flatten().map(|c| c.name)
}

/// `Layout::rects` taking the size as a pair, which is how it is held here.
trait RectsNow {
    fn rects_now(self, size: (u32, u32)) -> Vec<Rect>;
}

impl RectsNow for Layout {
    fn rects_now(self, (width, height): (u32, u32)) -> Vec<Rect> {
        self.rects(width, height)
    }
}

/* ── Commands ──────────────────────────────────────────────────────────────── */

use tauri::State;

use crate::services::Services;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutArgs {
    pub layout: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenArgs {
    pub layout: String,
    /// Channel id per tile, in tile order. A `null` leaves that tile empty.
    pub channel_ids: Vec<Option<i64>>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TileArgs {
    pub index: usize,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetTileArgs {
    pub index: usize,
    pub channel_id: Option<i64>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveArgs {
    pub name: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdArgs {
    pub id: i64,
}

fn layout_of(name: &str) -> Result<Layout> {
    Layout::parse(name).ok_or_else(|| AppError::Other(format!("unknown mosaic layout {name:?}")))
}

/// What the picker should say about a layout before anything is opened.
#[tauri::command(async)]
pub fn mosaic_check(services: State<'_, Services>, args: LayoutArgs) -> Result<BudgetView> {
    Ok(services.mosaic.check(layout_of(&args.layout)?))
}

/// Open a mosaic.
///
/// The main player is stopped first, and deliberately here rather than inside the
/// service: it holds a connection of its own, so leaving it running would make every
/// budget this feature computes wrong by one — and on a four-connection line that is
/// the difference between a 2×2 opening and the provider cutting something.
#[tauri::command(async)]
pub fn mosaic_open(services: State<'_, Services>, args: OpenArgs) -> Result<MosaicView> {
    let layout = layout_of(&args.layout)?;
    let _ = services.playback.stop();
    services
        .mosaic
        .open(layout, &args.channel_ids, crate::now_unix())
}

#[tauri::command(async)]
pub fn mosaic_close(services: State<'_, Services>) -> Result<MosaicView> {
    Ok(services.mosaic.close())
}

#[tauri::command(async)]
pub fn mosaic_state(services: State<'_, Services>) -> Result<MosaicView> {
    Ok(services.mosaic.state())
}

#[tauri::command(async)]
pub fn mosaic_focus(services: State<'_, Services>, args: TileArgs) -> Result<MosaicView> {
    services.mosaic.focus(args.index)
}

#[tauri::command(async)]
pub fn mosaic_set_tile(services: State<'_, Services>, args: SetTileArgs) -> Result<MosaicView> {
    services
        .mosaic
        .set_tile(args.index, args.channel_id, crate::now_unix())
}

/// Promote a tile to the main player (README §7.4: click a tile to fullscreen it).
///
/// The mosaic is closed only once the main player has actually tuned. The other order
/// would take away the picture the viewer had in order to show them an error.
#[tauri::command(async)]
pub fn mosaic_promote(
    services: State<'_, Services>,
    args: TileArgs,
) -> Result<aurora_player::PlayerState> {
    let channel_id = services.mosaic.channel_at(args.index)?;
    // Released before tuning, not after: on a line with exactly enough connections for
    // the layout, the main player's stream would be one over for as long as the tiles
    // were still up, and the provider would refuse the one the viewer just asked for.
    services.mosaic.close();
    services.playback.play_live(channel_id, crate::now_unix())
}

#[tauri::command(async)]
pub fn mosaic_save(services: State<'_, Services>, args: SaveArgs) -> Result<i64> {
    services.mosaic.save(&args.name, crate::now_unix())
}

#[tauri::command(async)]
pub fn mosaic_layouts(services: State<'_, Services>) -> Result<Vec<repo::SavedLayout>> {
    let db = services.db.lock();
    Ok(repo::list_layouts(&db)?)
}

/// Open a saved layout by id.
///
/// The channel ids are whatever was saved, holes included, and are resolved now rather
/// than when they were saved — so a layout naming a channel the provider has since
/// dropped opens with that one tile explaining itself and the rest playing.
#[tauri::command(async)]
pub fn mosaic_open_saved(services: State<'_, Services>, args: IdArgs) -> Result<MosaicView> {
    let saved = {
        let db = services.db.lock();
        repo::get_layout(&db, args.id)?
    }
    .ok_or_else(|| AppError::Other("That saved layout is gone.".into()))?;

    let layout = layout_of(&saved.layout)?;
    let _ = services.playback.stop();
    services
        .mosaic
        .open(layout, &saved.channels, crate::now_unix())
}

#[tauri::command(async)]
pub fn mosaic_delete_layout(services: State<'_, Services>, args: IdArgs) -> Result<bool> {
    let db = services.db.lock();
    Ok(repo::delete_layout(&db, args.id)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_player::NullBackend;

    const NOW: i64 = 1_760_000_000;

    /// A mosaic wired to `NullBackend`s: no decoding, but every decision this module
    /// makes is real.
    fn mosaic(limit: Option<i64>) -> (Mosaic, Arc<Mutex<Connection>>) {
        let db = aurora_db::open_memory().unwrap();
        db.execute(
            "INSERT INTO providers (id,name,kind,base_url,enabled,max_connections,created_at)
             VALUES (1,'P','xtream','https://example.com',1,?1,0)",
            [limit],
        )
        .unwrap();
        let db = Arc::new(Mutex::new(db));

        let dvr = Arc::new(crate::dvr::Dvr::new(
            Arc::clone(&db),
            Arc::new(NoRecorder),
            std::env::temp_dir(),
        ));
        let factory: TileFactory = Arc::new(|| Box::new(NullBackend::default()));
        (
            Mosaic::new(
                Arc::clone(&db),
                dvr,
                factory,
                std::env::temp_dir(),
                Arc::new(Mutex::new(Box::new(NullBackend::default()))),
            ),
            db,
        )
    }

    /// The same, with a fake handle for the main surface and every tile placement
    /// recorded.
    ///
    /// `NullBackend` has no window and reports no handle, so it cannot answer the
    /// question this is about: was each tile ordered *above* the main player's surface,
    /// or sent to the bottom of the z-order behind it.
    /// Every placement, in the order they were asked for, tagged with who was moved.
    ///
    /// The order is the whole assertion: the tiles go to the bottom of the z-order as
    /// they are built, so the main player's surface has to be sent to the bottom *after*
    /// them or it sits on top and the mosaic is audible and invisible.
    type Placements = Arc<Mutex<Vec<(&'static str, Rect)>>>;

    fn mosaic_watching_tiles(limit: Option<i64>) -> (Mosaic, Placements, Arc<Mutex<Connection>>) {
        let (m, db) = mosaic(limit);
        let placed: Placements = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&placed);
        let main: Arc<Mutex<Box<dyn PlayerBackend>>> = Arc::new(Mutex::new(Box::new(Spy {
            who: "main",
            placed: Arc::clone(&placed),
        })));
        let factory: TileFactory = Arc::new(move || {
            Box::new(Spy {
                who: "tile",
                placed: Arc::clone(&log),
            })
        });
        (Mosaic { main, factory, ..m }, placed, db)
    }

    /// Records what it was asked to do, and refuses to be hidden.
    struct Spy {
        who: &'static str,
        placed: Placements,
    }

    impl PlayerBackend for Spy {
        fn load(
            &mut self,
            _: &str,
            _: &aurora_player::backend::LoadOptions,
        ) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn stop(&mut self) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn set_paused(&mut self, _: bool) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn seek(&mut self, _: f64, _: bool) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn set_volume(&mut self, _: u32) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn set_muted(&mut self, _: bool) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn set_speed(&mut self, _: f64) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn set_audio_track(
            &mut self,
            _: Option<i64>,
        ) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn set_subtitle_track(
            &mut self,
            _: Option<i64>,
        ) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn set_aspect(
            &mut self,
            _: aurora_player::state::Aspect,
        ) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn pump(&mut self, _: f64) {}
        fn state(&self) -> aurora_player::PlayerState {
            aurora_player::PlayerState::default()
        }
        fn chapters(&self) -> Vec<aurora_core::markers::Chapter> {
            Vec::new()
        }
        fn resize(
            &mut self,
            _: u32,
            _: u32,
        ) -> std::result::Result<(), aurora_player::PlayerError> {
            Ok(())
        }
        fn place(&mut self, rect: Rect) -> std::result::Result<(), aurora_player::PlayerError> {
            self.placed.lock().push((self.who, rect));
            Ok(())
        }
        /// Nothing is hidden any more, so nothing has to be restored.
        ///
        /// Hiding the main surface was the first fix for the bug below, and it could not
        /// be undone: `SetWindowPos` on a window whose sibling belongs to a thread with
        /// no message loop does not return (AUDIT/findings.md F-36), so the window stayed
        /// black after closing. Panicking here is how these tests say that approach must
        /// not come back.
        fn set_surface_visible(
            &mut self,
            visible: bool,
        ) -> std::result::Result<(), aurora_player::PlayerError> {
            panic!(
                "asked to set the {} surface visible={visible}; see F-36",
                self.who
            );
        }
    }

    /// The bug this exists for: a mosaic you could hear and could not see.
    ///
    /// Every `place` goes to the bottom of the z-order, so the tiles land underneath the
    /// main player's full-window surface -- which is still there, still full-window, and
    /// black with nothing loaded. The main surface has to be sent down *after* them.
    #[test]
    fn the_main_surface_goes_under_the_tiles() {
        let (m, placed, db) = mosaic_watching_tiles(Some(8));
        // A channel with a stream: without one `build_tile` reports that and returns
        // before it ever places a surface.
        seed_channel(&db, 1, "One", true);
        m.open(Layout::Grid2x2, &[Some(1), None, None, None], NOW)
            .unwrap();

        let calls = placed.lock().clone();
        assert!(
            calls.iter().any(|(who, _)| *who == "tile"),
            "no tile was placed at all: {calls:?}"
        );
        assert_eq!(
            calls.last().map(|(who, _)| *who),
            Some("main"),
            "the main surface was not the last thing sent to the bottom, so it is on top \
             of the tiles: {calls:?}"
        );
    }

    /// And again on a resize, or the first drag of a window edge re-buries the mosaic.
    #[test]
    fn a_resize_puts_it_back_under_them() {
        let (m, placed, db) = mosaic_watching_tiles(Some(8));
        seed_channel(&db, 1, "One", true);
        m.open(Layout::Grid2x2, &[Some(1), None, None, None], NOW)
            .unwrap();
        placed.lock().clear();

        m.relayout(1920, 1080);
        let calls = placed.lock().clone();
        assert!(
            calls.iter().any(|(who, _)| *who == "tile"),
            "a resize moved no tile: {calls:?}"
        );
        assert_eq!(
            calls.last().map(|(who, _)| *who),
            Some("main"),
            "a resize left the main surface on top of the tiles: {calls:?}"
        );
        // And it covers the new client area, not the old one.
        let (_, rect) = calls.last().unwrap();
        assert_eq!((rect.width, rect.height), (1920, 1080));
    }

    /// Opening and closing one must not touch the main surface's visibility at all --
    /// `Spy::set_surface_visible` panics, so reaching the end of this is the assertion.
    #[test]
    fn a_mosaic_never_hides_the_main_surface() {
        let (m, _placed, db) = mosaic_watching_tiles(Some(8));
        seed_channel(&db, 1, "One", true);
        m.open(Layout::Grid2x2, &[Some(1), None, None, None], NOW)
            .unwrap();
        m.close();
    }

    /// Nothing in here records: the DVR is present only so the mosaic can ask how many
    /// recordings are holding a connection, and the answer in these tests is zero.
    struct NoRecorder;
    impl aurora_ingest::recorder::Recorder for NoRecorder {
        fn start(
            &self,
            _request: aurora_ingest::recorder::RecordRequest,
        ) -> std::result::Result<aurora_ingest::recorder::Handle, aurora_core::neterr::NetFailure>
        {
            Err(aurora_core::neterr::NetFailure::classify(
                "no recorder in this test",
            ))
        }
    }

    fn seed_channel(db: &Arc<Mutex<Connection>>, id: i64, name: &str, with_source: bool) {
        let db = db.lock();
        db.execute(
            "INSERT INTO channels (id, provider_id, provider_key, name, match_key, last_seen_at)
             VALUES (?1, 1, ?2, ?3, ?4, 0)",
            aurora_db::rusqlite::params![id, format!("c{id}"), name, name.to_lowercase()],
        )
        .unwrap();
        if with_source {
            db.execute(
                "INSERT INTO channel_sources (channel_id, url) VALUES (?1, ?2)",
                aurora_db::rusqlite::params![id, format!("http://example.com/live/{id}.ts")],
            )
            .unwrap();
        }
    }

    #[test]
    fn a_two_by_two_opens_four_tiles_each_in_its_own_rectangle() {
        let (m, db) = mosaic(Some(8));
        for id in 1..=4 {
            seed_channel(&db, id, &format!("Channel {id}"), true);
        }
        m.set_window(0, 1000, 800);

        let view = m
            .open(Layout::Grid2x2, &[Some(1), Some(2), Some(3), Some(4)], NOW)
            .expect("four channels on an eight-connection line");

        assert!(view.open);
        assert_eq!(view.tiles.len(), 4);
        assert_eq!(view.layout, Some(Layout::Grid2x2));

        // The rectangles the UI is told about are the ones the layout computes.
        let expected = Layout::Grid2x2.rects(1000, 800);
        let got: Vec<Rect> = view.tiles.iter().map(|t| t.rect).collect();
        assert_eq!(got, expected);

        for (i, tile) in view.tiles.iter().enumerate() {
            assert_eq!(tile.channel_id, Some(i as i64 + 1));
            assert_eq!(tile.name.as_deref(), Some(&*format!("Channel {}", i + 1)));
            assert_eq!(tile.status, PlayerStatus::Playing, "tile {i}");
            assert!(tile.error.is_none(), "tile {i}: {:?}", tile.error);
        }
    }

    /// Nine streams of sound at once is not a feature.
    #[test]
    fn exactly_one_tile_has_audio_and_focus_moves_it() {
        let (m, db) = mosaic(Some(8));
        for id in 1..=4 {
            seed_channel(&db, id, &format!("Channel {id}"), true);
        }
        m.set_window(0, 800, 600);
        let view = m
            .open(Layout::Grid2x2, &[Some(1), Some(2), Some(3), Some(4)], NOW)
            .unwrap();

        assert_eq!(view.focused, 0);
        assert_eq!(view.tiles.iter().filter(|t| t.focused).count(), 1);

        let moved = m.focus(2).expect("tile 3 has a picture");
        assert_eq!(moved.focused, 2);
        assert!(moved.tiles[2].focused && !moved.tiles[0].focused);
        assert_eq!(moved.tiles.iter().filter(|t| t.focused).count(), 1);
    }

    #[test]
    fn focusing_a_tile_that_is_not_there_is_refused_by_number() {
        let (m, db) = mosaic(Some(8));
        seed_channel(&db, 1, "One", true);
        m.open(Layout::Grid2x2, &[Some(1)], NOW).unwrap();

        let err = m.focus(7).unwrap_err().to_string();
        assert!(err.contains("tile 8"), "{err}");
    }

    /// The refusal this module exists for. The real panel allows one connection.
    #[test]
    fn a_single_connection_line_refuses_and_says_the_number() {
        let (m, db) = mosaic(Some(1));
        for id in 1..=4 {
            seed_channel(&db, id, &format!("Channel {id}"), true);
        }

        let err = m
            .open(Layout::Grid2x2, &[Some(1), Some(2), Some(3), Some(4)], NOW)
            .unwrap_err()
            .to_string();

        assert!(err.contains("4 streams"), "{err}");
        assert!(err.contains("allows 1"), "{err}");
        assert!(err.contains("Remove 3 channels"), "{err}");
        assert!(!m.is_open(), "nothing may be left open after a refusal");
    }

    /// An empty tile opens no connection, so it must not count against the limit.
    #[test]
    fn empty_tiles_cost_nothing() {
        let (m, db) = mosaic(Some(2));
        seed_channel(&db, 1, "One", true);
        seed_channel(&db, 2, "Two", true);

        // A 3x3 has nine tiles and this line allows two — but only two carry a stream.
        let view = m
            .open(Layout::Grid3x3, &[Some(1), Some(2)], NOW)
            .expect("two streams in a nine-tile layout fit a two-connection line");

        assert_eq!(view.tiles.len(), 9);
        assert_eq!(
            view.tiles.iter().filter(|t| t.channel_id.is_some()).count(),
            2
        );
        for tile in &view.tiles[2..] {
            assert_eq!(tile.status, PlayerStatus::Idle);
            assert!(tile.error.is_none());
        }
    }

    /// The picker's half: a layout's capacity, before any channel is chosen.
    #[test]
    fn the_check_reports_the_layout_against_the_line() {
        let (m, _db) = mosaic(Some(4));
        let two_by_two = m.check(Layout::Grid2x2);
        assert_eq!(two_by_two.tiles, 4);
        assert_eq!(
            two_by_two.budget,
            Budget::Fits {
                needed: 4,
                limit: 4
            }
        );
        assert_eq!(two_by_two.largest_fitting.map(Layout::tiles), Some(4));

        let nine = m.check(Layout::Grid3x3);
        assert!(nine.budget.is_refused());
        assert_eq!(nine.recordings, 0);
    }

    /// An M3U playlist declares nothing, which is the common case. It must open.
    #[test]
    fn a_provider_with_no_declared_limit_is_allowed_to_try() {
        let (m, db) = mosaic(None);
        for id in 1..=4 {
            seed_channel(&db, id, &format!("Channel {id}"), true);
        }
        let view = m
            .open(Layout::Grid2x2, &[Some(1), Some(2), Some(3), Some(4)], NOW)
            .expect("an undeclared limit must not block the attempt");
        assert_eq!(view.tiles.iter().filter(|t| t.error.is_none()).count(), 4);

        assert_eq!(
            m.check(Layout::Grid3x3).budget,
            Budget::Unknown { needed: 9 }
        );
    }

    /// One bad channel is one bad tile. This is the resilience the mosaic needs most:
    /// a provider that drops a stream must not black out the other eight.
    #[test]
    fn a_channel_with_no_stream_fails_its_own_tile_only() {
        let (m, db) = mosaic(Some(8));
        seed_channel(&db, 1, "Good", true);
        seed_channel(&db, 2, "No Source", false);
        seed_channel(&db, 3, "Also Good", true);

        let view = m
            .open(Layout::Grid2x2, &[Some(1), Some(2), Some(3), None], NOW)
            .unwrap();

        assert_eq!(view.tiles[0].status, PlayerStatus::Playing);
        assert_eq!(view.tiles[2].status, PlayerStatus::Playing);

        let bad = &view.tiles[1];
        assert_eq!(bad.status, PlayerStatus::Error);
        let message = bad.error.clone().expect("a reason");
        assert!(message.contains("without a stream"), "{message}");
        // And it is still named, so the viewer knows which channel failed.
        assert_eq!(bad.name.as_deref(), Some("No Source"));
    }

    /// A channel a refresh removed, named in a saved layout.
    #[test]
    fn a_channel_that_is_gone_is_one_tile_with_a_reason() {
        let (m, db) = mosaic(Some(8));
        seed_channel(&db, 1, "Still Here", true);

        let view = m
            .open(Layout::Grid2x2, &[Some(1), Some(404), None, None], NOW)
            .unwrap();

        assert_eq!(view.tiles[0].status, PlayerStatus::Playing);
        assert_eq!(view.tiles[1].status, PlayerStatus::Error);
        assert!(view.tiles[1].error.is_some());
    }

    #[test]
    fn opening_with_no_channels_at_all_is_refused() {
        let (m, _db) = mosaic(Some(8));
        let err = m
            .open(Layout::Grid2x2, &[None, None, None, None], NOW)
            .unwrap_err()
            .to_string();
        assert!(err.contains("at least one channel"), "{err}");
    }

    #[test]
    fn swapping_a_tile_leaves_the_others_alone() {
        let (m, db) = mosaic(Some(8));
        for id in 1..=5 {
            seed_channel(&db, id, &format!("Channel {id}"), true);
        }
        m.open(Layout::Grid2x2, &[Some(1), Some(2), Some(3), Some(4)], NOW)
            .unwrap();

        let view = m.set_tile(1, Some(5), NOW).unwrap();
        assert_eq!(view.tiles[1].channel_id, Some(5));
        assert_eq!(view.tiles[1].name.as_deref(), Some("Channel 5"));
        // The rest are untouched.
        assert_eq!(view.tiles[0].channel_id, Some(1));
        assert_eq!(view.tiles[2].channel_id, Some(3));
        assert_eq!(view.tiles[3].channel_id, Some(4));
        assert_eq!(view.tiles.iter().filter(|t| t.focused).count(), 1);
    }

    /// Filling a tile that was empty is one more stream, so it is checked.
    #[test]
    fn filling_an_empty_tile_is_checked_against_the_limit() {
        let (m, db) = mosaic(Some(2));
        seed_channel(&db, 1, "One", true);
        seed_channel(&db, 2, "Two", true);
        seed_channel(&db, 3, "Three", true);

        m.open(Layout::Grid2x2, &[Some(1), Some(2), None, None], NOW)
            .unwrap();

        let err = m.set_tile(2, Some(3), NOW).unwrap_err().to_string();
        assert!(err.contains("allows 2"), "{err}");

        // Swapping one for another is not one more stream, so it is allowed.
        let view = m
            .set_tile(1, Some(3), NOW)
            .expect("a swap is not an increase");
        assert_eq!(view.tiles[1].channel_id, Some(3));
    }

    #[test]
    fn emptying_the_focused_tile_moves_audio_to_one_that_has_a_picture() {
        let (m, db) = mosaic(Some(8));
        seed_channel(&db, 1, "One", true);
        seed_channel(&db, 2, "Two", true);
        m.open(Layout::Grid2x2, &[Some(1), Some(2), None, None], NOW)
            .unwrap();

        let view = m.set_tile(0, None, NOW).unwrap();
        assert_eq!(view.focused, 1, "audio followed the remaining picture");
        assert!(view.tiles[1].focused);
    }

    #[test]
    fn a_resize_moves_every_tile() {
        let (m, db) = mosaic(Some(8));
        for id in 1..=4 {
            seed_channel(&db, id, &format!("Channel {id}"), true);
        }
        m.set_window(0, 800, 600);
        m.open(Layout::Grid2x2, &[Some(1), Some(2), Some(3), Some(4)], NOW)
            .unwrap();

        m.relayout(1600, 900);
        let view = m.state();
        assert_eq!(
            view.tiles.iter().map(|t| t.rect).collect::<Vec<_>>(),
            Layout::Grid2x2.rects(1600, 900)
        );
    }

    #[test]
    fn closing_drops_every_tile() {
        let (m, db) = mosaic(Some(8));
        for id in 1..=4 {
            seed_channel(&db, id, &format!("Channel {id}"), true);
        }
        m.open(Layout::Grid2x2, &[Some(1), Some(2), Some(3), Some(4)], NOW)
            .unwrap();
        assert!(m.is_open());

        let view = m.close();
        assert!(!view.open);
        assert!(view.tiles.is_empty());
        assert!(!m.is_open());
        assert!(!m.state().open);
    }

    /// Opening twice must not leave the first mosaic's instances alive — that is nine
    /// connections held by a mosaic nobody can see.
    #[test]
    fn opening_again_replaces_rather_than_stacking() {
        let (m, db) = mosaic(Some(4));
        for id in 1..=4 {
            seed_channel(&db, id, &format!("Channel {id}"), true);
        }
        m.open(Layout::Grid2x2, &[Some(1), Some(2), Some(3), Some(4)], NOW)
            .unwrap();
        // The same line could not carry eight, so this only works if the first four
        // were released first.
        let view = m
            .open(Layout::Grid2x2, &[Some(4), Some(3), Some(2), Some(1)], NOW)
            .expect("the first mosaic's connections were released");
        assert_eq!(view.tiles[0].channel_id, Some(4));
    }

    #[test]
    fn a_tile_can_be_promoted_by_naming_its_channel() {
        let (m, db) = mosaic(Some(8));
        seed_channel(&db, 1, "One", true);
        seed_channel(&db, 2, "Two", true);
        m.open(Layout::Grid2x2, &[Some(1), Some(2), None, None], NOW)
            .unwrap();

        assert_eq!(m.channel_at(1).unwrap(), 2);
        assert!(
            m.channel_at(3).is_err(),
            "an empty tile has nothing to promote"
        );
        // Promotion does not close the mosaic: the caller does, once the main player
        // has actually tuned.
        assert!(m.is_open());
    }

    #[test]
    fn commands_on_a_closed_mosaic_say_so_rather_than_panicking() {
        let (m, _db) = mosaic(Some(8));
        for err in [
            m.focus(0).unwrap_err().to_string(),
            m.set_tile(0, Some(1), NOW).unwrap_err().to_string(),
            m.channel_at(0).unwrap_err().to_string(),
            m.save("x", NOW).unwrap_err().to_string(),
        ] {
            assert!(err.contains("not open"), "{err}");
        }
        assert!(m.tick().is_none());
        // And a resize with nothing open is a no-op rather than a panic.
        m.relayout(100, 100);
    }

    #[test]
    fn the_open_mosaic_can_be_saved_and_read_back() {
        let (m, db) = mosaic(Some(8));
        seed_channel(&db, 1, "One", true);
        seed_channel(&db, 2, "Two", true);
        m.open(Layout::OnePlusThree, &[Some(1), None, Some(2), None], NOW)
            .unwrap();

        let id = m.save("  Sunday  ", NOW).unwrap();
        let saved = repo::get_layout(&db.lock(), id).unwrap().unwrap();
        assert_eq!(saved.name, "Sunday");
        assert_eq!(saved.layout, "onePlusThree");
        assert_eq!(saved.channels, vec![Some(1), None, Some(2), None]);

        assert!(m.save("   ", NOW).is_err(), "a nameless layout is refused");
    }

    #[test]
    fn a_recording_in_flight_tightens_the_refusal_and_says_why() {
        // Four connections, three streams wanted, and one recording running: five is
        // one over. The message has to say the recording is part of it, or the number
        // looks like a lie.
        let message = refusal(5, 4, 1, 1);
        assert!(
            message.contains("1 recording is using the line"),
            "{message}"
        );
        let plural = refusal(6, 4, 2, 2);
        assert!(plural.contains("2 recordings are"), "{plural}");
        // And with nothing recording the clause is absent rather than empty.
        let clean = refusal(4, 1, 3, 0);
        assert!(!clean.contains("recording"), "{clean}");
    }
}
