//! Playback, and keeping it alive (README §7.14, §23 item 4).
//!
//! Two jobs the raw backend cannot do on its own.
//!
//! The first is failover. A channel has several URLs and a provider hands out streams
//! that 404 on a Tuesday and stall on a Wednesday, so tuning tries them in order and
//! records what happened — which is what makes the *next* tune start from the one that
//! worked.
//!
//! The second is telling anyone. The backend only knows its state when asked, and the
//! UI's OSD is driven by a `player.state` event that nothing was emitting, so a stream
//! that died left the interface showing it merrily playing. `tick` is the heartbeat
//! that fixes both: it notices the change, reports it, and — for live TV — quietly
//! moves to the next source instead of leaving a dead picture up.

use std::sync::Arc;

use aurora_db::repo::sources;
use aurora_db::rusqlite::Connection;
use aurora_player::backend::LoadOptions;
use aurora_player::{PlayerBackend, PlayerState, PlayerStatus};
use parking_lot::Mutex;

use crate::error::{AppError, Result};

/// How many times a single tune may roll to another source before giving up.
///
/// Bounded because a provider having a total outage would otherwise spin through every
/// URL it owns for ever, and a stopped picture with an error on it is more honest than
/// an endless reconnect.
pub const MAX_ROLLOVERS: u32 = 6;

/// The live channel currently on, and where we are in its list of URLs.
#[derive(Debug, Clone)]
struct LiveSession {
    channel_id: i64,
    /// Source id and URL, best bet first at the time of tuning.
    candidates: Vec<(i64, String)>,
    /// Index into `candidates` of the one currently loaded.
    current: usize,
    options: LoadOptions,
    rollovers: u32,
}

pub struct Playback {
    db: Arc<Mutex<Connection>>,
    player: Arc<Mutex<Box<dyn PlayerBackend>>>,
    session: Mutex<Option<LiveSession>>,
    /// The last state reported, so `tick` can tell a change from a repeat.
    last: Mutex<Option<PlayerState>>,
}

impl Playback {
    pub fn new(db: Arc<Mutex<Connection>>, player: Arc<Mutex<Box<dyn PlayerBackend>>>) -> Self {
        Self {
            db,
            player,
            session: Mutex::new(None),
            last: Mutex::new(None),
        }
    }

    pub fn state(&self) -> PlayerState {
        self.player.lock().state()
    }

    /// Tune a live channel, trying its sources in order until one loads.
    pub fn play_live(&self, channel_id: i64, now: i64) -> Result<PlayerState> {
        let (sources, options) = {
            let db = self.db.lock();
            crate::window::live_sources(&db, channel_id, now)?
        };
        if sources.is_empty() {
            *self.session.lock() = None;
            return Err(AppError::Other(format!(
                "channel {channel_id} has no stream URL"
            )));
        }

        let candidates: Vec<(i64, String)> = sources.into_iter().map(|s| (s.id, s.url)).collect();
        let mut session = LiveSession {
            channel_id,
            candidates,
            current: 0,
            options,
            rollovers: 0,
        };

        let mut last_error = None;
        for index in 0..session.candidates.len() {
            session.current = index;
            match self.load_current(&session, now) {
                Ok(state) => {
                    *self.session.lock() = Some(session);
                    return Ok(state);
                }
                Err(e) => last_error = Some(e),
            }
        }

        *self.session.lock() = None;
        Err(last_error.unwrap_or_else(|| {
            AppError::Other(format!("nothing would play on channel {channel_id}"))
        }))
    }

    /// Play something that is not live. Ends any failover session: a film has one URL,
    /// and leaving the session up would have a dead VOD stream reconnect to a channel.
    pub fn play_item(
        &self,
        kind: &str,
        id: i64,
        position_secs: Option<f64>,
    ) -> Result<PlayerState> {
        let (url, options) = {
            let db = self.db.lock();
            crate::window::resolve_playback(&db, kind, id, position_secs)?
        };
        *self.session.lock() = None;
        let mut player = self.player.lock();
        player.load(&url, &options)?;
        Ok(player.state())
    }

    /// Play a past programme (README §7.5). Also not a failover session: catch-up is
    /// addressed by a time window, and rolling to another source would silently serve a
    /// different recording.
    pub fn play_catchup(
        &self,
        channel_id: i64,
        start: i64,
        stop: i64,
        now: i64,
    ) -> Result<PlayerState> {
        let (url, options) = {
            let db = self.db.lock();
            crate::window::resolve_catchup(&db, channel_id, start, stop, now)?
        };
        *self.session.lock() = None;
        let mut player = self.player.lock();
        player.load(&url, &options)?;
        Ok(player.state())
    }

