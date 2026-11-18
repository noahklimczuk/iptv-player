//! The parental gate's commands (`parental.*` in `shared/ipc.ts`): the PIN that opens
//! locked content and the parental settings, and the locks themselves (README §11).
//!
//! What the gate decides lives in `aurora_db::repo::parental`; who is watching is chosen
//! by `profiles.select`. Changing a lock or how locked content is shown needs the
//! parental PIN, exactly as the other parental settings do (`profiles::require_parent`).

use aurora_db::repo::parental::{self, Lock, LockKind, ParentalState};
use aurora_db::repo::profiles::{self, PinOutcome};
use serde::Deserialize;
use tauri::State;

use crate::error::{AppError, Result};
use crate::now_unix;
use crate::profiles::require_parent;
use crate::services::Services;

#[tauri::command(async)]
pub fn parental_state(services: State<'_, Services>) -> Result<ParentalState> {
    let db = services.db.lock();
    Ok(parental::state(&db, now_unix())?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlockArgs {
    pub pin: String,
}

/// Enter the parental PIN: opens locked content and the parental settings for
/// `UNLOCK_SECS`. Throttled like every other PIN.
#[tauri::command(async)]
pub fn parental_unlock(services: State<'_, Services>, args: UnlockArgs) -> Result<PinOutcome> {
    let db = services.db.lock();
    let now = now_unix();
    if !profiles::parental_settings(&db)?.has_master_pin {
        return Err(AppError::Other(
            "There is no parental PIN yet. Set one in Settings → Parental controls.".into(),
        ));
    }
    let outcome = profiles::verify_master_pin(&db, &args.pin, now)?;
    if outcome == PinOutcome::Ok {
        parental::unlock(&db, now)?;
    }
    Ok(outcome)
}

/// Close what the PIN opened, as when leaving the parental settings.
#[tauri::command(async)]
pub fn parental_relock(services: State<'_, Services>) -> Result<ParentalState> {
    let db = services.db.lock();
    parental::relock(&db)?;
    Ok(parental::state(&db, now_unix())?)
}

#[tauri::command(async)]
pub fn parental_locks(services: State<'_, Services>) -> Result<Vec<Lock>> {
    let db = services.db.lock();
    Ok(parental::locks(&db)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetLockArgs {
    pub kind: LockKind,
    /// A channel's id, or a category's name.
    pub value: String,
    pub locked: bool,
}

#[tauri::command(async)]
pub fn parental_set_lock(services: State<'_, Services>, args: SetLockArgs) -> Result<Vec<Lock>> {
    let db = services.db.lock();
    require_parent(&db)?;
    if args.locked && !profiles::parental_settings(&db)?.has_master_pin {
        return Err(AppError::Other(
            "Set a parental PIN first: a lock is opened with it, so without one nothing              locked could be watched."
                .into(),
        ));
    }
    parental::set_lock(&db, args.kind, &args.value, args.locked)?;
    Ok(parental::locks(&db)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HideLockedArgs {
    pub hide: bool,
}

#[tauri::command(async)]
pub fn parental_set_hide_locked(
    services: State<'_, Services>,
    args: HideLockedArgs,
) -> Result<ParentalState> {
    let db = services.db.lock();
    require_parent(&db)?;
    parental::set_hide_locked(&db, args.hide)?;
    Ok(parental::state(&db, now_unix())?)
}
