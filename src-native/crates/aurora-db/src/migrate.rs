//! Forward-only migration runner.

use rusqlite::Connection;

use crate::error::{DbError, Result};
use crate::schema::{LATEST_VERSION, MIGRATIONS};

pub fn current_version(conn: &Connection) -> Result<u32> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as u32)
}

/// Apply every migration newer than the database's recorded version.
/// Each migration runs in its own transaction, so a failure leaves the DB at the last
/// version that fully applied rather than half-migrated.
pub fn run(conn: &Connection) -> Result<()> {
    let from = current_version(conn)?;

    if from > LATEST_VERSION {
        return Err(DbError::SchemaTooNew {
            found: from,
            supported: LATEST_VERSION,
        });
    }

    for m in MIGRATIONS.iter().filter(|m| m.version > from) {
        tracing::info!(version = m.version, name = m.name, "applying migration");
        conn.execute_batch("BEGIN")?;
        match conn.execute_batch(m.sql) {
            Ok(()) => {
                conn.execute_batch(&format!("PRAGMA user_version = {}", m.version))?;
                conn.execute_batch("COMMIT")?;
            }
            Err(source) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(DbError::Migration {
                    version: m.version,
                    source,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_a_fresh_database_to_latest() {
        let conn = crate::open_memory().unwrap();
        assert_eq!(current_version(&conn).unwrap(), LATEST_VERSION);
    }

    #[test]
    fn is_idempotent() {
        let conn = crate::open_memory().unwrap();
        run(&conn).unwrap();
        run(&conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), LATEST_VERSION);
    }

    #[test]
    fn migration_versions_are_unique_and_ordered() {
        let mut last = 0;
        for m in MIGRATIONS {
            assert!(m.version > last, "migration {} is out of order", m.version);
            last = m.version;
        }
        assert_eq!(last, LATEST_VERSION, "LATEST_VERSION is stale");
    }

    #[test]
    fn refuses_a_database_from_a_newer_build() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA user_version = 9999").unwrap();
        let err = run(&conn).unwrap_err();
        assert!(matches!(err, DbError::SchemaTooNew { .. }));
    }

    #[test]
    fn integrity_check_passes_on_a_fresh_db() {
        let conn = crate::open_memory().unwrap();
        assert!(crate::integrity_check(&conn).unwrap());
    }

    #[test]
    fn all_expected_tables_exist() {
        let conn = crate::open_memory().unwrap();
        for table in [
            "providers", "channels", "channel_sources", "epg_channels", "epg_programmes",
            "movies", "series", "episodes", "profiles", "watch_progress", "favorites",
            "my_list", "settings", "rules", "search_index", "epg_manual_map",
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name = ?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "missing table {table}");
        }
    }
}
