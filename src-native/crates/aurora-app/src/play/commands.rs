//! The player's commands (`player.*`, `pip.*`, `preview.*`, `sleep.*`, `timeshift.*`
//! in `shared/ipc.ts`). Thin: each one is a call on [`super::player::Player`].

use aurora_video::{Aspect, ItemKind, PlayerState, Seek};
use serde::Deserialize;
use tauri::State;

use super::placement::{Corner, PipView, Rect};
use super::preview::SeekPreview;
use super::resolve::What;
use super::sleep::{SleepAction, SleepAfter, SleepState};
use super::timeshift::{Change, TimeshiftSettings};
use crate::error::{AppError, Result};
use crate::now_unix;
use crate::services::Services;

fn logged(control: &'static str, outcome: Result<PlayerState>) -> Result<PlayerState> {
    match &outcome {
        Ok(s) => tracing::info!(
            control,
            status = ?s.report.status,
            position = s.report.position_secs,
            reconnecting = s.reconnecting,
            "player control"
        ),
        Err(AppError::Superseded) => tracing::debug!(control, "replaced by a later tune"),
        Err(e) => tracing::warn!(control, "player control refused: {e}"),
    }
    outcome
}

/* ── Playing ─────────────────────────────────────────────────────────────── */

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayArgs {
    pub kind: String,
    pub id: i64,
    pub position_secs: Option<f64>,
}

