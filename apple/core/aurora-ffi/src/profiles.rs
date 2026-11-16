//! Who's watching: the profiles to choose between, and the PIN that guards one.
//!
//! Straight calls into `aurora_db::repo::profiles`, as Windows' `profiles.rs` commands
//! are. The PIN check, its attempt count and its lockout all live there, so a wrong PIN
//! on the Apple TV counts against the same five tries as one on Windows would.

use aurora_db::repo::profiles;

use crate::error::{AuroraError, Result};
use crate::{now_unix, AuroraCore};

/// The age limit a new kids profile starts with. Windows' profile editor
/// (`src-ui/.../ProfilePicker.tsx`) sets `maxAge: kids ? 12 : null`; mirrors.rs checks
/// this still matches.
pub(crate) const KIDS_MAX_AGE: u8 = 12;
/// A profile PIN is exactly this many digits, as the Windows editor and PIN pad require.
pub(crate) const PIN_DIGITS: usize = 4;

/// What saving a profile does to its PIN.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PinChange {
    Keep,
    Set { pin: String },
    Remove,
}

fn clean_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AuroraError::other("A profile needs a name."));
    }
    Ok(trimmed.to_string())
}

fn check_pin(pin: &str) -> Result<()> {
    if pin.len() == PIN_DIGITS && pin.bytes().all(|b| b.is_ascii_digit()) {
        Ok(())
    } else {
        Err(AuroraError::other(format!("A PIN is {PIN_DIGITS} digits.")))
    }
}

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

    /// Add a profile, as Windows' editor does: a kids profile starts limited to age
    /// 12, and a PIN, if given, is set straight away. Returns its id.
    pub fn create_profile(&self, name: String, is_kids: bool, pin: Option<String>) -> Result<i64> {
        let name = clean_name(&name)?;
        if let Some(pin) = &pin {
            check_pin(pin)?;
        }
        let db = self.db.lock();
        let id = profiles::create(
            &db,
            &profiles::NewProfile {
                name,
                avatar: None,
                is_kids,
                max_age: is_kids.then_some(KIDS_MAX_AGE),
                allow_unrated: None,
                daily_limit_min: None,
            },
            now_unix(),
        )?;
        if let Some(pin) = pin {
            profiles::set_pin(&db, id, Some(&pin))?;
        }
        Ok(id)
    }

    /// Rename a profile and change its PIN, as Windows' editor saves one.
    pub fn update_profile(&self, profile_id: i64, name: String, pin: PinChange) -> Result<()> {
        let name = clean_name(&name)?;
        if let PinChange::Set { pin } = &pin {
            check_pin(pin)?;
        }
        let db = self.db.lock();
        let current = profiles::get(&db, profile_id)?
            .ok_or_else(|| AuroraError::not_found("That profile no longer exists."))?;
        if current.name != name {
            profiles::rename(&db, profile_id, &name)?;
        }
        match pin {
            PinChange::Keep => {}
            PinChange::Set { pin } => profiles::set_pin(&db, profile_id, Some(&pin))?,
            PinChange::Remove => profiles::set_pin(&db, profile_id, None)?,
        }
        Ok(())
    }

    /// Delete a profile and everything kept for it. The last profile can't be deleted
    /// (the shared database refuses), so this returns false for it.
    pub fn delete_profile(&self, profile_id: i64) -> Result<bool> {
        let db = self.db.lock();
        Ok(profiles::delete(&db, profile_id)?)
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
    fn profiles_are_added_edited_and_deleted_as_windows_does() {
        let (core, _dir) = core();
        let kid = core
            .create_profile("  Sam ".into(), true, Some("1357".into()))
            .unwrap();
        let sam = core
            .profiles()
            .unwrap()
            .into_iter()
            .find(|p| p.id == kid)
            .unwrap();
        assert_eq!(sam.name, "Sam", "names are trimmed");
        assert!(sam.is_kids && sam.has_pin);
        let stored = {
            let db = core.db.lock();
            aurora_db::repo::profiles::get(&db, kid).unwrap().unwrap()
        };
        assert_eq!(
            stored.max_age,
            Some(super::KIDS_MAX_AGE),
            "a kids profile starts at 12"
        );

        core.update_profile(kid, "Samira".into(), super::PinChange::Remove)
            .unwrap();
        let samira = core
            .profiles()
            .unwrap()
            .into_iter()
            .find(|p| p.id == kid)
            .unwrap();
        assert_eq!(samira.name, "Samira");
        assert!(!samira.has_pin);

        assert!(core.delete_profile(kid).unwrap());
        assert!(!core.delete_profile(1).unwrap(), "the last profile stays");
        assert_eq!(core.profiles().unwrap().len(), 1);
    }

    #[test]
    fn a_profile_needs_a_name_and_a_four_digit_pin() {
        let (core, _dir) = core();
        assert!(core.create_profile("   ".into(), false, None).is_err());
        for bad in ["123", "12345", "12a4"] {
            assert!(
                core.create_profile("Ann".into(), false, Some(bad.into()))
                    .is_err(),
                "{bad}"
            );
        }
        assert_eq!(core.profiles().unwrap().len(), 1, "nothing half-created");
        assert!(core
            .update_profile(1, "Me".into(), super::PinChange::Set { pin: "99".into() })
            .is_err());
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
