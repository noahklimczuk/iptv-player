//! EPG persistence and the guide-grid read path.

use std::collections::HashSet;

use aurora_core::model::{Credit, Programme};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::Result;

const FLAG_NEW: i64 = 1;
const FLAG_LIVE: i64 = 2;
const FLAG_PREMIERE: i64 = 4;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgrammeRow {
    pub id: i64,
    pub channel_id: String,
    pub start: i64,
    pub stop: i64,
    pub title: String,
    pub sub_title: Option<String>,
    pub description: Option<String>,
    pub categories: Vec<String>,
    /// The guide entry's own artwork, its rating out of ten, and its cast.
    ///
    /// All three are written by the parser on every import and none of them could be
    /// read back: there were no fields for `icon` or `credits`, and `star_rating` was
    /// fetched as column 11 and then skipped (R-51).
    pub icon: Option<String>,
    pub star_rating: Option<f32>,
    pub credits: Vec<Credit>,
    pub season: Option<u16>,
    pub episode: Option<u16>,
    pub rating: Option<String>,
    pub is_new: bool,
    pub is_live: bool,
    pub is_premiere: bool,
}

/// Bulk-insert programmes. Called in batches by the importer so a 1 GB XMLTV never lands
/// in memory at once (README §4.4).
pub fn insert_batch(conn: &mut Connection, programmes: &[Programme]) -> Result<usize> {
    let tx = conn.transaction()?;
    let mut n = 0;
    {
        let mut stmt = tx.prepare(
            r#"INSERT INTO epg_programmes
               (channel_id, start, stop, title, sub_title, description, categories,
                season, episode, icon, rating, star_rating, flags, credits)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)"#,
        )?;
        for p in programmes {
            let flags = (if p.is_new { FLAG_NEW } else { 0 })
                | (if p.is_live { FLAG_LIVE } else { 0 })
                | (if p.is_premiere { FLAG_PREMIERE } else { 0 });
            stmt.execute(params![
                p.channel_id,
                p.start,
                p.stop,
                p.title,
                p.sub_title,
                p.description,
                serde_json::to_string(&p.categories)?,
                p.season,
                p.episode,
                p.icon,
                p.rating,
                p.star_rating,
                flags,
                serde_json::to_string(&p.credits)?,
            ])?;
            n += 1;
        }
    }
    tx.commit()?;
    Ok(n)
}

pub fn upsert_channels(
    conn: &mut Connection,
    channels: &[aurora_core::model::EpgChannel],
) -> Result<usize> {
    let tx = conn.transaction()?;
    let mut n = 0;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO epg_channels (id, display_name, display_names, icon)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(id) DO UPDATE SET
               display_name = excluded.display_name,
               display_names = excluded.display_names,
               icon = COALESCE(excluded.icon, epg_channels.icon)",
        )?;
        for c in channels {
            // All of them, not only the first. XMLTV allows several `<display-name>`
            // elements and the alternative spellings are what fuzzy matching needs; the
            // column was singular, so everything after the first was dropped here and
            // nothing later could ever see it (R-50).
            //
            // `display_name` stays as the first one, because it is what every existing
            // query reads and what a list shows.
            let all = serde_json::to_string(&c.display_names).unwrap_or_else(|_| "[]".into());
            stmt.execute(params![c.id, c.display_names.first(), all, c.icon])?;
            n += 1;
        }
    }
    tx.commit()?;
    Ok(n)
}

fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ProgrammeRow> {
    let flags: i64 = r.get(12)?;
    // `Option`, because the column is nullable and this read is not optional.
    //
    // `epg_programmes.categories` has no `NOT NULL` and no default, and reading a NULL
    // into a `String` is `InvalidColumnType` -- which `grid_slice` turns into a failure
    // of the *whole* query, because it collects into `Result<Vec<_>, _>`. One NULL row
    // and the guide is blank rather than missing an entry. Not reachable from
    // `insert_batch`, which always writes `serde_json::to_string`, so the value is at
    // least `"[]"`; reachable from the next migration or second write path, and found
    // when a probe inserted rows without the column and the guide query died exactly
    // that way. The line below already does `unwrap_or_default` on an unparseable value,
    // so tolerating a bad one is the function's stated intent and only NULL escaped it
    // (R-79).
    let categories: String = r.get::<_, Option<String>>(7)?.unwrap_or_default();
    Ok(ProgrammeRow {
        id: r.get(0)?,
        channel_id: r.get(1)?,
        start: r.get(2)?,
        stop: r.get(3)?,
        title: r.get(4)?,
        sub_title: r.get(5)?,
        description: r.get(6)?,
        categories: serde_json::from_str(&categories).unwrap_or_default(),
        season: r.get::<_, Option<i64>>(8)?.map(|v| v as u16),
        episode: r.get::<_, Option<i64>>(9)?.map(|v| v as u16),
        rating: r.get(10)?,
        // Column 11 was fetched and then never read: `map_row` went straight from
        // `rating` at 10 to `flags` at 12. Together with `icon` and `credits`, which the
        // row had no fields for at all, that is three things written on every import and
        // unable to reach the UI -- the guide entry's own artwork, its cast list and its
        // star rating. This subscription sends none of them (R-31/R-132), which is
        // presumably why nobody noticed; on a guide that carries them the work was being
        // done and discarded (R-51).
        star_rating: r.get(11)?,
        icon: r.get(13)?,
        credits: serde_json::from_str::<Vec<Credit>>(
            &r.get::<_, Option<String>>(14)?.unwrap_or_default(),
        )
        .unwrap_or_default(),
        is_new: flags & FLAG_NEW != 0,
        is_live: flags & FLAG_LIVE != 0,
        is_premiere: flags & FLAG_PREMIERE != 0,
    })
}

const SELECT: &str = "SELECT id, channel_id, start, stop, title, sub_title, description,
                             categories, season, episode, rating, star_rating, flags,
                             icon, credits
                      FROM epg_programmes";

/// Fetch every programme overlapping `[from, to)` for the given EPG channels.
///
/// This is the guide grid's only query. The overlap test is `start < to AND stop > from`,
/// which catches programmes that began before the window opened — without it the leftmost
/// column of the grid is empty, which is the classic EPG bug.
/// The most channels one `IN (...)` may name.
///
/// There was one placeholder per channel and no bound on how many, which left the guide
/// a library's size away from failing outright: SQLite's `SQLITE_MAX_VARIABLE_NUMBER` is
/// 32,766 in the version bundled here and 999 in anything older than 3.32. The real line
/// carries 6,698 channels and carried 22,121 when docs/ROADMAP.md was written, and
/// `hostile_input.rs` treats a fifty-thousand-entry playlist as a shape to support — so
/// the ceiling was reachable by a viewer with a bigger subscription, and `prepare` would
/// have refused the whole guide rather than any one channel.
///
/// Chunked rather than capped, because a cap here means channels silently missing from
/// the guide. A larger library asks more questions instead.
const IDS_PER_QUERY: usize = 500;

/// The longest span one slice may cover.
///
/// `start < to AND stop > from` with nothing bounding `to - from` returns every
/// programme every named channel has: the real line holds 453,072 rows, and a caller
/// asking for a year would get all of them in one `Vec`, through one IPC reply, while
/// holding the writer lock. The guide shows hours at a time, so two days is already far
/// more than any view of it needs, and the window is clamped rather than refused — a
/// guide that draws a shorter span is better than one that errors.
pub const MAX_SLICE_SECS: i64 = 48 * 3600;

pub fn grid_slice(
    conn: &Connection,
    epg_channel_ids: &[String],
    from: i64,
    to: i64,
) -> Result<Vec<ProgrammeRow>> {
    if epg_channel_ids.is_empty() {
        return Ok(Vec::new());
    }
    let to = to.min(from.saturating_add(MAX_SLICE_SECS));
    if to <= from {
        return Ok(Vec::new());
    }

    let mut rows = Vec::new();
    for chunk in epg_channel_ids.chunks(IDS_PER_QUERY) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "{SELECT} WHERE channel_id IN ({placeholders}) AND start < ? AND stop > ?
             ORDER BY channel_id, start"
        );

        let mut stmt = conn.prepare(&sql)?;
        let mut args: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(chunk.len() + 2);
        for id in chunk {
            args.push(id);
        }
        args.push(&to);
        args.push(&from);

        rows.extend(
            stmt.query_map(args.as_slice(), map_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?,
        );
    }

    // The ordering is part of what this returns, and chunking only orders within a
    // chunk. Each channel lives in exactly one of them, so its programmes are already
    // in `start` order and this restores the rest.
    rows.sort_by(|a, b| {
        a.channel_id
            .cmp(&b.channel_id)
            .then_with(|| a.start.cmp(&b.start))
    });
    Ok(rows)
}

