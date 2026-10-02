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

/// Mark a film or show as liked, or stop.
///
/// Stored in `favorites` beside the channel hearts rather than in a table of its own,
/// because that is the table the recommender already reads: `repo::recommend::history`
/// asks `EXISTS (SELECT 1 FROM favorites … item_kind = 'movie')` for every watched film
/// and feeds the answer to `FAVOURITE_BOOST`.
///
/// Nothing had ever written one. The column was read, the boost was implemented, it had
/// a unit test, and the only writer in the codebase was the channel heart — so for films
/// and shows the flag was always false and the boost could not fire. The thumbs-up in the
/// interface was the other half of the same gap: a button that called nothing.
pub fn toggle_liked(
    conn: &Connection,
    profile_id: i64,
    kind: ItemKind,
    item_id: i64,
    now: i64,
) -> Result<bool> {
    let removed = conn.execute(
        "DELETE FROM favorites
         WHERE profile_id = ?1 AND list_name = ?2 AND item_kind = ?3 AND item_id = ?4",
        params![profile_id, FAVORITES, kind.as_str(), item_id],
    )?;
    if removed > 0 {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO favorites (profile_id, list_name, item_kind, item_id, sort_order, added_at)
         VALUES (?1, ?2, ?3, ?4, 0, ?5)",
        params![profile_id, FAVORITES, kind.as_str(), item_id, now],
    )?;
    Ok(true)
}

/// Everything this profile has liked, newest first. Channels are not included: they are
/// in the same table, under the same list, but they are a different affordance on a
/// different screen and the interface asks for them by `channels.list`.
pub fn liked(conn: &Connection, profile_id: i64) -> Result<Vec<(ItemKind, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT item_kind, item_id FROM favorites
          WHERE profile_id = ?1 AND list_name = ?2 AND item_kind IN ('movie','series')
          ORDER BY added_at DESC",
    )?;
    let rows = stmt.query_map(params![profile_id, FAVORITES], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (kind, id) = row?;
        match kind.as_str() {
            "movie" => out.push((ItemKind::Movie, id)),
            "series" => out.push((ItemKind::Series, id)),
            _ => {}
        }
    }
    Ok(out)
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

    /// The flag the recommender reads for a film had no writer at all: `toggle_favorite`
    /// is channels only, so `FAVOURITE_BOOST` could never fire for anything but a channel
    /// — and channels are not recommended.
    #[test]
    fn liking_a_film_is_what_the_recommender_reads() {
        let conn = db();
        assert!(toggle_liked(&conn, 1, ItemKind::Movie, 7, 100).unwrap());

        // Exactly the shape `repo::recommend::history` asks about.
        let seen: bool = conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM favorites
                                 WHERE profile_id = 1 AND item_kind = 'movie' AND item_id = 7)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(seen, "the recommender would still see this film as unliked");

        assert!(!toggle_liked(&conn, 1, ItemKind::Movie, 7, 200).unwrap());
        assert_eq!(liked(&conn, 1).unwrap(), vec![]);
    }

    /// Liking is not adding to a list. Two buttons, two meanings, two sets.
    #[test]
    fn liking_and_my_list_are_separate() {
        let conn = db();
        toggle_liked(&conn, 1, ItemKind::Movie, 7, 100).unwrap();
        assert_eq!(my_list(&conn, 1).unwrap(), vec![]);

        toggle_my_list(&conn, 1, ItemKind::Series, 9, 100).unwrap();
        assert_eq!(liked(&conn, 1).unwrap(), vec![(ItemKind::Movie, 7)]);
    }

    /// A film and a show with the same id are different things, as they are everywhere
    /// else in this table.
    #[test]
    fn liking_is_per_kind_and_per_profile() {
        // Profile 2 is the one `db()` creates; 1 is the default migration 5 seeds.
        let conn = db();
        toggle_liked(&conn, 1, ItemKind::Movie, 3, 100).unwrap();

        assert_eq!(liked(&conn, 1).unwrap(), vec![(ItemKind::Movie, 3)]);
        assert_eq!(liked(&conn, 2).unwrap(), vec![], "another profile's taste");
        assert!(
            toggle_liked(&conn, 1, ItemKind::Series, 3, 100).unwrap(),
            "a show with a film's id must be its own row"
        );
    }

    /// The channel hearts live in the same table under the same list name, and must not
    /// appear in a list of liked titles.
    #[test]
    fn a_favourite_channel_is_not_a_liked_title() {
        let conn = db();
        toggle_favorite(&conn, 1, 42, 100).unwrap();
        assert_eq!(liked(&conn, 1).unwrap(), vec![]);
        assert_eq!(favorite_channel_ids(&conn, 1).unwrap(), vec![42]);
    }

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
