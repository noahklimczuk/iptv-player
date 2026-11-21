//! Media keys and the Windows media overlay (System Media Transport Controls).
//!
//! Windows routes the keyboard's media keys, a headset's buttons and the overlay's own
//! buttons to whichever app has told it what is playing. Aurora plays through mpv, not a
//! web page's `<video>`, so until it says so itself Windows does not know anything is
//! playing at all, and every one of those buttons goes somewhere else.
//!
//! Three parts:
//!
//! - [`now_playing`] turns the player's state into what the overlay shows. Pure, so it
//!   is tested on any host.
//! - [`Publisher`] works out what changed since last time. The player's state arrives
//!   several times a second; the overlay wants a title once and the position now and then.
//! - [`MediaKeys`] runs both on a thread of its own, where it may look things up in the
//!   database without the player waiting -- the state sink is called from inside the
//!   session, and the database lock is not reentrant.
//!
//! A press comes back as a [`Button`], which the app hands to the page: the page already
//! owns play-pause and changing channel (`useHotkeys`), so a media key does exactly what
//! the same key on the page does. The page must therefore not bind the media keys itself,
//! or one press would act twice.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use aurora_db::repo::parental::{self, Target};
use aurora_db::rusqlite::{Connection, OptionalExtension};
use aurora_ingest::artwork;
use aurora_video::{ItemKind, PlayerState, Status};
use parking_lot::Mutex;
use serde::Serialize;

use crate::dblock::TimedMutex;

#[cfg(windows)]
mod smtc;
#[cfg(windows)]
pub use smtc::Smtc;

/// What the overlay says Aurora is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playback {
    /// Nothing playing: Aurora leaves the overlay.
    Closed,
    /// Opening, buffering or reconnecting.
    Changing,
    Playing,
    Paused,
}

/// Where the overlay's picture comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Artwork {
    /// Already in the artwork cache.
    File(PathBuf),
    /// Not cached, and on HTTPS: Windows fetches it itself. Plain HTTP is not handed over.
    Url(String),
}

/// The words and picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub title: String,
    pub subtitle: Option<String>,
    pub artwork: Option<Artwork>,
}

/// Where a film or an episode has got to. Live TV has none.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timeline {
    pub position_secs: f64,
    pub duration_secs: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NowPlaying {
    pub playback: Playback,
    pub shown: Shown,
    /// Next and previous change channel. Only on live TV: a film has no next.
    pub channels: bool,
    pub timeline: Option<Timeline>,
}

/// What is known about the thing playing beyond the player's own state, looked up once
/// per title (and once a minute on live TV, for the programme).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Item {
    /// Locked by a parent, or in an adult category: the overlay and the lock screen get
    /// "Aurora TV" and no picture (`parental::is_private`).
    pub private: bool,
    pub artwork: Option<Artwork>,
    /// On live TV, the programme on now.
    pub programme: Option<String>,
}

/// The name shown for anything private, and for something with no title.
pub const APP_NAME: &str = "Aurora TV";

pub fn now_playing(state: &PlayerState, item: &Item) -> NowPlaying {
    let r = &state.report;
    let playback = match r.status {
        Status::Idle | Status::Error => Playback::Closed,
        _ if state.reconnecting => Playback::Changing,
        Status::Loading | Status::Buffering => Playback::Changing,
        Status::Playing => Playback::Playing,
        Status::Paused => Playback::Paused,
    };
    let shown = if item.private {
        Shown {
            title: APP_NAME.into(),
            subtitle: None,
            artwork: None,
        }
    } else {
        let title = state
            .title
            .clone()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| APP_NAME.into());
        let subtitle = if r.is_live {
            item.programme.clone().or_else(|| state.subtitle.clone())
        } else {
            state.subtitle.clone()
        };
        Shown {
            title,
            subtitle: subtitle.filter(|s| !s.trim().is_empty()),
            artwork: item.artwork.clone(),
        }
    };
    let timeline = (!r.is_live && r.duration_secs > 0.0).then(|| Timeline {
        position_secs: r.position_secs.clamp(0.0, r.duration_secs),
        duration_secs: r.duration_secs,
    });
    NowPlaying {
        playback,
        shown,
        channels: r.is_live && playback != Playback::Closed,
        timeline,
    }
}

/// What to tell Windows this time. `None` is "as it was".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Changes {
    pub playback: Option<Playback>,
    pub shown: Option<Shown>,
    pub channels: Option<bool>,
    /// `Some(None)` takes the bar away.
    pub timeline: Option<Option<Timeline>>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// How far the position may drift from where it should be before it is sent again: a
