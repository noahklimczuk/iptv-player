//! What the player's controls need from the library: Skip Intro / Recap / Credits, Up
//! Next, the per-show switches, the picture's shape and the menus' values.
//!
//! Mirrors the Windows commands the player calls (`aurora-app/src/commands.rs`
//! `library_playback_aids`, `library_record_skip`, `library_observe_seek`,
//! `library_sync_chapters`, `library_series_prefs`, `library_set_series_prefs`) and
//! `aurora-app/src/community_markers.rs`. The rules themselves -- which markers win,
//! what counts as an intro, when Up Next appears, what a skip teaches the show -- are
//! the shared `aurora_core::markers` and `aurora_db::repo::markers` ones, called here
//! and never copied.
//!
//! One difference in shape: Windows reads a file's chapters from its own engine
//! (`engine().chapters()`). On Apple mpv is driven from Swift, so Swift reads the chapter
//! list and hands it over.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use aurora_core::markers::{self as rules, MarkerKind as CoreKind, MarkerSource as CoreSource};
use aurora_db::repo::{filtering, library, markers};
use aurora_db::rusqlite::Connection;
use aurora_ingest::http::HttpClient;
use aurora_ingest::introdb;
use aurora_video::Aspect;
use parking_lot::Mutex;

use crate::error::Result;
use crate::playback::MpvOption;
use crate::vod::Episode;
use crate::{now_unix, AuroraCore};

/// How long before the end the Up Next card appears when there is no credits marker.
/// `aurora-app/src/commands.rs` `UP_NEXT_TAIL_SECS`.
pub const UP_NEXT_TAIL_SECS: f64 = 45.0;
/// The Up Next countdown. `src-ui/.../UpNextCard.tsx` `COUNTDOWN_SECS`.
pub const UP_NEXT_COUNTDOWN_SECS: u32 = 10;
/// C13: 0.25x to 4x, what the engine accepts. `src-ui/.../model.ts` `SPEEDS`.
pub const SPEEDS: [f64; 10] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0, 4.0];
/// `src-ui/.../model.ts` `SLEEP_MINUTES` and `SLEEP_EXTEND_MINUTES`.
pub const SLEEP_MINUTES: [u32; 5] = [15, 30, 45, 60, 90];
pub const SLEEP_EXTEND_MINUTES: u32 = 30;
/// C1: the controls hide after this long without input. `model.ts` `HIDE_AFTER_MS`.
pub const HIDE_AFTER_MS: u32 = 3200;

/// The values the player's menus offer, so Swift draws Windows' choices rather than
/// its own.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PlayerMenus {
    pub speeds: Vec<f64>,
    pub sleep_minutes: Vec<u32>,
    pub sleep_extend_minutes: u32,
    pub up_next_countdown_secs: u32,
    pub hide_controls_after_ms: u32,
    pub aspects: Vec<AspectChoice>,
}

/// How the picture fills the screen. `aurora_video::Aspect`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, uniffi::Enum)]
pub enum PictureAspect {
    #[default]
    Auto,
    Wide,
    Standard,
    Cinema,
    Zoom,
    Stretch,
}

impl From<PictureAspect> for Aspect {
    fn from(a: PictureAspect) -> Self {
        match a {
            PictureAspect::Auto => Aspect::Auto,
            PictureAspect::Wide => Aspect::Wide,
            PictureAspect::Standard => Aspect::Standard,
            PictureAspect::Cinema => Aspect::Cinema,
            PictureAspect::Zoom => Aspect::Zoom,
            PictureAspect::Stretch => Aspect::Stretch,
        }
    }
}

/// One line of the Aspect menu. `model.ts` `ASPECTS`, in its order and words.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AspectChoice {
    pub aspect: PictureAspect,
    pub label: String,
    pub detail: String,
}

pub(crate) const ASPECTS: [(PictureAspect, &str, &str); 6] = [
    (PictureAspect::Auto, "Auto", "What the stream says"),
    (PictureAspect::Wide, "16:9", "Widescreen"),
    (PictureAspect::Standard, "4:3", "Older broadcasts"),
    (PictureAspect::Cinema, "21:9", "Cinematic"),
    (PictureAspect::Zoom, "Zoom", "Fill the screen, crop the edges"),
    (PictureAspect::Stretch, "Stretch", "Fill the screen, distort"),
];

