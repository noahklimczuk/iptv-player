use std::sync::mpsc;
use std::time::Duration;

use aurora_db::repo::parental::{set_lock, LockKind};
use aurora_db::repo::profiles;
use aurora_video::model::Report;

use super::*;

fn live(status: Status) -> PlayerState {
    PlayerState {
        title: Some("BBC One".into()),
        channel_id: Some(10),
        item_kind: Some(ItemKind::Live),
        report: Report {
            status,
            is_live: true,
            ..Report::default()
        },
        ..PlayerState::default()
    }
}

fn film(status: Status, position_secs: f64) -> PlayerState {
    PlayerState {
        title: Some("Heat".into()),
        subtitle: Some("1995".into()),
        item_kind: Some(ItemKind::Movie),
        item_id: Some(2),
        report: Report {
            status,
            position_secs,
            duration_secs: 6000.0,
            ..Report::default()
        },
        ..PlayerState::default()
    }
}

fn logo() -> Item {
    Item {
        artwork: Some(Artwork::Url("https://img.example/bbc.png".into())),
        programme: Some("The News".into()),
        ..Item::default()
    }
}

#[test]
fn live_tv_shows_the_channel_and_the_programme_and_changes_channel() {
    let n = now_playing(&live(Status::Playing), &logo());
    assert_eq!(n.playback, Playback::Playing);
    assert_eq!(n.shown.title, "BBC One");
    assert_eq!(n.shown.subtitle.as_deref(), Some("The News"));
    assert_eq!(n.shown.artwork, logo().artwork);
    assert!(n.channels);
    assert_eq!(n.timeline, None, "live TV has no bar");
}

#[test]
fn a_film_has_a_bar_and_no_next() {
    let n = now_playing(&film(Status::Paused, 7000.0), &Item::default());
    assert_eq!(n.playback, Playback::Paused);
    assert_eq!(n.shown.subtitle.as_deref(), Some("1995"));
    assert!(!n.channels);
    assert_eq!(
        n.timeline,
        Some(Timeline {
            position_secs: 6000.0,
            duration_secs: 6000.0
        }),
        "clamped to the length"
    );
}

#[test]
fn something_private_is_only_ever_aurora_tv() {
    let item = Item {
        private: true,
        ..logo()
    };
    let n = now_playing(&live(Status::Playing), &item);
    assert_eq!(
        n.shown,
        Shown {
            title: APP_NAME.into(),
            subtitle: None,
            artwork: None
        }
    );
    // The buttons still work: it is the words that are private, not the keys.
    assert!(n.channels);
}

#[test]
fn opening_reconnecting_and_stopping_read_as_windows_reads_them() {
    assert_eq!(
        now_playing(&live(Status::Loading), &Item::default()).playback,
        Playback::Changing
    );
    let mut dropped = live(Status::Playing);
    dropped.reconnecting = true;
    assert_eq!(
        now_playing(&dropped, &Item::default()).playback,
        Playback::Changing
    );
    let idle = now_playing(&live(Status::Idle), &Item::default());
    assert_eq!(idle.playback, Playback::Closed);
    assert!(!idle.channels);
    assert_eq!(
        now_playing(&film(Status::Error, 0.0), &Item::default()).playback,
        Playback::Closed
    );

    let mut untitled = live(Status::Playing);
    untitled.title = Some("  ".into());
    assert_eq!(
        now_playing(&untitled, &Item::default()).shown.title,
        APP_NAME
    );
}

#[test]
fn the_first_state_sends_everything_and_a_heartbeat_sends_nothing() {
    let mut p = Publisher::default();
    let first = p.next(now_playing(&live(Status::Playing), &logo()), 0.0);
    assert_eq!(first.playback, Some(Playback::Playing));
    assert_eq!(
        first.shown.as_ref().map(|s| s.title.as_str()),
        Some("BBC One")
    );
    assert_eq!(first.channels, Some(true));
    assert_eq!(
        first.timeline,
        Some(None),
        "the bar is taken away for live TV"
    );

    for t in 1..20 {
        let again = p.next(now_playing(&live(Status::Playing), &logo()), f64::from(t));
        assert!(again.is_empty(), "at {t}s: {again:?}");
    }
}