    pub fn stop(&self) -> Result<PlayerState> {
        *self.session.lock() = None;
        let mut player = self.player.lock();
        player.stop()?;
        Ok(player.state())
    }

    /// One heartbeat. Returns the state when the UI should be told about it.
    ///
    /// Safe to call as often as you like; it only reports changes.
    pub fn tick(&self, now: i64) -> Option<PlayerState> {
        let state = self.player.lock().state();

        if state.status == PlayerStatus::Error && self.recover(now) {
            // A rollover happened, so the interesting state is the new one.
            let recovered = self.player.lock().state();
            *self.last.lock() = Some(recovered.clone());
            return Some(recovered);
        }

        let mut last = self.last.lock();
        if last.as_ref() == Some(&state) {
            return None;
        }
        *last = Some(state.clone());
        Some(state)
    }

    /// Load whichever candidate the session is pointing at, recording the outcome.
    fn load_current(&self, session: &LiveSession, now: i64) -> Result<PlayerState> {
        let (source_id, url) = session
            .candidates
            .get(session.current)
            .cloned()
            .ok_or_else(|| AppError::Other("no source to load".into()))?;

        let outcome = {
            let mut player = self.player.lock();
            player.load(&url, &session.options)
        };
        let db = self.db.lock();
        match outcome {
            Ok(()) => {
                let _ = sources::record_ok(&db, source_id, now);
                drop(db);
                Ok(self.player.lock().state())
            }
            Err(e) => {
                let _ = sources::record_failure(&db, source_id, now);
                Err(e.into())
            }
        }
    }

