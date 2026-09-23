// Release builds must not pop a console window behind the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use aurora_app::{
    commands, dvr, library, metadata, now_unix, playlist, profiles, providers, services::Services,
    updates,
};

/// Send the log somewhere a person can read it.
///
/// A release build sets `windows_subsystem = "windows"`, so it has no console and
/// anything written to stdout goes nowhere — including the one line that distinguishes
/// "libmpv would not load" from "the video is behind the window". So release builds log
/// to a file beside their data, and debug builds keep the console they already have.
///
/// Truncated per run: the question being asked of a log is almost always about the
/// launch that just failed, not the twenty before it.
fn init_logging(data_dir: &std::path::Path) {
    let filter =
        tracing_subscriber::EnvFilter::try_from_env("AURORA_LOG").unwrap_or_else(|_| "info".into());

    if cfg!(debug_assertions) {
        tracing_subscriber::fmt().with_env_filter(filter).init();
        return;
    }

    let _ = std::fs::create_dir_all(data_dir);
    match std::fs::File::create(data_dir.join("aurora.log")) {
        Ok(file) => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
            .init(),
        // Nowhere to write and nowhere to say so; the app still runs.
        Err(_) => tracing_subscriber::fmt().with_env_filter(filter).init(),
    }
}

/// How often the player's state is read. Fast enough that the OSD's clock and buffer
/// readout look live, slow enough to be free.
const PLAYER_TICK: std::time::Duration = std::time::Duration::from_millis(250);