/// What is on now, and what is on next, for one channel — the channel banner's data.
pub fn now_next(
    conn: &Connection,
    epg_channel_id: &str,
    now: i64,
) -> Result<(Option<ProgrammeRow>, Option<ProgrammeRow>)> {
    let current = conn
        .query_row(
            // `ORDER BY start DESC`, because `LIMIT 1` without one picks whichever row
            // the query plan reaches first. A provider's XMLTV overlaps constantly — a
            // film listed 20:00–22:00 with a short at 21:00 both match 21:15 — and
            // without an order "what's on now" was whatever the index happened to
            // yield, which can differ between two calls a second apart. Descending
            // takes the most recently started of the overlapping entries, which is the
            // one a later listing is correcting an earlier one with; the `next` query
            // below has always been ordered.
            &format!(
                "{SELECT} WHERE channel_id = ?1 AND start <= ?2 AND stop > ?2
                 ORDER BY start DESC LIMIT 1"
            ),
            params![epg_channel_id, now],
            map_row,
        )
        .optional()?;
    let next = conn
        .query_row(
            &format!("{SELECT} WHERE channel_id = ?1 AND start > ?2 ORDER BY start LIMIT 1"),
            params![epg_channel_id, now],
            map_row,
        )
        .optional()?;
    Ok((current, next))
}

/// README §4.4: prune old programmes on a schedule so the DB does not grow unbounded.
pub fn prune_before(conn: &Connection, cutoff: i64) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM epg_programmes WHERE stop < ?1",
        params![cutoff],
    )?)
}

/// Remove everything for a set of channels before reimporting them, so a refresh does not
/// duplicate programmes.
pub fn clear_channels(conn: &mut Connection, epg_channel_ids: &[String]) -> Result<usize> {
    if epg_channel_ids.is_empty() {
        return Ok(0);
    }
    let tx = conn.transaction()?;
    let mut n = 0;
    {
        let mut stmt = tx.prepare("DELETE FROM epg_programmes WHERE channel_id = ?1")?;
        for id in epg_channel_ids {
            n += stmt.execute(params![id])?;
        }
    }
    tx.commit()?;
    Ok(n)
}

pub fn count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT count(*) FROM epg_programmes", [], |r| r.get(0))?)
}

