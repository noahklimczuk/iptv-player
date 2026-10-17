//! The thirty films whose year survived the cleaner, measured in the real library.
//!
//! `clean_movie_title` runs at ingest and its output is what is stored, with the year
//! lifted into its own column — so the provider's `EN ★ Inception - 2010` becomes
//! `EN Inception` plus `year = 2010`, and the matcher asks TMDB for "Inception". That
//! works for 27,464 of this subscription's films.
//!
//! Thirty share one shape it could not read: `- 2025[4K]`, with no space before the
//! bracket. The year regex's trailing set had no opening bracket in it, so no year was
//! found; `re_bracketed` then took `[4K]` away and left the year sitting in the title,
//! where it went into the stored name, the search and the match key.
//! `theacademy2025` scores 0.000 against "The Academy", so each of those films was
//! marked `nomatch` after a single attempt (R-59).
//!
//! ```text
//! cargo test -p aurora-ingest --test real_match_keys -- --ignored --nocapture
//! ```
//!
//! Reads a library a host run produced; writes nothing, and asks the panel nothing.

use aurora_core::{lang, title};
use aurora_db::rusqlite;

const DEFAULT_LIBRARY: &str = r"C:\aurora-dev\target\release\data\library.db";

fn library() -> rusqlite::Connection {
    let path = std::env::var("AURORA_REAL_LIBRARY").unwrap_or_else(|_| DEFAULT_LIBRARY.into());
    assert!(
        std::path::Path::new(&path).exists(),
        "no library at {path}. Run a host scenario first, or set AURORA_REAL_LIBRARY."
    );
    rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .expect("the library should open read-only")
}

/// Does this string carry a bare four-digit year anywhere in it?
fn has_year(s: &str) -> bool {
    let b = s.as_bytes();
    b.windows(4).any(|w| {
        w.iter().all(|c| c.is_ascii_digit())
            && (w[0] == b'1' && w[1] == b'9' || w[0] == b'2' && w[1] == b'0')
    })
}

/// Does it *end* with one, which is the shape a release year has?
///
/// The distinction matters, because a year inside a title is often the title. Three
/// films on this subscription prove it: `2010: Moby Dick` starts with one and
/// `トカレフ2010 運命の撃鉄` has one welded into a word, and the cleaner is right to
/// leave both alone — it only lifts a year that is preceded by a separator and not at
/// position 0. `The Comet Kids 2017The Beloved Rogue` is two names the provider glued
/// together, where taking the year out would produce a third wrong name rather than a
/// right one.
///
/// What R-59 was about is the year on the end: `The Academy 2025`, which is a release
/// year and is not part of what TMDB calls the film.
fn ends_with_year(s: &str) -> bool {
    let tail = s.trim_end_matches(|c: char| !c.is_alphanumeric());
    let last = tail.rsplit(|c: char| !c.is_alphanumeric()).next().unwrap_or("");
    last.len() == 4 && has_year(last)
}

#[test]
#[ignore = "real library: run with --ignored --nocapture"]
fn no_film_reaches_tmdb_with_the_year_still_in_its_name() {
    let conn = library();
    let mut stmt = conn
        .prepare("SELECT title, match_key, year FROM movies")
        .expect("movies");
    let films: Vec<(String, String, Option<i32>)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .expect("rows")
        .collect::<rusqlite::Result<_>>()
        .expect("rows");
    assert!(films.len() > 1000, "only {} films to measure", films.len());

    // What is stored today. These rows were written by the build that had the defect,
    // so this is the population R-59 counted and not an assertion about the fix.
    let stored_with_year: Vec<&(String, String, Option<i32>)> =
        films.iter().filter(|(_, key, _)| has_year(key)).collect();
    println!(
        "films: {}, match_key still holding a year: {}",
        films.len(),
        stored_with_year.len()
    );
    for (t, k, y) in stored_with_year.iter().take(5) {
        println!("   {t:?}  key={k:?}  year={y:?}");
    }

    // What the matcher now asks for, which is the fix: the same two steps `fetch_one`
    // takes -- strip the language tag, then run the cleaner over what is left.
    let mut trailing: Vec<(String, String)> = Vec::new();
    let mut embedded: Vec<(String, String)> = Vec::new();
    for (stored, _, _) in &films {
        let searchable = lang::strip_language_prefix(stored);
        let searchable = searchable.as_deref().unwrap_or(stored);
        let cleaned = title::clean_movie_title(searchable);
        if ends_with_year(&cleaned.title) {
            trailing.push((stored.clone(), cleaned.title));
        } else if has_year(&cleaned.title) {
            embedded.push((stored.clone(), cleaned.title));
        }
    }
    println!("searched with a release year on the end: {}", trailing.len());
    for (stored, asked) in trailing.iter().take(10) {
        println!("   {stored:?} -> {asked:?}");
    }
    println!(
        "searched with a year inside the name, which is usually the name: {}",
        embedded.len()
    );
    for (stored, asked) in embedded.iter().take(10) {
        println!("   {stored:?} -> {asked:?}");
    }

    assert!(
        trailing.is_empty(),
        "{} films would still be searched with the release year on the end, e.g. {:?}",
        trailing.len(),
        trailing.first()
    );

    // And the thirty are the ones this fixes, so the population has to be non-empty or
    // the test proves nothing about them.
    assert!(
        !stored_with_year.is_empty(),
        "no stored film carries a year any more, so this library was imported after the \
         fix and the assertion above is vacuous -- re-check against an older library, \
         or delete this test"
    );

    // The year is not lost, either: it is what the search passes to TMDB.
    for (stored, _, year) in stored_with_year.iter().take(30) {
        let searchable = lang::strip_language_prefix(stored);
        let searchable = searchable.as_deref().unwrap_or(stored);
        let cleaned = title::clean_movie_title(searchable);
        let asked = year.or(cleaned.year);
        assert!(
            asked.is_some(),
            "{stored:?} would be searched with no year at all, having had one in its name"
        );
    }
}
