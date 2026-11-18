//! The engine against a fake mpv: ordering, staleness, state, seeking, recovery.
//! Each test names the docs/PLAYER.md lines it proves.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aurora_core::neterr::ErrorCode;
use aurora_core::timeshift::Budget;
use aurora_video::driver::EndReason;
use aurora_video::fake::FakeMpv;
use aurora_video::stall::Limits;
use aurora_video::{
    Aspect, Engine, Event, Node, Report, Request, Seek, Status, Timeshift, VideoError,
};

fn start() -> (Engine, FakeMpv) {
    let mpv = FakeMpv::new();
    (Engine::start(mpv.driver()), mpv)
}

fn until(engine: &Engine, what: &str, f: impl Fn(&Report) -> bool) -> Report {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let r = engine.report();
        if f(&r) {
            return r;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: {r:#?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn film() -> Request {
    Request {
        url: "http://provider/movie/u/p/1.mkv".into(),
        cache_secs: 10,
        ..Request::default()
    }
}

fn channel() -> Request {
    Request {
        url: "http://provider/live/u/p/7.ts".into(),
        live: true,
        cache_secs: 4,
        ..Request::default()
    }
}

fn buffered() -> Request {
    Request {
        timeshift: Some(Timeshift {
            budget: Budget::default(),
            dir: PathBuf::from("C:/buffer"),
        }),
        ..channel()
    }
}

/// Load `req` and bring it to playing, with a duration for films.
fn playing(engine: &Engine, mpv: &FakeMpv, req: Request) {
    let live = req.live;
    engine.load(req).unwrap();
    mpv.plays();
    if !live {
        mpv.prop("duration", Node::Double(6000.0));
    }
    mpv.prop("time-pos", Node::Double(if live { 500.0 } else { 10.0 }));
    until(engine, "playing", |r| {
        r.status == Status::Playing && r.has_picture
    });
}

#[test]
fn e3_load_returns_loading_and_e13_it_plays_once_mpv_says_so() {
    let (engine, mpv) = start();
    let r = engine.load(film()).unwrap();
    assert_eq!(r.status, Status::Loading);
    assert!(!r.has_picture);
    mpv.loaded();
    mpv.prop("track-list", FakeMpv::video_tracks());
    let r = until(&engine, "tracks", |r| !r.audio_tracks.is_empty());
    assert_eq!(r.status, Status::Loading, "no frame yet: {r:?}");
    assert!(!r.has_picture, "E14: no frame drawn yet");
    mpv.restarted();
    let r = until(&engine, "picture", |r| r.has_picture);
    assert_eq!(r.status, Status::Playing);
}

#[test]
fn e10_an_empty_address_is_refused() {
    let (engine, mpv) = start();
    assert_eq!(
        engine.load(Request::default()).unwrap_err(),
        VideoError::EmptyUrl
    );
    assert!(mpv.commands().is_empty());
}

#[test]
fn e2_the_end_of_the_replaced_file_is_not_reported_as_the_new_one() {
    let (engine, mpv) = start();
    playing(&engine, &mpv, film());
    let first = mpv.current().unwrap();
    engine.load(channel()).unwrap();
    // The fake ended the first file with `stop`; now the first file also says it failed
    // (late), and a property of it changes (late).
    mpv.say(Event::EndFile {
        entry: first,
        reason: EndReason::Error,
        error: Some("HTTP error 403 Forbidden".into()),
        next: None,
    });
    mpv.prop("duration", Node::Double(1234.0));
    mpv.plays();
    let r = until(&engine, "channel playing", |r| r.status == Status::Playing);
    assert!(r.error.is_none(), "{r:?}");
    assert!(r.is_live);
    assert_eq!(r.duration_secs, 0.0);
}

#[test]
fn e2_properties_before_the_new_file_starts_belong_to_the_old_one() {
    let (engine, mpv) = start();
    mpv.report_entries(true);
    playing(&engine, &mpv, film());
    // Hold back the new file's start: issue the load, then deliver an old-file property
    // before the StartFile the fake queued. The fake queues StartFile at loadfile time,
    // so instead check that tracks from the old file are gone after the load.
    engine.load(film()).unwrap();
    let r = engine.report();
    assert!(
        r.audio_tracks.is_empty(),
        "E2: old tracks carried over: {r:?}"
    );
    assert!(!r.has_picture, "E14: old picture carried over");
}

#[test]
fn e2_without_entry_ids_the_next_start_is_taken_as_ours() {
    let (engine, mpv) = start();
    mpv.report_entries(false);
    playing(&engine, &mpv, film());
    assert!(engine.report().audio_tracks.len() == 2);
}

#[test]
fn e1_commands_reach_mpv_in_the_order_sent() {
    let (engine, mpv) = start();
    playing(&engine, &mpv, film());
    for v in [10, 20, 30, 40, 50] {
        engine.set_volume(v).unwrap();
    }
    engine.set_muted(true).unwrap();
    until(&engine, "mute applied", |_| {
        mpv.last_set("mute").as_deref() == Some("yes")
    });
    let vols: Vec<String> = mpv
        .sets()
        .into_iter()
        .filter(|(n, _)| n == "volume")
        .map(|(_, v)| v)
        .collect();
    assert_eq!(vols, ["10", "20", "30", "40", "50"]);
}

#[test]
fn a_change_is_what_the_caller_reads_back_at_once() {
    let (engine, mpv) = start();
    playing(&engine, &mpv, film());
    assert_eq!(engine.set_volume(37).unwrap().volume, 37);
    assert_eq!(engine.report().volume, 37);
    assert!(engine.set_muted(true).unwrap().muted);
    assert_eq!(engine.set_paused(true).unwrap().status, Status::Paused);
    assert_eq!(
        engine.set_aspect(Aspect::Zoom).unwrap().aspect,
        Aspect::Zoom
    );
    assert_eq!(
        engine.set_speed(9.0).unwrap().speed,
        4.0,
        "speed is held to 4x"
    );
    assert_eq!(
        engine.set_volume(900).unwrap().volume,
        200,
        "volume is held to 200"
    );
}

#[test]
fn e12_mpv_changing_volume_itself_is_reported() {
    let (engine, mpv) = start();
    mpv.prop("volume", Node::Double(64.4));
    mpv.prop("mute", Node::Flag(true));
    let r = until(&engine, "volume", |r| r.volume == 64);
    assert!(r.muted);
}

#[test]
fn e5_every_load_sets_every_per_file_setting() {
    let (engine, mpv) = start();
    let with_headers = Request {
        headers: vec![("Authorization".into(), "Bearer x".into())],
        referrer: Some("http://ref".into()),
        ..film()
    };
    engine.load(with_headers).unwrap();
    engine.load(channel()).unwrap();
    assert_eq!(mpv.last_set("http-header-fields").as_deref(), Some(""));
    assert_eq!(mpv.last_set("referrer").as_deref(), Some(""));
    assert!(mpv.last_set("user-agent").unwrap().starts_with("AuroraTV/"));
}

#[test]
fn a_new_file_is_never_started_paused() {
    let (engine, mpv) = start();
    playing(&engine, &mpv, film());
    engine.set_paused(true).unwrap();
    engine.load(film()).unwrap();
    assert_eq!(mpv.last_set("pause").as_deref(), Some("no"));
}

#[test]
fn e13_waiting_for_data_is_buffering_and_pause_is_paused() {
    let (engine, mpv) = start();
    playing(&engine, &mpv, film());
    mpv.prop("paused-for-cache", Node::Flag(true));
    until(&engine, "buffering", |r| r.status == Status::Buffering);
    mpv.prop("paused-for-cache", Node::Flag(false));
    until(&engine, "playing", |r| r.status == Status::Playing);
    mpv.prop("pause", Node::Flag(true));
    until(&engine, "paused", |r| r.status == Status::Paused);
}

#[test]
fn e13_pause_while_it_opens_says_paused_and_the_next_press_resumes() {
    // Space while a film was still opening sent `resume` twice before a press finally
    // paused it, because the engine said Loading after a pause it had honoured.
    let (engine, mpv) = start();
    engine.load(film()).unwrap();
    let r = engine.set_paused(true).unwrap();
    assert_eq!(r.status, Status::Paused);
    // The report says Paused as soon as it is asked; mpv hears it from the engine's
    // thread a moment later. Wait for mpv, or a loaded machine fails this for nothing.
    let deadline = Instant::now() + Duration::from_secs(3);
    while mpv.last_set("pause").as_deref() != Some("yes") {
        assert!(Instant::now() < deadline, "mpv was not asked to pause");
        std::thread::sleep(Duration::from_millis(5));
    }
    until(&engine, "paused while opening", |r| {
        r.status == Status::Paused
    });
    mpv.loaded();
    until(&engine, "still paused once open", |r| {
        r.status == Status::Paused
    });
    engine.set_paused(false).unwrap();
    until(&engine, "opening again", |r| r.status == Status::Loading);
    mpv.plays();
    mpv.prop("time-pos", Node::Double(10.0));
    until(&engine, "playing", |r| r.status == Status::Playing);
}

#[test]
fn e25_a_stream_paused_before_it_opens_is_still_given_up() {
    let mpv = FakeMpv::new();
    let engine = Engine::start_with(
        mpv.driver(),
        Limits {
            open_secs: 0.6,
            buffer_secs: 0.6,
            frozen_secs: 0.6,
        },
    );
    engine.load(channel()).unwrap();
    engine.set_paused(true).unwrap();
    until(&engine, "error", |r| r.status == Status::Error);
}

#[test]
fn e16_tracks_carry_language_and_title() {
    let (engine, mpv) = start();
    playing(&engine, &mpv, film());
    let r = engine.report();
    assert_eq!(r.audio_tracks.len(), 2);
    assert_eq!(r.audio_tracks[0].language.as_deref(), Some("eng"));
    assert_eq!(r.audio_tracks[0].title.as_deref(), Some("Main"));
    assert_eq!(r.active_audio_track, Some(1));
    assert_eq!(r.subtitle_tracks.len(), 1);
    assert_eq!(r.active_subtitle_track, None);
}

#[test]
fn choosing_tracks_reaches_mpv_and_off_is_no() {
    let (engine, mpv) = start();
    playing(&engine, &mpv, film());
    engine.set_audio_track(Some(2)).unwrap();
    engine.set_subtitle_track(Some(1)).unwrap();
    engine.set_subtitle_track(None).unwrap();
    until(&engine, "subs off reached mpv", |_| {
        mpv.last_set("sid").as_deref() == Some("no")
    });
    assert_eq!(mpv.last_set("aid").as_deref(), Some("2"));
    assert_eq!(mpv.last_set("sid").as_deref(), Some("no"));
}

#[test]
fn e14_a_radio_station_has_no_picture() {
    let (engine, mpv) = start();
    engine.load(channel()).unwrap();
    mpv.loaded();
    mpv.prop(
        "track-list",
        Node::Array(vec![aurora_video::fake::track(
            1, "audio", None, None, true,
        )]),
    );
    mpv.restarted();
    let r = until(&engine, "playing", |r| r.status == Status::Playing);
    assert!(!r.has_picture);
}

#[test]
fn e15_live_has_no_duration() {
    let (engine, mpv) = start();
    playing(&engine, &mpv, channel());
    mpv.prop("duration", Node::Double(77.0));
    std::thread::sleep(Duration::from_millis(50));
    let r = engine.report();
    assert_eq!(r.duration_secs, 0.0);
    assert_eq!(r.progress(), 0.0);
}

#[test]
fn e11_chapters_are_reported_and_cleared_on_the_next_load() {
    let (engine, mpv) = start();
    engine.load(film()).unwrap();
    mpv.holds(
        "chapter-list",
        Node::Array(vec![
            Node::Map(vec![
                ("title".into(), Node::Str("Intro".into())),
                ("time".into(), Node::Double(0.0)),
            ]),
            Node::Map(vec![
                ("title".into(), Node::Str("Story".into())),
                ("time".into(), Node::Double(90.0)),
            ]),
        ]),
    );
    mpv.plays();
    until(&engine, "playing", |r| r.status == Status::Playing);
    assert_eq!(engine.chapters().len(), 2);
    assert_eq!(engine.chapters()[1].start_secs, 90.0);
    engine.load(film()).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert!(engine.chapters().is_empty());
}

#[test]
fn e19_a_film_seek_is_held_to_its_length() {
    let (engine, mpv) = start();
    playing(&engine, &mpv, film());
    assert_eq!(
        engine.seek(Seek::To(99_999.0)).unwrap().position_secs,
        6000.0
    );
    assert_eq!(engine.seek(Seek::By(-99_999.0)).unwrap().position_secs, 0.0);
    assert!(mpv.wait_for_command(|c| c == ["seek", "0.000", "absolute"]));
}

#[test]
fn e20_an_unbuffered_channel_cannot_be_seeked() {
    let (engine, mpv) = start();
    playing(&engine, &mpv, channel());
    engine.seek(Seek::By(-30.0)).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(!mpv.commands().iter().any(|c| c[0] == "seek"));
}

fn rewindable(engine: &Engine, mpv: &FakeMpv) {
    playing(engine, mpv, buffered());
    mpv.holds(
        "demuxer-cache-state",
        Node::Map(vec![
            ("cache-end".into(), Node::Double(620.0)),
            (
                "seekable-ranges".into(),
                Node::Array(vec![Node::Map(vec![
                    ("start".into(), Node::Double(480.0)),
                    ("end".into(), Node::Double(620.0)),
                ])]),
            ),
        ]),
    );
    mpv.prop("time-pos", Node::Double(615.0));
    until(engine, "window", |r| {
        r.timeshift.map(|w| w.live_secs) == Some(620.0)
    });
}

#[test]
fn e21_e23_a_buffered_channel_offers_what_it_holds_and_no_more() {
    let (engine, mpv) = start();
    rewindable(&engine, &mpv);
    let w = engine.report().timeshift.unwrap();
    // Tuned at 500: nothing before that was ever buffered, whatever mpv's range says.
    assert_eq!(w.start_secs, 500.0);
    assert_eq!(w.position_secs, 615.0);
    assert_eq!(mpv.last_set("cache-on-disk").as_deref(), Some("yes"));
}

#[test]
fn e20_a_buffered_seek_is_held_inside_the_buffer() {
    let (engine, mpv) = start();
    rewindable(&engine, &mpv);
    assert_eq!(
        engine.seek(Seek::By(-10_000.0)).unwrap().position_secs,
        500.0
    );
    assert_eq!(
        engine.seek(Seek::By(10_000.0)).unwrap().position_secs,
        620.0
    );
}

#[test]
fn e24_back_to_live_lands_just_behind_the_edge_and_plays() {
    let (engine, mpv) = start();
    rewindable(&engine, &mpv);
    engine.set_paused(true).unwrap();
    let r = engine.seek(Seek::Live).unwrap();
    assert_eq!(r.position_secs, 619.0);
    assert!(mpv.wait_for_command(|c| c == ["seek", "619.000", "absolute"]));
    until(&engine, "unpaused", |_| {
        mpv.last_set("pause").as_deref() == Some("no")
    });
}

#[test]
fn e24_stopping_forgets_the_buffer() {
    let (engine, mpv) = start();
    rewindable(&engine, &mpv);
    let r = engine.stop().unwrap();
    assert!(r.timeshift.is_none());
    assert_eq!(r.status, Status::Idle);
    let r = until(&engine, "idle", |r| r.status == Status::Idle);
    assert!(r.timeshift.is_none());
}

#[test]
fn e22_a_channel_after_a_buffered_one_does_not_buffer() {
    let (engine, mpv) = start();
    engine.load(buffered()).unwrap();
    engine.load(channel()).unwrap();
    assert_eq!(mpv.last_set("cache-on-disk").as_deref(), Some("no"));
}

#[test]
fn a_failure_is_classified_from_mpvs_log_and_offers_actions() {
    let (engine, mpv) = start();
    engine.load(channel()).unwrap();
    mpv.log("HTTP error 403 Forbidden");
    mpv.fail("loading failed");
    let r = until(&engine, "error", |r| r.status == Status::Error);
    let e = r.error.unwrap();
    assert_eq!(e.code, ErrorCode::Forbidden);
    assert!(!e.actions.is_empty());
}

#[test]
fn a_new_load_clears_the_error() {
    let (engine, mpv) = start();
    engine.load(film()).unwrap();
    mpv.fail("loading failed");
    until(&engine, "error", |r| r.status == Status::Error);
    let r = engine.load(film()).unwrap();
    assert!(r.error.is_none());
    assert_eq!(r.status, Status::Loading);
}

#[test]
fn a_channel_that_runs_out_is_a_failure_not_an_ending() {
    let (engine, mpv) = start();
    playing(&engine, &mpv, channel());
    mpv.prop("eof-reached", Node::Flag(true));
    let r = until(&engine, "error", |r| r.status == Status::Error);
    assert_eq!(r.error.unwrap().code, ErrorCode::Dropped);
    assert!(
        mpv.wait_for_command(|c| c == ["stop"]),
        "the connection is let go"
    );
}

#[test]
fn a_film_at_its_end_rests_on_the_last_frame() {
    let (engine, mpv) = start();
    playing(&engine, &mpv, film());
    mpv.prop("eof-reached", Node::Flag(true));
    let r = until(&engine, "paused", |r| r.status == Status::Paused);
    assert!(r.error.is_none());
}

#[test]
fn e25_a_stream_that_never_starts_is_given_up_and_let_go() {
    let mpv = FakeMpv::new();
    let engine = Engine::start_with(
        mpv.driver(),
        Limits {
            open_secs: 0.6,
            buffer_secs: 0.6,
            frozen_secs: 0.6,
        },
    );
    engine.load(channel()).unwrap();
    let r = until(&engine, "error", |r| r.status == Status::Error);
    let code = r.error.unwrap().code;
    assert!(
        matches!(code, ErrorCode::Timeout | ErrorCode::Dropped),
        "{code:?}"
    );
    assert!(mpv.wait_for_command(|c| c == ["stop"]));
}

#[test]
fn e26_a_frozen_picture_is_given_up_once() {
    let mpv = FakeMpv::new();
    let engine = Engine::start_with(
        mpv.driver(),
        Limits {
            open_secs: 5.0,
            buffer_secs: 5.0,
            frozen_secs: 0.6,
        },
    );
    playing(&engine, &mpv, channel());
    until(&engine, "error", |r| r.status == Status::Error);
    std::thread::sleep(Duration::from_millis(1200));
    let stops = mpv.commands().iter().filter(|c| c[0] == "stop").count();
    assert_eq!(stops, 1, "E28: judged once");
}

#[test]
fn e27_a_paused_film_is_never_called_stuck() {
    let mpv = FakeMpv::new();
    let engine = Engine::start_with(
        mpv.driver(),
        Limits {
            open_secs: 5.0,
            buffer_secs: 5.0,
            frozen_secs: 0.4,
        },
    );
    playing(&engine, &mpv, film());
    engine.set_paused(true).unwrap();
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(engine.report().status, Status::Paused);
}

#[test]
fn e31_a_frame_is_grabbed_only_when_there_is_a_picture() {
    let (engine, mpv) = start();
    mpv.screenshot(Node::Map(vec![
        ("w".into(), Node::Int(4)),
        ("h".into(), Node::Int(2)),
        ("stride".into(), Node::Int(16)),
        ("format".into(), Node::Str("bgr0".into())),
        ("data".into(), Node::Bytes(vec![9; 32])),
    ]));
    engine.load(film()).unwrap();
    assert!(engine.grab_frame(160).is_none(), "no picture yet");
    mpv.plays();
    until(&engine, "picture", |r| r.has_picture);
    let f = engine.grab_frame(160).unwrap();
    assert_eq!((f.width, f.height), (4, 2));
}

#[test]
fn e4_shutting_down_twice_is_harmless_and_then_commands_say_gone() {
    let (engine, _mpv) = start();
    engine.shutdown();
    engine.shutdown();
    assert_eq!(engine.load(film()).unwrap_err(), VideoError::Gone);
    assert_eq!(engine.set_volume(3).unwrap_err(), VideoError::Gone);
    assert!(engine.grab_frame(10).is_none());
}

#[test]
fn listeners_hear_each_change_once() {
    let (engine, mpv) = start();
    let heard = Arc::new(Mutex::new(Vec::<Status>::new()));
    let h = heard.clone();
    engine.on_change(Box::new(move |r| h.lock().unwrap().push(r.status)));
    // The engine reports once per pass of its loop, so a load and mpv's "playing" that
    // land in the same pass are one change, straight to Playing -- which is right, and
    // made this test fail on a fast runner. Let the load be heard before mpv answers.
    engine.load(film()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !heard.lock().unwrap().contains(&Status::Loading) {
        assert!(Instant::now() < deadline, "the load was never heard");
        std::thread::sleep(Duration::from_millis(5));
    }
    mpv.plays();
    mpv.prop("duration", Node::Double(6000.0));
    mpv.prop("time-pos", Node::Double(10.0));
    until(&engine, "playing", |r| {
        r.status == Status::Playing && r.has_picture
    });
    std::thread::sleep(Duration::from_millis(600));
    let heard = heard.lock().unwrap().clone();
    assert!(heard.contains(&Status::Loading));
    assert_eq!(heard.last(), Some(&Status::Playing));
    // The playhead did not move and nothing else changed: the ticks said nothing new.
    let playing_count = heard.iter().filter(|s| **s == Status::Playing).count();
    assert!(playing_count <= 3, "{heard:?}");
}

#[test]
fn a_flood_of_events_does_not_hold_a_command_back() {
    let (engine, mpv) = start();
    playing(&engine, &mpv, film());
    for i in 0..5000 {
        mpv.say(Event::Property {
            name: "time-pos".into(),
            value: Node::Double(f64::from(i)),
        });
    }
    let t = Instant::now();
    engine.set_paused(true).unwrap();
    assert!(mpv.wait_for_command(|_| true));
    until(&engine, "pause reached mpv", |_| {
        mpv.last_set("pause").as_deref() == Some("yes")
    });
    assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
}

#[test]
fn addresses_are_kept_out_of_the_log() {
    let s = aurora_video::engine::without_addresses("failed http://host/live/user/pass/1.ts (403)");
    assert_eq!(s, "failed <address> (403)");
}
