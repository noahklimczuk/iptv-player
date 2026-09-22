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

use std::path::Path;

use rusqlite::Connection;

/// Open (or create) the library database with the pragmas the workload needs.
pub fn open(path: impl AsRef<Path>) -> Result<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate::run(&conn)?;
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

/// README §5: "automatic integrity check + repair on startup".
pub fn integrity_check(conn: &Connection) -> Result<bool> {
    let result: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    Ok(result == "ok")
}
