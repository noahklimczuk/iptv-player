//! SQLite persistence for Aurora TV.
//!
//! README §5. WAL mode, forward-only versioned migrations, FTS5 search, and batched writes —
//! the import path has to absorb 40,000 playlist entries and millions of EPG programmes
//! inside the §16 budget.

pub mod error;
pub mod migrate;
pub mod repo;
pub mod schema;

pub use error::{DbError, Result};
/// Re-exported so downstream crates share this crate's exact rusqlite version
/// rather than linking a second, incompatible one.
pub use rusqlite;

use std::path::Path;

use rusqlite::Connection;

/// Open (or create) the library database with the pragmas the workload needs.
pub fn open(path: impl AsRef<Path>) -> Result<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate::run(&conn)?;
    // A library that predates `analyze` being called at all — every library that
    // already exists — gets its statistics here, once. See `analyze`.
    if !has_statistics(&conn)? {
        analyze(&conn)?;
    }
    Ok(conn)
}

/// In-memory database, for tests.
pub fn open_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate::run(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    // journal_mode returns a row, so it needs query_row rather than execute.
    let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    conn.execute_batch(
        "PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA cache_size = -65536;", // 64 MiB page cache
    )?;
    Ok(())
}

/// Give the query planner the statistics it needs to choose between indexes.
///
/// **Without this the library is unusable at real scale, and nothing about the schema
/// or the queries looks wrong.** SQLite has no `sqlite_stat1` until `ANALYZE` has been
/// run, and with no statistics it guesses index selectivity from a fixed table of
/// assumptions. Those guesses were catastrophically wrong here.
///
/// The browse queries hide duplicates with
/// `NOT EXISTS (SELECT 1 FROM movies dup WHERE dup.match_key = movies.match_key AND …
/// (dup.quality_rank > … OR (dup.quality_rank = … AND dup.id < …)))`, and there is an
/// index built precisely for it: `idx_movies_dupe(match_key, quality_rank DESC, id)`.
/// Un-analysed, SQLite resolved the tie-break branch through `idx_movies_lang`
/// instead — `lang_code = 'en' AND rowid < ?`, which on an English-only filter means
/// scanning most of the English half of the table *once per candidate row*.
///
/// Measured on a real 117,510-film subscription:
///
/// | first page of Movies                    | time    |
/// |-----------------------------------------|---------|
/// | without duplicate hiding                |  0.01 s |
/// | with duplicate hiding, un-analysed      | 85.01 s |
/// | with duplicate hiding, after `ANALYZE`  |  0.04 s |
///
/// The heading's count query is the same shape and cost, so opening Movies was ~170
/// seconds of CPU for a page that renders in 40 ms. What a viewer saw was a blank grid
/// for over a minute, with no spinner, no error, and nothing in the log — because
/// nothing had failed.
///
/// `ANALYZE` itself takes about 0.1 s on that library. It is not a tuning knob; it is
/// the difference between the indexes being used and being ignored.
pub fn analyze(conn: &Connection) -> Result<()> {
    conn.execute_batch("ANALYZE")?;
    Ok(())
}

/// Whether `ANALYZE` has ever run on this database.
fn has_statistics(conn: &Connection) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'sqlite_stat1'",
        [],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// README §5: "automatic integrity check + repair on startup".
pub fn integrity_check(conn: &Connection) -> Result<bool> {
    let result: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    Ok(result == "ok")
}

/// What [`open_or_recover`] had to do to hand back a working database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    /// The file on disk was fine.
    Intact,
    /// It was not, so it was moved aside and a new one started. The library re-imports
    /// from the provider; the old file is kept in case anyone wants to look at it.
    Replaced { corrupt_copy: std::path::PathBuf },
}

/// Open the library, or start a new one if the file on disk cannot be used.
///
/// `open` alone propagates a corrupt file out of `Services::new`, which Tauri turns
/// into a start-up failure: no window, no dialog, and no console in a release build to
/// print to. The app simply did not launch and said nothing about why. A failed
/// `integrity_check` was worse — logged at error level and then used anyway.
///
/// The old file is renamed rather than deleted. It holds favourites, watch progress and
/// recordings metadata that somebody might want recovered, and deleting the one copy of
/// that to make an error message go away is not a trade this should make on its own.
pub fn open_or_recover(path: impl AsRef<Path>) -> Result<(Connection, Opened)> {
    let path = path.as_ref();

    let failure = match open(path) {
        Ok(conn) => match integrity_check(&conn) {
            Ok(true) => return Ok((conn, Opened::Intact)),
            Ok(false) => "failed its integrity check".to_string(),
            Err(e) => format!("could not be checked: {e}"),
        },
        Err(e) => e.to_string(),
    };

    // A database this build is too old for is a different problem, and replacing it
    // would silently throw away a newer library. Refuse instead, so the message says
    // to update rather than pretending nothing was there.
    if failure.contains("newer version of Aurora") || failure.contains("schema") {
        tracing::error!("the library database {failure}");
        return open(path).map(|c| (c, Opened::Intact));
    }

    tracing::error!("the library database {failure}; starting a new one");
    let corrupt_copy = aside(path);
    // Rename what is there, WAL sidecars included: leaving `-wal` and `-shm` behind
    // would have SQLite try to replay them into the fresh file.
    for suffix in ["", "-wal", "-shm"] {
        let from = with_suffix(path, suffix);
        if from.exists() {
            let _ = std::fs::rename(&from, with_suffix(&corrupt_copy, suffix));
        }
    }

    let conn = open(path)?;
    Ok((conn, Opened::Replaced { corrupt_copy }))
}

