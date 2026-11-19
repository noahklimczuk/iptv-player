//! Parental controls as Windows applies them (README §11, F-36 to F-38): who is watching
//! on this device, the parental PIN that opens locks and guarded settings, the locks
//! themselves, and the daily limit's clock.
//!
//! What the gate decides lives in the shared `aurora_db::repo::parental`, and every list
//! the shared crates build already passes through it. What is here is the host's part,
//! mirrored from `aurora-app`: `profiles.rs` (`require_parent`, `require_settings`,
//! `select`), `parental.rs` (the `parental.*` commands and `ViewingClock`) and
//! `play/resolve.rs` (`allowed`). `mirrors.rs` checks the copies.

use std::time::Instant;

use aurora_db::repo::parental::{self, LockKind, Target, Verdict};
use aurora_db::repo::profiles::{self, PinOutcome as DbPinOutcome};
use aurora_db::rusqlite::Connection;

use crate::error::{AuroraError, Refusal, Result};
use crate::profiles::PinOutcome;
use crate::{now_unix, AuroraCore};

/// Refuse a change to profiles or parental controls unless the parental PIN has been
/// entered recently, when there is one (`aurora-app` `profiles::require_parent`).
pub(crate) fn require_parent(db: &Connection) -> Result<()> {
    if profiles::parental_settings(db)?.has_master_pin && !parental::is_unlocked(db, now_unix())? {
        return Err(Refusal::PinRequired.into());
    }
    Ok(())
}

/// Refuse a change behind Settings while Settings are locked to whoever is watching
/// (`aurora-app` `profiles::require_settings`, F-38).
pub(crate) fn require_settings(db: &Connection) -> Result<()> {
    if parental::settings_locked(db, now_unix())? {
        return Err(Refusal::PinRequired.into());
    }
    Ok(())
}

/// Refuse what the profile watching may not play (`aurora-app` `play::resolve::allowed`).
pub(crate) fn allowed(db: &Connection, target: Target) -> Result<()> {
    match parental::check(db, target, now_unix())? {
        Verdict::Allowed => Ok(()),
        Verdict::NotAllowed => Err(Refusal::NotAllowed.into()),
        Verdict::Locked => Err(Refusal::Locked.into()),
        Verdict::TimeUp => Err(Refusal::TimeUp.into()),
    }
}

/// "Who's watching?": choose the profile whose limits apply, checking its PIN here
/// rather than trusting the screen to have (`aurora-app` `profiles::select`).
pub(crate) fn select(
    db: &Connection,
    profile_id: i64,
    pin: Option<&str>,
    now: i64,
) -> Result<DbPinOutcome> {
    let Some(profile) = profiles::get(db, profile_id)? else {
        return Err(AuroraError::other("That profile no longer exists."));
    };
    let outcome = match pin {
        Some(pin) => profiles::verify_profile_pin(db, profile_id, pin, now)?,
        None if profile.has_pin => {
            return Err(AuroraError::other(format!(
                "{} needs its PIN.",
                profile.name
            )))
        }
        None => DbPinOutcome::NotRequired,
    };
    if matches!(outcome, DbPinOutcome::Ok | DbPinOutcome::NotRequired) {
        parental::select_profile(db, profile_id)?;
    }
    Ok(outcome)
}

/// What the screens need to know about the gate (`ParentalState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ParentalState {
    pub profile_id: Option<i64>,
    /// Until when (Unix seconds) the PIN has opened locks and the parental settings.
    pub unlocked_until: Option<i64>,
    /// Locked content is hidden rather than shown with a padlock.
    pub hide_locked: bool,
    /// Anything at all is held back for the profile watching.
    pub restricted: bool,
    /// Every film and series without a rating is held back for the profile watching.
    pub unrated_hidden: bool,
    /// Films and series that carry a rating, and how many there are in all.
    pub rated_titles: i64,
    pub titles: i64,
    /// Seconds of today's viewing left; `None` when no daily limit applies.
    pub time_left_secs: Option<i64>,
    pub watched_today_secs: i64,
    /// Settings are closed until the parental PIN is entered.
    pub settings_locked: bool,
}

impl From<parental::ParentalState> for ParentalState {
    fn from(s: parental::ParentalState) -> Self {
        Self {
            profile_id: s.profile_id,
            unlocked_until: s.unlocked_until,
            hide_locked: s.hide_locked,
            restricted: s.restricted,
            unrated_hidden: s.unrated_hidden,
            rated_titles: s.rated_titles,
            titles: s.titles,
            time_left_secs: s.time_left_secs,
            watched_today_secs: s.watched_today_secs,
            settings_locked: s.settings_locked,
        }
    }
}

