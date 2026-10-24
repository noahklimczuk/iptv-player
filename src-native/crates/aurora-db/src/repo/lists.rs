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
    let at_end = next_sort_order(conn, profile_id);
    conn.execute(
        "INSERT INTO favorites (profile_id, list_name, item_kind, item_id, sort_order, added_at)
         VALUES (?1, ?2, 'channel', ?3, ?5, ?4)",
        params![profile_id, FAVORITES, channel_id, now, at_end],
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

/// Put this profile's favourite channels in the order given (README §4, "drag to
/// reorder").
///
/// The read side has always honoured an order -- `favorite_channel_ids` ends
/// `ORDER BY sort_order, added_at` -- and nothing ever wrote one. Both inserts in this
/// module put a literal `0` in the column, so every favourite tied on `sort_order` and
/// the tie-break did all the work: the list was in the order things were hearted, and
/// could not be in any other.
///
/// `ids` is the whole list in its new order. Taking the entire list rather than "move
/// this one to position N" is what makes the operation idempotent and the result exactly
/// what the viewer sees: a positional patch has to agree with the client about what the
/// order *was*, and two of them racing leaves a sequence neither asked for.
///
/// Rows not named in `ids` are left alone. A favourite hearted on another device between
/// the screen rendering and the drag landing is not something this knows about, and
/// deleting it -- or renumbering it to the front -- would be worse than letting it keep
/// its place.
pub fn reorder_favorites(conn: &mut Connection, profile_id: i64, ids: &[i64]) -> Result<usize> {
    let tx = conn.transaction()?;
    let mut moved = 0;
    {
        let mut stmt = tx.prepare(
            "UPDATE favorites SET sort_order = ?4
              WHERE profile_id = ?1 AND list_name = ?2
                AND item_kind = 'channel' AND item_id = ?3",
        )?;
        for (position, id) in ids.iter().enumerate() {
            // `position + 1`, so no row keeps the 0 every insert writes. Leaving a
            // reordered row at 0 would tie it with every favourite added afterwards, and
            // the tie-break is `added_at` -- so a channel dragged to the top would be
            // overtaken by the next thing hearted.
            moved += stmt.execute(params![
                profile_id,
                FAVORITES,
                id,
                i64::try_from(position)
                    .unwrap_or(i64::MAX)
                    .saturating_add(1)
            ])?;
        }
    }
    tx.commit()?;
    Ok(moved)
}

/// The next `sort_order` for a new favourite, so it lands at the end rather than tying.
///
/// Both inserts used a literal `0`. Once anything has been reordered that is no longer a
/// neutral value -- it is *first* -- so a newly hearted channel jumped to the top of a
/// list the viewer had arranged.
fn next_sort_order(conn: &Connection, profile_id: i64) -> i64 {
    conn.query_row(
        "SELECT COALESCE(MAX(sort_order), 0) + 1 FROM favorites
          WHERE profile_id = ?1 AND list_name = ?2 AND item_kind = 'channel'",
        params![profile_id, FAVORITES],
        |r| r.get(0),
    )
    .unwrap_or(1)
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

    /// The order the viewer drags things into is the order that comes back.
    ///
    /// Nothing ever wrote `sort_order` -- both inserts put a literal `0` -- so every
    /// favourite tied and `added_at` decided the list. It could only ever be in the
    /// order things were hearted.
    #[test]
    fn favourites_come_back_in_the_order_they_were_put_in() {
        let mut conn = crate::open_memory().unwrap();
        seed_channels(&conn, &[10, 20, 30]);
        for (i, id) in [10, 20, 30].iter().enumerate() {
            toggle_favorite(&conn, 1, *id, 100 + i as i64).unwrap();
        }
        assert_eq!(favorite_channel_ids(&conn, 1).unwrap(), vec![10, 20, 30]);

        // Dragged into reverse.
        assert_eq!(reorder_favorites(&mut conn, 1, &[30, 20, 10]).unwrap(), 3);
        assert_eq!(favorite_channel_ids(&conn, 1).unwrap(), vec![30, 20, 10]);
    }

    /// A newly hearted channel joins the end, not the top.
    ///
    /// Both inserts wrote `0`, which is a neutral value only while nothing has been
    /// reordered. Once something has, 0 is *first* -- so hearting a channel sent it to
    /// the top of a list somebody had arranged.
    #[test]
    fn a_new_favourite_lands_at_the_end_of_an_arranged_list() {
        let mut conn = crate::open_memory().unwrap();
        seed_channels(&conn, &[10, 20, 30]);
        toggle_favorite(&conn, 1, 10, 100).unwrap();
        toggle_favorite(&conn, 1, 20, 101).unwrap();
        reorder_favorites(&mut conn, 1, &[20, 10]).unwrap();

        toggle_favorite(&conn, 1, 30, 102).unwrap();
        assert_eq!(
            favorite_channel_ids(&conn, 1).unwrap(),
            vec![20, 10, 30],
            "a newly hearted channel jumped the order the viewer chose"
        );
    }

    /// Reordering is per profile, like the favourites themselves.
    #[test]
    fn one_profile_cannot_reorder_anothers_favourites() {
        let mut conn = crate::open_memory().unwrap();
        seed_channels(&conn, &[10, 20]);
        // `favorites.profile_id` references `profiles`, and only the default one exists
        // in a fresh database.
        conn.execute(
            "INSERT INTO profiles (id,name,created_at) VALUES (2,'Second',0)",
            [],
        )
        .unwrap();
        for p in [1, 2] {
            toggle_favorite(&conn, p, 10, 100).unwrap();
            toggle_favorite(&conn, p, 20, 101).unwrap();
        }
        reorder_favorites(&mut conn, 1, &[20, 10]).unwrap();
        assert_eq!(favorite_channel_ids(&conn, 1).unwrap(), vec![20, 10]);
        assert_eq!(
            favorite_channel_ids(&conn, 2).unwrap(),
            vec![10, 20],
            "the other profile's order moved"
        );
    }

    /// A favourite the caller did not name keeps its place rather than vanishing.
    #[test]
    fn a_favourite_missing_from_the_list_is_left_alone() {
        let mut conn = crate::open_memory().unwrap();
        seed_channels(&conn, &[10, 20, 30]);
        for (i, id) in [10, 20, 30].iter().enumerate() {
            toggle_favorite(&conn, 1, *id, 100 + i as i64).unwrap();
        }
        // Only two named, as a screen that rendered before the third was hearted would.
        assert_eq!(reorder_favorites(&mut conn, 1, &[20, 10]).unwrap(), 2);
        let after = favorite_channel_ids(&conn, 1).unwrap();
        assert!(
            after.contains(&30),
            "the unnamed favourite was lost: {after:?}"
        );
        assert_eq!(after.len(), 3);
    }

    /// Channels the reorder names but which are not favourites change nothing.
    #[test]
    fn reordering_cannot_invent_a_favourite() {
        let mut conn = crate::open_memory().unwrap();
        seed_channels(&conn, &[10, 20]);
        toggle_favorite(&conn, 1, 10, 100).unwrap();
        assert_eq!(reorder_favorites(&mut conn, 1, &[20, 10]).unwrap(), 1);
        assert_eq!(favorite_channel_ids(&conn, 1).unwrap(), vec![10]);
    }

    /// Channels for the tests above, since `favorites` references nothing itself but the
    /// profile and the list reads join against `channels` in the real query.
    fn seed_channels(conn: &Connection, ids: &[i64]) {
        conn.execute(
            "INSERT OR IGNORE INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        for id in ids {
            conn.execute(
                "INSERT OR IGNORE INTO channels
                   (id, provider_id, provider_key, name, match_key, last_seen_at)
                 VALUES (?1, 1, ?2, ?3, ?3, 0)",
                params![id, format!("k{id}"), format!("ch{id}")],
            )
            .unwrap();
        }
    }
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