/// Which guide channels actually have something scheduled.
///
/// A guide declares channels and schedules programmes against them, and the two sets are
/// not the same: of the 4,827 distinct ids this subscription's guide declares, 1,882
/// carry no `<programme>` element at all. Whether a channel has any is not knowable
/// until the whole document has been read, which is why this is asked of the table
/// afterwards rather than decided while parsing (R-21).
pub fn channels_with_programmes(conn: &Connection) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT DISTINCT channel_id FROM epg_programmes")?;
    let ids = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<HashSet<_>, _>>()?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::model::Programme;

    fn prog(channel: &str, start: i64, stop: i64, title: &str) -> Programme {
        Programme {
            channel_id: channel.into(),
            start,
            stop,
            title: title.into(),
            sub_title: None,
            description: None,
            categories: vec!["News".into()],
            season: None,
            episode: None,
            icon: None,
            rating: None,
            star_rating: None,
            is_new: false,
            is_live: false,
            is_premiere: false,
            credits: vec![],
        }
    }

    /// A NULL in a nullable column costs one row, not the whole guide (R-79).
    ///
    /// `epg_programmes.categories` has no `NOT NULL` and no default, and `map_row` read
    /// it straight into a `String` -- so a NULL was `InvalidColumnType`, and because
    /// `grid_slice` collects into `Result<Vec<_>, _>` that one row failed the entire
    /// query. The guide would be blank rather than missing an entry. `insert_batch` never
    /// writes one, so this inserts the row the way a migration or a second write path
    /// would.
    #[test]
    fn a_null_category_does_not_empty_the_guide() {
        let conn = seeded();
        let before = grid_slice(&conn, &["a".to_string()], 0, 7200)
            .unwrap()
            .len();
        assert!(before > 0, "nothing to lose");

        conn.execute(
            "INSERT INTO epg_programmes
                 (channel_id, start, stop, title, categories, flags)
             VALUES ('a', 4000, 5000, 'No categories', NULL, 0)",
            [],
        )
        .unwrap();

        let rows = grid_slice(&conn, &["a".to_string()], 0, 7200).unwrap();
        assert_eq!(
            rows.len(),
            before + 1,
            "the NULL row took the query with it"
        );
        let null_row = rows
            .iter()
            .find(|r| r.title == "No categories")
            .expect("the row with no categories is missing");
        assert!(
            null_row.categories.is_empty(),
            "a NULL reads as no categories, not as a failure"
        );
    }

    fn seeded() -> Connection {
        let mut conn = crate::open_memory().unwrap();
        insert_batch(
            &mut conn,
            &[
                prog("a", 0, 3600, "A1"),
                prog("a", 3600, 7200, "A2"),
                prog("a", 7200, 10800, "A3"),
                prog("b", 1800, 5400, "B1"),
            ],
        )
        .unwrap();
        conn
    }

    /// More channels than SQLite will take variables for.
    ///
    /// `SQLITE_MAX_VARIABLE_NUMBER` is 32,766 in the bundled build, so this asks about
    /// 33,000 and the unchunked version cannot even `prepare` the statement — it fails
    /// the whole guide with "too many SQL variables" rather than losing one channel.
    /// Most of these ids do not exist, which is the point: the cost being guarded is the
    /// *placeholders*, so the control needs that many ids and not that many rows.
    #[test]
    fn more_channels_than_sqlite_allows_variables_still_works() {
        let mut conn = crate::open_memory().unwrap();
        insert_batch(
            &mut conn,
            &[
                prog("ch00001", 0, 3600, "First"),
                prog("ch32999", 0, 3600, "Last"),
            ],
        )
        .unwrap();

        let ids: Vec<String> = (0..33_000).map(|i| format!("ch{i:05}")).collect();
        assert!(
            ids.len() > 32_766,
            "the control has to exceed the real limit"
        );

        let rows = grid_slice(&conn, &ids, 0, 3600).unwrap();
        let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, vec!["First", "Last"]);
    }

    /// Chunking must not lose a channel or reorder what it returns, which is the half
    /// the test above does not cover: it would pass on a library small enough to fit in
    /// one query.
    #[test]
    fn chunking_keeps_every_channel_and_the_order_it_promises() {
        let mut conn = crate::open_memory().unwrap();
        let count = IDS_PER_QUERY * 2 + 7;
        let batch: Vec<_> = (0..count)
            .map(|i| prog(&format!("ch{i:05}"), 0, 3600, &format!("P{i}")))
            .collect();
        insert_batch(&mut conn, &batch).unwrap();

        let ids: Vec<String> = (0..count).map(|i| format!("ch{i:05}")).collect();
        let rows = grid_slice(&conn, &ids, 0, 3600).unwrap();
        assert_eq!(rows.len(), count, "a channel was lost between chunks");

        let mut sorted = rows.clone();
        sorted.sort_by(|a, b| {
            a.channel_id
                .cmp(&b.channel_id)
                .then_with(|| a.start.cmp(&b.start))
        });
        let got: Vec<&str> = rows.iter().map(|r| r.channel_id.as_str()).collect();
        let want: Vec<&str> = sorted.iter().map(|r| r.channel_id.as_str()).collect();
        assert_eq!(got, want, "chunking left the rows out of order");
    }

    /// A window nothing bounds returns every programme a channel ever had. The guide
    /// shows hours; a caller asking for a year gets the clamp, not the library.
    #[test]
    fn an_absurd_window_is_clamped_rather_than_answered_in_full() {
        let mut conn = crate::open_memory().unwrap();
        // One programme an hour for sixty days, so there is something well past the cap.
        let batch: Vec<_> = (0..24 * 60)
            .map(|h| prog("a", h * 3600, (h + 1) * 3600, &format!("H{h}")))
            .collect();
        insert_batch(&mut conn, &batch).unwrap();

        let a_year = grid_slice(&conn, &["a".into()], 0, 365 * 86_400).unwrap();
        assert_eq!(
            a_year.len() as i64,
            MAX_SLICE_SECS / 3600,
            "the window was not clamped"
        );
        // The clamp is a ceiling, not a floor: an ordinary request is untouched.
        let three_hours = grid_slice(&conn, &["a".into()], 0, 3 * 3600).unwrap();
        assert_eq!(three_hours.len(), 3);
    }

    /// A backwards or empty window asks for nothing, and must not be turned into the
    /// whole cap by the clamp above.
    #[test]
    fn a_window_that_ends_before_it_starts_returns_nothing() {
        let conn = seeded();
        assert!(grid_slice(&conn, &["a".into()], 5000, 5000)
            .unwrap()
            .is_empty());
        assert!(grid_slice(&conn, &["a".into()], 5000, 1000)
            .unwrap()
            .is_empty());
    }

    /// Overlapping programmes are ordinary in a provider's XMLTV, and "what is on now"
    /// has to be the same answer twice. Without an order this returned whichever row the
    /// plan reached, so the Live TV strip could show one title and the guide another.
    #[test]
    fn overlapping_programmes_give_one_stable_answer_for_now() {
        let mut conn = crate::open_memory().unwrap();
        insert_batch(
            &mut conn,
            &[
                prog("a", 0, 7200, "The long film"),
                prog("a", 3600, 5400, "The short that overlaps it"),
                prog("a", 7200, 9000, "After both"),
            ],
        )
        .unwrap();

        // 4500 is inside both the film and the short.
        let (now, next) = now_next(&conn, "a", 4500).unwrap();
        let now = now.expect("something is on");
        assert_eq!(now.title, "The short that overlaps it");
        assert_eq!(next.expect("something is next").title, "After both");

        // And asking again gives the same one, which is the whole point.
        for _ in 0..5 {
            let (again, _) = now_next(&conn, "a", 4500).unwrap();
            assert_eq!(again.unwrap().title, now.title);
        }
    }

    #[test]
    fn inserts_and_counts() {
        let conn = seeded();
        assert_eq!(count(&conn).unwrap(), 4);
    }

    #[test]
    fn grid_slice_includes_programmes_already_in_progress() {
        let conn = seeded();
        // Window opens at 2000, mid-way through A1. A1 must still be returned or the
        // leftmost guide column renders blank.
        let rows = grid_slice(&conn, &["a".into()], 2000, 5000).unwrap();
        let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, vec!["A1", "A2"]);
    }

    #[test]
    fn grid_slice_excludes_programmes_outside_the_window() {
        let conn = seeded();
        let rows = grid_slice(&conn, &["a".into()], 0, 3600).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "A1");
    }

    #[test]
    fn grid_slice_spans_multiple_channels() {
        let conn = seeded();
        let rows = grid_slice(&conn, &["a".into(), "b".into()], 0, 10800).unwrap();
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn grid_slice_of_no_channels_is_empty_not_an_error() {
        let conn = seeded();
        assert!(grid_slice(&conn, &[], 0, 10800).unwrap().is_empty());
    }

    #[test]
    fn now_next_returns_the_right_pair() {
        let conn = seeded();
        let (now, next) = now_next(&conn, "a", 4000).unwrap();
        assert_eq!(now.unwrap().title, "A2");
        assert_eq!(next.unwrap().title, "A3");
    }

    #[test]
    fn now_next_handles_a_gap_in_the_schedule() {
        let conn = seeded();
        // Past the end of everything on "a".
        let (now, next) = now_next(&conn, "a", 20_000).unwrap();
        assert!(now.is_none());
        assert!(next.is_none());
    }

    #[test]
    fn categories_and_flags_round_trip() {
        let mut conn = crate::open_memory().unwrap();
        let mut p = prog("a", 0, 60, "Flagged");
        p.is_new = true;
        p.is_premiere = true;
        p.categories = vec!["Sports".into(), "Live".into()];
        insert_batch(&mut conn, &[p]).unwrap();

        let row = &grid_slice(&conn, &["a".into()], 0, 60).unwrap()[0];
        assert!(row.is_new);
        assert!(row.is_premiere);
        assert!(!row.is_live);
        assert_eq!(row.categories, vec!["Sports", "Live"]);
    }

    #[test]
    fn prune_removes_only_finished_programmes() {
        let conn = seeded();
        // Only A1 has finished before the cutoff: A2/A3 end later and B1 runs to 5400.
        let removed = prune_before(&conn, 5000).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(count(&conn).unwrap(), 3);

        // A cutoff past everything clears the table.
        prune_before(&conn, 100_000).unwrap();
        assert_eq!(count(&conn).unwrap(), 0);
    }

    #[test]
    fn clearing_a_channel_lets_reimport_avoid_duplicates() {
        let mut conn = seeded();
        clear_channels(&mut conn, &["a".into()]).unwrap();
        assert_eq!(count(&conn).unwrap(), 1);
        insert_batch(&mut conn, &[prog("a", 0, 3600, "A1")]).unwrap();
        assert_eq!(count(&conn).unwrap(), 2);
    }

    #[test]
    fn epg_channels_upsert_without_duplicating() {
        let mut conn = crate::open_memory().unwrap();
        let c = aurora_core::model::EpgChannel {
            id: "a".into(),
            display_names: vec!["Channel A".into()],
            icon: Some("https://example.com/a.png".into()),
        };
        upsert_channels(&mut conn, std::slice::from_ref(&c)).unwrap();
        upsert_channels(&mut conn, &[c]).unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM epg_channels", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }
}
