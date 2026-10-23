//! What a keystroke in the command palette costs, at the size a real library reaches.
//!
//! README §10 asks for search under 100 ms. Two structural costs are proportional to the
//! library rather than to the result set: `grouped` loads every visible id three times —
//! once each for channels, films and series — and the programme search is
//! `title LIKE '%term%'` over `epg_programmes`, which no index can serve and is therefore
//! a full scan of the guide.
//!
//! Measured on a real-sized library when the review was written, both were affordable:
//! 19.4 ms with no filters, 41.6 ms with duplicates hidden, 44.8 ms with "English only"
//! as well. What took it out of budget was [R-49] — every refresh appended another
//! complete copy of the guide, and the programme scan is linear in that:
//!
//! ```text
//! day 1:    546,992 programmes ->  19.9 ms per keystroke
//! day 3:  1,640,976            -> 152.9 ms   <- past the budget
//! day 5:  2,734,960            -> 145.4 ms, and 241.1 ms with the filters on
//! ```
//!
//! R-49 is fixed, so the guide no longer grows that way. This is the measurement that
//! said so, kept as a test rather than as a paragraph, because "search is inside its
//! budget" is a claim that can quietly stop being true — and because the thing that
//! broke it was not in this file (R-56).
//!
//! `#[ignore]` because it is a timing test and builds a 550,000-row guide to be one:
//!
//! ```text
//! cargo test -p aurora-db --test search_budget -- --ignored --nocapture
//! ```

use std::time::Instant;

use aurora_db::repo::{filtering, search};
use aurora_db::rusqlite::{params, Connection};

/// `(kind, ref_id, title, subtitle)`, as `search::index` takes them.
type IndexRow = (String, i64, String, Option<String>);

/// The subscription this project is measured against, rounded to the figures in the
/// review: 6,698 channels, 27,494 films, 8,086 shows, 546,992 programmes.
const CHANNELS: i64 = 6_698;
const MOVIES: i64 = 27_494;
const SERIES: i64 = 8_086;
/// Programmes per channel over seven days, which is what gives 546,992 in total.
const PER_CHANNEL: i64 = 82;

/// README §10. The budget is per keystroke, so the test does five and takes the worst.
///
/// Judged only in an optimized build. `rusqlite` bundles SQLite as C compiled at the
/// profile's own optimization level, so a `cargo test` build is measuring an unoptimized
/// database engine and reads several times slower for reasons that have nothing to do
/// with this query. A debug run still prints every figure and still fails on something
/// catastrophic, which is what `DEBUG_CEILING_MS` is for:
///
/// ```text
/// cargo test -p aurora-db --release --test search_budget -- --ignored --nocapture
/// ```
const BUDGET_MS: u128 = 100;

/// What a debug build is held to: loose enough that the unoptimized engine is not the
/// thing failing, tight enough that a return of R-49 — which took this from 20 ms to
/// 153 ms in release — still shows up here.
const DEBUG_CEILING_MS: u128 = 400;

const NOW: i64 = 1_700_000_000;

/// Typing one word, a character at a time, which is what the palette sees.
const KEYSTROKES: [&str; 5] = ["n", "ni", "nig", "nigh", "night"];

/// Enough words that a term matches a slice of the library rather than all of it.
///
/// This matters more than it looks. A fixture whose every title contains the search
/// term measures the one case a real library never presents: `grouped`'s programme
/// query is `ORDER BY p.start LIMIT 24`, so a term that matches half a million rows has
/// to sort half a million rows to return 24. An early version of this test did exactly
/// that and read 90-130 ms, which says nothing about search and everything about the
/// fixture.
const WORDS: [&str; 16] = [
    "Night", "Harbour", "Signal", "Winter", "Quiet", "Orbit", "Marsh", "Lantern", "Current",
    "Ledger", "Northern", "Anchor", "Copper", "Thistle", "Verge", "Hollow",
];

/// A word for row `i`, spread so that each one names about a sixteenth of the library.
fn word(i: i64, salt: i64) -> &'static str {
    WORDS[((i * 7 + salt * 3) as usize) % WORDS.len()]
}

