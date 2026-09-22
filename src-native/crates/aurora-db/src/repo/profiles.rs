//! Profiles and parental controls (README §11).

use aurora_core::parental::{self, AgeLimit};
use aurora_core::pin;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// README §11: "up to 6 profiles".
pub const MAX_PROFILES: usize = 6;

/// How many wrong PINs before the keypad locks, and for how long. This — not the
/// hash — is what makes guessing a 4-digit PIN impractical in practice.
pub const MAX_PIN_FAILURES: i64 = 5;
pub const PIN_LOCKOUT_SECS: i64 = 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: i64,
    pub name: String,
    pub avatar: Option<String>,
    pub is_kids: bool,
    /// Whether a PIN must be entered to use this profile. The hash never leaves the
    /// database layer.
    pub has_pin: bool,
    /// Highest age rating this profile may watch, if limited.
    pub max_age: Option<u8>,
    pub allow_unrated: bool,
    pub daily_limit_min: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct NewProfile {
    pub name: String,
    pub avatar: Option<String>,
    pub is_kids: bool,
    pub max_age: Option<u8>,
    pub allow_unrated: Option<bool>,
    pub daily_limit_min: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParentalSettings {
    pub has_master_pin: bool,
    pub hide_adult: bool,
    pub lock_settings: bool,
}

fn row_to_profile(r: &rusqlite::Row<'_>) -> rusqlite::Result<Profile> {
    let max_rating: Option<String> = r.get(4)?;
    Ok(Profile {
        id: r.get(0)?,
        name: r.get(1)?,
        avatar: r.get(2)?,
        is_kids: r.get::<_, i64>(3)? != 0,
        max_age: max_rating.as_deref().and_then(parental::min_age),
        has_pin: r.get::<_, Option<String>>(5)?.is_some(),
        allow_unrated: r.get::<_, i64>(6)? != 0,
        daily_limit_min: r.get::<_, Option<i64>>(7)?.map(|v| v as u32),
    })
}

const SELECT: &str = "SELECT id, name, avatar, is_kids, max_rating, pin_hash,
                             allow_unrated, daily_limit_min
                      FROM profiles";

pub fn list(conn: &Connection) -> Result<Vec<Profile>> {
    let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY sort_order, id"))?;
    let rows = stmt
        .query_map([], row_to_profile)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Profile>> {
    Ok(conn
        .query_row(
            &format!("{SELECT} WHERE id = ?1"),
            params![id],
            row_to_profile,
        )
        .optional()?)
}

/// Create a profile. Refuses past [`MAX_PROFILES`] rather than growing without bound.
pub fn create(conn: &Connection, new: &NewProfile, now: i64) -> Result<i64> {
    if list(conn)?.len() >= MAX_PROFILES {
        return Err(crate::DbError::Rejected(format!(
            "Aurora supports at most {MAX_PROFILES} profiles"
        )));
    }
    // A kids profile defaults to blocking unrated content: patchy IPTV metadata means
    // "no rating" is usually missing data, not a guarantee.
    let allow_unrated = new.allow_unrated.unwrap_or(!new.is_kids);
    conn.execute(
        "INSERT INTO profiles
           (name, avatar, is_kids, max_rating, allow_unrated, daily_limit_min, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            new.name,
            new.avatar,
            new.is_kids as i32,
            new.max_age.map(|a| a.to_string()),
            allow_unrated as i32,
            new.daily_limit_min,
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Delete a profile. The last remaining profile cannot be removed — there would be
/// nothing to sign in to, and every per-profile row would cascade away with it.
pub fn delete(conn: &Connection, id: i64) -> Result<bool> {
    if list(conn)?.len() <= 1 {
        return Ok(false);
    }
    let n = conn.execute("DELETE FROM profiles WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

pub fn rename(conn: &Connection, id: i64, name: &str) -> Result<()> {
    conn.execute(
        "UPDATE profiles SET name = ?2 WHERE id = ?1",
        params![id, name],
    )?;
    Ok(())
}

pub fn set_limits(
    conn: &Connection,
    id: i64,
    max_age: Option<u8>,
    allow_unrated: bool,
    daily_limit_min: Option<u32>,
) -> Result<()> {
    conn.execute(
        "UPDATE profiles SET max_rating = ?2, allow_unrated = ?3, daily_limit_min = ?4
         WHERE id = ?1",
        params![
            id,
            max_age.map(|a| a.to_string()),
            allow_unrated as i32,
            daily_limit_min
        ],
    )?;
    Ok(())
}

/// Set or clear a profile's PIN. `None` removes it.
pub fn set_pin(conn: &Connection, id: i64, new_pin: Option<&str>) -> Result<()> {
    let hash = match new_pin {
        Some(p) => Some(pin::hash(p).map_err(|e| crate::DbError::Rejected(e.to_string()))?),
        None => None,
    };
    conn.execute(
        "UPDATE profiles SET pin_hash = ?2 WHERE id = ?1",
        params![id, hash],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PinOutcome {
    Ok,
    Wrong {
        remaining: i64,
    },
    /// Too many failures; `until` is a Unix timestamp.
    LockedOut {
        until: i64,
    },
    /// No PIN is set for this scope, so nothing to check.
    NotRequired,
}

fn attempt_state(conn: &Connection, scope: &str) -> Result<(i64, i64)> {
    Ok(conn
        .query_row(
            "SELECT failures, locked_until FROM pin_attempts WHERE scope = ?1",
            params![scope],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .unwrap_or((0, 0)))
}

fn record_attempt(conn: &Connection, scope: &str, ok: bool, now: i64) -> Result<i64> {
    if ok {
        conn.execute("DELETE FROM pin_attempts WHERE scope = ?1", params![scope])?;
        return Ok(0);
    }
    let (failures, _) = attempt_state(conn, scope)?;
    let failures = failures + 1;
    let locked_until = if failures >= MAX_PIN_FAILURES {
        now + PIN_LOCKOUT_SECS
    } else {
        0
    };
    conn.execute(
        "INSERT INTO pin_attempts (scope, failures, locked_until) VALUES (?1,?2,?3)
         ON CONFLICT (scope) DO UPDATE SET failures = ?2, locked_until = ?3",
        params![scope, failures, locked_until],
    )?;
    Ok(failures)
}

fn verify_scoped(
    conn: &Connection,
    scope: &str,
    stored: Option<String>,
    candidate: &str,
    now: i64,
) -> Result<PinOutcome> {
    let Some(stored) = stored else {
        return Ok(PinOutcome::NotRequired);
    };

    let (_, locked_until) = attempt_state(conn, scope)?;
    if locked_until > now {
        return Ok(PinOutcome::LockedOut {
            until: locked_until,
        });
    }

    if pin::verify(candidate, &stored) {
        record_attempt(conn, scope, true, now)?;
        Ok(PinOutcome::Ok)
    } else {
        let failures = record_attempt(conn, scope, false, now)?;
        if failures >= MAX_PIN_FAILURES {
            Ok(PinOutcome::LockedOut {
                until: now + PIN_LOCKOUT_SECS,
            })
        } else {
            Ok(PinOutcome::Wrong {
                remaining: MAX_PIN_FAILURES - failures,
            })
        }
    }
}

pub fn verify_profile_pin(
    conn: &Connection,
    id: i64,
    candidate: &str,
    now: i64,
) -> Result<PinOutcome> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT pin_hash FROM profiles WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    verify_scoped(conn, &format!("profile:{id}"), stored, candidate, now)
}

pub fn verify_master_pin(conn: &Connection, candidate: &str, now: i64) -> Result<PinOutcome> {
    let stored: Option<String> = conn
        .query_row("SELECT master_pin FROM parental WHERE id = 1", [], |r| {
            r.get(0)
        })
        .optional()?
        .flatten();
    verify_scoped(conn, "master", stored, candidate, now)
}

pub fn parental_settings(conn: &Connection) -> Result<ParentalSettings> {
    Ok(conn
        .query_row(
            "SELECT master_pin, hide_adult, lock_settings FROM parental WHERE id = 1",
            [],
            |r| {
                Ok(ParentalSettings {
                    has_master_pin: r.get::<_, Option<String>>(0)?.is_some(),
                    hide_adult: r.get::<_, i64>(1)? != 0,
                    lock_settings: r.get::<_, i64>(2)? != 0,
                })
            },
        )
        .optional()?
        .unwrap_or(ParentalSettings {
            has_master_pin: false,
            hide_adult: true,
            lock_settings: false,
        }))
}

pub fn set_master_pin(conn: &Connection, new_pin: Option<&str>, now: i64) -> Result<()> {
    let hash = match new_pin {
        Some(p) => Some(pin::hash(p).map_err(|e| crate::DbError::Rejected(e.to_string()))?),
        None => None,
    };
    conn.execute(
        "UPDATE parental SET master_pin = ?1, updated_at = ?2 WHERE id = 1",
        params![hash, now],
    )?;
    Ok(())
}

pub fn set_parental_flags(
    conn: &Connection,
    hide_adult: bool,
    lock_settings: bool,
    now: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE parental SET hide_adult = ?1, lock_settings = ?2, updated_at = ?3 WHERE id = 1",
        params![hide_adult as i32, lock_settings as i32, now],
    )?;
    Ok(())
}

/// Whether a profile may watch something with this certification and category.
pub fn may_watch(
    profile: &Profile,
    settings: &ParentalSettings,
    certification: Option<&str>,
    group: Option<&str>,
) -> bool {
    if settings.hide_adult && parental::looks_adult(group) {
        return false;
    }
    parental::is_allowed(
        certification,
        profile.max_age.map(AgeLimit),
        profile.allow_unrated,
    )
}

/// Minutes this profile has watched since `day_start`, for the daily limit.
pub fn minutes_watched_since(conn: &Connection, profile_id: i64, since: i64) -> Result<u32> {
    // Derived from progress rows touched in the window. An approximation — it counts
    // position, not wall-clock — but it is the signal available without a separate
    // session log, and it is monotonic, which is what a limit needs.
    let secs: i64 = conn.query_row(
        "SELECT COALESCE(SUM(MIN(position_secs, duration_secs)), 0)
         FROM watch_progress WHERE profile_id = ?1 AND updated_at >= ?2",
        params![profile_id, since],
        |r| r.get(0),
    )?;
    Ok((secs / 60).max(0) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        crate::open_memory().unwrap()
    }

    #[test]
    fn a_fresh_database_already_has_a_usable_profile() {
        // The bug this migration fixes: every per-profile table has a foreign key onto
        // profiles, and nothing created a row, so the first write failed on a real
        // install while passing in tests that made their own.
        let conn = db();
        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, 1);

        crate::repo::progress::save(
            &conn,
            1,
            crate::repo::progress::ItemKind::Movie,
            1,
            100,
            1000,
            0,
        )
        .expect("saving progress for the default profile must not violate a foreign key");
    }

    #[test]
    fn creates_and_lists_profiles() {
        let conn = db();
        let id = create(
            &conn,
            &NewProfile {
                name: "Sam".into(),
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|p| p.id == id && p.name == "Sam"));
    }

    #[test]
    fn refuses_more_than_six_profiles() {
        let conn = db();
        for i in 0..MAX_PROFILES - 1 {
            create(
                &conn,
                &NewProfile {
                    name: format!("P{i}"),
                    ..Default::default()
                },
                0,
            )
            .unwrap();
        }
        assert_eq!(list(&conn).unwrap().len(), MAX_PROFILES);
        assert!(create(
            &conn,
            &NewProfile {
                name: "one too many".into(),
                ..Default::default()
            },
            0
        )
        .is_err());
    }

    #[test]
    fn the_last_profile_cannot_be_deleted() {
        let conn = db();
        assert!(
            !delete(&conn, 1).unwrap(),
            "there would be nothing left to sign in to"
        );

        let id = create(
            &conn,
            &NewProfile {
                name: "Sam".into(),
                ..Default::default()
            },
            0,
        )
        .unwrap();
        assert!(delete(&conn, id).unwrap());
        assert_eq!(list(&conn).unwrap().len(), 1);
    }

    #[test]
    fn deleting_a_profile_takes_its_data_with_it() {
        let conn = db();
        let id = create(
            &conn,
            &NewProfile {
                name: "Sam".into(),
                ..Default::default()
            },
            0,
        )
        .unwrap();
        crate::repo::progress::save(
            &conn,
            id,
            crate::repo::progress::ItemKind::Movie,
            5,
            100,
            1000,
            0,
        )
        .unwrap();

        delete(&conn, id).unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM watch_progress WHERE profile_id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn a_kids_profile_blocks_unrated_content_by_default() {
        let conn = db();
        let id = create(
            &conn,
            &NewProfile {
                name: "Kid".into(),
                is_kids: true,
                max_age: Some(12),
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let p = get(&conn, id).unwrap().unwrap();
        assert!(p.is_kids);
        assert_eq!(p.max_age, Some(12));
        assert!(
            !p.allow_unrated,
            "patchy metadata should not become a loophole"
        );
    }

    #[test]
    fn may_watch_respects_the_age_ceiling() {
        let conn = db();
        let id = create(
            &conn,
            &NewProfile {
                name: "Kid".into(),
                is_kids: true,
                max_age: Some(12),
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let kid = get(&conn, id).unwrap().unwrap();
        let settings = parental_settings(&conn).unwrap();

        assert!(may_watch(&kid, &settings, Some("PG"), Some("Kids")));
        assert!(!may_watch(&kid, &settings, Some("R"), Some("Movies")));
        assert!(
            !may_watch(&kid, &settings, None, Some("Movies")),
            "unrated is blocked"
        );

        let adult = get(&conn, 1).unwrap().unwrap();
        assert!(may_watch(&adult, &settings, Some("R"), Some("Movies")));
    }

    #[test]
    fn adult_categories_are_hidden_by_default_for_everyone() {
        let conn = db();
        let settings = parental_settings(&conn).unwrap();
        assert!(settings.hide_adult, "README §11: hidden on a fresh install");

        let adult_profile = get(&conn, 1).unwrap().unwrap();
        assert!(!may_watch(
            &adult_profile,
            &settings,
            Some("18"),
            Some("XXX")
        ));

        // ...until explicitly opted into.
        set_parental_flags(&conn, false, false, 0).unwrap();
        let relaxed = parental_settings(&conn).unwrap();
        assert!(may_watch(&adult_profile, &relaxed, Some("18"), Some("XXX")));
    }

    #[test]
    fn a_profile_without_a_pin_needs_none() {
        let conn = db();
        assert_eq!(
            verify_profile_pin(&conn, 1, "0000", 0).unwrap(),
            PinOutcome::NotRequired
        );
    }

    #[test]
    fn the_right_pin_is_accepted_and_the_wrong_one_counts_down() {
        let conn = db();
        set_pin(&conn, 1, Some("1234")).unwrap();

        assert_eq!(
            verify_profile_pin(&conn, 1, "1234", 0).unwrap(),
            PinOutcome::Ok
        );
        assert_eq!(
            verify_profile_pin(&conn, 1, "0000", 0).unwrap(),
            PinOutcome::Wrong {
                remaining: MAX_PIN_FAILURES - 1
            }
        );
    }

    #[test]
    fn repeated_wrong_pins_lock_the_keypad() {
        let conn = db();
        set_pin(&conn, 1, Some("1234")).unwrap();

        for _ in 0..MAX_PIN_FAILURES {
            let _ = verify_profile_pin(&conn, 1, "0000", 100).unwrap();
        }
        let outcome = verify_profile_pin(&conn, 1, "1234", 100).unwrap();
        assert!(
            matches!(outcome, PinOutcome::LockedOut { .. }),
            "even the correct PIN waits out the lockout: {outcome:?}"
        );

        // ...and it expires.
        assert_eq!(
            verify_profile_pin(&conn, 1, "1234", 100 + PIN_LOCKOUT_SECS + 1).unwrap(),
            PinOutcome::Ok
        );
    }

    #[test]
    fn a_correct_pin_clears_the_failure_count() {
        let conn = db();
        set_pin(&conn, 1, Some("1234")).unwrap();
        let _ = verify_profile_pin(&conn, 1, "0000", 0).unwrap();
        assert_eq!(
            verify_profile_pin(&conn, 1, "1234", 0).unwrap(),
            PinOutcome::Ok
        );
        assert_eq!(
            verify_profile_pin(&conn, 1, "0000", 0).unwrap(),
            PinOutcome::Wrong {
                remaining: MAX_PIN_FAILURES - 1
            },
            "the counter should have reset"
        );
    }

    #[test]
    fn profile_and_master_pins_are_throttled_separately() {
        let conn = db();
        set_pin(&conn, 1, Some("1111")).unwrap();
        set_master_pin(&conn, Some("2222"), 0).unwrap();

        for _ in 0..MAX_PIN_FAILURES {
            let _ = verify_profile_pin(&conn, 1, "0000", 0).unwrap();
        }
        // Locking the profile keypad must not lock the master one.
        assert_eq!(verify_master_pin(&conn, "2222", 0).unwrap(), PinOutcome::Ok);
    }

    #[test]
    fn a_pin_can_be_removed() {
        let conn = db();
        set_pin(&conn, 1, Some("1234")).unwrap();
        assert!(get(&conn, 1).unwrap().unwrap().has_pin);
        set_pin(&conn, 1, None).unwrap();
        assert!(!get(&conn, 1).unwrap().unwrap().has_pin);
        assert_eq!(
            verify_profile_pin(&conn, 1, "1234", 0).unwrap(),
            PinOutcome::NotRequired
        );
    }

    #[test]
    fn a_malformed_pin_is_refused_rather_than_stored() {
        let conn = db();
        assert!(set_pin(&conn, 1, Some("abc")).is_err());
        assert!(!get(&conn, 1).unwrap().unwrap().has_pin);
    }

    #[test]
    fn the_pin_hash_is_never_exposed_on_the_profile() {
        let conn = db();
        set_pin(&conn, 1, Some("1234")).unwrap();
        let json = serde_json::to_string(&get(&conn, 1).unwrap().unwrap()).unwrap();
        assert!(json.contains("hasPin"));
        assert!(
            !json.contains("argon2"),
            "the hash must not cross the IPC boundary: {json}"
        );
        assert!(!json.contains("1234"));
    }

    #[test]
    fn counts_minutes_watched_in_a_window() {
        let conn = db();
        crate::repo::progress::save(
            &conn,
            1,
            crate::repo::progress::ItemKind::Movie,
            1,
            1800,
            7200,
            1000,
        )
        .unwrap();
        crate::repo::progress::save(
            &conn,
            1,
            crate::repo::progress::ItemKind::Episode,
            2,
            600,
            2400,
            1000,
        )
        .unwrap();

        assert_eq!(minutes_watched_since(&conn, 1, 0).unwrap(), 40);
        assert_eq!(
            minutes_watched_since(&conn, 1, 2000).unwrap(),
            0,
            "outside the window"
        );
    }

    #[test]
    fn limits_round_trip() {
        let conn = db();
        set_limits(&conn, 1, Some(15), false, Some(120)).unwrap();
        let p = get(&conn, 1).unwrap().unwrap();
        assert_eq!(p.max_age, Some(15));
        assert!(!p.allow_unrated);
        assert_eq!(p.daily_limit_min, Some(120));
    }
}
