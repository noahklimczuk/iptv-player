//! The configured providers, as the settings screen and the first-run check see them.
//!
//! Credentials are never here. The table stores a `credential_ref`, and the secret
//! itself lives in the Windows Credential Manager (README C10) — a row from this
//! module is safe to log, export or put in a screenshot.

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRow {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub max_connections: Option<i64>,
    /// Not tracked yet; the contract carries it so the UI can show it once it is.
    pub active_connections: Option<i64>,
    pub expires_at: Option<i64>,
    pub last_refresh_at: Option<i64>,
    pub channel_count: i64,
    pub movie_count: i64,
    pub series_count: i64,
}

/// Every provider, with what each one contributed to the library.
///
/// The counts are correlated subqueries rather than joins: a provider with no channels
/// must still appear — it is exactly the provider someone is in Settings to look at —
/// and an inner join would drop it.
pub fn list(conn: &Connection) -> Result<Vec<ProviderRow>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, p.kind, p.enabled, p.max_connections, p.expires_at,
                p.last_refresh_at,
                (SELECT COUNT(*) FROM channels c WHERE c.provider_id = p.id),
                (SELECT COUNT(*) FROM movies m   WHERE m.provider_id = p.id),
                (SELECT COUNT(*) FROM series s   WHERE s.provider_id = p.id)
         FROM providers p
         ORDER BY p.sort_order, p.id",
    )?;

    let rows = stmt.query_map([], |r| {
        Ok(ProviderRow {
            id: r.get(0)?,
            name: r.get(1)?,
            kind: r.get(2)?,
            enabled: r.get::<_, i64>(3)? != 0,
            max_connections: r.get(4)?,
            active_connections: None,
            expires_at: r.get(5)?,
            last_refresh_at: r.get(6)?,
            channel_count: r.get(7)?,
            movie_count: r.get(8)?,
            series_count: r.get(9)?,
        })
    })?;

    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Whether anything is configured at all. What "is this a first run?" reduces to.
pub fn any(conn: &Connection) -> Result<bool> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM providers", [], |r| r.get(0))?;
    Ok(count > 0)
}

pub fn set_enabled(conn: &Connection, provider_id: i64, enabled: bool) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE providers SET enabled = ?2 WHERE id = ?1",
        params![provider_id, enabled as i64],
    )? > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert(conn: &Connection, id: i64, name: &str) {
        conn.execute(
            "INSERT INTO providers (id, name, kind, base_url, enabled, sort_order, created_at)
             VALUES (?1, ?2, 'xtream', 'http://example.com', 1, 0, 0)",
            params![id, name],
        )
        .expect("insert provider");
    }

    #[test]
    fn an_empty_library_has_no_providers() {
        let conn = crate::open_memory().unwrap();
        assert!(list(&conn).unwrap().is_empty());
        assert!(!any(&conn).unwrap());
    }

    #[test]
    fn a_provider_with_nothing_imported_still_appears() {
        // The regression an inner join would introduce, and the one case that matters:
        // a provider whose import failed is precisely what someone opens Settings for.
        let conn = crate::open_memory().unwrap();
        insert(&conn, 1, "Mine");

        let rows = list(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Mine");
        assert_eq!(rows[0].channel_count, 0);
        assert_eq!(rows[0].movie_count, 0);
        assert!(any(&conn).unwrap());
    }

    #[test]
    fn counts_are_per_provider() {
        let conn = crate::open_memory().unwrap();
        insert(&conn, 1, "One");
        insert(&conn, 2, "Two");
        for (provider, key) in [(1, "a"), (1, "b"), (2, "c")] {
            conn.execute(
                "INSERT INTO channels (provider_id, provider_key, name, match_key, last_seen_at)
                 VALUES (?1, ?2, 'Ch', ?2, 0)",
                params![provider, key],
            )
            .unwrap();
        }

        let rows = list(&conn).unwrap();
        assert_eq!(rows[0].channel_count, 2);
        assert_eq!(rows[1].channel_count, 1);
    }

    #[test]
    fn a_disabled_provider_is_still_listed() {
        let conn = crate::open_memory().unwrap();
        insert(&conn, 1, "Mine");
        assert!(set_enabled(&conn, 1, false).unwrap());

        let rows = list(&conn).unwrap();
        assert_eq!(rows.len(), 1, "disabling is not deleting");
        assert!(!rows[0].enabled);
    }
}