#[test]
fn the_position_goes_out_on_a_seek_a_pause_and_every_few_seconds() {
    let mut p = Publisher::default();
    let first = p.next(
        now_playing(&film(Status::Playing, 100.0), &Item::default()),
        0.0,
    );
    assert!(first.timeline.is_some());

    // Playing on as expected: nothing.
    let on = p.next(
        now_playing(&film(Status::Playing, 101.0), &Item::default()),
        1.0,
    );
    assert!(on.is_empty(), "{on:?}");
    // Five seconds on, the bar is moved along anyway.
    let refresh = p.next(
        now_playing(&film(Status::Playing, 105.0), &Item::default()),
        5.0,
    );
    assert!(refresh.timeline.is_some());
    // A seek: far from where playback would have taken it.
    let seek = p.next(
        now_playing(&film(Status::Playing, 900.0), &Item::default()),
        6.0,
    );
    assert_eq!(
        seek.timeline,
        Some(Some(Timeline {
            position_secs: 900.0,
            duration_secs: 6000.0
        }))
    );
    // A pause sends where it stopped, then stays quiet.
    let paused = p.next(
        now_playing(&film(Status::Paused, 900.5), &Item::default()),
        6.5,
    );
    assert_eq!(paused.playback, Some(Playback::Paused));
    assert!(paused.timeline.is_some());
    let still = p.next(
        now_playing(&film(Status::Paused, 900.5), &Item::default()),
        20.0,
    );
    assert!(
        still.is_empty(),
        "paused, the position does not move: {still:?}"
    );
}

#[test]
fn closing_says_only_that_and_the_next_title_starts_afresh() {
    let mut p = Publisher::default();
    p.next(now_playing(&live(Status::Playing), &logo()), 0.0);
    let closed = p.next(now_playing(&live(Status::Idle), &logo()), 1.0);
    assert_eq!(
        closed,
        Changes {
            playback: Some(Playback::Closed),
            ..Changes::default()
        }
    );
    assert!(p
        .next(now_playing(&live(Status::Idle), &logo()), 2.0)
        .is_empty());

    // The same channel again: Windows forgot it when it closed, so it is all sent again.
    let back = p.next(now_playing(&live(Status::Loading), &logo()), 3.0);
    assert_eq!(back.playback, Some(Playback::Changing));
    assert!(back.shown.is_some());
    assert_eq!(back.channels, Some(true));
    assert_eq!(back.timeline, Some(None));
}

#[test]
fn a_press_reaches_the_page_in_its_words() {
    let v = |b: Button| serde_json::to_value(b).unwrap();
    assert_eq!(v(Button::Play), serde_json::json!({ "button": "play" }));
    assert_eq!(
        v(Button::Previous),
        serde_json::json!({ "button": "previous" })
    );
    assert_eq!(
        v(Button::Seek {
            position_secs: 12.5
        }),
        serde_json::json!({ "button": "seek", "positionSecs": 12.5 })
    );
}

