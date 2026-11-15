//! The read-only connection the screens use (`aurora_db::open_reader`).
//!
//! What it is for: a catalogue refresh held the one shared connection for 7.4 s on a
//! real library, and every page waited behind it. A reader on its own connection reads
//! the last committed state while a write is in progress, and must never write.

use aurora_db::{open, open_reader};

#[test]
fn a_reader_is_not_held_up_by_a_write_in_progress_and_sees_it_once_committed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let writer = open(&path).unwrap();
    let reader = open_reader(&path).unwrap();
    let count = |c: &aurora_db::rusqlite::Connection| -> i64 {
        c.query_row("SELECT count(*) FROM profiles", [], |r| r.get(0))
            .unwrap()
    };
    let before = count(&reader);

    // A write transaction left open, as a refresh's is for seconds.
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    writer
        .execute(
            "INSERT INTO profiles (name, created_at) VALUES ('probe', 0)",
            [],
        )
        .unwrap();
    let started = std::time::Instant::now();
    assert_eq!(
        count(&reader),
        before,
        "a reader sees the last committed state"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_millis(500),
        "the read waited for the writer"
    );

    writer.execute_batch("COMMIT").unwrap();
    assert_eq!(
        count(&reader),
        before + 1,
        "and the new state once it is committed"
    );
}

#[test]
fn a_reader_refuses_to_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    drop(open(&path).unwrap());
    let reader = open_reader(&path).unwrap();
    let err = reader
        .execute(
            "INSERT INTO profiles (name, created_at) VALUES ('x', 0)",
            [],
        )
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("readonly") || err.contains("read-only"),
        "{err}"
    );
}

#[test]
fn a_reader_has_the_functions_the_queries_use() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    drop(open(&path).unwrap());
    let reader = open_reader(&path).unwrap();
    let shown: String = reader
        .query_row("SELECT shown_title('EN - Heat')", [], |r| r.get(0))
        .unwrap();
    assert_eq!(shown, "Heat");
}
