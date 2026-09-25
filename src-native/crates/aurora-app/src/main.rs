// Release builds must not pop a console window behind the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use aurora_app::{
    commands, dvr, library, metadata, now_unix, playlist, profiles, providers,
    services::Services,
    supervise::{log_panics, supervised},
    timeshift, updates, window,
};

/// Send the log somewhere a person can read it.
///
/// A release build sets `windows_subsystem = "windows"`, so it has no console and
/// anything written to stdout goes nowhere — including the one line that distinguishes
/// "libmpv would not load" from "the video is behind the window". So release builds log
/// to a file beside their data, and debug builds keep the console they already have.
///
/// The previous run is kept as `aurora.log.1` rather than overwritten. This used to
/// truncate, which meant the one sequence that matters — it crashed, I relaunched to
/// report it — was also the one that destroyed the evidence.
fn init_logging(data_dir: &std::path::Path) {
    let filter =
        tracing_subscriber::EnvFilter::try_from_env("AURORA_LOG").unwrap_or_else(|_| "info".into());

    if cfg!(debug_assertions) {
        tracing_subscriber::fmt().with_env_filter(filter).init();
        return;
    }

    let _ = std::fs::create_dir_all(data_dir);
    match std::fs::File::create(aurora_app::logging::rotate(data_dir)) {
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

/// Ask GitHub whether there is a newer build.
///
/// On its own thread and after a pause, because nothing about this is urgent and the
/// first seconds after launch belong to getting a picture on screen.
fn check_for_updates(app: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let services = app.state::<Services>();

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
            let _ = app.emit("update.available", &check);
        }
        Ok(_) => tracing::debug!("this is the newest published build"),
        // Never a dialog: failing to reach GitHub is not the viewer's problem and must
        // not interrupt whatever they are watching.
        Err(e) => tracing::info!("update check failed: {e}"),
    }
}

/// Swap in a staged update before the app starts, and relaunch into it.
///
/// Ahead of Tauri on purpose, and it is the only moment that is both safe and simple:
/// nothing is open yet, so every file about to be replaced is closed. On Windows the
/// running `.exe` still cannot be deleted — but it can be renamed, which is what the
/// swap does.
///
/// Returns true when the caller should stop: the new binary is on disk and has been
/// launched, and this process is the old one.
fn take_staged_update() -> bool {
    // Where the data lives has to be worked out before `Services`, because this runs
    // before anything is built. A portable copy is the only one that stages an update,
    // and it is the only one whose data directory is knowable this early.
    let Some(data_dir) = aurora_app::portable_dir() else {
        return false;
    };
    if !updates::apply_staged_update(&data_dir) {
        return false;
    }

    // Relaunch the path we came from, which now holds the new binary.
    match std::env::current_exe() {
        Ok(exe) => match std::process::Command::new(&exe).spawn() {
            Ok(_) => true,
            Err(e) => {
                // The files are already swapped, so the update succeeded even though
                // the relaunch did not. Carrying on would run the *old* binary that is
                // still in memory while the new one is on disk — confusing, but
                // working, and far better than exiting into nothing.
                eprintln!("Aurora updated but could not relaunch ({e}); continuing.");
                false
            }
        },
        Err(e) => {
            eprintln!("Aurora updated but cannot find its own executable ({e}); continuing.");
            false
        }
    }
}