#[uniffi::export]
pub fn player_menus() -> PlayerMenus {
    PlayerMenus {
        speeds: SPEEDS.to_vec(),
        sleep_minutes: SLEEP_MINUTES.to_vec(),
        sleep_extend_minutes: SLEEP_EXTEND_MINUTES,
        up_next_countdown_secs: UP_NEXT_COUNTDOWN_SECS,
        hide_controls_after_ms: HIDE_AFTER_MS,
        aspects: ASPECTS
            .iter()
            .map(|(aspect, label, detail)| AspectChoice {
                aspect: *aspect,
                label: (*label).into(),
                detail: (*detail).into(),
            })
            .collect(),
    }
}

/// The mpv properties that give the picture this shape: Windows' own
/// (`aurora_video::Aspect::mpv`), all of them every time, so going from Zoom to 4:3 does
/// not leave the zoom in force.
#[uniffi::export]
pub fn aspect_options(aspect: PictureAspect) -> Vec<MpvOption> {
    Aspect::from(aspect)
        .mpv()
        .iter()
        .map(|(name, value)| MpvOption::Text { name: (*name).into(), value: (*value).into() })
        .collect()
}

/// A speed the engine accepts: `aurora_video::Engine::set_speed`'s clamp.
#[uniffi::export]
pub fn clamp_speed(speed: f64) -> f64 {
    if speed.is_finite() {
        speed.clamp(0.25, 4.0)
    } else {
        1.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MarkerKind {
    Intro,
    Recap,
    Credits,
}

impl From<CoreKind> for MarkerKind {
    fn from(k: CoreKind) -> Self {
        match k {
            CoreKind::Intro => Self::Intro,
            CoreKind::Recap => Self::Recap,
            CoreKind::Credits => Self::Credits,
        }
    }
}

impl From<MarkerKind> for CoreKind {
    fn from(k: MarkerKind) -> Self {
        match k {
            MarkerKind::Intro => Self::Intro,
            MarkerKind::Recap => Self::Recap,
            MarkerKind::Credits => Self::Credits,
        }
    }
}

/// Where a marker came from. The player says so when the library learned it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MarkerSource {
    Chapters,
    User,
    Learned,
    Seek,
    Convention,
    Community,
}

impl From<CoreSource> for MarkerSource {
    fn from(s: CoreSource) -> Self {
        match s {
            CoreSource::Chapters => Self::Chapters,
            CoreSource::User => Self::User,
            CoreSource::Learned => Self::Learned,
            CoreSource::Seek => Self::Seek,
            CoreSource::Convention => Self::Convention,
            CoreSource::Community => Self::Community,
        }
    }
}

/// A stretch the viewer can skip.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SkipMarker {
    pub kind: MarkerKind,
    pub start_secs: f64,
    pub end_secs: f64,
    pub source: MarkerSource,
    /// "Skip Intro", "Skip Recap", "Skip Credits".
    pub label: String,
    /// Whether "always skip" may jump over it without being asked. Decided here, by
    /// `MarkerSource::may_auto_skip`, so Swift does not keep a second copy of the rule
    /// (Windows' page does, and has had to explain why it must match).
    pub may_auto_skip: bool,
}

impl From<rules::SkipMarker> for SkipMarker {
    fn from(m: rules::SkipMarker) -> Self {
        Self {
            kind: m.kind.into(),
            start_secs: m.start_secs,
            end_secs: m.end_secs,
            source: m.source.into(),
            label: m.kind.label().into(),
            may_auto_skip: m.source.may_auto_skip(),
        }
    }
}

/// A show's switches, per profile. Autoplay on and auto-skip off until asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct SeriesPrefs {
    pub always_skip_intro: bool,
    pub always_skip_recap: bool,
    pub autoplay_next: bool,
}

