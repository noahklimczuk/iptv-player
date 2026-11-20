//! The parental gate's commands (`parental.*` in `shared/ipc.ts`): the PIN that opens
//! locked content and the parental settings, and the locks themselves (README §11).
//!
//! What the gate decides lives in `aurora_db::repo::parental`; who is watching is chosen
//! by `profiles.select`. Changing a lock or how locked content is shown needs the
//! parental PIN, exactly as the other parental settings do (`profiles::require_parent`).

use aurora_db::repo::parental::{self, Lock, LockKind, ParentalState};
use aurora_db::repo::profiles::{self, PinOutcome};
use aurora_db::rusqlite::Connection;
use serde::{Deserialize, Serialize};
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
            "Set a parental PIN first: a lock is opened with it, so without one nothing \
             locked could be watched."
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

/* ── The daily limit's clock (F-37) ───────────────────────────────────────── */

/// How much counted viewing is held before it is written down. The limit is enforced
/// to within this much, and the database is not written four times a second.
pub const FLUSH_SECS: f64 = 10.0;
/// When the warning comes: five minutes before the day's time runs out.
pub const WARN_SECS: i64 = 5 * 60;
/// The most one sample may count. A heartbeat that stalled (a suspended laptop, a
/// debugger) is not an hour of television.
const MAX_GAP_SECS: f64 = 5.0;

/// What the clock has to say, as the `parental.clock` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ClockEvent {
    /// The day's time is nearly used up.
    #[serde(rename_all = "camelCase")]
    Warning { secs_left: i64 },
    /// It is used up: the host has stopped playback.
    TimeUp,
}

/// Counts viewing time against the daily limit. Kept by the host's heartbeat, not the
/// page: a backgrounded window is still a television that is on.
#[derive(Debug, Default)]
pub struct ViewingClock {
    counted: f64,
    last_sample: Option<f64>,
    warned: bool,
}

impl ViewingClock {
    /// Note whether anything is playing at `mono`, a monotonic time in seconds. True when
    /// enough has been counted to write down with [`ViewingClock::flush`].
    pub fn sample(&mut self, watching: bool, mono: f64) -> bool {
        if let (true, Some(last)) = (watching, self.last_sample) {
            self.counted += (mono - last).clamp(0.0, MAX_GAP_SECS);
        }
        self.last_sample = Some(mono);
        self.counted >= FLUSH_SECS
    }

    /// Write down what was counted, and say what it means for whoever is watching.
    pub fn flush(&mut self, db: &Connection, now: i64) -> Result<Option<ClockEvent>> {
        let whole = self.counted.floor();
        parental::add_watch_time(db, now, whole as i64)?;
        self.counted -= whole;
        Ok(match parental::time_left(db, now)? {
            Some(left) if left <= 0 => Some(ClockEvent::TimeUp),
            Some(left) if left <= WARN_SECS => {
                if self.warned {
                    None
                } else {
                    self.warned = true;
                    Some(ClockEvent::Warning { secs_left: left })
                }
            }
            _ => {
                self.warned = false;
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_db::repo::profiles::NewProfile;

    const NOW: i64 = 1_800_000_000;

    /// A kids profile with an hour a day, chosen, and a parental PIN so the limit holds.
    fn an_hour_a_day() -> Connection {
        let db = aurora_db::open_memory().unwrap();
        profiles::set_master_pin(&db, Some("2468"), 0).unwrap();
        let sam = profiles::create(
            &db,
            &NewProfile {
                name: "Sam".into(),
                is_kids: true,
                max_age: Some(12),
                daily_limit_min: Some(60),
                ..Default::default()
            },
            0,
        )
        .unwrap();
        parental::select_profile(&db, sam).unwrap();
        db
    }

    #[test]
    fn the_clock_counts_only_what_plays_and_never_a_stalled_heartbeat_as_hours() {
        let mut clock = ViewingClock::default();
        assert!(
            !clock.sample(true, 0.0),
            "the first sample only starts the clock"
        );
        assert!(
            !clock.sample(false, 100.0),
            "nothing playing counts nothing"
        );
        assert!(!clock.sample(true, 104.0));
        // A heartbeat that stalled for an hour counts as one gap, not an hour.
        assert!(!clock.sample(true, 3_704.0));
        assert_eq!(clock.counted, 9.0, "4s, then a capped gap of 5s");
        assert!(
            clock.sample(true, 3_705.0),
            "ten seconds is worth writing down"
        );
    }

    #[test]
    fn the_clock_warns_once_and_then_says_time_is_up() {
        let db = an_hour_a_day();
        let mut clock = ViewingClock {
            counted: 3_000.0,
            ..Default::default()
        };
        assert_eq!(clock.flush(&db, NOW).unwrap(), None);
        assert_eq!(parental::watched_today(&db, NOW).unwrap(), 3_000);

        clock.counted = 400.0;
        assert_eq!(
            clock.flush(&db, NOW).unwrap(),
            Some(ClockEvent::Warning { secs_left: 200 })
        );
        clock.counted = 100.0;
        assert_eq!(clock.flush(&db, NOW).unwrap(), None, "warned once");

        clock.counted = 100.5;
        assert_eq!(clock.flush(&db, NOW).unwrap(), Some(ClockEvent::TimeUp));
        assert_eq!(
            clock.counted, 0.5,
            "the part of a second is kept for next time"
        );

        // The PIN gives more time: nothing to say until it is closed again.
        parental::unlock(&db, NOW).unwrap();
        clock.counted = 10.0;
        assert_eq!(clock.flush(&db, NOW).unwrap(), None);
    }

    #[test]
    fn the_clock_event_is_tagged_for_the_page() {
        assert_eq!(
            serde_json::to_value(ClockEvent::Warning { secs_left: 300 }).unwrap(),
            serde_json::json!({ "kind": "warning", "secsLeft": 300 })
        );
        assert_eq!(
            serde_json::to_value(ClockEvent::TimeUp).unwrap(),
            serde_json::json!({ "kind": "timeUp" })
        );
    }
}
