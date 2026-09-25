//! Soak tests: the things that only go wrong after a while (README §16, Phase 11).
//!
//! Two questions nothing else here asks. Does zapping quickly ever leave the wrong
//! channel on, or wedge? And does an hour of use grow without bound?
//!
//! These are `#[ignore]` by default and run with `cargo test -- --ignored`: they take
//! tens of seconds and their job is to be run deliberately before a release, not on
//! every commit.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use aurora_app::playback::Playback;
use aurora_db::rusqlite::params;
use aurora_player::backend::LoadOptions;
use aurora_player::{PlayerBackend, PlayerError, PlayerState, PlayerStatus};
use parking_lot::Mutex;

const NOW: i64 = 1_760_000_000;
const CHANNELS: i64 = 400;
const SOURCES_PER_CHANNEL: usize = 3;

/// A backend that records what it was last asked to play and counts everything, so a
/// soak can assert on the end state rather than on timing.
#[derive(Default)]
struct CounterInner {
    last_loaded: Option<String>,
    loads: u64,
    state: PlayerState,
}

#[derive(Clone)]
struct Counter(Arc<Mutex<CounterInner>>, Arc<AtomicU64>);

impl Counter {
    fn new() -> Self {
        Self(
            Arc::new(Mutex::new(CounterInner::default())),
            Arc::new(AtomicU64::new(0)),
        )
    }
    fn last_loaded(&self) -> Option<String> {
        self.0.lock().last_loaded.clone()
    }
    fn loads(&self) -> u64 {
        self.0.lock().loads
    }
    /// Make the stream look dead, the way a provider dropping a connection does.
    fn die(&self) {
        self.0.lock().state.status = PlayerStatus::Error;
    }
    fn backend(&self) -> Box<dyn PlayerBackend> {
        Box::new(Self(Arc::clone(&self.0), Arc::clone(&self.1)))
    }
}

impl PlayerBackend for Counter {
    fn load(&mut self, url: &str, _o: &LoadOptions) -> Result<(), PlayerError> {
        let mut inner = self.0.lock();
        inner.loads += 1;
        inner.last_loaded = Some(url.to_string());
        inner.state = PlayerState {
            status: PlayerStatus::Playing,
            is_live: true,
            ..Default::default()
        };
        Ok(())
    }
    fn stop(&mut self) -> Result<(), PlayerError> {
        self.0.lock().state = PlayerState::default();
        Ok(())
    }
    fn set_paused(&mut self, _: bool) -> Result<(), PlayerError> {
        Ok(())
    }
    fn seek(&mut self, _: f64, _: bool) -> Result<(), PlayerError> {
        Ok(())
    }
    fn set_volume(&mut self, _: u32) -> Result<(), PlayerError> {
        Ok(())
    }
    fn set_muted(&mut self, _: bool) -> Result<(), PlayerError> {
        Ok(())
    }
    fn set_speed(&mut self, _: f64) -> Result<(), PlayerError> {
        Ok(())
    }
    fn set_audio_track(&mut self, _: Option<i64>) -> Result<(), PlayerError> {
        Ok(())
    }
    fn set_subtitle_track(&mut self, _: Option<i64>) -> Result<(), PlayerError> {
        Ok(())
    }
    fn set_aspect(&mut self, _: aurora_player::state::Aspect) -> Result<(), PlayerError> {
        Ok(())
    }
    fn state(&self) -> PlayerState {
        self.0.lock().state.clone()
    }
    fn chapters(&self) -> Vec<aurora_core::markers::Chapter> {
        Vec::new()
    }
    fn resize(&mut self, _: u32, _: u32) -> Result<(), PlayerError> {
        Ok(())
    }
}

fn url_for(channel: i64, source: usize) -> String {
    format!("http://example.com/ch{channel}-{source}.ts")
}

