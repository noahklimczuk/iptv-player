//! Window composition and playback URL resolution.

use aurora_core::catchup;
use aurora_db::repo::channels;
use aurora_db::rusqlite::Connection;
use aurora_player::backend::LoadOptions;

use crate::error::{AppError, Result};

/// Network buffer presets (README §6.1).
pub fn cache_secs_for(is_live: bool) -> u32 {
    if is_live {
        8
    } else {
        30
    }
}

/// Where a channel's catch-up configuration lives, for `resolve_catchup`.
#[derive(Debug, Clone)]
pub struct CatchupChannel {
    pub name: String,
    pub stream_url: String,
    pub mode: Option<String>,
    pub source: Option<String>,
    pub days: u16,
}

pub fn catchup_channel(db: &Connection, channel_id: i64) -> Result<CatchupChannel> {
    db.query_row(
        "SELECT c.name, c.catchup_mode, c.catchup_source, c.catchup_days,
                (SELECT url FROM channel_sources s WHERE s.channel_id = c.id
                 ORDER BY s.priority, s.fail_count LIMIT 1)
         FROM channels c WHERE c.id = ?1",
        [channel_id],
        |r| {
            Ok(CatchupChannel {
                name: r.get(0)?,
                mode: r.get(1)?,
                source: r.get(2)?,
                days: r.get::<_, i64>(3)?.clamp(0, i64::from(u16::MAX)) as u16,
                stream_url: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
            })
        },
    )
    .map_err(|_| AppError::Other(format!("unknown channel {channel_id}")))
}

/// Resolve "watch this programme from the start".
///
/// Fails with something the user can act on rather than a blank player: a channel
/// without catch-up, a programme past the provider's window, and a provider whose
/// configuration does not say how to ask are three different problems.
pub fn resolve_catchup(
    db: &Connection,
    channel_id: i64,
    start: i64,
    stop: i64,
    now: i64,
) -> Result<(String, LoadOptions)> {
    let ch = catchup_channel(db, channel_id)?;

    let Some(mode) = ch.mode.as_deref().and_then(catchup::Mode::parse) else {
        return Err(AppError::Other(format!(
            "{} does not offer catch-up",
            ch.name
        )));
    };
    if !catchup::is_available(start, now, ch.days) {
        return Err(AppError::Other(if start > now {
            format!("{} has not aired yet", ch.name)
        } else {
            format!(
                "That programme is outside {}'s {}-day catch-up window",
                ch.name, ch.days
            )
        }));
    }

    let url = catchup::url_for(&catchup::Request {
        stream_url: &ch.stream_url,
        mode,
        source: ch.source.as_deref(),
        start,
        stop,
        now,
    })
    .ok_or_else(|| {
        AppError::Other(format!(
            "{} says it has catch-up but not how to request it",
            ch.name
        ))
    })?;

    Ok((
        url,
        LoadOptions {
            // Catch-up is a recording being served back: seekable, and worth buffering
            // like VOD rather than like a live edge.
            is_live: false,
            cache_secs: cache_secs_for(false),
            title: Some(ch.name),
            ..Default::default()
        },
    ))
}

/// Every URL a channel can be played from, best bet first, plus the options they share.
///
/// Returns the list rather than a winner because one URL is only a guess: providers
/// hand out streams that 404, stall, or die mid-programme, and the next one along is
/// usually fine (README §7.14, §23 item 4).
pub fn live_sources(
    db: &Connection,
    channel_id: i64,
    now: i64,
) -> Result<(Vec<aurora_db::repo::sources::Source>, LoadOptions)> {
    let ch = channels::list(db, &channels::ChannelFilter::default())?
        .into_iter()
        .find(|c| c.id == channel_id)
        .ok_or_else(|| AppError::Other(format!("unknown channel {channel_id}")))?;

    let sources = aurora_db::repo::sources::for_channel(db, channel_id, now)?;
    Ok((
        sources,
        LoadOptions {
            is_live: true,
            cache_secs: cache_secs_for(true),
            title: Some(ch.name),
            ..Default::default()
        },
    ))
}

