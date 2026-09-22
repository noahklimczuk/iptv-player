//! Window composition and playback URL resolution.

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
            let ch = channels::list(db, &channels::ChannelFilter::default())?
                .into_iter()
                .find(|c| c.id == id)
                .ok_or_else(|| AppError::Other(format!("unknown channel {id}")))?;

            let url: String = db
                .query_row(
                    "SELECT url FROM channel_sources WHERE channel_id = ?1
                     ORDER BY priority, fail_count LIMIT 1",
                    [id],
                    |r| r.get::<_, String>(0),
                )
                .map_err(aurora_db::DbError::from)?;

            Ok((
                url,
                LoadOptions {
                    is_live: true,
                    cache_secs: cache_secs_for(true),
                    title: Some(ch.name),
                    ..Default::default()
                },
            ))
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
}