    /// A live stream died. Move to the next source, if there is one worth trying.
    ///
    /// Returns whether anything was actually attempted, so `tick` knows whether the
    /// error it saw is still the truth.
    fn recover(&self, now: i64) -> bool {
        let Some(mut session) = self.session.lock().clone() else {
            return false;
        };
        if session.rollovers >= MAX_ROLLOVERS {
            return false;
        }

        // Whatever was playing just stopped playing, so that is a failure against the
        // source, not just against this attempt.
        if let Some((source_id, _)) = session.candidates.get(session.current) {
            let db = self.db.lock();
            let _ = sources::record_failure(&db, *source_id, now);
        }

        let next = session.current + 1;
        if next >= session.candidates.len() {
            // Out of URLs: leave the error up rather than looping over the same dead
            // list. The next deliberate tune re-reads the order, by which time the
            // failures just recorded will have moved the good one to the front.
            *self.session.lock() = None;
            return false;
        }

        session.current = next;
        session.rollovers += 1;
        tracing::info!(
            "channel {} source {} failed, trying source {} of {}",
            session.channel_id,
            next,
            next + 1,
            session.candidates.len()
        );

        let loaded = self.load_current(&session, now);
        *self.session.lock() = Some(session);
        loaded.is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::markers::Chapter;
    use aurora_player::state::Aspect;
    use aurora_player::PlayerError;

    /// A backend that can be told which URLs refuse to load, and made to die
    /// mid-stream the way a real one does when a provider drops the connection.
    #[derive(Default)]
    struct FakeInner {
        loaded: Vec<String>,
        refuse: Vec<String>,
        state: PlayerState,
    }

    #[derive(Clone)]
    struct Fake(Arc<Mutex<FakeInner>>);

    impl Fake {
        fn new() -> Self {
            Self(Arc::new(Mutex::new(FakeInner::default())))
        }
        fn refuse(&self, url: &str) {
            self.0.lock().refuse.push(url.to_string());
        }
        fn loaded(&self) -> Vec<String> {
            self.0.lock().loaded.clone()
        }
        /// What a dropped connection looks like from the outside.
        fn die(&self) {
            self.0.lock().state.status = PlayerStatus::Error;
        }
        fn backend(&self) -> Box<dyn PlayerBackend> {
            Box::new(Self(Arc::clone(&self.0)))
        }
    }

    impl PlayerBackend for Fake {
        fn load(
            &mut self,
            url: &str,
            _options: &LoadOptions,
        ) -> std::result::Result<(), PlayerError> {
            let mut inner = self.0.lock();
            if inner.refuse.iter().any(|u| u == url) {
                return Err(PlayerError::Command(format!("refused {url}")));
            }
            inner.loaded.push(url.to_string());
            inner.state = PlayerState {
                status: PlayerStatus::Playing,
                is_live: true,
                ..Default::default()
            };
            Ok(())
        }
        fn stop(&mut self) -> std::result::Result<(), PlayerError> {
            self.0.lock().state = PlayerState::default();
            Ok(())
        }
        fn set_paused(&mut self, _: bool) -> std::result::Result<(), PlayerError> {
            Ok(())
        }
        fn seek(&mut self, _: f64, _: bool) -> std::result::Result<(), PlayerError> {
            Ok(())
        }
        fn set_volume(&mut self, _: u32) -> std::result::Result<(), PlayerError> {
            Ok(())
        }
        fn set_muted(&mut self, _: bool) -> std::result::Result<(), PlayerError> {
            Ok(())
        }
        fn set_speed(&mut self, _: f64) -> std::result::Result<(), PlayerError> {
            Ok(())
        }
        fn set_audio_track(&mut self, _: Option<i64>) -> std::result::Result<(), PlayerError> {
            Ok(())
        }
        fn set_subtitle_track(&mut self, _: Option<i64>) -> std::result::Result<(), PlayerError> {
            Ok(())
        }
        fn set_aspect(&mut self, _: Aspect) -> std::result::Result<(), PlayerError> {
            Ok(())
        }
        fn state(&self) -> PlayerState {
            self.0.lock().state.clone()
        }
        fn chapters(&self) -> Vec<Chapter> {
            Vec::new()
        }
        fn resize(&mut self, _: u32, _: u32) -> std::result::Result<(), PlayerError> {
            Ok(())
        }
    }

    const NOW: i64 = 1_760_000_000;

    fn harness(source_count: usize) -> (Playback, Fake, Arc<Mutex<Connection>>) {
        let conn = aurora_db::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO channels (id, provider_id, provider_key, name, match_key, last_seen_at)
             VALUES (1, 1, 'c1', 'BBC One', 'bbcone', 0)",
            [],
        )
        .unwrap();
        for i in 0..source_count {
            conn.execute(
                "INSERT INTO channel_sources (channel_id, url, priority) VALUES (1, ?1, ?2)",
                aurora_db::rusqlite::params![url(i), i as i64],
            )
            .unwrap();
        }
        let db = Arc::new(Mutex::new(conn));
        let fake = Fake::new();
        let player: Arc<Mutex<Box<dyn PlayerBackend>>> = Arc::new(Mutex::new(fake.backend()));
        (Playback::new(Arc::clone(&db), player), fake, db)
    }

    fn url(i: usize) -> String {
        format!("http://example.com/{i}.ts")
    }

