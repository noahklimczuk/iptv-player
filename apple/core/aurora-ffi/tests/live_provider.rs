//! The same path as `end_to_end.rs`, against a real Xtream panel: add it, import its
//! live TV, list, search, draw the guide, tune.
//!
//! The panel's address and login come from a local file that is never committed:
//! `~/.config/aurora-tv/test-provider.env` (or the file `AURORA_TEST_PROVIDER_FILE`
//! names), with `AURORA_TEST_XTREAM_URL`, `_USER` and `_PASS`. Without it the test says
//! so and passes, so a machine or CI without the file still runs the hermetic suite.
//!
//! Nothing here prints the password or a stream URL (an Xtream URL carries the
//! password): failures name counts and ids only.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use aurora_ffi::{
    AuroraCore, Browse, BrowseSort, ImportContent, ImportListener, ImportPhase, ListingListener,
    SecretError, SecretStore,
};

struct Panel {
    url: String,
    user: String,
    pass: String,
}

fn panel() -> Option<Panel> {
    let path = std::env::var_os("AURORA_TEST_PROVIDER_FILE")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".config/aurora-tv/test-provider.env"))
        })?;
    let text = std::fs::read_to_string(&path).ok()?;
    let values: HashMap<&str, &str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect();
    Some(Panel {
        url: values.get("AURORA_TEST_XTREAM_URL")?.to_string(),
        user: values.get("AURORA_TEST_XTREAM_USER")?.to_string(),
        pass: values.get("AURORA_TEST_XTREAM_PASS")?.to_string(),
    })
}

#[derive(Default)]
struct Secrets(Mutex<HashMap<String, String>>);

impl SecretStore for Secrets {
    fn set(&self, key: String, secret: String) -> Result<(), SecretError> {
        self.0.lock().unwrap().insert(key, secret);
        Ok(())
    }
    fn get(&self, key: String) -> Result<String, SecretError> {
        self.0
            .lock()
            .unwrap()
            .get(&key)
            .cloned()
            .ok_or(SecretError::NotFound)
    }
    fn delete(&self, key: String) -> Result<(), SecretError> {
        self.0.lock().unwrap().remove(&key);
        Ok(())
    }
}

struct Quiet;
impl ImportListener for Quiet {
    fn progress(&self, _phase: ImportPhase, _done: u64, _total: u64) {}
}

