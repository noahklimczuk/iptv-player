//! PIN hashing for profile and parental locks (README §11).
//!
//! What this can and cannot do, stated plainly because it matters:
//!
//! A four-digit PIN has ten thousand possibilities. Argon2 makes each guess expensive
//! enough that guessing at the keypad is hopeless, and means the database never holds
//! the PIN itself. It does **not** meaningfully protect against someone who copies the
//! database file and is willing to spend an afternoon on it. This is a lock that keeps
//! a nine-year-old out of the horror section, not a security boundary — and the UI
//! should not imply otherwise.
//!
//! Attempt throttling (see `aurora_db::repo::profiles`) is what actually limits
//! guessing in practice.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

#[derive(Debug, thiserror::Error)]
pub enum PinError {
    #[error("a PIN must be {MIN_LENGTH}-{MAX_LENGTH} digits")]
    Malformed,
    #[error("could not hash the PIN: {0}")]
    Hash(String),
    #[error("no source of randomness available: {0}")]
    Entropy(String),
}

pub const MIN_LENGTH: usize = 4;
pub const MAX_LENGTH: usize = 8;

/// Reject anything that is not a plausible PIN before it reaches the hasher.
pub fn is_valid_shape(pin: &str) -> bool {
    let len = pin.chars().count();
    (MIN_LENGTH..=MAX_LENGTH).contains(&len) && pin.chars().all(|c| c.is_ascii_digit())
}

pub fn hash(pin: &str) -> Result<String, PinError> {
    if !is_valid_shape(pin) {
        return Err(PinError::Malformed);
    }
    // Drawn straight from the OS rather than through an RNG adapter, so the salt
    // source is explicit and does not depend on optional crate features lining up.
    let mut raw = [0u8; 16];
    getrandom::fill(&mut raw).map_err(|e| PinError::Entropy(e.to_string()))?;
    let salt = SaltString::encode_b64(&raw).map_err(|e| PinError::Hash(e.to_string()))?;
    Argon2::default()
        .hash_password(pin.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| PinError::Hash(e.to_string()))
}

/// Check a PIN against a stored hash. A malformed stored hash verifies as `false`
/// rather than erroring, so a corrupted row locks the profile instead of opening it.
pub fn verify(pin: &str, stored_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(pin.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plausible_pins() {
        for pin in ["0000", "1234", "99999999"] {
            assert!(is_valid_shape(pin), "{pin}");
        }
    }

    #[test]
    fn rejects_implausible_ones() {
        for pin in ["", "1", "123", "123456789", "12a4", "12 4", "١٢٣٤"] {
            assert!(!is_valid_shape(pin), "{pin:?}");
        }
    }

    #[test]
    fn a_pin_round_trips() {
        let h = hash("1234").unwrap();
        assert!(verify("1234", &h));
    }

    #[test]
    fn the_wrong_pin_is_rejected() {
        let h = hash("1234").unwrap();
        assert!(!verify("4321", &h));
        assert!(!verify("12345", &h));
        assert!(!verify("", &h));
    }

    #[test]
    fn the_hash_does_not_contain_the_pin() {
        let h = hash("1234").unwrap();
        assert!(!h.contains("1234"), "{h}");
    }

    #[test]
    fn the_same_pin_hashes_differently_each_time() {
        // Distinct salts, so two profiles with PIN 1234 do not look identical in the
        // database.
        assert_ne!(hash("1234").unwrap(), hash("1234").unwrap());
    }

    #[test]
    fn hashing_refuses_a_malformed_pin() {
        assert!(matches!(hash("abc"), Err(PinError::Malformed)));
        assert!(matches!(hash("1"), Err(PinError::Malformed)));
    }

    #[test]
    fn a_corrupt_stored_hash_locks_rather_than_opens() {
        for bad in ["", "not-a-hash", "$argon2id$garbage"] {
            assert!(!verify("1234", bad), "{bad:?} must not verify");
        }
    }
}