#[tauri::command(async)]
pub fn player_play(services: State<'_, Services>, args: PlayArgs) -> Result<PlayerState> {
    let what = match ItemKind::parse(&args.kind) {
        Some(ItemKind::Live) => What::Channel(args.id),
        Some(ItemKind::Movie) => What::Movie(args.id),
        Some(ItemKind::Episode) => What::Episode(args.id),
        Some(ItemKind::Recording) => What::Recording(args.id),
        None => {
            return Err(AppError::Other(format!(
                "Aurora does not know how to play a {:?}.",
                args.kind
            )))
        }
    };
    logged("play", services.player.play(what, args.position_secs))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatchupArgs {
    pub channel_id: i64,
    pub start: i64,
    pub stop: i64,
}

#[tauri::command(async)]
pub fn player_play_catchup(
    services: State<'_, Services>,
    args: CatchupArgs,
) -> Result<PlayerState> {
    if args.stop <= args.start {
        return Err(AppError::Other("That programme has no length.".into()));
    }
    let what = What::Catchup {
        channel: args.channel_id,
        start: args.start,
        stop: args.stop,
    };
    logged("catchup", services.player.play(what, None))
}

#[tauri::command(async)]
pub fn player_pause(services: State<'_, Services>) -> Result<PlayerState> {
    logged(
        "pause",
        services.player.session.apply(|e| e.set_paused(true)),
    )
}

#[tauri::command(async)]
pub fn player_resume(services: State<'_, Services>) -> Result<PlayerState> {
    logged(
        "resume",
        services.player.session.apply(|e| e.set_paused(false)),
    )
}

/// Stop, and leave fullscreen: leaving the player always does (C15), whichever way
/// the page left it.
#[tauri::command(async)]
pub fn player_stop(window: tauri::Window, services: State<'_, Services>) -> Result<PlayerState> {
    if window.is_fullscreen().unwrap_or(false) {
        if let Err(e) = window.set_fullscreen(false) {
            tracing::warn!("could not leave fullscreen on stop: {e}");
        }
    }
    logged("stop", services.player.stop())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeekArgs {
    pub position_secs: f64,
    pub relative: Option<bool>,
}

#[tauri::command(async)]
pub fn player_seek(services: State<'_, Services>, args: SeekArgs) -> Result<PlayerState> {
    if !args.position_secs.is_finite() {
        return Err(AppError::Other("That is not a position.".into()));
    }
    let seek = if args.relative.unwrap_or(false) {
        Seek::By(args.position_secs)
    } else {
        Seek::To(args.position_secs)
    };
    logged("seek", services.player.session.apply(|e| e.seek(seek)))
}

#[tauri::command(async)]
pub fn player_back_to_live(services: State<'_, Services>) -> Result<PlayerState> {
    logged("backToLive", services.player.session.back_to_live())
}

#[derive(Debug, Deserialize)]
pub struct VolumeArgs {
    pub volume: u32,
}

#[tauri::command(async)]
pub fn player_set_volume(services: State<'_, Services>, args: VolumeArgs) -> Result<PlayerState> {
    let v = args.volume.min(100);
    services.player.session.apply(|e| e.set_volume(v))
}

#[derive(Debug, Deserialize)]
pub struct MutedArgs {
    pub muted: bool,
}

#[tauri::command(async)]
pub fn player_set_muted(services: State<'_, Services>, args: MutedArgs) -> Result<PlayerState> {
    services.player.session.apply(|e| e.set_muted(args.muted))
}

#[derive(Debug, Deserialize)]
pub struct SpeedArgs {
    pub speed: f64,
}

#[tauri::command(async)]
pub fn player_set_speed(services: State<'_, Services>, args: SpeedArgs) -> Result<PlayerState> {
    services.player.session.apply(|e| e.set_speed(args.speed))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackArgs {
    pub track_id: Option<i64>,
}

#[tauri::command(async)]
pub fn player_set_audio_track(
    services: State<'_, Services>,
    args: TrackArgs,
) -> Result<PlayerState> {
    services
        .player
        .session
        .apply(|e| e.set_audio_track(args.track_id))
}

#[tauri::command(async)]
pub fn player_set_subtitle_track(
    services: State<'_, Services>,
    args: TrackArgs,
) -> Result<PlayerState> {
    services
        .player
        .session
        .apply(|e| e.set_subtitle_track(args.track_id))
}

#[derive(Debug, Deserialize)]
pub struct AspectArgs {
    pub aspect: Aspect,
}

#[tauri::command(async)]
pub fn player_set_aspect(services: State<'_, Services>, args: AspectArgs) -> Result<PlayerState> {
    services.player.session.apply(|e| e.set_aspect(args.aspect))
}

#[tauri::command(async)]
pub fn player_state(services: State<'_, Services>) -> Result<PlayerState> {
    Ok(services.player.session.state())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeekPreviewArgs {
    pub position_secs: f64,
}

#[tauri::command(async)]
pub fn player_seek_preview(
    services: State<'_, Services>,
    args: SeekPreviewArgs,
) -> Result<SeekPreview> {
    services.player.seek_preview(args.position_secs, now_unix())
}

/* ── Picture-in-picture and the inlay ────────────────────────────────────── */

#[tauri::command(async)]
pub fn pip_state(services: State<'_, Services>) -> Result<PipView> {
    Ok(services.player.pip())
}

#[tauri::command(async)]
pub fn pip_toggle(services: State<'_, Services>) -> Result<PipView> {
    services.player.pip_toggle(services.mosaic.is_open())
}

#[derive(Debug, Deserialize)]
pub struct PipEnabledArgs {
    pub enabled: bool,
}

#[tauri::command(async)]
pub fn pip_set_enabled(services: State<'_, Services>, args: PipEnabledArgs) -> Result<PipView> {
    services
        .player
        .pip_set_enabled(args.enabled, services.mosaic.is_open())
}

#[derive(Debug, Deserialize)]
pub struct PipCornerArgs {
    pub corner: Option<Corner>,
}

#[tauri::command(async)]
pub fn pip_set_corner(services: State<'_, Services>, args: PipCornerArgs) -> Result<PipView> {
    Ok(services.player.pip_set_corner(args.corner))
}

#[tauri::command(async)]
pub fn preview_place(services: State<'_, Services>, args: Rect) -> Result<PipView> {
    services.player.inlay(args, services.mosaic.is_open())
}

#[tauri::command(async)]
pub fn preview_clear(services: State<'_, Services>) -> Result<PipView> {
    Ok(services.player.clear_inlay())
}

/* ── Sleep timer ─────────────────────────────────────────────────────────── */

#[tauri::command(async)]
pub fn sleep_state(services: State<'_, Services>) -> Result<SleepState> {
    Ok(services.player.sleep_state(now_unix()))
}

#[derive(Debug, Deserialize)]
pub struct SleepSetArgs {
    pub after: SleepAfter,
    #[serde(default)]
    pub action: SleepAction,
}

#[tauri::command(async)]
pub fn sleep_set(services: State<'_, Services>, args: SleepSetArgs) -> Result<SleepState> {
    services
        .player
        .sleep_set(args.after, args.action, now_unix())
}

#[tauri::command(async)]
pub fn sleep_cancel(services: State<'_, Services>) -> Result<SleepState> {
    Ok(services.player.sleep_cancel(now_unix()))
}

#[derive(Debug, Deserialize)]
pub struct SleepExtendArgs {
    pub minutes: u32,
}

#[tauri::command(async)]
pub fn sleep_extend(services: State<'_, Services>, args: SleepExtendArgs) -> Result<SleepState> {
    services.player.sleep_extend(args.minutes, now_unix())
}

/* ── Timeshift ───────────────────────────────────────────────────────────── */

#[tauri::command(async)]
pub fn timeshift_settings(services: State<'_, Services>) -> Result<TimeshiftSettings> {
    services.player.timeshift_settings()
}

#[tauri::command(async)]
pub fn timeshift_set_settings(
    services: State<'_, Services>,
    args: Change,
) -> Result<TimeshiftSettings> {
    crate::profiles::require_settings(&services.db.lock())?;
    services.player.timeshift_change(args)
}

#[tauri::command(async)]
pub fn timeshift_clear(services: State<'_, Services>) -> Result<u64> {
    crate::profiles::require_settings(&services.db.lock())?;
    services.player.timeshift_clear()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Logging is observation only: a wrapper that swallowed an error or changed a
    /// state would be the very class of bug it was added to find.
    #[test]
    fn logging_a_control_does_not_change_what_it_answered() {
        let state = PlayerState {
            title: Some("x".into()),
            ..PlayerState::default()
        };
        assert_eq!(logged("pause", Ok(state.clone())).unwrap(), state);
        let err = logged("pause", Err(AppError::Other("mpv said no".into())));
        assert_eq!(err.unwrap_err().to_string(), "mpv said no");
        assert!(matches!(
            logged("play", Err(AppError::Superseded)),
            Err(AppError::Superseded)
        ));
    }
}