fn harness() -> (Arc<Playback>, Counter) {
    let conn = aurora_db::open_memory().unwrap();
    conn.execute(
        "INSERT INTO providers (id,name,kind,base_url,created_at)
         VALUES (1,'P','m3u','https://example.com',0)",
        [],
    )
    .unwrap();
    for id in 1..=CHANNELS {
        conn.execute(
            "INSERT INTO channels (id, provider_id, provider_key, name, match_key, last_seen_at)
             VALUES (?1, 1, ?2, ?3, ?3, 0)",
            params![id, format!("c{id}"), format!("Channel {id}")],
        )
        .unwrap();
        for n in 0..SOURCES_PER_CHANNEL {
            conn.execute(
                "INSERT INTO channel_sources (channel_id, url, priority) VALUES (?1, ?2, ?3)",
                params![id, url_for(id, n), n as i64],
            )
            .unwrap();
        }
    }
    aurora_db::repo::settings::set(&conn, "timeshift.enabled", &false).unwrap();

    let db = Arc::new(Mutex::new(conn));
    let counter = Counter::new();
    let player: Arc<Mutex<Box<dyn PlayerBackend>>> = Arc::new(Mutex::new(counter.backend()));
    let dir = std::env::temp_dir().join(format!("aurora-stress-{}", std::process::id()));
    (Arc::new(Playback::new(db, player, dir)), counter)
}