    fn fail_count(db: &Arc<Mutex<Connection>>, source_id: i64) -> i64 {
        db.lock()
            .query_row(
                "SELECT fail_count FROM channel_sources WHERE id = ?1",
                [source_id],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn tuning_takes_the_first_source_and_records_that_it_worked() {
        let (playback, fake, db) = harness(3);
        let state = playback.play_live(1, NOW).unwrap();

        assert_eq!(state.status, PlayerStatus::Playing);
        assert_eq!(fake.loaded(), vec![url(0)]);
        let ok_at: Option<i64> = db
            .lock()
            .query_row(
                "SELECT last_ok_at FROM channel_sources WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ok_at, Some(NOW));
    }

    #[test]
    fn a_source_that_will_not_load_rolls_straight_to_the_next() {
        let (playback, fake, db) = harness(3);
        fake.refuse(&url(0));

        let state = playback.play_live(1, NOW).unwrap();
        assert_eq!(state.status, PlayerStatus::Playing);
        assert_eq!(
            fake.loaded(),
            vec![url(1)],
            "it moved on without being asked"
        );
        assert_eq!(fail_count(&db, 1), 1);
        assert_eq!(fail_count(&db, 2), 0);
    }

    #[test]
    fn when_nothing_will_play_the_failure_is_reported_not_swallowed() {
        let (playback, fake, db) = harness(2);
        fake.refuse(&url(0));
        fake.refuse(&url(1));

        assert!(playback.play_live(1, NOW).is_err());
        assert_eq!(fail_count(&db, 1), 1);
        assert_eq!(fail_count(&db, 2), 1);
        // No session left behind, so a later tick does not try to rescue a tune that
        // never happened.
        assert!(playback.tick(NOW).is_some());
        assert_eq!(fake.loaded().len(), 0);
    }

    #[test]
    fn a_channel_with_no_sources_says_so() {
        let (playback, _fake, _db) = harness(0);
        let err = playback.play_live(1, NOW).unwrap_err().to_string();
        assert!(err.contains("no stream URL"), "{err}");
    }

    #[test]
    fn a_stream_that_dies_mid_programme_rolls_over_by_itself() {
        // README §23 item 4: "survives a mid-stream disconnect without user
        // intervention" — nobody presses anything in this test.
        let (playback, fake, db) = harness(3);
        playback.play_live(1, NOW).unwrap();
        assert_eq!(fake.loaded(), vec![url(0)]);

        fake.die();
        let state = playback
            .tick(NOW + 30)
            .expect("the tick should report the recovery");

        assert_eq!(state.status, PlayerStatus::Playing);
        assert_eq!(fake.loaded(), vec![url(0), url(1)]);
        assert_eq!(fail_count(&db, 1), 1, "the source that died is marked down");
    }

    #[test]
    fn rolling_over_stops_when_the_sources_run_out() {
        let (playback, fake, _db) = harness(2);
        playback.play_live(1, NOW).unwrap();

        fake.die();
        playback.tick(NOW + 1); // rolls to source 2
        assert_eq!(fake.loaded(), vec![url(0), url(1)]);

        fake.die();
        playback.tick(NOW + 2); // nothing left
        assert_eq!(
            fake.loaded(),
            vec![url(0), url(1)],
            "an error stays up rather than looping over a dead list"
        );
        assert_eq!(playback.state().status, PlayerStatus::Error);
    }

    #[test]
    fn rollovers_are_bounded_even_with_a_long_source_list() {
        let (playback, fake, _db) = harness(40);
        playback.play_live(1, NOW).unwrap();

        for i in 0..(MAX_ROLLOVERS + 4) {
            fake.die();
            playback.tick(NOW + i64::from(i));
        }
        assert_eq!(
            fake.loaded().len() as u32,
            MAX_ROLLOVERS + 1,
            "one tune plus a bounded number of rollovers"
        );
    }

    #[test]
    fn the_next_tune_starts_from_whatever_worked_last_time() {
        let (playback, fake, _db) = harness(3);
        fake.refuse(&url(0));
        playback.play_live(1, NOW).unwrap();
        assert_eq!(fake.loaded(), vec![url(1)]);

        // Same channel again: the one that failed is still cooling down, so the one
        // that worked is now the first thing tried.
        playback.play_live(1, NOW + 1).unwrap();
        assert_eq!(fake.loaded(), vec![url(1), url(1)]);
    }

    #[test]
    fn a_film_is_not_a_failover_session() {
        let (playback, fake, db) = harness(3);
        playback.play_live(1, NOW).unwrap();
        db.lock()
            .execute(
                "INSERT INTO movies (id, provider_id, provider_key, title, match_key, url,
                                     last_seen_at)
                 VALUES (1,1,'m1','A Film','afilm','http://example.com/film.mkv',0)",
                [],
            )
            .unwrap();

        playback.play_item("movie", 1, None).unwrap();
        fake.die();
        playback.tick(NOW + 5);

        // A film has one URL. Rolling to a channel's next source here would start
        // playing something else entirely.
        assert_eq!(
            fake.loaded(),
            vec![url(0), "http://example.com/film.mkv".to_string()]
        );
    }

    #[test]
    fn stopping_ends_the_session() {
        let (playback, fake, _db) = harness(3);
        playback.play_live(1, NOW).unwrap();
        playback.stop().unwrap();

        fake.die();
        playback.tick(NOW + 5);
        assert_eq!(
            fake.loaded(),
            vec![url(0)],
            "a closed channel does not reconnect"
        );
    }

    #[test]
    fn the_heartbeat_reports_changes_and_stays_quiet_otherwise() {
        let (playback, _fake, _db) = harness(3);
        assert!(playback.tick(NOW).is_some(), "the first reading is news");
        assert!(playback.tick(NOW).is_none(), "nothing changed");

        playback.play_live(1, NOW).unwrap();
        assert!(playback.tick(NOW).is_some(), "now it is playing");
        assert!(playback.tick(NOW).is_none());
    }
}
