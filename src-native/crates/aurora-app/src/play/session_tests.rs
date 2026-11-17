//! The session against a fake mpv and an in-memory library (docs/PLAYER.md H1–H12).

use std::sync::Arc;
use std::time::{Duration, Instant};

use aurora_core::neterr::ErrorAction;
use aurora_db::rusqlite::Connection;
use aurora_ingest::credentials::MemoryStore;
use aurora_video::fake::FakeMpv;
use aurora_video::{Engine, Node, PlayerState, Status};

use super::*;
use crate::dblock::TimedMutex;
use crate::play::recovery::BACKOFF_SECS;

const LIVE_A: &str = "http://provider.test/live/a.ts";
const LIVE_B: &str = "http://provider.test/live/b.ts";
const FILM: &str = "http://provider.test/movie/film.mkv";

struct Rig {
    session: Arc<Session>,
    mpv: FakeMpv,
    db: Arc<TimedMutex<Connection>>,
}

fn library() -> Connection {
    let conn = aurora_db::open_memory().unwrap();
    conn.execute_batch(&format!(
        "INSERT INTO providers (id,name,kind,base_url,created_at,user_agent,referrer)
           VALUES (1,'P','m3u','https://provider.test',0,'VLC/3.0.20',' ');
         INSERT INTO channels (id,provider_id,provider_key,name,match_key,last_seen_at)
           VALUES (1,1,'one','One Source','one source',0),
                  (2,1,'two','Two Sources','two sources',0);
         INSERT INTO channel_sources (channel_id,url,priority) VALUES
           (1,'{LIVE_A}',0), (2,'{LIVE_A}',0), (2,'{LIVE_B}',1);
         INSERT INTO movies (id,provider_id,provider_key,title,match_key,url,last_seen_at)
           VALUES (1,1,'m1','A Film','a film','{FILM}',0);"
    ))
    .unwrap();
    conn
}

fn rig() -> Rig {
    let mpv = FakeMpv::new();
    let engine = Arc::new(Engine::start(mpv.driver()));
    let db = Arc::new(TimedMutex::new(library()));
    let data = std::env::temp_dir().join(format!("aurora-session-{}", std::process::id()));
    let session = Session::new(engine, db.clone(), Arc::new(MemoryStore::default()), data);
    Rig { session, mpv, db }
}

impl Rig {
    /// The addresses mpv was asked to open, in order.
    fn opened(&self) -> Vec<String> {
        self.mpv
            .commands()
            .into_iter()
            .filter(|c| c.first().map(String::as_str) == Some("loadfile"))
            .map(|c| c[1].clone())
            .collect()
    }