/// 2,000 zaps as fast as the machine will do them, with the heartbeat running
/// throughout — which is what a person leaning on Ch+ actually produces.
///
/// The invariant is the one the race fix exists for: whatever the sequence, the
/// channel playing at the end is the last one asked for, and the session agrees.
#[test]
#[ignore = "soak: run with --ignored"]
fn two_thousand_zaps_always_end_on_the_channel_last_asked_for() {
    const ZAPS: i64 = 2_000;
    let (playback, counter) = harness();

    // The real heartbeat, at the real interval, for the whole run.
    let beating = Arc::new(AtomicU64::new(1));
    let ticker = {
        let playback = Arc::clone(&playback);
        let beating = Arc::clone(&beating);
        std::thread::spawn(move || {
            let mut ticks = 0u64;
            while beating.load(Ordering::SeqCst) == 1 {
                playback.tick(NOW);
                ticks += 1;
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            ticks
        })
    };

    let started = std::time::Instant::now();
    let mut refused = 0u64;
    for i in 0..ZAPS {
        let channel = (i % CHANNELS) + 1;
        // Every so often the stream dies mid-zap, which is what puts the heartbeat's
        // rollover in contention with the next tune.
        if i % 50 == 0 {
            counter.die();
        }
        if playback.play_live(channel, NOW).is_err() {
            refused += 1;
        }
    }
    let elapsed = started.elapsed();

    beating.store(0, Ordering::SeqCst);
    let ticks = ticker.join().unwrap();

    // Settle: let any rollover in flight finish before reading the end state.
    std::thread::sleep(std::time::Duration::from_millis(50));
    playback.tick(NOW);

    let last = ((ZAPS - 1) % CHANNELS) + 1;
    assert_eq!(
        counter.last_loaded(),
        Some(url_for(last, 0)),
        "after {ZAPS} zaps ({elapsed:?}, {ticks} heartbeats, {refused} refused) the \
         player is not on the channel last asked for"
    );

    eprintln!(
        "{ZAPS} zaps in {elapsed:?} ({:.0}/s), {} loads, {ticks} heartbeats, {refused} refused",
        ZAPS as f64 / elapsed.as_secs_f64(),
        counter.loads()
    );
}

/// The same thing from several threads, which is what the IPC surface actually is:
/// every Tauri command runs on its own thread, so nothing stops two tunes overlapping.
#[test]
#[ignore = "soak: run with --ignored"]
fn concurrent_zapping_never_deadlocks_or_wedges() {
    const PER_THREAD: i64 = 300;
    const THREADS: i64 = 8;
    let (playback, counter) = harness();

    let beating = Arc::new(AtomicU64::new(1));
    let ticker = {
        let playback = Arc::clone(&playback);
        let beating = Arc::clone(&beating);
        std::thread::spawn(move || {
            while beating.load(Ordering::SeqCst) == 1 {
                playback.tick(NOW);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        })
    };

    let started = std::time::Instant::now();
    let workers: Vec<_> = (0..THREADS)
        .map(|t| {
            let playback = Arc::clone(&playback);
            std::thread::spawn(move || {
                let mut ok = 0u64;
                for i in 0..PER_THREAD {
                    let channel = ((t * PER_THREAD + i) % CHANNELS) + 1;
                    // A tune another thread replaced comes back Superseded, which is
                    // the mechanism working rather than a failure — it is what stops
                    // the wrong channel ending up on screen.
                    if playback.play_live(channel, NOW).is_ok() {
                        ok += 1;
                    }
                }
                ok
            })
        })
        .collect();

    let succeeded: u64 = workers.into_iter().map(|w| w.join().unwrap()).sum();
    let elapsed = started.elapsed();
    beating.store(0, Ordering::SeqCst);
    ticker.join().unwrap();

    // Nothing wedged: every thread finished, and the player is playing something.
    assert!(succeeded > 0);
    assert!(counter.last_loaded().is_some());
    assert_eq!(playback.state().status, PlayerStatus::Playing);

    // And the app is still usable afterwards: one more deliberate tune lands.
    playback.play_live(7, NOW).unwrap();
    assert_eq!(counter.last_loaded(), Some(url_for(7, 0)));

    eprintln!(
        "{} concurrent tunes across {THREADS} threads in {elapsed:?}, {succeeded} won, \
         {} loads",
        THREADS * PER_THREAD,
        counter.loads()
    );
}

/// Memory over a long session.
///
/// Measured through the process's own RSS on Linux, because a leak here would be in
/// the session bookkeeping — a `Vec` of candidates kept per tune, a `last` state never
/// replaced — and that is visible in RSS without any instrumentation. The threshold is
/// deliberately loose: the question is "does this grow without bound", not "how many
/// bytes".
#[test]
#[ignore = "soak: run with --ignored"]
fn a_long_session_does_not_grow_without_bound() {
    const WARMUP: i64 = 2_000;
    const MEASURED: i64 = 20_000;
    let (playback, _counter) = harness();

    let rss_kb = || -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        status
            .lines()
            .find(|l| l.starts_with("VmRSS:"))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    };

    // Warm up: allocators grow for a while before they settle, and measuring through
    // that would report a leak that is just a heap finding its size.
    for i in 0..WARMUP {
        let _ = playback.play_live((i % CHANNELS) + 1, NOW);
        playback.tick(NOW);
    }
    let Some(before) = rss_kb() else {
        eprintln!("no /proc/self/status on this platform; skipping the RSS check");
        return;
    };

    let started = std::time::Instant::now();
    for i in 0..MEASURED {
        let _ = playback.play_live((i % CHANNELS) + 1, NOW);
        playback.tick(NOW);
        if i % 500 == 0 {
            let _ = playback.seek(30.0, true);
            let _ = playback.stop();
        }
    }
    let elapsed = started.elapsed();
    let after = rss_kb().unwrap();

    let growth_kb = after.saturating_sub(before);
    eprintln!(
        "{MEASURED} tunes + ticks in {elapsed:?}: RSS {before} kB -> {after} kB \
         ({growth_kb} kB, {:.3} kB per tune)",
        growth_kb as f64 / MEASURED as f64
    );

    // 20,000 tunes is far more than a viewer will do in a session. Anything that
    // retains per-tune state shows up as tens of megabytes here; a stable working set
    // shows up as a few.
    assert!(
        growth_kb < 32_768,
        "RSS grew {growth_kb} kB over {MEASURED} tunes, which is not a stable working set"
    );
}