/// The household's parental settings (`profiles.parental`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ParentalSettings {
    pub has_master_pin: bool,
    /// Categories named adult, XXX or 18+ are left out for everybody.
    pub hide_adult: bool,
    /// Settings ask for the parental PIN for everybody, not only a kids profile.
    pub lock_settings: bool,
}

/// What saving the parental settings does to the PIN.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum MasterPinChange {
    Keep,
    Set { pin: String },
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ParentalLockKind {
    Channel,
    Category,
}

impl From<ParentalLockKind> for LockKind {
    fn from(k: ParentalLockKind) -> Self {
        match k {
            ParentalLockKind::Channel => LockKind::Channel,
            ParentalLockKind::Category => LockKind::Category,
        }
    }
}

/// One lock, as Parental controls lists it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ParentalLock {
    pub kind: ParentalLockKind,
    /// A channel's id, or a category's name.
    pub value: String,
    /// The channel's name as the viewer sees it, or the category.
    pub label: String,
}

impl From<parental::Lock> for ParentalLock {
    fn from(l: parental::Lock) -> Self {
        Self {
            kind: match l.kind {
                LockKind::Channel => ParentalLockKind::Channel,
                LockKind::Category => ParentalLockKind::Category,
            },
            value: l.value,
            label: l.label,
        }
    }
}

/* ── The daily limit's clock (F-37) ───────────────────────────────────────── */

/// How much counted viewing is held before it is written down.
pub const FLUSH_SECS: f64 = 10.0;
/// When the warning comes: five minutes before the day's time runs out.
pub const WARN_SECS: i64 = 5 * 60;
/// The most one sample may count. A heartbeat that stalled is not an hour of television.
const MAX_GAP_SECS: f64 = 5.0;

/// What the clock has to say (Windows' `parental.clock` event).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ClockEvent {
    /// The day's time is nearly used up.
    Warning { secs_left: i64 },
    /// It is used up: stop playing.
    TimeUp,
}

/// Counts viewing time against the daily limit: Windows' `ViewingClock`, line for line.
/// On Windows the host's heartbeat samples it; here the player does, once a second while
/// it is open (`AuroraCore::viewing_sample`).
#[derive(Debug, Default)]
pub(crate) struct ViewingClock {
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

/// The clock and the moment it started, for monotonic seconds.
pub(crate) struct Viewing {
    clock: ViewingClock,
    started: Instant,
}

impl Default for Viewing {
    fn default() -> Self {
        Self {
            clock: ViewingClock::default(),
            started: Instant::now(),
        }
    }
}

#[uniffi::export]
impl AuroraCore {
    /// Choose who is watching, checking the profile's PIN when it has one. Anything the
    /// parental PIN had opened closes.
    pub fn select_profile(&self, profile_id: i64, pin: Option<String>) -> Result<PinOutcome> {
        let db = self.db.lock();
        Ok(select(&db, profile_id, pin.as_deref(), now_unix())?.into())
    }

    pub fn parental_state(&self) -> Result<ParentalState> {
        let db = self.db.lock();
        Ok(parental::state(&db, now_unix())?.into())
    }

    pub fn parental_settings(&self) -> Result<ParentalSettings> {
        let db = self.db.lock();
        let s = profiles::parental_settings(&db)?;
        Ok(ParentalSettings {
            has_master_pin: s.has_master_pin,
            hide_adult: s.hide_adult,
            lock_settings: s.lock_settings,
        })
    }

    /// Save the household's parental settings, and set, change or remove the PIN
    /// (`profiles_set_parental`).
    pub fn set_parental(
        &self,
        hide_adult: bool,
        lock_settings: bool,
        master_pin: MasterPinChange,
    ) -> Result<()> {
        if let MasterPinChange::Set { pin } = &master_pin {
            crate::profiles::check_pin(pin)?;
        }
        let db = self.db.lock();
        require_parent(&db)?;
        profiles::set_parental_flags(&db, hide_adult, lock_settings, now_unix())?;
        match master_pin {
            MasterPinChange::Keep => {}
            MasterPinChange::Set { pin } => {
                profiles::set_master_pin(&db, Some(&pin), now_unix())?;
                // Whoever just chose the PIN knows it: the rest of this visit to the
                // settings should not ask for it straight back.
                parental::unlock(&db, now_unix())?;
            }
            MasterPinChange::Remove => profiles::set_master_pin(&db, None, now_unix())?,
        }
        Ok(())
    }

