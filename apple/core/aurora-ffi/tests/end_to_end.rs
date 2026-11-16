//! The spike's whole path, through the API Swift calls: add a provider, import it,
//! list what arrived, tune a channel, and report how the tune went.
//!
//! Served from a loopback HTTP server so it needs no network. The free Pluto guides are
//! switched off for the same reason.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use aurora_ffi::{
    AuroraCore, ImportContent, ImportListener, ImportPhase, SecretError, SecretStore,
};

const PLAYLIST: &str = "#EXTM3U
#EXTINF:-1 tvg-id=\"news.uk\" group-title=\"News\",Aurora News HD
http://streams.example/news.ts
#EXTINF:-1 tvg-id=\"sport.uk\" group-title=\"Sport\",Aurora Sport
http://streams.example/sport.ts
";

/// Answers every request with the playlist, until the test ends.
fn serve_playlist() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            // Read the request head and ignore it.
            while reader.read_line(&mut line).map(|n| n > 2).unwrap_or(false) {
                line.clear();
            }
            let mut stream = &stream;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: audio/x-mpegurl\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                PLAYLIST.len(),
                PLAYLIST
            );
        }
    });
    format!("http://{addr}/playlist.m3u")
}

#[derive(Default)]
struct Secrets(Mutex<std::collections::HashMap<String, String>>);

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

#[derive(Default)]
struct Counting {
    calls: AtomicUsize,
    finished: Mutex<bool>,
}

impl ImportListener for Counting {
    fn progress(&self, phase: ImportPhase, _done: u64, _total: u64) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if phase == ImportPhase::Done {
            *self.finished.lock().unwrap() = true;
        }
    }
}

#[test]
fn a_playlist_is_imported_listed_and_tuned() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().display().to_string();

    // Stop the import from fetching the public Pluto guides: this test is offline.
    {
        let conn = aurora_db::open(dir.path().join("library.db")).unwrap();
        aurora_db::repo::settings::set(&conn, "guide.free_sources", &false).unwrap();
    }

    let core = AuroraCore::open(data_dir, Arc::new(Secrets::default())).unwrap();
    let id = core.add_m3u("Test list".into(), serve_playlist()).unwrap();

    let listener = Arc::new(Counting::default());
    let report = core
        .refresh(
            id,
            ImportContent {
                live: true,
                vod: false,
                series: false,
            },
            listener.clone(),
        )
        .unwrap();
    assert_eq!(report.channels, 2, "warnings: {:?}", report.warnings);
    assert!(
        listener.calls.load(Ordering::SeqCst) > 0,
        "no progress was reported"
    );

    let providers = core.providers().unwrap();
    assert_eq!(providers[0].channel_count, 2);
    assert!(providers[0].last_refresh_at.is_some());

    let groups: Vec<String> = core
        .channel_groups()
        .unwrap()
        .into_iter()
        .map(|g| g.name)
        .collect();
    assert_eq!(groups, vec!["News".to_string(), "Sport".to_string()]);

    let news = core.channels(1, Some("News".into()), false, 50, 0).unwrap();
    assert_eq!(news.len(), 1);
    assert_eq!(news[0].name, "Aurora News HD");
    assert!(
        !news[0].stale,
        "a channel the refresh just listed is not dropped"
    );

    let tune = core.tune_live(news[0].id).unwrap();
    assert_eq!(tune.title, "Aurora News HD");
    assert_eq!(tune.sources.len(), 1);
    assert_eq!(tune.sources[0].url, "http://streams.example/news.ts");
    let option = |name: &str| {
        tune.mpv_options
            .iter()
            .find(|o| o.name == name)
            .map(|o| o.value.clone())
    };
    assert!(option("user-agent").is_some_and(|ua| !ua.is_empty()));
    assert_eq!(option("cache-secs").as_deref(), Some("8"));

    // Favourites are per profile, and the list filter honours them.
    assert!(core.toggle_favorite(1, news[0].id).unwrap());
    let favourites = core.channels(1, None, true, 50, 0).unwrap();
    assert_eq!(favourites.len(), 1);
    assert!(favourites[0].favorite);
    assert!(!core.toggle_favorite(1, news[0].id).unwrap());

    // The import indexed the channels for search.
    let found = core.search("sport".into(), 10).unwrap();
    assert_eq!(found.channels.len(), 1, "{found:?}");
    assert_eq!(found.channels[0].title, "Aurora Sport");

    core.report_source(tune.sources[0].source_id, true).unwrap();
    core.report_source(tune.sources[0].source_id, false)
        .unwrap();
}