impl From<markers::SeriesPrefs> for SeriesPrefs {
    fn from(p: markers::SeriesPrefs) -> Self {
        Self {
            always_skip_intro: p.always_skip_intro,
            always_skip_recap: p.always_skip_recap,
            autoplay_next: p.autoplay_next,
        }
    }
}

impl From<SeriesPrefs> for markers::SeriesPrefs {
    fn from(p: SeriesPrefs) -> Self {
        Self {
            always_skip_intro: p.always_skip_intro,
            always_skip_recap: p.always_skip_recap,
            autoplay_next: p.autoplay_next,
        }
    }
}

/// Everything Skip and Up Next need for the episode playing.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PlaybackAids {
    pub markers: Vec<SkipMarker>,
    /// Where the Up Next card appears, when the episode's length is known from anywhere.
    pub up_next_at_secs: Option<f64>,
    pub next_episode: Option<Episode>,
    /// The show, for its switches. None for an episode the library can't place.
    pub series_id: Option<i64>,
    pub prefs: SeriesPrefs,
}

/// A chapter in the loaded file, as mpv lists it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ChapterMark {
    pub title: Option<String>,
    pub start_secs: f64,
}

#[uniffi::export]
impl AuroraCore {
    /// Skip markers, Up Next and the show's switches for an episode
    /// (`library_playback_aids`). `duration_secs` is what the player knows, routinely 0
    /// on a provider's stream: then the episode's own runtime is used, as on Windows.
    ///
    /// May go to TheIntroDB, once per episode per session, before taking the library.
    pub fn playback_aids(
        &self,
        profile_id: i64,
        episode_id: i64,
        duration_secs: f64,
    ) -> Result<PlaybackAids> {
        let community_key = community_key(&self.db.lock(), episode_id)?;
        let community_duration = (duration_secs > 0.0).then_some(duration_secs);
        let community = community_key
            .map(|key| community_markers(key, community_duration))
            .unwrap_or_default();

        let db = self.db.lock();
        let duration = if duration_secs > 0.0 {
            Some(duration_secs)
        } else {
            library::episode_runtime_secs(&db, episode_id)?
        };
        // The file's chapters and the viewer's own presses still win; TheIntroDB outranks
        // anything inferred from other episodes and the clock (`MarkerSource::trust`).
        let resolved = rules::merge(&[
            markers::resolve(&db, episode_id, markers::DEFAULT_MIN_SAMPLES, duration)?,
            community,
        ]);
        let series_id = markers::series_id_of(&db, episode_id)?;
        let prefs = match series_id {
            Some(series_id) => markers::prefs(&db, profile_id, series_id)?,
            None => markers::SeriesPrefs::default(),
        };
        // Across every copy of the show, as its episode list is: a season from one copy
        // runs out and the next one belongs to another row.
        let group = match series_id {
            Some(series_id) => filtering::LibraryFilter::load(&db)?.group_of(
                &db,
                filtering::Kind::Series,
                series_id,
            )?,
            None => Vec::new(),
        };
        Ok(PlaybackAids {
            up_next_at_secs: rules::up_next_at(
                &resolved,
                duration.unwrap_or(0.0),
                UP_NEXT_TAIL_SECS,
            ),
            markers: resolved.into_iter().map(SkipMarker::from).collect(),
            next_episode: library::following_episode_in(&db, episode_id, &group)?
                .map(Episode::from),
            series_id,
            prefs: prefs.into(),
        })
    }

    /// Store the markers this file's own chapters give (`library_sync_chapters`).
    /// Call once the file is open -- before that mpv has no chapter list -- and before
    /// `playback_aids`, which reports only what is stored.
    pub fn sync_chapters(
        &self,
        episode_id: i64,
        chapters: Vec<ChapterMark>,
        duration_secs: f64,
    ) -> Result<u32> {
        let chapters: Vec<rules::Chapter> = chapters
            .into_iter()
            .map(|c| rules::Chapter { title: c.title, start_secs: c.start_secs })
            .collect();
        let derived = rules::from_chapters(&chapters, duration_secs);
        let mut db = self.db.lock();
        Ok(markers::set_chapter_markers(&mut db, episode_id, &derived, now_unix())? as u32)
    }

