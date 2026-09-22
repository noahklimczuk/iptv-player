//! Global and per-profile settings (README §5).
//!
//! Values are JSON so a setting can gain structure later without a migration. `scope` is
//! either `"global"` or a profile id as a string; [`get`] falls back from the profile to
//! the global value, which is what makes "inherit unless overridden" the default.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};

use crate::error::Result;

pub const GLOBAL: &str = "global";

pub fn profile_scope(profile_id: i64) -> String {
    profile_id.to_string()
}

/// Read a setting from one scope exactly, with no fallback.
pub fn get_in<T: DeserializeOwned>(conn: &Connection, scope: &str, key: &str) -> Result<Option<T>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE scope = ?1 AND key = ?2",
            params![scope, key],
            |r| r.get(0),
        )
        .optional()?;
    match raw {
        // A value written by an older build may no longer parse. Treating that as
        // "unset" is better than refusing to start.
        Some(raw) => Ok(serde_json::from_str(&raw).ok()),
        None => Ok(None),
    }
}

/// Read a global setting.
pub fn get<T: DeserializeOwned>(conn: &Connection, key: &str) -> Result<Option<T>> {
    get_in(conn, GLOBAL, key)
}

/// Read a profile's setting, falling back to the global value.
pub fn get_for<T: DeserializeOwned>(
    conn: &Connection,
    profile_id: i64,
    key: &str,
) -> Result<Option<T>> {
    match get_in(conn, &profile_scope(profile_id), key)? {
        Some(v) => Ok(Some(v)),
        None => get(conn, key),
    }
}

/// Read a setting, or the supplied default if it is unset.
pub fn get_or<T: DeserializeOwned>(conn: &Connection, key: &str, fallback: T) -> Result<T> {
    Ok(get(conn, key)?.unwrap_or(fallback))
}

pub fn set_in<T: Serialize>(conn: &Connection, scope: &str, key: &str, value: &T) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (scope, key, value) VALUES (?1,?2,?3)
         ON CONFLICT (scope, key) DO UPDATE SET value = excluded.value",
        params![scope, key, serde_json::to_string(value)?],
    )?;
    Ok(())
}

pub fn set<T: Serialize>(conn: &Connection, key: &str, value: &T) -> Result<()> {
    set_in(conn, GLOBAL, key, value)
}

/// Clear an override so the scope inherits again.
pub fn clear(conn: &Connection, scope: &str, key: &str) -> Result<bool> {
    Ok(conn.execute(
        "DELETE FROM settings WHERE scope = ?1 AND key = ?2",
        params![scope, key],
    )? > 0)
}

/// Every setting in a scope, for the settings screen and for export.
pub fn all_in(conn: &Connection, scope: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings WHERE scope = ?1 ORDER BY key")?;
    let rows = stmt.query_map([scope], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_through_json() {
        let conn = crate::open_memory().unwrap();
        set(&conn, "dvr.quotaBytes", &500_000_000i64).unwrap();
        set(&conn, "dvr.folder", &"D:\\Recordings".to_string()).unwrap();
        assert_eq!(
            get::<i64>(&conn, "dvr.quotaBytes").unwrap(),
            Some(500_000_000)
        );
        assert_eq!(
            get::<String>(&conn, "dvr.folder").unwrap().as_deref(),
            Some("D:\\Recordings")
        );
    }

    #[test]
    fn writing_the_same_key_replaces_it() {
        let conn = crate::open_memory().unwrap();
        set(&conn, "k", &1i64).unwrap();
        set(&conn, "k", &2i64).unwrap();
        assert_eq!(get::<i64>(&conn, "k").unwrap(), Some(2));
        assert_eq!(all_in(&conn, GLOBAL).unwrap().len(), 1);
    }

    #[test]
    fn an_unset_key_is_none_not_an_error() {
        let conn = crate::open_memory().unwrap();
        assert_eq!(get::<i64>(&conn, "missing").unwrap(), None);
        assert_eq!(get_or(&conn, "missing", 42i64).unwrap(), 42);
    }

    #[test]
    fn a_profile_inherits_the_global_value_until_it_overrides_it() {
        let conn = crate::open_memory().unwrap();
        set(&conn, "player.subtitles", &"en".to_string()).unwrap();
        assert_eq!(
            get_for::<String>(&conn, 1, "player.subtitles")
                .unwrap()
                .as_deref(),
            Some("en")
        );

        set_in(
            &conn,
            &profile_scope(1),
            "player.subtitles",
            &"de".to_string(),
        )
        .unwrap();
        assert_eq!(
            get_for::<String>(&conn, 1, "player.subtitles")
                .unwrap()
                .as_deref(),
            Some("de")
        );
        // The global value is untouched, and another profile still sees it.
        assert_eq!(
            get_for::<String>(&conn, 2, "player.subtitles")
                .unwrap()
                .as_deref(),
            Some("en")
        );

        assert!(clear(&conn, &profile_scope(1), "player.subtitles").unwrap());
        assert_eq!(
            get_for::<String>(&conn, 1, "player.subtitles")
                .unwrap()
                .as_deref(),
            Some("en")
        );
    }

    #[test]
    fn a_value_an_older_build_wrote_differently_reads_as_unset() {
        let conn = crate::open_memory().unwrap();
        // Was a string, is now a number: better to fall back to the default than to
        // fail every read from then on.
        set(&conn, "dvr.quotaBytes", &"lots".to_string()).unwrap();
        assert_eq!(get::<i64>(&conn, "dvr.quotaBytes").unwrap(), None);
        assert_eq!(get_or(&conn, "dvr.quotaBytes", 10i64).unwrap(), 10);
    }
}
