//! Who's watching: the profiles to choose between, and the PIN that guards one.
//!
//! Straight calls into `aurora_db::repo::profiles`, as Windows' `profiles.rs` commands
//! are. The PIN check, its attempt count and its lockout all live there, so a wrong PIN
//! on the Apple TV counts against the same five tries as one on Windows would.

use aurora_db::repo::profiles;

use crate::error::Result;
use crate::{now_unix, AuroraCore};

/// A profile, as the picker shows it. The PIN hash never leaves the database layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct Profile {
    pub id: i64,
    pub name: String,
    pub avatar: Option<String>,
    pub is_kids: bool,
    /// A PIN must be entered to use it.
    pub has_pin: bool,
}

impl From<profiles::Profile> for Profile {
    fn from(p: profiles::Profile) -> Self {
        Self {
            id: p.id,
            name: p.name,
            avatar: p.avatar,
            is_kids: p.is_kids,
            has_pin: p.has_pin,
        }
    }
}

/// What a PIN attempt came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PinOutcome {
    Ok,
    Wrong {
        remaining: i64,
    },
    /// Too many wrong tries. `until` is Unix seconds.
    LockedOut {
        until: i64,
    },
    /// The profile has no PIN.
    NotRequired,
}

impl From<profiles::PinOutcome> for PinOutcome {
    fn from(o: profiles::PinOutcome) -> Self {
        match o {
            profiles::PinOutcome::Ok => Self::Ok,
            profiles::PinOutcome::Wrong { remaining } => Self::Wrong { remaining },
            profiles::PinOutcome::LockedOut { until } => Self::LockedOut { until },
            profiles::PinOutcome::NotRequired => Self::NotRequired,
        }
    }
}

#[uniffi::export]
impl AuroraCore {
    /// Every profile. A fresh library already has one (id 1).
    pub fn profiles(&self) -> Result<Vec<Profile>> {
        let db = self.db.lock();
        Ok(profiles::list(&db)?
            .into_iter()
            .map(Profile::from)
            .collect())
    }

    pub fn verify_profile_pin(&self, profile_id: i64, pin: String) -> Result<PinOutcome> {
        let db = self.db.lock();
        Ok(profiles::verify_profile_pin(&db, profile_id, &pin, now_unix())?.into())
    }
}

#[cfg(test)]
mod tests {
    use super::PinOutcome;
    use crate::testing::core;

    #[test]
    fn a_fresh_library_has_one_open_profile() {
        let (core, _dir) = core();
        let all = core.profiles().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, 1);
        assert!(!all[0].has_pin);
        assert_eq!(
            core.verify_profile_pin(1, "1234".into()).unwrap(),
            PinOutcome::NotRequired
        );
    }

    #[test]
    fn wrong_pins_count_down_to_a_lockout() {
        let (core, _dir) = core();
        {
            let db = core.db.lock();
            aurora_db::repo::profiles::set_pin(&db, 1, Some("2468")).unwrap();
        }
        assert!(core.profiles().unwrap()[0].has_pin);
        assert_eq!(
            core.verify_profile_pin(1, "0000".into()).unwrap(),
            PinOutcome::Wrong { remaining: 4 }
        );
        assert_eq!(
            core.verify_profile_pin(1, "2468".into()).unwrap(),
            PinOutcome::Ok
        );
        for _ in 0..4 {
            core.verify_profile_pin(1, "0000".into()).unwrap();
        }
        assert!(matches!(
            core.verify_profile_pin(1, "0000".into()).unwrap(),
            PinOutcome::LockedOut { .. }
        ));
        // Locked out means locked out: even the right PIN waits.
        assert!(matches!(
            core.verify_profile_pin(1, "2468".into()).unwrap(),
            PinOutcome::LockedOut { .. }
        ));
    }
}
