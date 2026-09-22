//! EPG persistence and the guide-grid read path.

use aurora_core::model::Programme;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::Result;

const FLAG_NEW: i64 = 1;
const FLAG_LIVE: i64 = 2;
const FLAG_PREMIERE: i64 = 4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
            "INSERT INTO epg_channels (id, display_name, icon) VALUES (?1,?2,?3)
             ON CONFLICT(id) DO UPDATE SET
               display_name = excluded.display_name,
               icon = COALESCE(excluded.icon, epg_channels.icon)",
        )?;
        for c in channels {
            stmt.execute(params![c.id, c.display_names.first(), c.icon])?;
            n += 1;
        }
    }
    tx.commit()?;
    Ok(n)
}

fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ProgrammeRow> {
    let flags: i64 = r.get(12)?;
    let categories: String = r.get(7)?;
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
        is_new: flags & FLAG_NEW != 0,
        is_live: flags & FLAG_LIVE != 0,
        is_premiere: flags & FLAG_PREMIERE != 0,
    })
}

const SELECT: &str = "SELECT id, channel_id, start, stop, title, sub_title, description,
                             categories, season, episode, rating, star_rating, flags
                      FROM epg_programmes";

/// Fetch every programme overlapping `[from, to)` for the given EPG channels.
///
/// This is the guide grid's only query. The overlap test is `start < to AND stop > from`,
/// which catches programmes that began before the window opened — without it the leftmost
/// column of the grid is empty, which is the classic EPG bug.
pub fn grid_slice(
    conn: &Connection,
    epg_channel_ids: &[String],
    from: i64,
    to: i64,
) -> Result<Vec<ProgrammeRow>> {
    if epg_channel_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat("?")
        .take(epg_channel_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "{SELECT} WHERE channel_id IN ({placeholders}) AND start < ? AND stop > ?
         ORDER BY channel_id, start"
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut args: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(epg_channel_ids.len() + 2);
    for id in epg_channel_ids {
        args.push(id);
    }
    args.push(&to);
    args.push(&from);

    let rows = stmt
        .query_map(args.as_slice(), map_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
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
            &format!("{SELECT} WHERE channel_id = ?1 AND start <= ?2 AND stop > ?2 LIMIT 1"),
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
    Ok(conn.execute("DELETE FROM epg_programmes WHERE stop < ?1", params![cutoff])? )
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
        upsert_channels(&mut conn, &[c.clone()]).unwrap();
        upsert_channels(&mut conn, &[c]).unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM epg_channels", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }
}