fn library() -> Connection {
    let mut rows: Vec<IndexRow> = Vec::new();
    // On disk rather than in memory: the point is a query plan over a real-sized table,
    // and `:memory:` flatters every scan in it.
    let dir = std::env::temp_dir().join("aurora-search-budget");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let conn = aurora_db::open(dir.join("library.db")).expect("a fresh library");

    conn.execute(
        "INSERT INTO providers (id,name,kind,base_url,created_at)
         VALUES (1,'P','xtream','https://example.invalid',0)",
        [],
    )
    .unwrap();

    let tx = |conn: &Connection, sql: &str| conn.execute_batch(sql).unwrap();
    tx(&conn, "BEGIN");

    for i in 1..=CHANNELS {
        let name = format!("{} {} {i}", word(i, 0), word(i, 1));
        conn.execute(
            "INSERT INTO channels (id, provider_id, provider_key, name, match_key,
                                   number, epg_channel_id, lang_code, hidden,
                                   last_seen_at)
             VALUES (?1, 1, ?2, ?3, ?3, ?1, ?4, 'en', 0, 0)",
            params![i, format!("ch:{i}"), &name, format!("ch{i}.example")],
        )
        .unwrap();
        rows.push(("channel".into(), i, name, None));
    }

    for i in 1..=MOVIES {
        let title = format!("{} {} {i}", word(i, 2), word(i, 3));
        conn.execute(
            "INSERT INTO movies (id, provider_id, provider_key, title, match_key,
                                 url, lang_code, hidden, added_at, last_seen_at)
             VALUES (?1, 1, ?2, ?3, ?3, '', 'en', 0, 0, 0)",
            params![i, format!("mv:{i}"), &title],
        )
        .unwrap();
        rows.push(("movie".into(), i, title, None));
    }

    for i in 1..=SERIES {
        let title = format!("{} {} {i}", word(i, 4), word(i, 5));
        conn.execute(
            "INSERT INTO series (id, provider_id, provider_key, title, match_key,
                                 lang_code, hidden, added_at, last_seen_at)
             VALUES (?1, 1, ?2, ?3, ?3, 'en', 0, 0, 0)",
            params![i, format!("se:{i}"), &title],
        )
        .unwrap();
        rows.push(("series".into(), i, title, None));
    }

    // The guide: seven days of it, on the channels above, so the programme scan has the
    // same shape and the same joins as the real one.
    let mut stmt = conn
        .prepare(
            "INSERT INTO epg_programmes (channel_id, start, stop, title)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .unwrap();
    for ch in 1..=CHANNELS {
        let channel = format!("ch{ch}.example");
        for p in 0..PER_CHANNEL {
            let start = NOW + p * 2 * 3600;
            stmt.execute(params![
                channel,
                start,
                start + 2 * 3600,
                format!("{} {} {p}", word(ch, p), word(p, ch))
            ])
            .unwrap();
        }
    }
    drop(stmt);
    tx(&conn, "COMMIT");

    // The index in one transaction, which is how the importer builds it.
    let mut conn = conn;
    search::index(&mut conn, &rows).unwrap();

    // As the app does after an import, and as the measurement did: without it SQLite is
    // guessing at every one of these tables.
    conn.execute_batch("ANALYZE").unwrap();
    conn
}

/// The worst of five keystrokes, in milliseconds.
fn worst_keystroke(conn: &Connection) -> u128 {
    let mut worst = 0;
    for text in KEYSTROKES {
        let at = Instant::now();
        let out = search::grouped(conn, text, NOW, 24).unwrap();
        let ms = at.elapsed().as_millis();
        // Something has to come back, or this is timing an empty query.
        let hits = out.channels.len() + out.movies.len() + out.series.len();
        // Or this is timing a query that found nothing, which every plan is fast at.
        assert!(hits > 0, "{text:?} matched nothing in the library");
        println!(
            "  {text:>6?} -> {ms:>4} ms, {hits} hits, {} on now",
            out.on_now.len()
        );
        worst = worst.max(ms);
    }
    worst
}

#[test]
#[ignore = "timing: builds a 550,000-row guide. Run with --ignored --nocapture"]
fn a_keystroke_stays_inside_the_budget_at_the_size_a_real_library_reaches() {
    let conn = library();

    let programmes: i64 = conn
        .query_row("SELECT COUNT(*) FROM epg_programmes", [], |r| r.get(0))
        .unwrap();
    println!(
        "library: {CHANNELS} channels, {MOVIES} films, {SERIES} shows, {programmes} programmes"
    );

    println!("no library filters");
    let plain = worst_keystroke(&conn);

    // The filters are what make `visible_ids` expensive: `hide_duplicates` is a
    // correlated NOT EXISTS over the whole table, and `grouped` asks three times.
    let mut filter = filtering::LibraryFilter {
        hide_duplicates: true,
        ..Default::default()
    };
    filter.save(&conn).unwrap();
    println!("hide duplicates");
    let deduped = worst_keystroke(&conn);

    filter.english_only = true;
    filter.save(&conn).unwrap();
    println!("english only + hide duplicates");
    let filtered = worst_keystroke(&conn);

    let (ceiling, profile) = if cfg!(debug_assertions) {
        (DEBUG_CEILING_MS, "debug")
    } else {
        (BUDGET_MS, "release")
    };
    for (what, ms) in [
        ("no filters", plain),
        ("hide duplicates", deduped),
        ("english only + hide duplicates", filtered),
    ] {
        assert!(
            ms <= ceiling,
            "a keystroke took {ms} ms with {what}, over the {ceiling} ms this {profile} \
             build is held to (README §10 asks for {BUDGET_MS} ms). If the guide has \
             grown beyond seven days per channel, R-49 has come back."
        );
    }
}