    /// Enter the parental PIN: opens locks and the parental settings for half an hour.
    /// Throttled like every other PIN.
    pub fn parental_unlock(&self, pin: String) -> Result<PinOutcome> {
        let db = self.db.lock();
        let now = now_unix();
        if !profiles::parental_settings(&db)?.has_master_pin {
            return Err(AuroraError::other(
                "There is no parental PIN yet. Set one in Settings → Parental controls.",
            ));
        }
        let outcome = profiles::verify_master_pin(&db, &pin, now)?;
        if outcome == DbPinOutcome::Ok {
            parental::unlock(&db, now)?;
        }
        Ok(outcome.into())
    }

    /// Close what the PIN opened, as when leaving Settings.
    pub fn parental_relock(&self) -> Result<ParentalState> {
        let db = self.db.lock();
        parental::relock(&db)?;
        Ok(parental::state(&db, now_unix())?.into())
    }

    pub fn parental_locks(&self) -> Result<Vec<ParentalLock>> {
        let db = self.db.lock();
        Ok(parental::locks(&db)?
            .into_iter()
            .map(ParentalLock::from)
            .collect())
    }

    pub fn parental_set_lock(
        &self,
        kind: ParentalLockKind,
        value: String,
        locked: bool,
    ) -> Result<Vec<ParentalLock>> {
        let db = self.db.lock();
        require_parent(&db)?;
        if locked && !profiles::parental_settings(&db)?.has_master_pin {
            return Err(AuroraError::other(
                "Set a parental PIN first: a lock is opened with it, so without one nothing \
                 locked could be watched.",
            ));
        }
        parental::set_lock(&db, kind.into(), &value, locked)?;
        Ok(parental::locks(&db)?
            .into_iter()
            .map(ParentalLock::from)
            .collect())
    }

    pub fn parental_set_hide_locked(&self, hide: bool) -> Result<ParentalState> {
        let db = self.db.lock();
        require_parent(&db)?;
        parental::set_hide_locked(&db, hide)?;
        Ok(parental::state(&db, now_unix())?.into())
    }

    /// A profile's age limit and daily limit (`profiles_set_limits`). `max_age` is a
    /// minimum age as `aurora_core::parental` reads one; `None` is no limit.
    pub fn set_profile_limits(
        &self,
        profile_id: i64,
        max_age: Option<u8>,
        allow_unrated: bool,
        daily_limit_min: Option<u32>,
    ) -> Result<()> {
        let db = self.db.lock();
        require_parent(&db)?;
        profiles::set_limits(&db, profile_id, max_age, allow_unrated, daily_limit_min)?;
        Ok(())
    }

