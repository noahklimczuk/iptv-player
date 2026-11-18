//! Profile and parental-control commands (README §11).

use aurora_db::repo::parental;
use aurora_db::repo::profiles::{self, NewProfile, ParentalSettings, PinOutcome, Profile};
use aurora_db::rusqlite::Connection;
use serde::Deserialize;
use tauri::State;

use crate::error::{AppError, Result};
use crate::services::Services;

/// Refuse a change to profiles or parental controls unless the parental PIN has been
/// entered recently (`parental.unlock`), when there is one.
///
/// These were open to anybody at the keyboard: a kids profile's age limit could be
/// lifted, a new unrestricted profile made beside it, or the parent's own profile PIN
/// removed, from the very profile the limit was meant to hold. With no master PIN set
/// the household has chosen not to guard them, and nothing is asked.
pub(crate) fn require_parent(db: &Connection) -> Result<()> {
    if profiles::parental_settings(db)?.has_master_pin && !parental::is_unlocked(db, now_unix())? {
        return Err(AppError::ParentalPinRequired);
    }
    Ok(())
}

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
    require_parent(&db)?;
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
    require_parent(&db)?;
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
    require_parent(&db)?;
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
    // Your own PIN is yours to change: you had to know it to be watching as yourself.
    // Anybody else's is a parent's to change.
    if parental::active_profile(&db)? != Some(args.profile_id) {
        require_parent(&db)?;
    }
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectArgs {
    pub profile_id: i64,
    /// The profile's own PIN, when it has one.
    pub pin: Option<String>,
}

/// "Who's watching?": choose the profile whose limits apply from now on, checking its
/// PIN here rather than trusting the screen to have checked it. Anything the parental
/// PIN had opened closes.
#[tauri::command(async)]
pub fn profiles_select(services: State<'_, Services>, args: SelectArgs) -> Result<PinOutcome> {
    let db = services.db.lock();
    select(&db, args.profile_id, args.pin.as_deref(), now_unix())
}

pub(crate) fn select(
    db: &Connection,
    profile_id: i64,
    pin: Option<&str>,
    now: i64,
) -> Result<PinOutcome> {
    let Some(profile) = profiles::get(db, profile_id)? else {
        return Err(AppError::Other("That profile no longer exists.".into()));
    };
    let outcome = match pin {
        Some(pin) => profiles::verify_profile_pin(db, profile_id, pin, now)?,
        None if profile.has_pin => {
            return Err(AppError::Other(format!("{} needs its PIN.", profile.name)))
        }
        None => PinOutcome::NotRequired,
    };
    if matches!(outcome, PinOutcome::Ok | PinOutcome::NotRequired) {
        parental::select_profile(db, profile_id)?;
    }
    Ok(outcome)
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
    require_parent(&db)?;
    profiles::set_parental_flags(&db, args.hide_adult, args.lock_settings, now_unix())?;
    if let Some(pin) = args.master_pin {
        profiles::set_master_pin(&db, pin.as_deref(), now_unix())?;
        // Whoever just chose the PIN knows it: the rest of this visit to the settings
        // should not ask for it straight back.
        if pin.is_some() {
            parental::unlock(&db, now_unix())?;
        }
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
    fn with_a_parental_pin_set_changes_wait_for_it_and_your_own_pin_does_not() {
        let db = aurora_db::open_memory().unwrap();
        // No parental PIN: the household chose not to guard anything.
        require_parent(&db).unwrap();

        profiles::set_master_pin(&db, Some("2468"), 0).unwrap();
        assert!(matches!(
            require_parent(&db),
            Err(AppError::ParentalPinRequired)
        ));
        parental::unlock(&db, now_unix()).unwrap();
        require_parent(&db).unwrap();

        // Picking a profile closes it again.
        assert_eq!(
            select(&db, 1, None, now_unix()).unwrap(),
            PinOutcome::NotRequired
        );
        assert!(matches!(
            require_parent(&db),
            Err(AppError::ParentalPinRequired)
        ));
    }

    #[test]
    fn a_profile_with_a_pin_is_chosen_only_with_it() {
        let db = aurora_db::open_memory().unwrap();
        profiles::set_pin(&db, 1, Some("1357")).unwrap();
        assert!(select(&db, 1, None, 0).is_err(), "no PIN sent");
        assert!(matches!(
            select(&db, 1, Some("0000"), 0).unwrap(),
            PinOutcome::Wrong { .. }
        ));
        assert_eq!(parental::active_profile(&db).unwrap(), None);
        assert_eq!(select(&db, 1, Some("1357"), 0).unwrap(), PinOutcome::Ok);
        assert_eq!(parental::active_profile(&db).unwrap(), Some(1));
        assert!(select(&db, 42, None, 0).is_err(), "no such profile");
    }

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