/// seek, rather than playback moving it on.
const DRIFT_SECS: f64 = 2.0;
/// How often the position is sent while playing anyway. Windows does not move the bar on
/// by itself.
const REFRESH_SECS: f64 = 5.0;

/// Remembers what was last sent, so only what changed is sent again.
#[derive(Debug, Default)]
pub struct Publisher {
    last: Option<NowPlaying>,
    /// The position last sent and when, on the caller's clock.
    sent_at: Option<(Timeline, f64)>,
}

impl Publisher {
    pub fn next(&mut self, now: NowPlaying, clock: f64) -> Changes {
        let mut out = Changes::default();
        let last = self.last.as_ref();
        if last.map(|l| l.playback) != Some(now.playback) {
            out.playback = Some(now.playback);
        }
        if now.playback == Playback::Closed {
            // Nothing else matters to a closed overlay, and whatever opens next starts
            // from nothing.
            self.last = Some(now);
            self.sent_at = None;
            return out;
        }
        // Opening after nothing: everything is sent, since closing forgot it all.
        let reopened = last.is_none_or(|l| l.playback == Playback::Closed);
        if reopened || last.map(|l| &l.shown) != Some(&now.shown) {
            out.shown = Some(now.shown.clone());
        }
        if reopened || last.map(|l| l.channels) != Some(now.channels) {
            out.channels = Some(now.channels);
        }
        let timeline_due = match (now.timeline, self.sent_at) {
            (None, None) => reopened,
            (None, Some(_)) => true,
            (Some(_), None) => true,
            (Some(t), Some((sent, at))) => {
                let speed = if last.is_some_and(|l| l.playback == Playback::Playing) {
                    1.0
                } else {
                    0.0
                };
                let expected = sent.position_secs + (clock - at) * speed;
                (t.duration_secs - sent.duration_secs).abs() > 0.5
                    || (t.position_secs - expected).abs() > DRIFT_SECS
                    || out.playback.is_some()
                    || (now.playback == Playback::Playing && clock - at >= REFRESH_SECS)
            }
        };
        if timeline_due {
            out.timeline = Some(now.timeline);
            self.sent_at = now.timeline.map(|t| (t, clock));
        }
        self.last = Some(now);
        out
    }
}

/// A press on a media key, a headset or the overlay, for the page to act on.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "button", rename_all = "camelCase")]
pub enum Button {
    Play,
    Pause,
    Next,
    Previous,
    /// The overlay's bar was dragged.
    #[serde(rename_all = "camelCase")]
    Seek {
        position_secs: f64,
    },
}

/// Where the changes go: Windows, or nowhere.
pub trait Controls: Send {
    fn apply(&mut self, changes: &Changes) -> Result<(), String>;
}

/// Which title a lookup was for. A new one is looked up again.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    kind: Option<ItemKind>,
    channel_id: Option<i64>,
    item_id: Option<i64>,
    title: Option<String>,
}

impl Key {
    fn of(s: &PlayerState) -> Self {
        Self {
            kind: s.item_kind,
            channel_id: s.channel_id,
            item_id: s.item_id,
            title: s.title.clone(),
        }
    }
}

/// How often live TV's programme is looked up again.
const PROGRAMME_EVERY: Duration = Duration::from_secs(60);

/// Feeds the overlay from the player, on a thread of its own.
pub struct MediaKeys {
    tx: Mutex<Option<mpsc::Sender<PlayerState>>>,
}

impl MediaKeys {
    /// Nothing to tell: off Windows, or when the controls could not be had.
    pub fn none() -> Self {
        Self {
            tx: Mutex::new(None),
        }
    }

