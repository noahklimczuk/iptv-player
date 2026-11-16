//! Soak tests: what only goes wrong after a while (README §16, docs/PLAYER.md H4).
//!
//! Does zapping quickly ever leave the wrong channel on, or wedge? Does a long session
//! grow without bound? `#[ignore]`d: they take a while, and are run deliberately
//! before a release with `cargo test -p aurora-app --test stress -- --ignored`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use aurora_app::dblock::TimedMutex;
use aurora_app::play::resolve::What;
use aurora_app::play::session::Session;
use aurora_ingest::credentials::MemoryStore;
use aurora_video::fake::FakeMpv;
use aurora_video::Engine;

const CHANNELS: i64 = 50;

fn url(channel: i64) -> String {
    format!("http://provider.test/live/{channel}.ts")
}

fn rig() -> (Arc<Session>, FakeMpv) {
    let conn = aurora_db::open_memory().unwrap();
    conn.execute(
        "INSERT INTO providers (id,name,kind,base_url,created_at)
         VALUES (1,'P','m3u','https://provider.test',0)",
        [],
    )
    .unwrap();
    for id in 1..=CHANNELS {
        conn.execute(
            "INSERT INTO channels (id,provider_id,provider_key,name,match_key,last_seen_at)
             VALUES (?1,1,?2,?2,?2,0)",
            aurora_db::rusqlite::params![id, format!("ch{id}")],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO channel_sources (channel_id,url,priority) VALUES (?1,?2,0)",
            aurora_db::rusqlite::params![id, url(id)],
        )
        .unwrap();
    }
    let mpv = FakeMpv::new();
    let engine = Arc::new(Engine::start(mpv.driver()));
    let session = Session::new(
        engine,
        Arc::new(TimedMutex::new(conn)),
        Arc::new(MemoryStore::default()),
        std::env::temp_dir().join(format!("aurora-stress-{}", std::process::id())),
    );
    // Recovery runs throughout, as it does in the app.
    session.spawn_loop();
    (session, mpv)
}

fn last_opened(mpv: &FakeMpv) -> Option<String> {
    mpv.commands()
        .into_iter()
        .rev()
        .find(|c| c.first().map(String::as_str) == Some("loadfile"))
        .map(|c| c[1].clone())
}

/// A cheap, repeatable sequence of channels.
fn channel(i: u64) -> i64 {
    (i.wrapping_mul(2_654_435_761) % CHANNELS as u64) as i64 + 1
}

#[test]
#[ignore = "soak: run with --ignored"]
fn two_thousand_zaps_always_end_on_the_channel_last_asked_for() {
    let (session, mpv) = rig();
    for i in 0..2_000u64 {
        let ch = channel(i);
        session.play(What::Channel(ch), None).unwrap();
        // Now and then the stream starts, and now and then it dies, so recovery has
        // something to do while the zapping goes on.
        match i % 7 {
            0 => mpv.plays(),
            3 => mpv.fail("HTTP error 503 Service Unavailable"),
            _ => {}
        }
        let state = session.state();
        assert_eq!(state.channel_id, Some(ch), "zap {i}: the session disagrees");
    }
    let last = channel(1_999);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(session.state().channel_id, Some(last));
    assert_eq!(
        last_opened(&mpv),
        Some(url(last)),
        "mpv is not on the last channel"
    );
}

#[test]
#[ignore = "soak: run with --ignored"]
fn concurrent_zapping_never_deadlocks_or_wedges() {
    let (session, mpv) = rig();
    let deadline = Instant::now() + Duration::from_secs(60);
    let threads: Vec<_> = (0..8u64)
        .map(|t| {
            let session = session.clone();
            std::thread::spawn(move || {
                for i in 0..250u64 {
                    let ch = channel(t * 1_000 + i);
                    // A replaced tune is the expected answer to overlapping requests.
                    let _ = session.play(What::Channel(ch), None);
                    if i % 50 == 0 {
                        let _ = session.stop();
                    }
                }
            })
        })
        .collect();
    for t in threads {
        while !t.is_finished() {
            assert!(Instant::now() < deadline, "zapping wedged");
            std::thread::sleep(Duration::from_millis(20));
        }
        t.join().unwrap();
    }
    // Still answering, and a final tune still wins.
    session.play(What::Channel(7), None).unwrap();
    assert_eq!(session.state().channel_id, Some(7));
    assert_eq!(last_opened(&mpv), Some(url(7)));
}

#[cfg(target_os = "linux")]
fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap()
}

/// Memory over a long session. Loose on purpose: the question is "does it grow without
/// bound", and the fake mpv's own record of every command is most of what grows.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "soak: run with --ignored"]
fn a_long_session_does_not_grow_without_bound() {
    let (session, mpv) = rig();
    let zap = |from: u64, to: u64| {
        for i in from..to {
            session.play(What::Channel(channel(i)), None).unwrap();
            if i % 3 == 0 {
                mpv.plays();
            }
        }
    };
    zap(0, 1_000);
    let before = rss_kib();
    zap(1_000, 6_000);
    let grown = rss_kib().saturating_sub(before);
    assert!(grown < 96 * 1024, "grew {grown} KiB over 5,000 zaps");
}