/// Turn a library item into a playable URL plus the options it needs.
///
/// Credentials live in Windows Credential Manager, not the database (README C10), so
/// the provider layer injects them here rather than storing resolved URLs.
pub fn resolve_playback(
    db: &Connection,
    kind: &str,
    id: i64,
    position_secs: Option<f64>,
) -> Result<(String, LoadOptions)> {
    match kind {
        "live" => {
            let (sources, options) = live_sources(db, id, crate::now_unix())?;
            let first = sources
                .into_iter()
                .next()
                .ok_or_else(|| AppError::Other(format!("channel {id} has no stream URL")))?;
            Ok((first.url, options))
        }
        "movie" => {
            let (url, title): (String, String) = db
                .query_row("SELECT url, title FROM movies WHERE id = ?1", [id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(aurora_db::DbError::from)?;
            Ok((
                url,
                LoadOptions {
                    is_live: false,
                    cache_secs: cache_secs_for(false),
                    start_at_secs: position_secs,
                    title: Some(title),
                    ..Default::default()
                },
            ))
        }
        "episode" => {
            let (url, title): (String, Option<String>) = db
                .query_row("SELECT url, title FROM episodes WHERE id = ?1", [id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
                })
                .map_err(aurora_db::DbError::from)?;
            Ok((
                url,
                LoadOptions {
                    is_live: false,
                    cache_secs: cache_secs_for(false),
                    start_at_secs: position_secs,
                    title,
                    ..Default::default()
                },
            ))
        }
        other => Err(AppError::Other(format!("unknown media kind {other:?}"))),
    }
}

/// Compose the video surface behind the transparent WebView2 (README §2.1).
///
/// UNVERIFIED on real hardware — this is the Phase 0 spike (docs/ROADMAP.md).
#[cfg(windows)]
pub fn attach_video_surface(
    window: &tauri::WebviewWindow,
    player: &mut dyn aurora_player::PlayerBackend,
) -> Result<()> {
    use tauri::window::Color;

    // A transparent WebView2 background is what lets the UI float over mpv's output.
    let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));

    let size = window
        .inner_size()
        .map_err(|e| AppError::Other(format!("inner_size failed: {e}")))?;
    player.resize(size.width, size.height)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_buffers_shallower_than_vod() {
        assert!(cache_secs_for(true) < cache_secs_for(false));
    }

    #[test]
    fn unknown_kinds_are_rejected_with_a_clear_message() {
        let db = aurora_db::open_memory().unwrap();
        let err = resolve_playback(&db, "podcast", 1, None).unwrap_err();
        assert!(err.to_string().contains("podcast"));
    }

    #[test]
    fn resolves_a_movie_url_and_start_position() {
        let mut db = aurora_db::open_memory().unwrap();
        db.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        aurora_db::repo::library::upsert_movies(
            &mut db,
            1,
            &[aurora_db::repo::library::NewMovie {
                provider_key: "m1".into(),
                title: "Example Film".into(),
                match_key: "examplefilm".into(),
                url: "https://example.com/m/1.mkv".into(),
                ..Default::default()
            }],
            0,
        )
        .unwrap();

        let id: i64 = db
            .query_row("SELECT id FROM movies LIMIT 1", [], |r| r.get(0))
            .unwrap();
        let (url, opts) = resolve_playback(&db, "movie", id, Some(90.0)).unwrap();
        assert_eq!(url, "https://example.com/m/1.mkv");
        assert!(!opts.is_live);
        assert_eq!(opts.start_at_secs, Some(90.0));
        assert_eq!(opts.title.as_deref(), Some("Example Film"));
    }

    #[test]
    fn a_missing_item_is_an_error_not_a_panic() {
        let db = aurora_db::open_memory().unwrap();
        assert!(resolve_playback(&db, "movie", 9999, None).is_err());
    }

    /// A channel with catch-up configured the way a provider would report it.
    fn seed_channel(db: &Connection, mode: Option<&str>, days: i64, url: &str) -> i64 {
        db.execute(
            "INSERT OR IGNORE INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','xtream','https://example.com',0)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO channels (provider_id, provider_key, name, match_key,
                                   catchup_mode, catchup_days, last_seen_at)
             VALUES (1, 'c1', 'Channel Four', 'channelfour', ?1, ?2, 0)",
            aurora_db::rusqlite::params![mode, days],
        )
        .unwrap();
        let id = db.last_insert_rowid();
        db.execute(
            "INSERT INTO channel_sources (channel_id, url) VALUES (?1, ?2)",
            aurora_db::rusqlite::params![id, url],
        )
        .unwrap();
        id
    }

    const NOW: i64 = 1_760_000_000;

    #[test]
    fn catch_up_plays_back_seekable_and_named_after_the_channel() {
        let db = aurora_db::open_memory().unwrap();
        let id = seed_channel(&db, Some("shift"), 7, "http://example.com/live/a/b/9.ts");

        let (url, opts) = resolve_catchup(&db, id, NOW - 7200, NOW - 3600, NOW).unwrap();

        // A replay, not a live edge: seekable, buffered like VOD.
        assert!(url.contains("utc=") && url.contains("lutc="), "{url}");
        assert!(!opts.is_live);
        assert_eq!(opts.cache_secs, cache_secs_for(false));
        assert_eq!(opts.title.as_deref(), Some("Channel Four"));
    }

    #[test]
    fn a_channel_without_catch_up_is_refused_by_name() {
        let db = aurora_db::open_memory().unwrap();
        let id = seed_channel(&db, None, 0, "http://example.com/live/a/b/9.ts");

        let err = resolve_catchup(&db, id, NOW - 7200, NOW - 3600, NOW)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Channel Four"), "{err}");
        assert!(err.contains("does not offer catch-up"), "{err}");
    }

    #[test]
    fn too_early_and_too_late_are_different_refusals() {
        let db = aurora_db::open_memory().unwrap();
        let id = seed_channel(&db, Some("shift"), 7, "http://example.com/live/a/b/9.ts");

        // Tomorrow: nothing to replay yet.
        let future = resolve_catchup(&db, id, NOW + 86400, NOW + 90000, NOW)
            .unwrap_err()
            .to_string();
        assert!(future.contains("has not aired yet"), "{future}");

        // A month ago: the provider stopped keeping it, and the window is named so the
        // viewer knows how far back they can go.
        let old = resolve_catchup(&db, id, NOW - 30 * 86400, NOW - 29 * 86400, NOW)
            .unwrap_err()
            .to_string();
        assert!(old.contains("7-day catch-up window"), "{old}");
    }

    #[test]
    fn a_channel_that_cannot_be_asked_says_that_rather_than_guessing() {
        let db = aurora_db::open_memory().unwrap();
        // Xtream catch-up needs credentials out of the stream URL; this one has none,
        // and inventing them would produce a URL that fails silently at the player.
        let id = seed_channel(&db, Some("xtream"), 7, "http://example.com/stream.ts");

        let err = resolve_catchup(&db, id, NOW - 7200, NOW - 3600, NOW)
            .unwrap_err()
            .to_string();
        assert!(err.contains("not how to request it"), "{err}");
    }

    #[test]
    fn an_unknown_channel_is_an_error_not_a_panic() {
        let db = aurora_db::open_memory().unwrap();
        assert!(resolve_catchup(&db, 4242, NOW - 7200, NOW - 3600, NOW).is_err());
    }
}