    /// The viewer pressed Skip: remember where this show's intro (or recap, or credits)
    /// is, so later episodes offer it (`library_record_skip`). An accidental nudge, or a
    /// stretch that sits where no intro could, teaches nothing.
    pub fn record_skip(
        &self,
        episode_id: i64,
        kind: MarkerKind,
        start_secs: f64,
        end_secs: f64,
        duration_secs: f64,
    ) -> Result<()> {
        let marker = rules::SkipMarker::new(kind.into(), start_secs, end_secs, CoreSource::User);
        if !marker.is_plausible() || !rules::position_fits_kind(&marker, duration_secs) {
            return Ok(());
        }
        markers::record(&self.db.lock(), episode_id, &marker, now_unix())?;
        Ok(())
    }

    /// A forward jump the viewer made by hand, offered as evidence of an intro
    /// (`library_observe_seek`). True when it was taken, so the caller asks for the
    /// markers again and the button appears for the rest of this episode too.
    pub fn observe_seek(
        &self,
        episode_id: i64,
        from_secs: f64,
        to_secs: f64,
        duration_secs: f64,
    ) -> Result<bool> {
        let db = self.db.lock();
        let known = markers::resolve(
            &db,
            episode_id,
            markers::DEFAULT_MIN_SAMPLES,
            (duration_secs > 0.0).then_some(duration_secs),
        )?;
        let Some(marker) = rules::seek_evidence(from_secs, to_secs, duration_secs, &known) else {
            return Ok(false);
        };
        markers::record(&db, episode_id, &marker, now_unix())?;
        Ok(true)
    }

    pub fn series_prefs(&self, profile_id: i64, series_id: i64) -> Result<SeriesPrefs> {
        Ok(markers::prefs(&self.db.lock(), profile_id, series_id)?.into())
    }

    pub fn set_series_prefs(
        &self,
        profile_id: i64,
        series_id: i64,
        prefs: SeriesPrefs,
    ) -> Result<()> {
        markers::set_prefs(&self.db.lock(), profile_id, series_id, &prefs.into(), now_unix())?;
        Ok(())
    }
}

// ── TheIntroDB (`aurora-app/src/community_markers.rs`) ────────────────────────────
//
// Asked once per episode per session and kept: the aids are asked for several times as
// an episode opens. The body is kept rather than the markers, because an open-ended
// credits segment can only be placed once the runtime is known. Never written to the
// library. A failure is remembered for ten minutes so a service that is down does not
// cost every call its timeout.

const RETRY_FAILED_AFTER: Duration = Duration::from_secs(600);

/// The show's TMDB id, season and episode.
type CommunityKey = (i64, u16, u16);

enum Entry {
    Answered(Option<String>),
    Failed(Instant),
}

fn cache() -> &'static Mutex<HashMap<CommunityKey, Entry>> {
    static CACHE: OnceLock<Mutex<HashMap<CommunityKey, Entry>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn client() -> Option<&'static HttpClient> {
    static CLIENT: OnceLock<Option<HttpClient>> = OnceLock::new();
    CLIENT
        .get_or_init(|| match introdb::client() {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::warn!("TheIntroDB client would not build: {}", e.message);
                None
            }
        })
        .as_ref()
}