/// `library.db` → `library.corrupt-1760000000.db`, and never over something that is
/// already there.
fn aside(path: &Path) -> std::path::PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "library".to_string());
    let extension = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let dir = path.parent().unwrap_or(Path::new("."));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    for n in 0..1000 {
        let name = if n == 0 {
            format!("{stem}.corrupt-{stamp}{extension}")
        } else {
            format!("{stem}.corrupt-{stamp}-{n}{extension}")
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}.corrupt{extension}"))
}

fn with_suffix(path: &Path, suffix: &str) -> std::path::PathBuf {
    if suffix.is_empty() {
        return path.to_path_buf();
    }
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    std::path::PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aurora-db-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_healthy_database_is_opened_as_it_is() {
        let dir = scratch("intact");
        let path = dir.join("library.db");
        {
            let conn = open(&path).unwrap();
            conn.execute(
                "INSERT INTO providers (name, kind, base_url, created_at)
                 VALUES ('P','m3u','https://example.com',0)",
                [],
            )
            .unwrap();
        }

        let (conn, how) = open_or_recover(&path).unwrap();
        assert_eq!(how, Opened::Intact);
        let n: i64 = conn
            .query_row("SELECT count(*) FROM providers", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "an intact library must not be replaced");
    }

    /// The bug: a corrupt file propagated out of `Services::new`, Tauri turned that
    /// into a start-up failure, and a release build has no console to say so in. The
    /// app did not launch and gave no reason.
    #[test]
    fn a_corrupt_database_is_moved_aside_and_replaced() {
        let dir = scratch("corrupt");
        let path = dir.join("library.db");
        std::fs::write(&path, b"this is not a database at all, it is just bytes").unwrap();

        let (conn, how) = open_or_recover(&path).unwrap();
        let Opened::Replaced { corrupt_copy } = how else {
            panic!("a corrupt file must be replaced, not used");
        };

        // The new one works…
        assert_eq!(
            migrate::current_version(&conn).unwrap(),
            schema::LATEST_VERSION
        );
        assert!(integrity_check(&conn).unwrap());

        // …and the old one is kept rather than deleted.
        assert!(corrupt_copy.exists(), "{corrupt_copy:?}");
        assert_eq!(
            std::fs::read(&corrupt_copy).unwrap(),
            b"this is not a database at all, it is just bytes"
        );
    }

    #[test]
    fn a_truncated_header_is_recovered_too() {
        let dir = scratch("truncated");
        let path = dir.join("library.db");
        {
            let _ = open(&path).unwrap();
        }
        // Scribble over the page that holds the schema.
        let mut bytes = std::fs::read(&path).unwrap();
        for b in bytes.iter_mut().take(200) {
            *b = 0x41;
        }
        std::fs::write(&path, &bytes).unwrap();

        let (conn, how) = open_or_recover(&path).unwrap();
        assert!(matches!(how, Opened::Replaced { .. }));
        assert!(integrity_check(&conn).unwrap());
    }

    /// A stale `-wal` beside a replaced database is not a leftover, it is a hazard:
    /// SQLite would try to replay it into the fresh file.
    #[test]
    fn no_stale_sidecar_is_left_beside_the_new_database() {
        let dir = scratch("wal");
        let path = dir.join("library.db");
        std::fs::write(&path, b"not a database").unwrap();
        std::fs::write(dir.join("library.db-wal"), b"stale wal").unwrap();
        std::fs::write(dir.join("library.db-shm"), b"stale shm").unwrap();

        let (conn, how) = open_or_recover(&path).unwrap();
        assert!(matches!(how, Opened::Replaced { .. }));
        assert!(integrity_check(&conn).unwrap());
        drop(conn);

        for suffix in ["-wal", "-shm"] {
            let beside = with_suffix(&path, suffix);
            if beside.exists() {
                let bytes = std::fs::read(&beside).unwrap();
                assert!(
                    bytes != b"stale wal" && bytes != b"stale shm",
                    "the old {suffix} was left beside the new database"
                );
            }
        }
    }

    #[test]
    fn two_bad_launches_do_not_overwrite_the_first_copy() {
        let dir = scratch("twice");
        let path = dir.join("library.db");

        // Dropped between launches: the app opens one connection at startup, and a
        // live one holds a WAL the next open would recover the database from.
        std::fs::write(&path, b"first failure").unwrap();
        let (c, first) = open_or_recover(&path).unwrap();
        drop(c);
        for suffix in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(with_suffix(&path, suffix));
        }

        std::fs::write(&path, b"second failure").unwrap();
        let (c, second) = open_or_recover(&path).unwrap();
        drop(c);

        let (Opened::Replaced { corrupt_copy: a }, Opened::Replaced { corrupt_copy: b }) =
            (first, second)
        else {
            panic!("expected two replacements");
        };
        assert_ne!(a, b);
        assert_eq!(std::fs::read(&a).unwrap(), b"first failure");
        assert_eq!(std::fs::read(&b).unwrap(), b"second failure");
    }
}