/// How long after launch the update check runs. Far enough back to be out of the way
/// of the first frame, near enough that Settings has an answer by the time anyone
/// opens it.
const UPDATE_CHECK_DELAY: std::time::Duration = std::time::Duration::from_secs(8);

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            use tauri::Manager;
            // Portable mode (README §13): a marker file next to the exe keeps all data
            // local, with no %LOCALAPPDATA% and no registry.
            let portable = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("portable.txt")))
                .filter(|p| p.exists())
                .and_then(|p| p.parent().map(|d| d.join("data")));

            let data_dir = match portable {
                Some(dir) => dir,
                None => app
                    .path()
                    .app_local_data_dir()
                    .map_err(|e| format!("no data dir: {e}"))?,
            };

            init_logging(&data_dir);
            tracing::info!(
                "Aurora TV {} starting, data in {}",
                env!("CARGO_PKG_VERSION"),
                data_dir.display()
            );

            let services = Services::new(data_dir)?;
            let scheduler = std::sync::Arc::clone(&services.dvr);
            let playback_handle = std::sync::Arc::clone(&services.playback);
            app.manage(services);

            // The player's heartbeat. The backend only knows its state when asked, and
            // the UI's OSD is driven by a `player.state` event, so without this a
            // stream that died leaves the interface showing it playing — and nothing
            // would ever notice a live channel needs rolling to its next source.
            use tauri::Emitter;
            let playback = std::sync::Arc::clone(&playback_handle);
            let player_handle = app.handle().clone();
            std::thread::Builder::new()
                .name("aurora-player".into())
                .spawn(move || loop {
                    std::thread::sleep(PLAYER_TICK);
                    if let Some(state) = playback.tick(now_unix()) {
                        let _ = player_handle.emit("player.state", &state);
                    }
                })
                .expect("spawning the player thread");

            // The DVR has to keep its own time: nothing in the UI is guaranteed to be
            // open when a recording is due, and a minimised window still records.
            let handle = app.handle().clone();
            std::thread::Builder::new()
                .name("aurora-dvr".into())
                .spawn(move || loop {
                    std::thread::sleep(dvr::TICK_INTERVAL);
                    match scheduler.tick(now_unix()) {
                        Ok(report) if !report.is_empty() => {
                            use tauri::Emitter;
                            let _ = handle.emit("dvr.tick", &report);
                        }
                        Ok(_) => {}
                        Err(e) => tracing::error!("DVR tick failed: {e}"),
                    }
                })?;

            // Ask GitHub whether there is a newer build. On its own thread and after a
            // pause, because nothing about this is urgent and the first seconds after
            // launch belong to getting a picture on screen.
            let updates_handle = app.handle().clone();
            std::thread::Builder::new()
                .name("aurora-updates".into())
                .spawn(move || {
                    std::thread::sleep(UPDATE_CHECK_DELAY);
                    use tauri::Manager;
                    let services = updates_handle.state::<Services>();

                    let (automatic, last) = {
                        let db = services.db.lock();
                        (updates::automatic(&db), updates::last_checked(&db))
                    };
                    if !automatic || !aurora_ingest::updates::due(last, now_unix()) {
                        return;
                    }

                    match updates::run(&services, now_unix(), false) {
                        Ok(check) if check.available => {
                            tracing::info!(current = %check.current, "a newer build is published");
                            let _ = updates_handle.emit("update.available", &check);
                        }
                        Ok(_) => tracing::debug!("this is the newest published build"),
                        // Never a dialog: failing to reach GitHub is not the viewer's
                        // problem and must not interrupt whatever they are watching.
                        Err(e) => tracing::info!("update check failed: {e}"),
                    }
                })
                .expect("spawning the update thread");

            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window must not orphan a recording in flight: finalise it so
            // the file is flushed and the row says what actually happened.
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                use tauri::Manager;
                if let Some(services) = window.try_state::<Services>() {
                    services.dvr.shutdown(now_unix());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::channels_list,
            commands::channels_groups,
            commands::channels_by_number,
            commands::epg_grid_slice,
            commands::epg_now_next,
            commands::library_movies,
            commands::library_series,
            commands::library_genres,
            commands::library_episodes,
            commands::library_playback_aids,
            commands::library_record_skip,
            commands::library_sync_chapters,
            commands::library_series_prefs,
            commands::library_set_series_prefs,
            commands::search_query,
            commands::player_play,
            commands::player_play_catchup,
            commands::player_pause,
            commands::player_resume,
            commands::player_stop,
            commands::player_seek,
            commands::player_set_volume,
            commands::player_set_muted,
            commands::player_set_audio_track,
            commands::player_set_subtitle_track,
            commands::player_set_aspect,
            commands::player_state,
            commands::progress_save,
            dvr::dvr_list,
            dvr::dvr_schedule,
            dvr::dvr_cancel,
            dvr::dvr_delete,
            dvr::dvr_set_keep,
            dvr::dvr_set_watched,
            dvr::dvr_conflicts,
            dvr::dvr_rules,
            dvr::dvr_create_rule,
            dvr::dvr_delete_rule,
            dvr::dvr_set_rule_enabled,
            dvr::dvr_reminders,
            dvr::dvr_add_reminder,
            dvr::dvr_remove_reminder,
            dvr::dvr_storage,
            metadata::metadata_status,
            metadata::metadata_set_key,
            metadata::metadata_run,
            metadata::metadata_credits,
            metadata::metadata_rematch,
            metadata::artwork_status,
            metadata::artwork_prefetch,
            metadata::artwork_clear,
            playlist::library_filters,
            playlist::library_set_filters,
            playlist::library_filter_counts,
            playlist::library_alternates,
            playlist::playlist_list,
            playlist::playlist_groups,
            playlist::playlist_update,
            playlist::playlist_set_hidden,
            playlist::playlist_hide_matching,
            playlist::playlist_reset,
            profiles::profiles_list,
            profiles::profiles_create,
            profiles::profiles_delete,
            profiles::profiles_rename,
            profiles::profiles_set_limits,
            profiles::profiles_set_pin,
            profiles::profiles_verify_pin,
            profiles::profiles_parental,
            profiles::profiles_set_parental,
            profiles::profiles_watched_today,
            providers::providers_detect,
            providers::providers_validate,
            providers::providers_save,
            providers::providers_refresh,
            commands::player_set_speed,
            library::providers_list,
            library::library_stats,
            library::library_rails,
            library::mylist_toggle,
            library::favorites_toggle,
            library::progress_get,
            updates::updates_check,
            updates::updates_set_automatic,
            updates::updates_open_releases,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Aurora TV");
}