fn community_key(conn: &Connection, episode_id: i64) -> Result<Option<CommunityKey>> {
    use aurora_db::rusqlite::{params, OptionalExtension};
    Ok(conn
        .query_row(
            "SELECT s.tmdb_id, e.season, e.episode
               FROM episodes e JOIN series s ON s.id = e.series_id
              WHERE e.id = ?1 AND s.tmdb_id IS NOT NULL",
            params![episode_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?)
}

/// Must be called without the library lock held: it may go to the network.
fn community_markers(key: CommunityKey, duration_secs: Option<f64>) -> Vec<rules::SkipMarker> {
    let cached = match cache().lock().get(&key) {
        Some(Entry::Answered(body)) => Some(body.clone()),
        Some(Entry::Failed(at)) if at.elapsed() < RETRY_FAILED_AFTER => return Vec::new(),
        _ => None,
    };
    let body = match cached {
        Some(body) => body,
        None => {
            let Some(client) = client() else {
                return Vec::new();
            };
            let (tmdb_id, season, episode) = key;
            match client.fetch_string(&introdb::url(tmdb_id, season, episode)) {
                Ok(body) => {
                    cache().lock().insert(key, Entry::Answered(Some(body.clone())));
                    Some(body)
                }
                Err(e) if e.code == aurora_core::neterr::ErrorCode::NotFound => {
                    cache().lock().insert(key, Entry::Answered(None));
                    None
                }
                Err(e) => {
                    tracing::info!(?key, "TheIntroDB did not answer: {}", e.message);
                    cache().lock().insert(key, Entry::Failed(Instant::now()));
                    None
                }
            }
        }
    };
    body.map(|b| introdb::parse(&b, duration_secs)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A series of `n` episodes in season 1 with no TMDB match (so nothing goes to
    /// TheIntroDB), returning the core and the episode ids in order.
    fn seeded(n: u16) -> (std::sync::Arc<AuroraCore>, tempfile::TempDir, Vec<i64>) {
        use aurora_db::repo::library::{upsert_episodes, upsert_series, NewEpisode, NewSeries};
        let (core, dir) = crate::testing::core();
        let ids = {
            let mut db = core.db.lock();
            db.execute(
                "INSERT INTO providers (id,name,kind,base_url,created_at)
                 VALUES (1,'P','m3u','https://example.com',0)",
                [],
            )
            .unwrap();
            db.execute("INSERT OR IGNORE INTO profiles (id,name,created_at) VALUES (1,'Me',0)", [])
                .unwrap();
            let sid = upsert_series(
                &mut db,
                1,
                &NewSeries { provider_key: "s", title: "Show", match_key: "show", ..Default::default() },
                0,
            )
            .unwrap();
            let eps: Vec<NewEpisode> = (1..=n)
                .map(|e| NewEpisode {
                    season: 1,
                    episode: e,
                    title: Some(format!("Episode {e}")),
                    url: format!("https://example.com/{e}.mkv"),
                    runtime_mins: Some(45),
                    ..Default::default()
                })
                .collect();
            upsert_episodes(&mut db, sid, &eps, 0).unwrap();
            library::episodes_for(&db, sid, None).unwrap().into_iter().map(|e| e.id).collect()
        };
        (core, dir, ids)
    }

    #[test]
    fn the_menus_carry_windows_values() {
        let m = player_menus();
        assert_eq!(m.speeds.first(), Some(&0.25));
        assert_eq!(m.speeds.last(), Some(&4.0));
        assert!(m.speeds.contains(&1.0));
        assert_eq!(m.aspects.len(), 6);
        assert_eq!(m.aspects[0].label, "Auto");
        assert_eq!(clamp_speed(9.0), 4.0);
        assert_eq!(clamp_speed(f64::NAN), 1.0);
    }

    #[test]
    fn every_aspect_sets_all_three_properties() {
        for (aspect, ..) in ASPECTS {
            let names: Vec<String> =
                aspect_options(aspect).iter().map(|o| o.name().to_string()).collect();
            assert_eq!(names, ["video-aspect-override", "keepaspect", "panscan"], "{aspect:?}");
        }
        assert!(aspect_options(PictureAspect::Zoom)
            .contains(&MpvOption::Text { name: "panscan".into(), value: "1".into() }));
    }

    #[test]
    fn an_episode_gets_credits_up_next_and_the_following_episode_from_its_runtime() {
        let (core, _dir, ids) = seeded(3);
        // No duration from the player, as on most provider streams: the stored runtime
        // (45 minutes) places the conventional credits and Up Next.
        let aids = core.playback_aids(1, ids[0], 0.0).unwrap();
        assert_eq!(aids.next_episode.as_ref().map(|e| e.id), Some(ids[1]));
        assert!(aids.series_id.is_some());
        assert!(aids.up_next_at_secs.is_some_and(|t| t > 0.0 && t < 2700.0));
        assert!(aids.markers.iter().all(|m| !m.may_auto_skip || m.source != MarkerSource::Convention));
        // Autoplay on and auto-skip off until asked.
        assert_eq!(
            aids.prefs,
            SeriesPrefs { always_skip_intro: false, always_skip_recap: false, autoplay_next: true }
        );
        // The last episode has nothing after it.
        assert!(core.playback_aids(1, ids[2], 0.0).unwrap().next_episode.is_none());
    }

    #[test]
    fn a_pressed_skip_is_offered_on_the_next_episode_once_it_agrees_with_another() {
        let (core, _dir, ids) = seeded(4);
        let duration = 2700.0;
        // An intro at 60-120s, pressed on two episodes: the show learns it.
        core.record_skip(ids[0], MarkerKind::Intro, 60.0, 120.0, duration).unwrap();
        core.record_skip(ids[1], MarkerKind::Intro, 61.0, 121.0, duration).unwrap();
        let aids = core.playback_aids(1, ids[2], duration).unwrap();
        let intro = aids
            .markers
            .iter()
            .find(|m| m.kind == MarkerKind::Intro)
            .expect("a learned intro on a later episode");
        assert_eq!(intro.source, MarkerSource::Learned);
        assert!(intro.may_auto_skip);
        assert_eq!(intro.label, "Skip Intro");
        // An accidental one-second press teaches nothing.
        core.record_skip(ids[3], MarkerKind::Recap, 10.0, 11.0, duration).unwrap();
        assert!(!core
            .playback_aids(1, ids[3], duration)
            .unwrap()
            .markers
            .iter()
            .any(|m| m.kind == MarkerKind::Recap));
    }

    #[test]
    fn chapters_named_intro_become_a_marker_on_that_episode() {
        let (core, _dir, ids) = seeded(1);
        let stored = core
            .sync_chapters(
                ids[0],
                vec![
                    ChapterMark { title: Some("Cold open".into()), start_secs: 0.0 },
                    ChapterMark { title: Some("Intro".into()), start_secs: 90.0 },
                    ChapterMark { title: Some("Episode".into()), start_secs: 150.0 },
                ],
                2700.0,
            )
            .unwrap();
        assert_eq!(stored, 1);
        let aids = core.playback_aids(1, ids[0], 2700.0).unwrap();
        let intro = aids.markers.iter().find(|m| m.kind == MarkerKind::Intro).unwrap();
        assert_eq!((intro.start_secs, intro.end_secs), (90.0, 150.0));
        assert_eq!(intro.source, MarkerSource::Chapters);
    }

    #[test]
    fn a_hand_seek_past_the_titles_offers_a_button_but_never_auto_skips() {
        let (core, _dir, ids) = seeded(1);
        assert!(core.observe_seek(ids[0], 40.0, 110.0, 2700.0).unwrap());
        let aids = core.playback_aids(1, ids[0], 2700.0).unwrap();
        let intro = aids.markers.iter().find(|m| m.kind == MarkerKind::Intro).unwrap();
        assert_eq!(intro.source, MarkerSource::Seek);
        assert!(!intro.may_auto_skip, "an uncorroborated drag must never jump by itself");
        // Backwards is not evidence.
        assert!(!core.observe_seek(ids[0], 300.0, 100.0, 2700.0).unwrap());
    }

    #[test]
    fn a_shows_switches_are_kept_per_profile() {
        let (core, _dir, _ids) = seeded(1);
        core.db.lock().execute("INSERT INTO profiles (id,name,created_at) VALUES (2,'You',0)", []).unwrap();
        let on = SeriesPrefs { always_skip_intro: true, always_skip_recap: false, autoplay_next: false };
        let series = core.playback_aids(1, _ids[0], 0.0).unwrap().series_id.unwrap();
        core.set_series_prefs(1, series, on).unwrap();
        assert_eq!(core.series_prefs(1, series).unwrap(), on);
        assert_eq!(core.playback_aids(1, _ids[0], 0.0).unwrap().prefs, on);
        let other = core.series_prefs(2, series).unwrap();
        assert!(other.autoplay_next && !other.always_skip_intro);
    }
}
