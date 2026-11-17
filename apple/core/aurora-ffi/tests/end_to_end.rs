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

/// `{guide}` becomes the address of the guide this server also serves.
const PLAYLIST: &str = "#EXTM3U url-tvg=\"{guide}\"
#EXTINF:-1 tvg-id=\"news.uk\" group-title=\"News\" catchup=\"append\" catchup-days=\"3\",Aurora News HD
http://streams.example/news.ts
#EXTINF:-1 tvg-id=\"sport.uk\" group-title=\"Sport\",Aurora Sport
http://streams.example/sport.ts
";

/// Answers every request with the playlist, until the test ends.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// XMLTV's time format, in UTC: `20261116094500 +0000`.
fn xmltv_time(unix: i64) -> String {
    // Days to a civil date (Howard Hinnant's algorithm), to need no date crate.
    let (days, secs) = (unix.div_euclid(86_400), unix.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}{:02}{:02}{:02} +0000",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

/// The News channel's guide around `at`: one programme that has aired (and is in its
/// three-day archive), one on now, one still to come. Sport has none.
fn guide_around(at: i64) -> String {
    let hour = 3600;
    let programme = |start: i64, stop: i64, title: &str| {
        format!(
            "<programme start=\"{}\" stop=\"{}\" channel=\"news.uk\"><title>{title}</title></programme>",
            xmltv_time(start),
            xmltv_time(stop)
        )
    };
    format!(
        "<?xml version=\"1.0\"?><tv><channel id=\"news.uk\"><display-name>Aurora News HD</display-name></channel>{}{}{}</tv>",
        programme(at - 2 * hour, at - hour, "Morning Briefing"),
        programme(at - hour, at + hour, "The Midday News"),
        programme(at + hour, at + 2 * hour, "Evening Report"),
    )
}

/// Serves the playlist, and the guide its header points at. Returns the playlist's URL
/// and the time the guide is centred on.
fn serve_playlist() -> (String, i64) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let at = now();
    let playlist = PLAYLIST.replace("{guide}", &format!("http://{addr}/guide.xml"));
    let guide = guide_around(at);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            let mut first = String::new();
            // Read the request head; only its first line matters.
            while reader.read_line(&mut line).map(|n| n > 2).unwrap_or(false) {
                if first.is_empty() {
                    first = line.clone();
                }
                line.clear();
            }
            let (kind, body) = if first.contains("/guide.xml") {
                ("application/xml", &guide)
            } else {
                ("audio/x-mpegurl", &playlist)
            };
            let mut stream = &stream;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (format!("http://{addr}/playlist.m3u"), at)
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
    let (playlist, at) = serve_playlist();
    let id = core.add_m3u("Test list".into(), playlist).unwrap();

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

    let news = core.channels(1, Some("News".into()), false, None, 50, 0).unwrap();
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
            .find(|o| o.name() == name)
            .and_then(|o| o.text().map(str::to_string))
    };
    assert!(option("user-agent").is_some_and(|ua| !ua.is_empty()));
    assert_eq!(option("cache-secs").as_deref(), Some("8"));

    // Favourites are per profile, and the list filter honours them.
    assert!(core.toggle_favorite(1, news[0].id, None).unwrap());
    let favourites = core.channels(1, None, true, None, 50, 0).unwrap();
    assert_eq!(favourites.len(), 1);
    assert!(favourites[0].favorite);
    assert!(!core.toggle_favorite(1, news[0].id, None).unwrap());

    // Named lists, as on Windows: a list exists once something is in it, the default
    // is always offered, the flag answers for the list asked about, and a list keeps
    // the order it was given.
    let sport = core.channels(1, Some("Sport".into()), false, None, 50, 0).unwrap();
    let names = |core: &AuroraCore| -> Vec<(String, u32)> {
        core.favorite_lists(1).unwrap().into_iter().map(|l| (l.name, l.channels)).collect()
    };
    assert_eq!(names(&core).len(), 1, "only the default, empty: {:?}", names(&core));
    assert!(core.toggle_favorite(1, news[0].id, Some("Evening".into())).unwrap());
    assert!(core.toggle_favorite(1, sport[0].id, Some("Evening".into())).unwrap());
    assert!(names(&core).contains(&("Evening".to_string(), 2)));
    let evening = core.channels(1, None, true, Some("Evening".into()), 50, 0).unwrap();
    assert_eq!(evening.iter().map(|c| c.id).collect::<Vec<_>>(), vec![news[0].id, sport[0].id]);
    assert!(evening.iter().all(|c| c.favorite));
    assert!(core.channels(1, None, true, None, 50, 0).unwrap().is_empty(), "not in the default");
    core.reorder_favorites(1, Some("Evening".into()), vec![sport[0].id, news[0].id]).unwrap();
    let evening = core.channels(1, None, true, Some("Evening".into()), 50, 0).unwrap();
    assert_eq!(evening.iter().map(|c| c.id).collect::<Vec<_>>(), vec![sport[0].id, news[0].id]);
    assert_eq!(core.rename_favorite_list(1, "Evening".into(), "Nights".into()).unwrap(), 2);
    assert!(names(&core).contains(&("Nights".to_string(), 2)));
    assert_eq!(core.delete_favorite_list(1, "Nights".into()).unwrap(), 2);
    assert_eq!(names(&core).len(), 1);

    // The import indexed the channels for search.
    let found = core.search("sport".into(), 10).unwrap();
    assert_eq!(found.channels.len(), 1, "{found:?}");
    assert_eq!(found.channels[0].title, "Aurora Sport");

    // Catch-up: News keeps a three-day archive, so an hour ago replays; tomorrow and
    // ten days ago don't. The URL is the shared rule's (`append` adds utc and lutc).
    assert!(news[0].has_catchup);
    let now = now();
    let hour_ago = now - 3600;
    assert_eq!(
        core.catch_up_available(news[0].id, vec![hour_ago, now + 86_400, now - 10 * 86_400])
            .unwrap(),
        [true, false, false]
    );
    let replay = core
        .tune_catchup(news[0].id, hour_ago, hour_ago + 1800)
        .unwrap();
    assert!(
        replay.url.starts_with(&format!(
            "http://streams.example/news.ts?utc={hour_ago}&lutc="
        )),
        "{}",
        replay.url
    );
    assert!(replay
        .mpv_options
        .iter()
        .any(|o| o.name() == "cache-secs" && o.text() == Some("30")));

    // The guide arrived from the playlist's own url-tvg, and the grid shows it: only the
    // rows asked for, one per channel in order, the aired programme replayable.
    let sport_id = core
        .channels(1, Some("Sport".into()), false, None, 50, 0)
        .unwrap()[0]
        .id;
    let rows = core
        .guide_rows(vec![news[0].id, sport_id], at - 3 * 3600, at + 3 * 3600)
        .unwrap();
    assert_eq!(rows.len(), 2);
    let titles: Vec<&str> = rows[0]
        .cells
        .iter()
        .map(|c| c.programme.title.as_str())
        .collect();
    assert_eq!(
        titles,
        ["Morning Briefing", "The Midday News", "Evening Report"]
    );
    let replayable: Vec<bool> = rows[0].cells.iter().map(|c| c.replayable).collect();
    assert_eq!(
        replayable,
        [true, false, false],
        "only what has aired, and is archived, replays"
    );
    assert!(
        rows[1].cells.is_empty(),
        "Sport has no guide: an empty row, not a missing one"
    );
    let on_now = core.now_next(vec![news[0].id]).unwrap();
    assert_eq!(
        on_now[0].now.as_ref().map(|p| p.title.as_str()),
        Some("The Midday News")
    );
    assert_eq!(
        on_now[0].next.as_ref().map(|p| p.title.as_str()),
        Some("Evening Report")
    );

    core.report_source(tune.sources[0].source_id, true).unwrap();
    core.report_source(tune.sources[0].source_id, false)
        .unwrap();
}
