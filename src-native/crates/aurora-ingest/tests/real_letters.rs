//! The A-Z letter bar, run against a library a real import produced.
//!
//! The letter bar had a fixture test the whole time and it passed: seven invented titles,
//! none of them carrying what every real name on this subscription carries -- a language
//! code in front. 27,416 of its 27,494 film names begin `EN *` or similar, so the bar
//! sent nearly the whole library to "E" and eight letters returned nothing at all
//! (R-52).
//!
//! `#[ignore]` because it needs a real library on disk. After a host run:
//!
//! ```text
//! cargo test -p aurora-ingest --test real_letters -- --ignored --nocapture
//! ```
//!
//! Point `AURORA_REAL_LIBRARY` at a different `library.db` to use another one.

use aurora_db::repo::library::{self, BrowseQuery, MovieSort};
use aurora_db::rusqlite;

const DEFAULT_LIBRARY: &str = r"C:\aurora-dev\target\release\data\library.db";

/// A writable copy, because `list_movies` is a read but `open` runs migrations.
///
/// Copied rather than opened in place: the file belongs to a test harness run and
/// nothing here should be the reason it changes. See the note on never pointing a test
/// at anything the real app uses.
fn library() -> rusqlite::Connection {
    let src = std::env::var("AURORA_REAL_LIBRARY").unwrap_or_else(|_| DEFAULT_LIBRARY.into());
    assert!(
        std::path::Path::new(&src).exists(),
        "no library at {src}. Run a host scenario first, or set AURORA_REAL_LIBRARY."
    );
    let copy = std::env::temp_dir().join("aurora-real-letters.db");
    let _ = std::fs::remove_file(&copy);
    std::fs::copy(&src, &copy).expect("the library should be copyable");
    aurora_db::open(&copy).expect("the copy should open")
}

fn by_letter(conn: &rusqlite::Connection, letter: &str) -> usize {
    library::list_movies(
        conn,
        &BrowseQuery {
            letter: Some(letter.to_string()),
            sort: MovieSort::Title,
            limit: 100_000,
            ..Default::default()
        },
    )
    .expect("the letter bar should answer")
    .len()
}

/// Every letter button returns the films whose displayed name starts with it.
///
/// Before: "E" returned 26,796 of 27,494 films because almost every raw name begins
/// `EN *`, "T" returned 13 of the 5,372 that begin with T, and I, J, K, O, V, X, Y and Z
/// returned nothing at all. Every film was reachable by some button, under a letter with
/// nothing to do with its name -- which is worse than being missing, because the bar
/// looks like it works.
#[test]
#[ignore = "real library: run with --ignored"]
fn every_letter_returns_the_films_that_start_with_it() {
    let conn = library();

    // What the viewer reads, straight from the rows the browse query returns.
    let all = library::list_movies(
        &conn,
        &BrowseQuery {
            sort: MovieSort::Title,
            limit: 100_000,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        all.len() > 1_000,
        "only {} films -- is this a real library?",
        all.len()
    );

    let mut expected: std::collections::HashMap<char, usize> = std::collections::HashMap::new();
    for m in &all {
        let first = m
            .title
            .chars()
            .next()
            .map(|c| c.to_ascii_uppercase())
            .unwrap_or('#');
        let bucket = if first.is_ascii_uppercase() {
            first
        } else {
            '#'
        };
        *expected.entry(bucket).or_default() += 1;
    }

    let mut wrong = Vec::new();
    let mut reported = 0;
    for letter in 'A'..='Z' {
        let got = by_letter(&conn, &letter.to_string());
        let want = expected.get(&letter).copied().unwrap_or(0);
        reported += got;
        if got != want {
            wrong.push(format!("{letter}: button {got}, displayed names {want}"));
        }
    }
    let hash_got = by_letter(&conn, "#");
    let hash_want = expected.get(&'#').copied().unwrap_or(0);
    reported += hash_got;
    if hash_got != hash_want {
        wrong.push(format!("#: button {hash_got}, displayed names {hash_want}"));
    }

    println!(
        "   {} films, {reported} returned across the whole bar",
        all.len()
    );
    for line in wrong.iter().take(12) {
        println!("      {line}");
    }
    assert!(
        wrong.is_empty(),
        "{} letter buttons disagree with the names on screen",
        wrong.len()
    );
    // And every film is under exactly one button.
    assert_eq!(
        reported,
        all.len(),
        "the buttons returned {reported} films for a library of {}",
        all.len()
    );
}
