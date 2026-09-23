//! My List and channel favourites (README §11).
//!
//! Both are per-profile sets with an added-at, and both are toggles rather than
//! separate add/remove calls: the UI's affordance is one heart, and a command that
//! mirrors it cannot get out of step with what is on screen.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// What a My List row can point at. Channels belong in favourites instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    Movie,
    Series,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Movie => "movie",
            ItemKind::Series => "series",
        }
    }
}

/// Add or remove, and say which it ended up being.
///
/// Returns true when the item is now on the list. The caller does not have to ask
/// first, which also closes the gap where two clicks in quick succession both read
/// "not present" and both insert.
pub fn toggle_my_list(
    conn: &Connection,
    profile_id: i64,
    kind: ItemKind,
    item_id: i64,
    now: i64,
) -> Result<bool> {
    let removed = conn.execute(
        "DELETE FROM my_list WHERE profile_id = ?1 AND item_kind = ?2 AND item_id = ?3",
        params![profile_id, kind.as_str(), item_id],
    )?;
    if removed > 0 {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO my_list (profile_id, item_kind, item_id, added_at) VALUES (?1,?2,?3,?4)",
        params![profile_id, kind.as_str(), item_id, now],
    )?;
    Ok(true)
}

pub fn in_my_list(
    conn: &Connection,
    profile_id: i64,
    kind: ItemKind,
    item_id: i64,
) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM my_list WHERE profile_id = ?1 AND item_kind = ?2 AND item_id = ?3",
            params![profile_id, kind.as_str(), item_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// A profile's list, newest first, as (kind, id) pairs for the caller to hydrate.
pub fn my_list(conn: &Connection, profile_id: i64) -> Result<Vec<(ItemKind, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT item_kind, item_id FROM my_list WHERE profile_id = ?1 ORDER BY added_at DESC",
    )?;
    let rows = stmt.query_map(params![profile_id], |r| {
        let kind: String = r.get(0)?;
        Ok((kind, r.get::<_, i64>(1)?))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (kind, id) = row?;
        // A row whose kind predates this enum is skipped rather than fatal: a list that
        // will not load is worse than a list missing one entry.
        match kind.as_str() {
            "movie" => out.push((ItemKind::Movie, id)),
            "series" => out.push((ItemKind::Series, id)),
            _ => {}
        }
    }
    Ok(out)
}

/// The favourites list is channels only, under its default name.
const FAVORITES: &str = "Favorites";

pub fn toggle_favorite(
    conn: &Connection,
    profile_id: i64,
    channel_id: i64,
    now: i64,
) -> Result<bool> {
    let removed = conn.execute(
        "DELETE FROM favorites
         WHERE profile_id = ?1 AND list_name = ?2 AND item_kind = 'channel' AND item_id = ?3",
        params![profile_id, FAVORITES, channel_id],
    )?;
    if removed > 0 {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO favorites (profile_id, list_name, item_kind, item_id, sort_order, added_at)
         VALUES (?1, ?2, 'channel', ?3, 0, ?4)",
        params![profile_id, FAVORITES, channel_id, now],
    )?;
    Ok(true)
}

pub fn favorite_channel_ids(conn: &Connection, profile_id: i64) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT item_id FROM favorites
         WHERE profile_id = ?1 AND list_name = ?2 AND item_kind = 'channel'
         ORDER BY sort_order, added_at",
    )?;
    let rows = stmt.query_map(params![profile_id, FAVORITES], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<i64>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::profiles;

    fn db() -> Connection {
        let conn = crate::open_memory().expect("open");
        profiles::create(
            &conn,
            &profiles::NewProfile {
                name: "Me".into(),
                ..Default::default()
            },
            0,
        )
        .expect("profile");
        conn
    }

    #[test]
    fn toggling_adds_then_removes() {
        let conn = db();
        assert!(toggle_my_list(&conn, 1, ItemKind::Movie, 7, 100).unwrap());
        assert!(in_my_list(&conn, 1, ItemKind::Movie, 7).unwrap());

        assert!(!toggle_my_list(&conn, 1, ItemKind::Movie, 7, 200).unwrap());
        assert!(!in_my_list(&conn, 1, ItemKind::Movie, 7).unwrap());
    }

    #[test]
    fn a_movie_and_a_series_with_the_same_id_are_different_entries() {
        // The ids come from different tables, so they collide constantly.
        let conn = db();
        toggle_my_list(&conn, 1, ItemKind::Movie, 3, 100).unwrap();
        assert!(!in_my_list(&conn, 1, ItemKind::Series, 3).unwrap());
    }

    #[test]
    fn the_list_is_newest_first() {
        let conn = db();
        toggle_my_list(&conn, 1, ItemKind::Movie, 1, 100).unwrap();
        toggle_my_list(&conn, 1, ItemKind::Series, 2, 200).unwrap();
        toggle_my_list(&conn, 1, ItemKind::Movie, 3, 300).unwrap();

        assert_eq!(
            my_list(&conn, 1).unwrap(),
            vec![
                (ItemKind::Movie, 3),
                (ItemKind::Series, 2),
                (ItemKind::Movie, 1)
            ]
        );
    }

    #[test]
    fn one_profiles_list_is_not_anothers() {
        let conn = db();
        profiles::create(
            &conn,
            &profiles::NewProfile {
                name: "Sam".into(),
                ..Default::default()
            },
            0,
        )
        .unwrap();

        toggle_my_list(&conn, 1, ItemKind::Movie, 7, 100).unwrap();
        assert!(!in_my_list(&conn, 2, ItemKind::Movie, 7).unwrap());
        assert!(my_list(&conn, 2).unwrap().is_empty());
    }

    #[test]
    fn favorites_toggle_the_same_way() {
        let conn = db();
        assert!(toggle_favorite(&conn, 1, 42, 100).unwrap());
        assert_eq!(favorite_channel_ids(&conn, 1).unwrap(), vec![42]);

        assert!(!toggle_favorite(&conn, 1, 42, 200).unwrap());
        assert!(favorite_channel_ids(&conn, 1).unwrap().is_empty());
    }

    #[test]
    fn a_row_of_an_unknown_kind_is_skipped_not_fatal() {
        let conn = db();
        toggle_my_list(&conn, 1, ItemKind::Movie, 1, 100).unwrap();
        conn.execute(
            "INSERT INTO my_list (profile_id, item_kind, item_id, added_at)
             VALUES (1, 'collection', 9, 150)",
            [],
        )
        .unwrap();

        // The known row still comes back rather than the whole read failing.
        assert_eq!(my_list(&conn, 1).unwrap(), vec![(ItemKind::Movie, 1)]);
    }
}
