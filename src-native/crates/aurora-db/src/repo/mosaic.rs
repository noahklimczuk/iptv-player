//! Saved multi-view layouts (README §7.4).
//!
//! A layout is a name, which of the four arrangements it uses, and which channel sits
//! in each tile. The channel list is positional and may have holes: a 2×2 with three
//! channels chosen is a layout somebody saved, not a half-written row.
//!
//! Channel ids are not foreign keys, for the reason favourites are not either — a
//! provider dropping a channel for a day must leave the layout alone rather than
//! quietly rewriting what the viewer saved. They are resolved when the layout is
//! opened, and a tile whose channel has gone says so.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// A saved layout, as the UI lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedLayout {
    pub id: i64,
    pub name: String,
    /// One of `aurora_core::mosaic::Layout`'s names. Stored as text so a layout saved
    /// by a future version with another arrangement is skipped rather than failing the
    /// whole list.
    pub layout: String,
    /// Channel id per tile, in tile order. `None` is an empty tile.
    pub channels: Vec<Option<i64>>,
    pub created_at: i64,
}

/// Save a layout, replacing any with the same name.
///
/// Replacing rather than erroring because the name is the handle: a viewer who saves
/// "Sunday football" twice means the second one, and a unique-constraint failure
/// surfaced as "That name is taken" would be a dialog asking them to invent a synonym.
pub fn upsert_layout(
    conn: &Connection,
    name: &str,
    layout: &str,
    channels: &[Option<i64>],
    now: i64,
) -> Result<i64> {
    let name = name.trim();
    let encoded = serde_json::to_string(channels).unwrap_or_else(|_| "[]".into());
    conn.execute(
        "INSERT INTO mosaic_layouts (name, layout, channels, created_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(name) DO UPDATE SET
            layout     = excluded.layout,
            channels   = excluded.channels,
            created_at = excluded.created_at",
        params![name, layout, encoded, now],
    )?;
    Ok(conn.query_row(
        "SELECT id FROM mosaic_layouts WHERE name = ?1",
        [name],
        |r| r.get(0),
    )?)
}

/// Every saved layout, newest first.
///
/// A row whose `channels` will not parse is dropped rather than failing the list: the
/// screen that lists layouts is also the screen you would go to in order to delete a
/// bad one, so taking it down with the row is the one unhelpful option.
pub fn list_layouts(conn: &Connection) -> Result<Vec<SavedLayout>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, layout, channels, created_at
         FROM mosaic_layouts ORDER BY created_at DESC, id DESC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(rows
        .into_iter()
        .filter_map(|(id, name, layout, channels, created_at)| {
            let channels = serde_json::from_str::<Vec<Option<i64>>>(&channels).ok()?;
            Some(SavedLayout {
                id,
                name,
                layout,
                channels,
                created_at,
            })
        })
        .collect())
}

pub fn get_layout(conn: &Connection, id: i64) -> Result<Option<SavedLayout>> {
    let row = conn
        .query_row(
            "SELECT id, name, layout, channels, created_at FROM mosaic_layouts WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()?;

    Ok(row.and_then(|(id, name, layout, channels, created_at)| {
        Some(SavedLayout {
            id,
            name,
            layout,
            channels: serde_json::from_str(&channels).ok()?,
            created_at,
        })
    }))
}

/// Returns whether a row was removed, so a stale list can be told from a real delete.
pub fn delete_layout(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn.execute("DELETE FROM mosaic_layouts WHERE id = ?1", [id])? > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_760_000_000;

    fn db() -> Connection {
        crate::open_memory().unwrap()
    }

    #[test]
    fn a_layout_round_trips_with_its_holes_intact() {
        let conn = db();
        let channels = vec![Some(7), None, Some(9), None];
        let id = upsert_layout(&conn, "Sunday football", "grid2x2", &channels, NOW).unwrap();

        let got = get_layout(&conn, id).unwrap().expect("the layout");
        assert_eq!(got.name, "Sunday football");
        assert_eq!(got.layout, "grid2x2");
        // The positions are the point: tile 2 is channel 9, not "the second channel".
        assert_eq!(got.channels, channels);
    }

    #[test]
    fn saving_the_same_name_replaces_rather_than_refusing() {
        let conn = db();
        let first = upsert_layout(&conn, "News", "grid2x2", &[Some(1)], NOW).unwrap();
        let second =
            upsert_layout(&conn, "News", "onePlusThree", &[Some(2), Some(3)], NOW + 60).unwrap();

        assert_eq!(first, second, "the row is updated, not duplicated");
        assert_eq!(list_layouts(&conn).unwrap().len(), 1);

        let got = get_layout(&conn, first).unwrap().unwrap();
        assert_eq!(got.layout, "onePlusThree");
        assert_eq!(got.channels, vec![Some(2), Some(3)]);
    }

    #[test]
    fn a_name_is_trimmed_so_two_spellings_are_not_two_layouts() {
        let conn = db();
        upsert_layout(&conn, "News", "grid2x2", &[Some(1)], NOW).unwrap();
        upsert_layout(&conn, "  News  ", "grid3x3", &[Some(2)], NOW).unwrap();
        assert_eq!(list_layouts(&conn).unwrap().len(), 1);
    }

    #[test]
    fn layouts_are_listed_newest_first() {
        let conn = db();
        upsert_layout(&conn, "Older", "grid2x2", &[], NOW).unwrap();
        upsert_layout(&conn, "Newer", "grid2x2", &[], NOW + 100).unwrap();

        let names: Vec<String> = list_layouts(&conn)
            .unwrap()
            .into_iter()
            .map(|l| l.name)
            .collect();
        assert_eq!(names, vec!["Newer", "Older"]);
    }

    #[test]
    fn deleting_says_whether_there_was_anything_to_delete() {
        let conn = db();
        let id = upsert_layout(&conn, "News", "grid2x2", &[], NOW).unwrap();
        assert!(delete_layout(&conn, id).unwrap());
        assert!(
            !delete_layout(&conn, id).unwrap(),
            "a second delete is a no-op"
        );
        assert!(get_layout(&conn, id).unwrap().is_none());
    }

    /// A channel the provider stopped listing must not take the layout with it. There
    /// is no foreign key, deliberately — this is the test that says so.
    #[test]
    fn a_layout_survives_the_channel_it_names_being_deleted() {
        let conn = db();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO channels (id, provider_id, provider_key, name, match_key, last_seen_at)
             VALUES (42, 1, 'c1', 'Channel 42', 'channel42', 0)",
            [],
        )
        .unwrap();

        let id = upsert_layout(&conn, "With 42", "grid2x2", &[Some(42)], NOW).unwrap();
        conn.execute("DELETE FROM channels WHERE id = 42", [])
            .unwrap();

        let got = get_layout(&conn, id)
            .unwrap()
            .expect("the layout is still there");
        assert_eq!(
            got.channels,
            vec![Some(42)],
            "the id is kept so a tile can say it is gone"
        );
    }

    /// A row written by a version that stored something else in `channels` is skipped,
    /// not fatal: this is the screen you would use to delete it.
    #[test]
    fn a_row_that_will_not_parse_is_dropped_from_the_list() {
        let conn = db();
        upsert_layout(&conn, "Good", "grid2x2", &[Some(1)], NOW).unwrap();
        conn.execute(
            "INSERT INTO mosaic_layouts (name, layout, channels, created_at)
             VALUES ('Bad', 'grid2x2', 'not json', ?1)",
            [NOW + 1],
        )
        .unwrap();

        let listed = list_layouts(&conn).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Good");
    }
}
