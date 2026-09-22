// Release builds must not pop a console window behind the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use aurora_app::{commands, profiles, providers, services::Services};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("AURORA_LOG")
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

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

            let services = Services::new(data_dir)?;
            app.manage(services);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::channels_list,
            commands::channels_groups,
            commands::channels_by_number,
            commands::epg_grid_slice,
            commands::epg_now_next,
            commands::library_movies,
            commands::library_episodes,
            commands::library_playback_aids,
            commands::library_record_skip,
            commands::library_sync_chapters,
            commands::library_series_prefs,
            commands::library_set_series_prefs,
            commands::search_query,
            commands::player_play,
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
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Aurora TV");
}