#[test]
fn a_real_panel_is_imported_listed_guided_and_tuned() {
    let Some(panel) = panel() else {
        eprintln!("live_provider: no ~/.config/aurora-tv/test-provider.env; skipped");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let core = AuroraCore::open(
        dir.path().display().to_string(),
        std::sync::Arc::new(Secrets::default()),
    )
    .unwrap();

    let id = core
        .add_xtream(
            "Live test".into(),
            panel.url.clone(),
            panel.user.clone(),
            panel.pass.clone(),
        )
        .unwrap();
    let started = std::time::Instant::now();
    let report = core
        .refresh(
            id,
            ImportContent {
                live: true,
                vod: false,
                series: false,
            },
            std::sync::Arc::new(Quiet),
        )
        .unwrap_or_else(|e| panic!("the import failed: {}", redact(&e.to_string(), &panel)));
    eprintln!(
        "live_provider: imported {} channels in {:.1}s",
        report.channels,
        started.elapsed().as_secs_f64()
    );
    assert!(report.channels > 0, "the panel listed no live channels");

    // Listing and groups.
    let groups = core.channel_groups().unwrap();
    assert!(!groups.is_empty(), "channels arrived with no groups");
    let first = core.channels(1, None, false, 50, 0).unwrap();
    assert!(!first.is_empty());

    // The password is in the Keychain stand-in, not in the database.
    for file in ["library.db", "library.db-wal"] {
        let Ok(bytes) = std::fs::read(dir.path().join(file)) else {
            continue;
        };
        assert!(
            !bytes
                .windows(panel.pass.len())
                .any(|w| w == panel.pass.as_bytes()),
            "the panel password was written into {file}"
        );
    }

    // Tuning rebuilds the stream URL from the stream id and the stored password.
    let tune = core.tune_live(first[0].id).unwrap();
    assert!(
        !tune.sources.is_empty(),
        "channel {} has no playable source",
        first[0].id
    );
    assert!(
        tune.sources[0].url.contains(&panel.user),
        "channel {}'s stream URL wasn't built from the login",
        first[0].id
    );
    assert!(tune
        .mpv_options
        .iter()
        .any(|o| o.name() == "cache-secs" && o.text() == Some("8")));

    // The guide grid answers one row per channel on screen, in order.
    let ids: Vec<i64> = first.iter().take(20).map(|c| c.id).collect();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let rows = core
        .guide_rows(ids.clone(), now - 3600, now + 3 * 3600)
        .unwrap();
    assert_eq!(rows.iter().map(|r| r.channel_id).collect::<Vec<_>>(), ids);
    eprintln!(
        "live_provider: {} of {} channels on screen have guide data",
        rows.iter().filter(|r| !r.cells.is_empty()).count(),
        rows.len()
    );

    // Search finds the first channel by a word of its name.
    let word = first[0]
        .name
        .split(|c: char| !c.is_alphanumeric())
        .find(|w| w.len() >= 3)
        .unwrap_or(&first[0].name)
        .to_string();
    let found = core.search(word, 30).unwrap();
    assert!(
        !found.channels.is_empty(),
        "searching a word of a channel's name found nothing"
    );

    // Removing the panel takes its password with it.
    assert!(core.remove_provider(id).unwrap());
    assert!(core.channels(1, None, false, 5, 0).unwrap().is_empty());
}

/// An error message with the login taken out, in case one ever carries a URL.
/// Stops a sweep once it has finished `limit` shows: enough to see it work without
/// asking a real panel about thousands of them.
struct StopAfter(u64);
impl ListingListener for StopAfter {
    fn progress(&self, done: u64, _total: u64) -> bool {
        done < self.0
    }
}

/// Opening a show fetches its episodes from the panel (an import stores only the show),
/// and the background sweep lists others.
#[test]
fn a_real_panels_show_lists_its_episodes_when_opened() {
    let Some(panel) = panel() else {
        eprintln!("live_provider: no ~/.config/aurora-tv/test-provider.env; skipped");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let core = AuroraCore::open(dir.path().display().to_string(), std::sync::Arc::new(Secrets::default())).unwrap();
    let id = core
        .add_xtream("Live test".into(), panel.url.clone(), panel.user.clone(), panel.pass.clone())
        .unwrap();
    let report = core
        .refresh(id, ImportContent { live: false, vod: false, series: true }, std::sync::Arc::new(Quiet))
        .unwrap_or_else(|e| panic!("the import failed: {}", redact(&e.to_string(), &panel)));
    assert!(report.series > 0, "the panel listed no series");

    let shows = core
        .series_list(Browse {
            sort: BrowseSort::RecentlyAdded,
            category: None,
            genre: None,
            query: None,
            letter: None,
            limit: 10,
            offset: 0,
        })
        .unwrap();
    // The first of the newest shows that lists anything: a panel can carry a show with
    // no episodes yet.
    let (show, episodes) = shows
        .iter()
        .find_map(|s| {
            let eps = core.episodes(s.id, None).ok()?;
            (!eps.is_empty()).then(|| (s.id, eps))
        })
        .expect("none of the ten newest shows listed an episode");
    eprintln!("live_provider: show {show} listed {} episodes", episodes.len());
    assert!(episodes.windows(2).all(|w| (w[0].season, w[0].episode) <= (w[1].season, w[1].episode)));
    // A season asked for on its own is a subset.
    let season = episodes[0].season;
    assert!(core.episodes(show, Some(season)).unwrap().iter().all(|e| e.season == season));
    // And it plays.
    let tune = core.tune_episode(episodes[0].id, None).unwrap();
    assert!(!tune.url.is_empty());

    let swept = core
        .list_episodes(std::sync::Arc::new(StopAfter(6)))
        .unwrap_or_else(|e| panic!("the sweep failed: {}", redact(&e.to_string(), &panel)));
    eprintln!("live_provider: sweep asked {} listed {} failed {}", swept.asked, swept.listed, swept.failed);
    assert!(swept.asked >= 6, "the sweep stopped before asking about six shows");
    assert!(swept.asked <= 6 + 4, "the sweep ran on after being told to stop");
}

fn redact(message: &str, panel: &Panel) -> String {
    message
        .replace(&panel.pass, "***")
        .replace(&panel.user, "***")
}