    fn until(&self, what: &str, f: impl Fn(&PlayerState) -> bool) -> PlayerState {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let s = self.session.state();
            if f(&s) {
                return s;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}: {s:#?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Wait until `n` plays and stops have taken their generation: a tune on another
    /// thread has started and is now stuck on whatever the test is holding. Sleeping
    /// for it instead lost the race on a loaded machine.
    fn taken(&self, n: u64) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.session.generation.load(Ordering::SeqCst) < n {
            assert!(Instant::now() < deadline, "no tune took generation {n}");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn plays(&self, position: f64) {
        self.mpv.plays();
        self.mpv.prop("time-pos", Node::Double(position));
        self.until("playing", |s| {
            s.report.status == Status::Playing && (s.report.position_secs - position).abs() < 0.01
        });
    }

    /// The current file fails, and the engine has said so.
    fn dies(&self, why: &str) {
        self.mpv.fail(why);
        self.until("the failure", |s| {
            s.reconnecting || s.report.status == Status::Error
        });
    }

    fn fail_count(&self, url: &str) -> i64 {
        self.db
            .lock()
            .query_row(
                "SELECT COALESCE(SUM(fail_count),0) FROM channel_sources WHERE url = ?1",
                [url],
                |r| r.get(0),
            )
            .unwrap()
    }
}

#[test]
fn h1_a_channel_plays_under_its_name_with_the_provider_user_agent() {
    let r = rig();
    let s = r.session.play(What::Channel(1), None).unwrap();
    assert_eq!(s.title.as_deref(), Some("One Source"));
    assert_eq!(s.channel_id, Some(1));
    assert_eq!(s.item_kind, Some(aurora_video::ItemKind::Live));
    assert_eq!(r.opened(), vec![LIVE_A]);
    assert_eq!(r.mpv.last_set("user-agent").as_deref(), Some("VLC/3.0.20"));
    // A Referer of only spaces is no Referer.
    assert_eq!(r.mpv.last_set("referrer").as_deref(), Some(""));
}

#[test]
fn h1_a_film_starts_where_it_was_left() {
    let r = rig();
    r.session.play(What::Movie(1), Some(1234.0)).unwrap();
    assert_eq!(r.opened(), vec![FILM]);
    assert_eq!(r.mpv.last_set("start").as_deref(), Some("1234.000"));
}

#[test]
fn h2_something_no_longer_in_the_library_says_so_and_stops_the_old_picture() {
    let r = rig();
    r.session.play(What::Channel(1), None).unwrap();
    r.plays(10.0);
    let e = r
        .session
        .play(What::Movie(99), None)
        .unwrap_err()
        .to_string();
    assert!(e.contains("no longer in your library"), "{e}");
    let s = r.until("stopped", |s| s.report.status == Status::Idle);
    assert_eq!(s.title, None);
}

#[test]
fn h4_a_later_tune_wins_however_slowly_the_earlier_one_resolves() {
    let r = rig();
    // Hold the library so both tunes are stuck resolving, then let them race.
    let held = r.db.lock();
    let first = {
        let s = r.session.clone();
        std::thread::spawn(move || s.play(What::Channel(1), None))
    };
    r.taken(1);
    let second = {
        let s = r.session.clone();
        std::thread::spawn(move || s.play(What::Movie(1), None))
    };
    r.taken(2);
    drop(held);
    let first = first.join().unwrap();
    let second = second.join().unwrap();
    assert!(matches!(first, Err(AppError::Superseded)), "{first:?}");
    assert_eq!(second.unwrap().title.as_deref(), Some("A Film"));
    assert_eq!(r.opened(), vec![FILM], "the replaced tune still loaded");
}

#[test]
fn h4_the_same_play_pressed_again_while_it_opens_does_not_reopen_it() {
    // Space on the Resume button that still had focus sent this five times in ten
    // seconds on the real line, and each one restarted the stream.
    let r = rig();
    r.session.play(What::Movie(1), Some(43.0)).unwrap();
    r.until("loading", |s| s.report.status == Status::Loading);
    for _ in 0..4 {
        let s = r.session.play(What::Movie(1), Some(43.0)).unwrap();
        assert_eq!(s.title.as_deref(), Some("A Film"));
    }
    assert_eq!(r.opened(), vec![FILM], "a repeat press reopened the stream");
    r.plays(43.0);
}

#[test]
fn h4_the_same_play_again_still_replaces_another_tune_in_flight() {
    // Film opening, then a channel asked for (stuck resolving), then the film's Play
    // pressed again: the film was asked for last, so the channel must not load.
    let r = rig();
    r.session.play(What::Movie(1), None).unwrap();
    r.until("loading", |s| s.report.status == Status::Loading);
    let held = r.db.lock();
    let channel = {
        let s = r.session.clone();
        std::thread::spawn(move || s.play(What::Channel(1), None))
    };
    r.taken(2);
    let again = r.session.play(What::Movie(1), None).unwrap();
    assert_eq!(again.title.as_deref(), Some("A Film"));
    drop(held);
    assert!(matches!(channel.join().unwrap(), Err(AppError::Superseded)));
    assert_eq!(r.opened(), vec![FILM]);
}

#[test]
fn h4_another_start_point_while_it_opens_is_a_new_request() {
    let r = rig();
    r.session.play(What::Movie(1), Some(43.0)).unwrap();
    r.until("loading", |s| s.report.status == Status::Loading);
    r.session.play(What::Movie(1), Some(600.0)).unwrap();
    assert_eq!(r.opened(), vec![FILM, FILM]);
    assert_eq!(r.mpv.last_set("start").as_deref(), Some("600.000"));
}

#[test]
fn h4_playing_it_again_once_it_plays_still_reloads() {
    let r = rig();
    r.session.play(What::Movie(1), None).unwrap();
    r.plays(10.0);
    r.session.play(What::Movie(1), None).unwrap();
    assert_eq!(r.opened(), vec![FILM, FILM]);
}

#[test]
fn h4_pressing_play_while_it_reconnects_reopens_it_now() {
    let r = rig();
    r.session.play(What::Movie(1), Some(43.0)).unwrap();
    r.plays(50.0);
    r.dies("connection refused");
    let before = r.opened().len();
    r.session.play(What::Movie(1), Some(43.0)).unwrap();
    assert!(r.opened().len() > before, "Retry did nothing");
}

#[test]
fn h4_stopping_during_a_slow_tune_leaves_the_player_stopped() {
    let r = rig();
    let held = r.db.lock();
    let tune = {
        let s = r.session.clone();
        std::thread::spawn(move || s.play(What::Channel(1), None))
    };
    r.taken(1);
    r.session.stop().unwrap();
    drop(held);
    assert!(matches!(tune.join().unwrap(), Err(AppError::Superseded)));
    assert!(r.opened().is_empty());
    assert_eq!(r.session.state().title, None);
}

#[test]
fn h5_a_source_that_plays_is_remembered() {
    let r = rig();
    r.session.play(What::Channel(1), None).unwrap();
    r.plays(10.0);
    r.session.tick(100.0);
    let ok: Option<i64> =
        r.db.lock()
            .query_row(
                "SELECT last_ok_at FROM channel_sources WHERE channel_id = 1",
                [],
                |x| x.get(0),
            )
            .unwrap();
    assert!(ok.is_some());
}

#[test]
fn h6_a_source_that_dies_rolls_straight_to_the_next() {
    let r = rig();
    r.session.play(What::Channel(2), None).unwrap();
    r.plays(10.0);
    r.dies("HTTP error 503 Service Unavailable");
    r.session.tick(100.0);
    assert_eq!(r.opened(), vec![LIVE_A, LIVE_B]);
    assert_eq!(r.fail_count(LIVE_A), 1);
}

#[test]
fn h6_a_source_mpv_refuses_outright_rolls_to_the_next_inside_the_tune() {
    let r = rig();
    r.mpv.refuse_loads(Some("loading failed"));
    let _ = r.session.play(What::Channel(2), None);
    // Both were asked for; neither opened, so a backoff is now being waited out.
    assert_eq!(r.fail_count(LIVE_A) + r.fail_count(LIVE_B), 2);
    let s = r.session.state();
    assert!(s.reconnecting, "{s:#?}");
    assert_eq!(s.report.error, None);
}

#[test]
fn h7_and_h12_a_single_source_waits_out_its_backoff_and_says_it_is_reconnecting() {
    let r = rig();
    r.session.play(What::Channel(1), None).unwrap();
    r.plays(10.0);
    r.dies("HTTP error 503 Service Unavailable");
    r.session.tick(100.0);
    let s = r.session.state();
    assert!(s.reconnecting);
    assert_eq!(s.report.status, Status::Buffering);
    assert_eq!(
        s.report.error, None,
        "an error was shown while still retrying"
    );
    assert_eq!(r.opened().len(), 1, "reloaded before the backoff");
    r.session.tick(100.0 + BACKOFF_SECS[0]);
    assert_eq!(r.opened(), vec![LIVE_A, LIVE_A]);
}

#[test]
fn h9_a_failure_counts_once_however_often_it_is_looked_at() {
    let r = rig();
    r.session.play(What::Channel(1), None).unwrap();
    r.plays(10.0);
    r.dies("HTTP error 503 Service Unavailable");
    for t in 0..10 {
        r.session.tick(100.0 + f64::from(t) * 0.05);
    }
    assert_eq!(r.fail_count(LIVE_A), 1);
}

#[test]
fn h8_a_film_whose_stream_dies_is_reloaded_where_it_died() {
    let r = rig();
    r.session.play(What::Movie(1), None).unwrap();
    r.plays(2400.0);
    r.session.tick(50.0);
    r.dies("stream ended unexpectedly: connection reset");
    r.session.tick(51.0);
    r.session.tick(51.0 + BACKOFF_SECS[0]);
    assert_eq!(r.opened(), vec![FILM, FILM]);
    assert_eq!(r.mpv.last_set("start").as_deref(), Some("2400.000"));
}

#[test]
fn h11_a_film_that_is_not_there_is_reported_with_only_actions_that_work() {
    let r = rig();
    r.session.play(What::Movie(1), None).unwrap();
    r.dies("HTTP error 404 Not Found");
    r.session.tick(10.0);
    let s = r.until("the error", |s| s.report.status == Status::Error);
    let e = s.report.error.expect("an error");
    assert!(!e.actions.contains(&ErrorAction::TryAnotherSource), "{e:?}");
    assert!(!e.actions.contains(&ErrorAction::ReportBroken), "{e:?}");
    assert!(!s.reconnecting);
    assert_eq!(r.opened().len(), 1, "a 404 was asked for twice");
}

#[test]
fn stopping_forgets_what_was_playing() {
    let r = rig();
    r.session.play(What::Channel(1), None).unwrap();
    r.plays(10.0);
    let s = r.session.stop().unwrap();
    assert_eq!(s.title, None);
    assert_eq!(s.report.status, Status::Idle);
    // And recovery has nothing to recover.
    r.session.tick(1000.0);
    assert_eq!(r.opened().len(), 1);
}

#[test]
fn every_state_is_published_to_the_page() {
    let r = rig();
    let seen = Arc::new(parking_lot::Mutex::new(Vec::<PlayerState>::new()));
    let sink = seen.clone();
    r.session
        .on_state(Box::new(move |s| sink.lock().push(s.clone())));
    r.session.play(What::Channel(1), None).unwrap();
    r.plays(10.0);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !seen
        .lock()
        .iter()
        .any(|s| s.report.status == Status::Playing)
    {
        assert!(Instant::now() < deadline, "playing was never published");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(seen
        .lock()
        .iter()
        .all(|s| s.title.as_deref() == Some("One Source")));
}