fn library() -> TimedMutex<Connection> {
    let conn = aurora_db::open_memory().unwrap();
    conn.execute_batch(
        "INSERT INTO providers (id,name,kind,base_url,created_at)
              VALUES (1,'Line','xtream','http://panel.example',0);
         INSERT INTO channels (id,provider_id,provider_key,name,match_key,group_title,logo,last_seen_at)
              VALUES (10,1,'10','BBC One','bbcone','UK','https://img.example/bbc.png',0),
                     (11,1,'11','Hot TV','hottv','Adult 18+','https://img.example/hot.png',0),
                     (12,1,'12','Local','local','UK','http://img.example/local.png',0),
                     (13,1,'13','Kids','kids','Kids','https://img.example/kids.png',0);
         INSERT INTO movies (id,provider_id,provider_key,title,match_key,url,poster,group_title,last_seen_at)
              VALUES (2,1,'2','Heat','heat','http://x/2','https://img.example/heat.jpg','Action',0);
         INSERT INTO series (id,provider_id,provider_key,title,match_key,poster,group_title,last_seen_at)
              VALUES (1,1,'1','Bluey','bluey','https://img.example/bluey.jpg','Kids',0);
         INSERT INTO episodes (id,series_id,season,episode,title,url,still)
              VALUES (1,1,1,1,'Magic Xylophone','http://x/e1','https://img.example/e1.jpg');",
    )
    .unwrap();
    profiles::set_master_pin(&conn, Some("2468"), 0).unwrap();
    set_lock(&conn, LockKind::Channel, "13", true).unwrap();
    TimedMutex::new(conn)
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("aurora-media-keys-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_lookup_finds_the_picture_and_keeps_private_things_private() {
    let db = library();
    let scratch = Scratch::new("lookup");
    let art = artwork::Cache::new(&scratch.0);

    let bbc = lookup(&db, &art, &live(Status::Playing));
    assert!(!bbc.private);
    assert_eq!(
        bbc.artwork,
        Some(Artwork::Url("https://img.example/bbc.png".into()))
    );

    // Once cached, the file is used rather than the network.
    let cached = art.path_for("https://img.example/bbc.png");
    std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
    std::fs::write(&cached, b"png").unwrap();
    assert_eq!(
        lookup(&db, &art, &live(Status::Playing)).artwork,
        Some(Artwork::File(cached))
    );

    let on = |ch: i64| {
        let mut s = live(Status::Playing);
        s.channel_id = Some(ch);
        lookup(&db, &art, &s)
    };
    // Plain HTTP is not handed to Windows to fetch.
    assert_eq!(on(12).artwork, None);
    // Adult, and locked: nothing but the name of the app, and no picture.
    assert_eq!(
        on(11),
        Item {
            private: true,
            ..Item::default()
        }
    );
    assert_eq!(
        on(13),
        Item {
            private: true,
            ..Item::default()
        }
    );

    assert_eq!(
        lookup(&db, &art, &film(Status::Playing, 0.0)).artwork,
        Some(Artwork::Url("https://img.example/heat.jpg".into()))
    );
    let episode = PlayerState {
        item_kind: Some(ItemKind::Episode),
        item_id: Some(1),
        ..film(Status::Playing, 0.0)
    };
    assert_eq!(
        lookup(&db, &art, &episode).artwork,
        Some(Artwork::Url("https://img.example/bluey.jpg".into())),
        "an episode is shown with its show's poster"
    );
    // Something played by address is nobody's: no lookup at all.
    let address = PlayerState {
        item_kind: None,
        channel_id: None,
        item_id: None,
        ..film(Status::Playing, 0.0)
    };
    assert_eq!(lookup(&db, &art, &address), Item::default());
}

struct Recorder(mpsc::Sender<Changes>);

impl Controls for Recorder {
    fn apply(&mut self, changes: &Changes) -> Result<(), String> {
        self.0.send(changes.clone()).map_err(|e| e.to_string())
    }
}

#[test]
fn the_thread_looks_up_once_per_title_and_sends_only_changes() {
    let (tx, rx) = mpsc::channel();
    let (asked_tx, asked) = mpsc::channel();
    let keys = MediaKeys::start(
        move || Some(Box::new(Recorder(tx)) as Box<dyn Controls>),
        move |s: &PlayerState| {
            asked_tx.send(s.title.clone()).unwrap();
            logo()
        },
    );
    let wait = Duration::from_secs(5);

    keys.push(&film(Status::Playing, 10.0));
    let first = rx.recv_timeout(wait).unwrap();
    assert_eq!(first.shown.map(|s| s.title), Some("Heat".into()));
    keys.push(&film(Status::Paused, 10.2));
    assert_eq!(
        rx.recv_timeout(wait).unwrap().playback,
        Some(Playback::Paused)
    );
    keys.push(&live(Status::Playing));
    let zapped = rx.recv_timeout(wait).unwrap();
    assert_eq!(zapped.shown.map(|s| s.title), Some("BBC One".into()));
    assert_eq!(zapped.channels, Some(true));

    let titles: Vec<_> = asked.try_iter().collect();
    assert_eq!(
        titles,
        [Some("Heat".to_string()), Some("BBC One".to_string())]
    );
}