    /// `make` runs on the new thread, which is where the controls then live: COM objects
    /// are made and used on the thread that set up COM.
    pub fn start(
        make: impl FnOnce() -> Option<Box<dyn Controls>> + Send + 'static,
        lookup: impl Fn(&PlayerState) -> Item + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<PlayerState>();
        let spawned = std::thread::Builder::new()
            .name("media-keys".into())
            .spawn(move || {
                let Some(mut controls) = make() else {
                    return;
                };
                let origin = Instant::now();
                let mut publisher = Publisher::default();
                let mut looked_up: Option<(Key, Item, Instant)> = None;
                while let Ok(mut state) = rx.recv() {
                    // Only the newest state matters; the rest are already out of date.
                    while let Ok(newer) = rx.try_recv() {
                        state = newer;
                    }
                    let key = Key::of(&state);
                    let stale = match &looked_up {
                        Some((k, _, at)) => {
                            *k != key || (state.report.is_live && at.elapsed() >= PROGRAMME_EVERY)
                        }
                        None => true,
                    };
                    if stale && state.report.status != Status::Idle {
                        looked_up = Some((key, lookup(&state), Instant::now()));
                    }
                    let item = looked_up
                        .as_ref()
                        .map(|(_, i, _)| i.clone())
                        .unwrap_or_default();
                    let changes =
                        publisher.next(now_playing(&state, &item), origin.elapsed().as_secs_f64());
                    if changes.is_empty() {
                        continue;
                    }
                    if let Err(e) = controls.apply(&changes) {
                        tracing::warn!("the Windows media overlay did not take an update: {e}");
                    }
                }
            });
        match spawned {
            Ok(_) => Self {
                tx: Mutex::new(Some(tx)),
            },
            Err(e) => {
                tracing::warn!("media keys are off: the thread did not start: {e}");
                Self::none()
            }
        }
    }

    /// Called with every state the player reaches. Never blocks.
    pub fn push(&self, state: &PlayerState) {
        let mut tx = self.tx.lock();
        if let Some(t) = tx.as_ref() {
            if t.send(state.clone()).is_err() {
                // The thread has gone; stop cloning states for nobody.
                *tx = None;
            }
        }
    }
}

/// What is playing, looked up: whether it is private, its picture, and on live TV the
/// programme. Errors look up nothing, and the overlay shows the title alone.
pub fn lookup(db: &TimedMutex<Connection>, art: &artwork::Cache, state: &PlayerState) -> Item {
    let target = match (state.item_kind, state.channel_id, state.item_id) {
        (Some(ItemKind::Live), Some(ch), _) | (None, Some(ch), _) => Target::Channel(ch),
        (Some(ItemKind::Movie), _, Some(id)) => Target::Movie(id),
        (Some(ItemKind::Episode), _, Some(id)) => Target::Episode(id),
        (Some(ItemKind::Recording), _, Some(id)) => Target::Recording(id),
        // Played by address, from nowhere in the library.
        _ => return Item::default(),
    };
    let conn = db.lock();
    let private = match parental::is_private(&conn, target) {
        Ok(p) => p,
        Err(e) => {
            // Fail closed: if it cannot be told, it is not shown.
            tracing::warn!("could not tell whether this may show on the lock screen: {e}");
            true
        }
    };
    if private {
        return Item {
            private,
            ..Item::default()
        };
    }
    let url = artwork_url(&conn, target).unwrap_or_else(|e| {
        tracing::debug!("no artwork for the media overlay: {e}");
        None
    });
    let programme = match target {
        Target::Channel(ch) => crate::play::guide::on_at(&conn, ch, crate::now_unix())
            .ok()
            .flatten()
            .map(|p| p.title),
        _ => None,
    };
    drop(conn);
    Item {
        private,
        artwork: url.and_then(|u| artwork_for(art, &u)),
        programme,
    }
}

/// The picture a title is shown with elsewhere in Aurora: a channel's logo (the viewer's
/// own first), a film's poster, an episode's show's poster before its still.
fn artwork_url(conn: &Connection, target: Target) -> aurora_db::rusqlite::Result<Option<String>> {
    let (sql, id) = match target {
        Target::Channel(id) => (
            "SELECT COALESCE(NULLIF(custom_logo, ''), logo) FROM channels WHERE id = ?1",
            id,
        ),
        Target::Movie(id) => ("SELECT poster FROM movies WHERE id = ?1", id),
        Target::Episode(id) => (
            "SELECT COALESCE(NULLIF(s.poster, ''), e.still)
               FROM episodes e JOIN series s ON s.id = e.series_id
              WHERE e.id = ?1",
            id,
        ),
        Target::Recording(id) => (
            "SELECT COALESCE(NULLIF(c.custom_logo, ''), c.logo)
               FROM recordings r JOIN channels c ON c.id = r.channel_id
              WHERE r.id = ?1",
            id,
        ),
    };
    Ok(conn
        .query_row(sql, [id], |r| r.get::<_, Option<String>>(0))
        .optional()?
        .flatten()
        .filter(|u| !u.trim().is_empty()))
}

fn artwork_for(art: &artwork::Cache, url: &str) -> Option<Artwork> {
    if art.contains(url) {
        return Some(Artwork::File(art.path_for(url)));
    }
    url.starts_with("https://")
        .then(|| Artwork::Url(url.to_string()))
}

#[cfg(test)]
mod tests;