fn main() {
    // Before the logger, because the logger opens a file in the folder being replaced.
    if take_staged_update() {
        return;
    }

    tauri::Builder::default()
        .setup(|app| {
            use tauri::Manager;
            // Portable mode (README §13): a marker file next to the exe keeps all data
            // local, with no %LOCALAPPDATA% and no registry.
            let data_dir = match aurora_app::portable_dir() {
                Some(dir) => dir,
                None => app
                    .path()
                    .app_local_data_dir()
                    .map_err(|e| format!("no data dir: {e}"))?,
            };

            init_logging(&data_dir);
            // After the subscriber exists, so the hook has somewhere to write. Before
            // any thread is spawned, so none of them can panic unrecorded.
            log_panics();
            // The pre-launch update swap runs before any of this exists, so whatever it
            // did is waiting in a file rather than in the log.
            updates::report_last_apply(&data_dir);
            tracing::info!(
                "Aurora TV {} starting, data in {}",
                env!("CARGO_PKG_VERSION"),
                data_dir.display()
            );

            let services = Services::new(data_dir)?;
            let scheduler = std::sync::Arc::clone(&services.dvr);
            let playback_handle = std::sync::Arc::clone(&services.playback);
            let player_backend = std::sync::Arc::clone(&services.player);

            // The Phase 0 spike, finally connected (docs/ROADMAP.md). `attach` creates
            // the child window mpv draws into and hands it the handle; without it there
            // is no surface and mpv has nowhere to put a frame. It was written, and
            // unreachable, because the app layer holds a `Box<dyn PlayerBackend>` and
            // the method was not on the trait.
            //
            // A failure here is not a reason to refuse to start: an app running with no
            // picture and a line in the log is something a person can report; a window
            // that never appears is not.
            match app.get_webview_window("main") {
                Some(main_window) => {
                    let mut player = player_backend.lock();
                    match window::attach_video_surface(&main_window, player.as_mut()) {
                        Ok(()) => tracing::info!("video surface ready"),
                        Err(e) => tracing::error!("no video surface: {e}"),
                    }
                }
                None => tracing::error!("no main window at setup; video cannot composite"),
            }

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
                    supervised("player", || {
                        if let Some(state) = playback.tick(now_unix()) {
                            let _ = player_handle.emit("player.state", &state);
                        }
                    });
                })
                .expect("spawning the player thread");

            // The DVR has to keep its own time: nothing in the UI is guaranteed to be
            // open when a recording is due, and a minimised window still records.
            let handle = app.handle().clone();
            std::thread::Builder::new()
                .name("aurora-dvr".into())
                .spawn(move || loop {
                    std::thread::sleep(dvr::TICK_INTERVAL);
                    // A panic here used to end the scheduler outright, and a DVR that
                    // has silently stopped has no symptom until the programme is gone.
                    supervised("DVR", || match scheduler.tick(now_unix()) {
                        Ok(report) if !report.is_empty() => {
                            use tauri::Emitter;
                            let _ = handle.emit("dvr.tick", &report);
                        }
                        Ok(_) => {}
                        Err(e) => tracing::error!("DVR tick failed: {e}"),
                    });
                })?;

            // Ask GitHub whether there is a newer build. On its own thread and after a
            // pause, because nothing about this is urgent and the first seconds after
            // launch belong to getting a picture on screen.
            let updates_handle = app.handle().clone();
            std::thread::Builder::new()
                .name("aurora-updates".into())
                .spawn(move || {
                    std::thread::sleep(UPDATE_CHECK_DELAY);
                    supervised("update", || check_for_updates(&updates_handle));
                })
                .expect("spawning the update thread");

            Ok(())
        })
        .on_window_event(|win, event| {
            use tauri::Manager;
            match event {
                // Closing the window must not orphan a recording in flight: finalise it
                // so the file is flushed and the row says what actually happened.
                tauri::WindowEvent::CloseRequested { .. } => {
                    if let Some(services) = win.try_state::<Services>() {
                        services.dvr.shutdown(now_unix());
                    }
                }
                // The video surface is a sibling of the WebView2, not a child of it, so
                // nothing moves it on its own. Repositioning it in the same event the
                // resize arrives on is what stops the two tearing apart (README §2.1).
                tauri::WindowEvent::Resized(size) => {
                    if let Some(services) = win.try_state::<Services>() {
                        if let Err(e) = services.player.lock().resize(size.width, size.height) {
                            tracing::warn!("could not resize the video surface: {e}");
                        }
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::channels_list,
            commands::channels_groups,
            commands::channels_by_number,
            commands::epg_grid_slice,
            commands::epg_now_next,
            commands::epg_now_next_many,
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
            commands::player_back_to_live,
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
            providers::providers_credentials,
            providers::providers_update,
            providers::providers_delete,
            commands::player_set_speed,
            aurora_app::logging::app_about,
            aurora_app::logging::app_diagnostics,
            aurora_app::logging::logs_export,
            library::providers_list,
            library::library_stats,
            library::library_rails,
            library::mylist_toggle,
            library::favorites_toggle,
            library::progress_get,
            timeshift::timeshift_settings,
            timeshift::timeshift_set_settings,
            timeshift::timeshift_clear,
            updates::updates_check,
            updates::updates_set_automatic,
            updates::updates_open_releases,
            updates::updates_download,
            updates::updates_install,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Aurora TV");
}
