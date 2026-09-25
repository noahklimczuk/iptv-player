//! Profile and parental-control commands (README §11).

use aurora_db::repo::profiles::{self, NewProfile, ParentalSettings, PinOutcome, Profile};
use serde::Deserialize;
use tauri::State;

use crate::error::Result;
use crate::services::Services;

#[tauri::command(async)]
pub fn profiles_list(services: State<'_, Services>) -> Result<Vec<Profile>> {
    let db = services.db.lock();
    Ok(profiles::list(&db)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProfileArgs {
    pub name: String,
    pub avatar: Option<String>,
    pub is_kids: bool,
    pub max_age: Option<u8>,
    pub daily_limit_min: Option<u32>,
}

#[tauri::command(async)]
pub fn profiles_create(services: State<'_, Services>, args: CreateProfileArgs) -> Result<i64> {
    let db = services.db.lock();
    Ok(profiles::create(
        &db,
        &NewProfile {
            name: args.name,
            avatar: args.avatar,
            is_kids: args.is_kids,
            max_age: args.max_age,
            allow_unrated: None,
            daily_limit_min: args.daily_limit_min,
        },
        now_unix(),
    )?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileIdArgs {
    pub profile_id: i64,
}

#[tauri::command(async)]
pub fn profiles_delete(services: State<'_, Services>, args: ProfileIdArgs) -> Result<bool> {
    let db = services.db.lock();
    Ok(profiles::delete(&db, args.profile_id)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameArgs {
    pub profile_id: i64,
    pub name: String,
}

#[tauri::command(async)]
pub fn profiles_rename(services: State<'_, Services>, args: RenameArgs) -> Result<()> {
    let db = services.db.lock();
    profiles::rename(&db, args.profile_id, &args.name)?;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitsArgs {
    pub profile_id: i64,
    pub max_age: Option<u8>,
    pub allow_unrated: bool,
    pub daily_limit_min: Option<u32>,
}

#[tauri::command(async)]
pub fn profiles_set_limits(services: State<'_, Services>, args: LimitsArgs) -> Result<()> {
    let db = services.db.lock();
    profiles::set_limits(
        &db,
        args.profile_id,
        args.max_age,
        args.allow_unrated,
        args.daily_limit_min,
    )?;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPinArgs {
    pub profile_id: i64,
    /// `None` removes the PIN.
    pub pin: Option<String>,
}

#[tauri::command(async)]
pub fn profiles_set_pin(services: State<'_, Services>, args: SetPinArgs) -> Result<()> {
    let db = services.db.lock();
    profiles::set_pin(&db, args.profile_id, args.pin.as_deref())?;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyPinArgs {
    /// Omit for the master PIN.
    pub profile_id: Option<i64>,
    pub pin: String,
}

#[tauri::command(async)]
pub fn profiles_verify_pin(
    services: State<'_, Services>,
    args: VerifyPinArgs,
) -> Result<PinOutcome> {
    let db = services.db.lock();
    Ok(match args.profile_id {
        Some(id) => profiles::verify_profile_pin(&db, id, &args.pin, now_unix())?,
        None => profiles::verify_master_pin(&db, &args.pin, now_unix())?,
    })
}

#[tauri::command(async)]
pub fn profiles_parental(services: State<'_, Services>) -> Result<ParentalSettings> {
    let db = services.db.lock();
    Ok(profiles::parental_settings(&db)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParentalArgs {
    pub hide_adult: bool,
    pub lock_settings: bool,
    /// Present only when changing it; `Some(None)` clears it.
    pub master_pin: Option<Option<String>>,
}

#[tauri::command(async)]
pub fn profiles_set_parental(services: State<'_, Services>, args: ParentalArgs) -> Result<()> {
    let db = services.db.lock();
    profiles::set_parental_flags(&db, args.hide_adult, args.lock_settings, now_unix())?;
    if let Some(pin) = args.master_pin {
        profiles::set_master_pin(&db, pin.as_deref(), now_unix())?;
    }
    Ok(())
}

/// Minutes watched today, for the daily limit on a kids profile.
#[tauri::command(async)]
pub fn profiles_watched_today(services: State<'_, Services>, args: ProfileIdArgs) -> Result<u32> {
    let db = services.db.lock();
    let midnight = start_of_local_day(now_unix());
    Ok(profiles::minutes_watched_since(
        &db,
        args.profile_id,
        midnight,
    )?)
}

/// Midnight in the *viewer's* zone, not UTC — a limit that resets at 1am local
/// because the host is in UTC+1 would be its own small bug.
fn start_of_local_day(now: i64) -> i64 {
    let offset = local_utc_offset_secs();
    let local = now + offset;
    let midnight_local = local - local.rem_euclid(86_400);
    midnight_local - offset
}

fn local_utc_offset_secs() -> i64 {
    time::OffsetDateTime::now_local()
        .map(|t| t.offset().whole_seconds() as i64)
        .unwrap_or(0)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_day_boundary_is_local_not_utc() {
        // Whatever the host offset, midnight must land on a local day boundary.
        let now = 1_705_320_000; // 2024-01-15T12:00:00Z
        let start = start_of_local_day(now);
        assert!(start <= now);
        assert!(now - start < 86_400);

        let offset = local_utc_offset_secs();
        assert_eq!(
            (start + offset).rem_euclid(86_400),
            0,
            "not a local midnight"
        );
    }
}