    /// The player's heartbeat for the daily limit: call once a second while the player
    /// is open, saying whether it is playing or buffering, and once with `false` as it
    /// closes. `TimeUp` means stop now; a `Warning` comes once, five minutes before.
    pub fn viewing_sample(&self, watching: bool) -> Option<ClockEvent> {
        let mut viewing = self.viewing.lock();
        let mono = viewing.started.elapsed().as_secs_f64();
        if !viewing.clock.sample(watching, mono) {
            return None;
        }
        let db = self.db.lock();
        match viewing.clock.flush(&db, now_unix()) {
            Ok(event) => {
                if event == Some(ClockEvent::TimeUp) {
                    tracing::info!("today's viewing time is used up");
                }
                event
            }
            Err(e) => {
                tracing::warn!("could not count viewing time: {e}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::core;
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
        assert_eq!(clock.counted, 0.5);

        parental::unlock(&db, NOW).unwrap();
        clock.counted = 10.0;
        assert_eq!(
            clock.flush(&db, NOW).unwrap(),
            None,
            "the PIN gave more time"
        );
    }

    fn refusal<T: std::fmt::Debug>(r: Result<T>) -> Refusal {
        match r {
            Err(AuroraError::Parental { refusal, .. }) => refusal,
            other => panic!("expected a parental refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_launch_starts_with_nobody_chosen_and_nothing_unlocked() {
        let (core, dir) = core();
        core.set_parental(false, false, MasterPinChange::Set { pin: "2468".into() })
            .unwrap();
        assert!(
            core.parental_state().unwrap().unlocked_until.is_some(),
            "setting the PIN opens the rest of the visit"
        );
        core.select_profile(1, None).unwrap();
        drop(core);
        let again = AuroraCore::open(
            dir.path().display().to_string(),
            std::sync::Arc::new(crate::secrets::tests::MemorySecrets::default()),
        )
        .unwrap();
        let state = again.parental_state().unwrap();
        assert_eq!(state.profile_id, None);
        assert_eq!(state.unlocked_until, None);
    }

    #[test]
    fn with_a_parental_pin_set_profile_and_parental_changes_wait_for_it() {
        let (core, _dir) = core();
        // No parental PIN: nothing is guarded.
        let kid = core.create_profile("Sam".into(), true, None).unwrap();
        core.set_parental(true, false, MasterPinChange::Set { pin: "2468".into() })
            .unwrap();
        core.select_profile(kid, None).unwrap();

        assert_eq!(
            refusal(core.create_profile("Free".into(), false, None)),
            Refusal::PinRequired
        );
        assert_eq!(refusal(core.delete_profile(1)), Refusal::PinRequired);
        assert_eq!(
            refusal(core.set_profile_limits(kid, None, true, None)),
            Refusal::PinRequired
        );
        assert_eq!(
            refusal(core.update_profile(1, "Parent".into(), crate::PinChange::Remove)),
            Refusal::PinRequired,
            "somebody else's PIN is a parent's to change"
        );
        core.update_profile(
            kid,
            "Sam".into(),
            crate::PinChange::Set { pin: "1357".into() },
        )
        .expect("your own PIN is yours");
        assert_eq!(
            refusal(core.set_parental(false, false, MasterPinChange::Keep)),
            Refusal::PinRequired
        );

        assert!(matches!(
            core.parental_unlock("0000".into()).unwrap(),
            PinOutcome::Wrong { .. }
        ));
        assert_eq!(core.parental_unlock("2468".into()).unwrap(), PinOutcome::Ok);
        core.set_profile_limits(kid, Some(6), false, Some(60))
            .unwrap();
        let sam = core
            .profiles()
            .unwrap()
            .into_iter()
            .find(|p| p.id == kid)
            .unwrap();
        assert_eq!(
            (sam.max_age, sam.allow_unrated, sam.daily_limit_min),
            (Some(6), false, Some(60))
        );
    }

    #[test]
    fn a_kids_profile_is_kept_out_of_settings_until_the_pin() {
        let (core, _dir) = core();
        let kid = core.create_profile("Sam".into(), true, None).unwrap();
        core.set_parental(true, false, MasterPinChange::Set { pin: "2468".into() })
            .unwrap();
        core.select_profile(1, None).unwrap();
        assert!(
            !core.parental_state().unwrap().settings_locked,
            "a grown-up gets in"
        );
        core.set_library_filters(core.library_filters().unwrap())
            .unwrap();

        core.select_profile(kid, None).unwrap();
        assert!(core.parental_state().unwrap().settings_locked);
        let filters = core.library_filters().unwrap();
        assert_eq!(
            refusal(core.set_library_filters(filters)),
            Refusal::PinRequired
        );
        assert_eq!(refusal(core.remove_provider(1)), Refusal::PinRequired);
        assert_eq!(refusal(core.provider_settings(1)), Refusal::PinRequired);
        assert_eq!(refusal(core.empty_timeshift()), Refusal::PinRequired);

        core.parental_unlock("2468".into()).unwrap();
        assert!(!core.parental_state().unwrap().settings_locked);
        core.set_library_filters(filters).unwrap();
        core.parental_relock().unwrap();
        assert!(core.parental_state().unwrap().settings_locked);
    }

    #[test]
    fn a_profile_with_a_pin_is_chosen_only_with_it() {
        let (core, _dir) = core();
        core.update_profile(1, "Me".into(), crate::PinChange::Set { pin: "1357".into() })
            .unwrap();
        assert!(core.select_profile(1, None).is_err(), "no PIN sent");
        assert!(matches!(
            core.select_profile(1, Some("0000".into())).unwrap(),
            PinOutcome::Wrong { .. }
        ));
        assert_eq!(core.parental_state().unwrap().profile_id, None);
        assert_eq!(
            core.select_profile(1, Some("1357".into())).unwrap(),
            PinOutcome::Ok
        );
        assert_eq!(core.parental_state().unwrap().profile_id, Some(1));
        assert!(core.select_profile(42, None).is_err(), "no such profile");
    }

    #[test]
    fn a_lock_needs_a_pin_and_is_listed_by_name() {
        let (core, _dir) = core();
        assert!(core
            .parental_set_lock(ParentalLockKind::Category, "News".into(), true)
            .is_err());
        core.set_parental(true, false, MasterPinChange::Set { pin: "2468".into() })
            .unwrap();
        let locks = core
            .parental_set_lock(ParentalLockKind::Category, "News".into(), true)
            .unwrap();
        assert_eq!(
            locks,
            vec![ParentalLock {
                kind: ParentalLockKind::Category,
                value: "News".into(),
                label: "News".into()
            }]
        );
        assert!(!core.parental_state().unwrap().hide_locked);
        assert!(core.parental_set_hide_locked(true).unwrap().hide_locked);
        core.parental_set_lock(ParentalLockKind::Category, "News".into(), false)
            .unwrap();
        assert!(core.parental_locks().unwrap().is_empty());
    }
}
