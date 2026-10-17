//! Does the real library, on disk, contain the password it was imported with?
//!
//! The strongest form this check can take. Every other test here builds its own rows and
//! then asserts about them; this one opens the file a real import produced, with the real
//! credentials, and looks for them — which is what the finding was: Xtream stream URLs
//! carry the credentials in their path and were stored verbatim, hundreds of thousands of
//! times, while `docs/PRIVACY.md` promised the file "contains no credentials" (R-126).
//!
//! `#[ignore]` because it needs both a real library on disk and the credentials. After a
//! host run:
//!
//! ```text
//! set -a; . /c/aurora-dev/panel.env; set +a
//! cargo test -p aurora-ingest --test real_library_secrets -- --ignored --nocapture
//! ```
//!
//! Point `AURORA_REAL_LIBRARY` at a different `library.db` to use another one. Read-only,
//! always: this opens somebody's library and has no business writing to it.

use aurora_db::rusqlite;

const DEFAULT_LIBRARY: &str = r"C:\aurora-dev\target\release\data\library.db";

fn library() -> rusqlite::Connection {
    let path = std::env::var("AURORA_REAL_LIBRARY").unwrap_or_else(|_| DEFAULT_LIBRARY.into());
    assert!(
        std::path::Path::new(&path).exists(),
        "no library at {path}. Run `python tests-host/run.py real_panel` first, or set \
         AURORA_REAL_LIBRARY."
    );
    rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .expect("the library should open read-only")
}

fn secret(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| {
        panic!(
            "{key} is not set. Source the panel credentials first:\n  \
             set -a; . /c/aurora-dev/panel.env; set +a"
        )
    })
}

/// Not one row in the three URL columns mentions the password.
#[test]
#[ignore = "needs a real library and the real credentials"]
fn the_library_on_disk_holds_no_password() {
    let db = library();
    let password = secret("AURORA_PANEL_PASS");
    assert!(
        password.len() > 3,
        "the password is too short to search for"
    );

    let mut found = Vec::new();
    for (table, column) in [
        ("channel_sources", "url"),
        ("movies", "url"),
        ("episodes", "url"),
    ] {
        let n: i64 = db
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE instr({column}, ?1) > 0"),
                [&password],
                |r| r.get(0),
            )
            .unwrap_or(0);
        println!("{table}.{column}: {n} rows containing the password");
        if n > 0 {
            found.push(format!("{table}.{column} ({n} rows)"));
        }
    }

    assert!(
        found.is_empty(),
        "the password is still in the library: {}",
        found.join(", ")
    );
}

/// And the rows that lost their URL kept enough to rebuild one.
///
/// Without this the test above would pass on an empty library, or on one where the
/// backfill had emptied `url` and stored nothing — which would be a different and worse
/// bug than the one it is checking for.
#[test]
#[ignore = "needs a real library"]
fn every_row_can_still_be_played() {
    let db = library();

    let mut checked = 0i64;
    for (table, join) in [
        (
            "channel_sources",
            "JOIN channels c ON c.id = channel_sources.channel_id
             JOIN providers p ON p.id = c.provider_id",
        ),
        ("movies", "JOIN providers p ON p.id = movies.provider_id"),
        (
            "episodes",
            "JOIN series r ON r.id = episodes.series_id
             JOIN providers p ON p.id = r.provider_id",
        ),
    ] {
        let total: i64 = db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap_or(0);
        // A row is playable if it has an address or a reference to rebuild one from.
        let stranded: i64 = db
            .query_row(
                &format!(
                    "SELECT count(*) FROM {table} {join}
                      WHERE (url IS NULL OR url = '')
                        AND (stream_id IS NULL OR stream_id = '')"
                ),
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        println!("{table}: {total} rows, {stranded} with neither an address nor an id");
        assert_eq!(stranded, 0, "{table} has rows that can no longer be played");
        checked += total;
    }

    assert!(checked > 0, "the library is empty, so this checked nothing");
}

/// What an Xtream row looks like now, printed rather than asserted.
///
/// A row of evidence for the PR, and a way to see at a glance that the id and the
/// container survived the rewrite.
#[test]
#[ignore = "needs a real library"]
fn show_what_is_stored() {
    let db = library();
    let mut stmt = db
        .prepare(
            "SELECT m.title, m.url, m.stream_id, m.container, p.kind
             FROM movies m JOIN providers p ON p.id = m.provider_id
             LIMIT 3",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .unwrap();
    for row in rows {
        let (title, url, id, container, kind) = row.unwrap();
        println!("{kind:7} {title:40.40} url={url:?} stream_id={id:?} container={container:?}");
    }
}

/// The password is not in the *file*, which is a different question from the columns.
///
/// Blanking a column does not remove the bytes from a SQLite database: the old row stays
/// in a freed page, and a freed page keeps its contents until something reuses it.
/// Measured on the real library after the first version of this fix: every one of the
/// three columns was clean and **34,702 raw occurrences of the password were still in the
/// file**. Since the entire point of the work is that `library.db` can be handed to
/// somebody, the column check alone would have been a false pass (R-126).
#[test]
#[ignore = "needs a real library and the real credentials"]
fn the_file_itself_holds_no_password() {
    use std::io::Read;

    let path = std::env::var("AURORA_REAL_LIBRARY").unwrap_or_else(|_| DEFAULT_LIBRARY.into());
    let password = secret("AURORA_PANEL_PASS").into_bytes();
    assert!(
        password.len() > 3,
        "the password is too short to search a 250 MB file for"
    );

    let mut file = std::fs::File::open(&path).expect("the library should open");
    let mut buffer = vec![0u8; 8 << 20];
    // The overlap, so a match that straddles two reads is still found.
    let mut carry: Vec<u8> = Vec::new();
    let mut found = 0usize;
    loop {
        let read = file.read(&mut buffer).expect("the library should read");
        if read == 0 {
            break;
        }
        let mut window = std::mem::take(&mut carry);
        window.extend_from_slice(&buffer[..read]);
        found += window
            .windows(password.len())
            .filter(|w| *w == password.as_slice())
            .count();
        let keep = window.len().saturating_sub(password.len() - 1);
        carry = window[keep..].to_vec();
    }

    assert_eq!(
        found, 0,
        "the password appears {found} times in the bytes of {path}. The columns may be \
         clean while the old rows survive in freed pages — `VACUUM` after the rewrite is \
         what removes them."
    );
}
