//! The engine on the real libmpv. Windows only, and ignored by default: it needs
//! `libmpv-2.dll` on the path and a short local video in `AURORA_TEST_VIDEO`.
//!
//! cargo test -p aurora-video --test real_mpv -- --ignored
#![cfg(windows)]

use std::time::{Duration, Instant};

use aurora_video::libmpv::{Config, LibMpv};
use aurora_video::{Engine, Report, Request, Seek, Status};

fn until(engine: &Engine, what: &str, secs: u64, f: impl Fn(&Report) -> bool) -> Report {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let r = engine.report();
        if f(&r) {
            return r;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: {r:#?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn engine() -> Engine {
    let mpv = LibMpv::open(&Config {
        wid: None,
        headless: true,
    })
    .expect("libmpv");
    Engine::start(mpv)
}

fn video() -> String {
    std::env::var("AURORA_TEST_VIDEO").expect("AURORA_TEST_VIDEO names a short local video")
}

#[test]
#[ignore]
fn a_local_film_plays_seeks_pauses_and_gives_a_frame() {
    let e = engine();
    e.load(Request {
        url: video(),
        cache_secs: 10,
        ..Request::default()
    })
    .unwrap();
    let r = until(&e, "playing with a picture", 20, |r| {
        r.status == Status::Playing && r.has_picture
    });
    assert!(r.duration_secs > 1.0, "{r:?}");
    assert!(!r.audio_tracks.is_empty() || r.duration_secs > 0.0);
    let half = r.duration_secs / 2.0;
    e.seek(Seek::To(half)).unwrap();
    until(&e, "seeked", 10, |r| {
        (r.position_secs - half).abs() < 3.0 && r.status == Status::Playing
    });
    e.set_paused(true).unwrap();
    until(&e, "paused", 5, |r| r.status == Status::Paused);
    let f = e.grab_frame(160).expect("a frame");
    assert!(f.width <= 160 && f.width > 0 && f.rgb.len() == (f.width * f.height * 3) as usize);
    e.set_paused(false).unwrap();
    e.set_volume(55).unwrap();
    until(&e, "volume", 5, |r| {
        r.volume == 55 && r.status == Status::Playing
    });
    e.stop().unwrap();
    until(&e, "idle", 5, |r| {
        r.status == Status::Idle && !r.has_picture
    });
    e.shutdown();
}

#[test]
#[ignore]
fn nothing_listening_is_an_error_with_a_reason() {
    let e = engine();
    e.load(Request {
        url: "http://127.0.0.1:9/live/x.ts".into(),
        live: true,
        cache_secs: 2,
        ..Request::default()
    })
    .unwrap();
    let r = until(&e, "error", 30, |r| r.status == Status::Error);
    let err = r.error.unwrap();
    eprintln!("{:?}: {}", err.code, err.message);
    assert!(err.retryable || !err.actions.is_empty());
}

#[test]
#[ignore]
fn a_load_replacing_a_load_ends_on_the_second() {
    let e = engine();
    e.load(Request {
        url: "http://127.0.0.1:9/never.ts".into(),
        live: true,
        cache_secs: 2,
        ..Request::default()
    })
    .unwrap();
    e.load(Request {
        url: video(),
        cache_secs: 10,
        ..Request::default()
    })
    .unwrap();
    let r = until(&e, "the second playing", 20, |r| {
        r.status == Status::Playing
    });
    assert!(r.error.is_none(), "{r:?}");
    assert!(!r.is_live);
}
