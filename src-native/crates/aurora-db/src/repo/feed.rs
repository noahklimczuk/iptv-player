//! The rows the home feed is built from.
//!
//! `aurora_core::feed` is the algorithm and never opens a database; this is everything
//! SQLite-shaped it needs. Unlike `repo::recommend`, which samples the library because
//! it was written for one rail of twenty, the feed reads every title's *features* --
//! a few short columns, not the row -- because every rail now holds all of what belongs
//! on it, and a rail cannot be complete from a sample. Measured on the subscription
//! this was built against, that is 33,660 rows in well under a tenth of a second.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension};

use aurora_core::feed::{Credit, Item, Key, Kind, Watch};

use crate::repo::filtering::{Kind as ListKind, LibraryFilter};
use crate::Result;

/// How many watched titles are read. Far past what the decay leaves meaningful.
const HISTORY_LIMIT: u32 = 400;

/// Cast billed below this is not who a film is "with".
const CAST_DEPTH: i64 = 8;

fn kind_of(raw: &str) -> Option<Kind> {
    match raw {
        "movie" => Some(Kind::Movie),
        "series" => Some(Kind::Series),
        _ => None,
    }
}

fn kind_str(kind: Kind) -> &'static str {
    kind.as_str()
}

fn parse_genres(raw: Option<String>) -> Vec<String> {
    let Some(raw) = raw.filter(|r| !r.trim().is_empty()) else {
        return Vec::new();
    };
    // Stored as a JSON array by enrichment; a provider's own field can be a plain
    // comma list.
    serde_json::from_str::<Vec<String>>(&raw).unwrap_or_else(|_| {
        raw.split([',', '/', '|'])
            .map(|g| g.trim().to_string())
            .filter(|g| !g.is_empty())
            .collect()
    })
}

/// Every film and show, with what the feed reads off it.
///
/// `listed` is computed in the same query from the viewer's library filters, so the
/// feed hides exactly what every other list hides -- and still knows what a hidden
/// title was about, if the viewer watched it before the filter went on.
pub fn items(conn: &Connection, filter: &LibraryFilter) -> Result<Vec<Item>> {
    let mut out = Vec::new();
    for (table, kind, list_kind, runtime) in [
        ("movies", Kind::Movie, ListKind::Movies, "runtime_mins"),
        ("series", Kind::Series, ListKind::Series, "NULL"),
    ] {
        let sql = format!(
            "SELECT {table}.id, COALESCE(custom_title, tmdb_title, title), match_key, year,
                    genres, group_title, rating, added_at, {runtime}, quality_rank,
                    poster IS NOT NULL AND poster <> '',
                    backdrop IS NOT NULL AND backdrop <> '',
                    tmdb_id, lang_code,
                    CASE WHEN {table}.hidden = 0{filters} THEN 1 ELSE 0 END
               FROM {table}",
            filters = filter.where_sql(list_kind),
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            let match_key: String = r.get(2)?;
            let year: Option<i32> = r.get(3)?;
            Ok(Item {
                kind,
                id: r.get(0)?,
                title: r.get(1)?,
                group: format!("{}|{}", match_key, year.unwrap_or(0)),
                genres: parse_genres(r.get(4)?),
                shelf: r.get(5)?,
                year,
                rating: r.get(6)?,
                added_at: r.get(7)?,
                runtime_mins: r.get(8)?,
                quality_rank: r.get(9)?,
                has_poster: r.get(10)?,
                has_backdrop: r.get(11)?,
                tmdb_id: r.get(12)?,
                lang: r.get(13)?,
                listed: r.get(14)?,
            })
        })?;
        for row in rows {
            out.push(row?);
        }
    }
    Ok(out)
}

/// What the viewer has watched, newest first, with episodes folded into their show.
///
/// A show is credited with its furthest-through episode, and counts as finished if any
/// episode was: five episodes in is a commitment whatever the fraction of the fifth.
pub fn watches(conn: &Connection, profile_id: i64) -> Result<Vec<Watch>> {
    let mut out = Vec::new();
    let mut films = conn.prepare(
        "SELECT item_id,
                CASE WHEN completed = 1 THEN 1.0
                     WHEN duration_secs > 0 THEN CAST(position_secs AS REAL) / duration_secs
                     ELSE 0.0 END,
                updated_at
           FROM watch_progress
          WHERE profile_id = ?1 AND item_kind = 'movie'
          ORDER BY updated_at DESC LIMIT ?2",
    )?;
    for row in films.query_map(params![profile_id, HISTORY_LIMIT], |r| {
        Ok(Watch {
            key: (Kind::Movie, r.get(0)?),
            fraction: r.get::<_, f64>(1)? as f32,
            updated_at: r.get(2)?,
        })
    })? {
        out.push(row?);
    }
    let mut shows = conn.prepare(
        "SELECT e.series_id,
                MAX(CASE WHEN p.completed = 1 THEN 1.0
                         WHEN p.duration_secs > 0
                         THEN CAST(p.position_secs AS REAL) / p.duration_secs
                         ELSE 0.0 END),
                MAX(p.updated_at)
           FROM watch_progress p JOIN episodes e ON e.id = p.item_id
          WHERE p.profile_id = ?1 AND p.item_kind = 'episode'
          GROUP BY e.series_id
          ORDER BY MAX(p.updated_at) DESC LIMIT ?2",
    )?;
    for row in shows.query_map(params![profile_id, HISTORY_LIMIT], |r| {
        Ok(Watch {
            key: (Kind::Series, r.get(0)?),
            fraction: r.get::<_, f64>(1)? as f32,
            updated_at: r.get(2)?,
        })
    })? {
        out.push(row?);
    }
    out.sort_by_key(|w| std::cmp::Reverse(w.updated_at));
    Ok(out)
}

/// The titles this profile has said "not for me" about.
pub fn not_interested(conn: &Connection, profile_id: i64) -> Result<HashSet<Key>> {
    let mut stmt =
        conn.prepare("SELECT item_kind, item_id FROM not_interested WHERE profile_id = ?1")?;
    let rows = stmt.query_map(params![profile_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })?;
    let mut out = HashSet::new();
    for row in rows {
        let (kind, id) = row?;
        if let Some(kind) = kind_of(&kind) {
            out.insert((kind, id));
        }
    }
    Ok(out)
}

/// Say, or take back, "not for me". Returns whether it is set afterwards.
pub fn set_not_interested(
    conn: &Connection,
    profile_id: i64,
    key: Key,
    on: bool,
    now: i64,
) -> Result<bool> {
    if on {
        conn.execute(
            "INSERT OR IGNORE INTO not_interested (profile_id, item_kind, item_id, added_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![profile_id, kind_str(key.0), key.1, now],
        )?;
    } else {
        conn.execute(
            "DELETE FROM not_interested
              WHERE profile_id = ?1 AND item_kind = ?2 AND item_id = ?3",
            params![profile_id, kind_str(key.0), key.1],
        )?;
    }
    Ok(on)
}

fn push_credit(out: &mut HashMap<Key, Vec<Credit>>, kind: &str, id: i64, credit: Credit) {
    if let Some(kind) = kind_of(kind) {
        let list = out.entry((kind, id)).or_default();
        if !list
            .iter()
            .any(|c| c.person == credit.person && c.director == credit.director)
        {
            list.push(credit);
        }
    }
}

const CREDIT_COLUMNS: &str = "c.item_kind, c.item_id, c.person_id, p.name, c.is_cast, c.ord";

fn credit_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<(String, i64, Credit)> {
    let is_cast: bool = r.get(4)?;
    Ok((
        r.get(0)?,
        r.get(1)?,
        Credit {
            person: r.get(2)?,
            name: r.get(3)?,
            director: !is_cast,
            order: if is_cast { r.get(5)? } else { 0 },
        },
    ))
}

/// The leading cast and the directors of these titles.
pub fn credits_for(conn: &Connection, keys: &[Key]) -> Result<HashMap<Key, Vec<Credit>>> {
    let mut out = HashMap::new();
    let sql = format!(
        "SELECT {CREDIT_COLUMNS}
           FROM credits c JOIN people p ON p.tmdb_id = c.person_id
          WHERE c.item_kind = ?1 AND c.item_id = ?2
            AND ((c.is_cast = 1 AND c.ord < ?3) OR (c.is_cast = 0 AND c.role = 'Director'))"
    );
    let mut stmt = conn.prepare(&sql)?;
    for (kind, id) in keys {
        let rows = stmt.query_map(params![kind_str(*kind), id, CAST_DEPTH], credit_from)?;
        for row in rows {
            let (k, i, c) = row?;
            push_credit(&mut out, &k, i, c);
        }
    }
    Ok(out)
}

/// Everything these people are credited on, as lead cast or as director.
///
/// The reverse of `credits_for`, and why `idx_credits_person` exists: one indexed lookup
/// per person rather than a scan of 1.75 million rows.
pub fn filmographies(conn: &Connection, people: &[i64]) -> Result<HashMap<Key, Vec<Credit>>> {
    let mut out = HashMap::new();
    let sql = format!(
        "SELECT {CREDIT_COLUMNS}
           FROM credits c JOIN people p ON p.tmdb_id = c.person_id
          WHERE c.person_id = ?1
            AND ((c.is_cast = 1 AND c.ord < ?2) OR (c.is_cast = 0 AND c.role = 'Director'))"
    );
    let mut stmt = conn.prepare(&sql)?;
    for person in people {
        let rows = stmt.query_map(params![person, CAST_DEPTH], credit_from)?;
        for row in rows {
            let (k, i, c) = row?;
            push_credit(&mut out, &k, i, c);
        }
    }
    Ok(out)
}

/// A TMDB list, best first, as `(kind, tmdb_id)` pairs.
pub type TmdbList = Vec<(Kind, i64)>;

/// A cached TMDB list and when it was fetched, if there is one.
pub fn tmdb_list(conn: &Connection, key: &str) -> Result<Option<(TmdbList, i64)>> {
    let row: Option<(String, i64)> = conn
        .query_row(
            "SELECT items, fetched_at FROM tmdb_lists WHERE list_key = ?1",
            params![key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((raw, at)) = row else {
        return Ok(None);
    };
    let pairs: Vec<(String, i64)> = serde_json::from_str(&raw).unwrap_or_default();
    let items = pairs
        .into_iter()
        .filter_map(|(k, id)| kind_of(&k).map(|k| (k, id)))
        .collect();
    Ok(Some((items, at)))
}

pub fn store_tmdb_list(
    conn: &Connection,
    key: &str,
    items: &[(Kind, i64)],
    now: i64,
) -> Result<()> {
    let pairs: Vec<(&str, i64)> = items.iter().map(|(k, id)| (kind_str(*k), *id)).collect();
    let raw = serde_json::to_string(&pairs).unwrap_or_else(|_| "[]".into());
    conn.execute(
        "INSERT INTO tmdb_lists (list_key, items, fetched_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(list_key) DO UPDATE SET items = excluded.items, fetched_at = excluded.fetched_at",
        params![key, raw, now],
    )?;
    Ok(())
}

/// A fingerprint of everything the feed is built from, cheap enough to ask on every
/// visit to the home screen.
///
/// Equal fingerprints mean an equal feed, so the screen can be answered from the last
/// build instead of reading 33,000 rows and scoring them again -- which is the common
/// case: going to Movies and back. Each part is an aggregate over a small table: what
/// was watched and when, the lists, likes and dismissals, the last refresh of any
/// provider, enrichment, the TMDB cache, and the library filters.
pub fn stamp(conn: &Connection, profile_id: i64, filter: &LibraryFilter) -> Result<String> {
    let parts: [i64; 10] = conn.query_row(
        "SELECT (SELECT COALESCE(MAX(updated_at), 0) FROM watch_progress WHERE profile_id = ?1),
                (SELECT COALESCE(SUM(position_secs), 0) + count(*) FROM watch_progress WHERE profile_id = ?1),
                (SELECT COALESCE(MAX(added_at), 0) + count(*) FROM my_list WHERE profile_id = ?1),
                (SELECT COALESCE(SUM(item_id), 0) FROM my_list WHERE profile_id = ?1),
                (SELECT COALESCE(MAX(added_at), 0) + count(*) FROM favorites WHERE profile_id = ?1),
                (SELECT COALESCE(SUM(item_id), 0) FROM favorites WHERE profile_id = ?1),
                (SELECT COALESCE(SUM(item_id + added_at), 0) + count(*) FROM not_interested WHERE profile_id = ?1),
                (SELECT COALESCE(MAX(last_refresh_at), 0) + count(*) FROM providers),
                (SELECT COALESCE(MAX(attempted_at), 0) + count(*) FROM enrichment),
                (SELECT COALESCE(MAX(fetched_at), 0) + count(*) FROM tmdb_lists)",
        params![profile_id],
        |r| {
            let mut out = [0i64; 10];
            for (i, slot) in out.iter_mut().enumerate() {
                *slot = r.get(i)?;
            }
            Ok(out)
        },
    )?;
    // Who is watching and what they may see: switching to a kids profile, or a lock, must
    // not be answered from a home screen built for somebody else.
    let gate = crate::repo::parental::fingerprint(conn)?;
    Ok(format!("{parts:?}/{filter:?}/{gate}"))
}

/// The key a title's "also liked" list is cached under.
pub fn also_liked_key(kind: Kind, tmdb_id: i64) -> String {
    format!("recs:{}:{tmdb_id}", kind_str(kind))
}

pub const TRENDING_KEY: &str = "trending";

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = crate::open_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO providers (id, name, kind, base_url, created_at)
                  VALUES (1, 'p', 'm3u', 'http://x', 0);
             INSERT OR IGNORE INTO profiles (id, name, created_at) VALUES (1, 'me', 0);",
        )
        .unwrap();
        conn
    }

    fn movie(conn: &Connection, id: i64, title: &str, key: &str, quality_rank: i64, hidden: i64) {
        conn.execute(
            "INSERT INTO movies (id, provider_id, provider_key, title, match_key, year, url,
                                 genres, group_title, rating, quality_rank, hidden, poster,
                                 last_seen_at, tmdb_id)
             VALUES (?1, 1, ?1, ?2, ?3, 2000, 'u', '[\"Crime\"]', 'EN ✪ CRIME', 7.5, ?4, ?5,
                     'p.jpg', 0, ?1 + 100)",
            params![id, title, key, quality_rank, hidden],
        )
        .unwrap();
    }

    #[test]
    fn items_carry_their_features_and_whether_they_may_be_shown() {
        let conn = db();
        movie(&conn, 1, "Heat", "heat", 3, 0);
        movie(&conn, 2, "Hidden", "hidden", 3, 1);
        let items = items(&conn, &LibraryFilter::default()).unwrap();
        let heat = items.iter().find(|i| i.id == 1).unwrap();
        assert_eq!(heat.genres, vec!["Crime"]);
        assert_eq!(heat.group, "heat|2000");
        assert_eq!(heat.shelf.as_deref(), Some("EN ✪ CRIME"));
        assert_eq!(heat.tmdb_id, Some(101));
        assert!(heat.has_poster && heat.listed);
        assert!(
            !items.iter().find(|i| i.id == 2).unwrap().listed,
            "hidden is not listed"
        );
    }

    #[test]
    fn episodes_fold_into_their_show_and_the_newest_comes_first() {
        let conn = db();
        movie(&conn, 1, "Heat", "heat", 3, 0);
        conn.execute_batch(
            "INSERT INTO series (id, provider_id, provider_key, title, match_key, last_seen_at)
                  VALUES (7, 1, 's', 'Show', 'show', 0);
             INSERT INTO episodes (id, series_id, season, episode, title, url)
                  VALUES (70, 7, 1, 1, 'a', 'u'), (71, 7, 1, 2, 'b', 'u');
             INSERT INTO watch_progress VALUES (1, 'episode', 70, 100, 100, 1, 50);
             INSERT INTO watch_progress VALUES (1, 'episode', 71, 10, 100, 0, 90);
             INSERT INTO watch_progress VALUES (1, 'movie', 1, 30, 120, 0, 60);",
        )
        .unwrap();
        let w = watches(&conn, 1).unwrap();
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].key, (Kind::Series, 7));
        assert_eq!(w[0].fraction, 1.0, "an episode finished");
        assert_eq!(w[0].updated_at, 90);
        assert_eq!(w[1].key, (Kind::Movie, 1));
        assert!((w[1].fraction - 0.25).abs() < 1e-6);
    }

    #[test]
    fn not_for_me_is_stored_per_profile_and_can_be_taken_back() {
        let conn = db();
        set_not_interested(&conn, 1, (Kind::Movie, 4), true, 10).unwrap();
        set_not_interested(&conn, 1, (Kind::Movie, 4), true, 11).unwrap();
        assert_eq!(
            not_interested(&conn, 1).unwrap(),
            HashSet::from([(Kind::Movie, 4)])
        );
        set_not_interested(&conn, 1, (Kind::Movie, 4), false, 12).unwrap();
        assert!(not_interested(&conn, 1).unwrap().is_empty());
    }

    #[test]
    fn credits_go_both_ways_lead_cast_and_directors_only() {
        let conn = db();
        movie(&conn, 1, "Heat", "heat", 3, 0);
        movie(&conn, 2, "Collateral", "collateral", 3, 0);
        conn.execute_batch(
            "INSERT INTO people (tmdb_id, name) VALUES (10, 'Michael Mann'), (11, 'Al Pacino'),
                                                       (12, 'An Extra');
             INSERT INTO credits VALUES ('movie', 1, 10, 'Director', 0, 0),
                                        ('movie', 1, 11, 'Vincent', 1, 0),
                                        ('movie', 1, 12, 'Guard', 1, 40),
                                        ('movie', 1, 10, 'Writer', 0, 0),
                                        ('movie', 2, 10, 'Director', 0, 0);",
        )
        .unwrap();
        let mine = credits_for(&conn, &[(Kind::Movie, 1)]).unwrap();
        let heat = &mine[&(Kind::Movie, 1)];
        assert_eq!(
            heat.len(),
            2,
            "the director once, the lead, not the extra: {heat:?}"
        );
        assert!(heat.iter().any(|c| c.director && c.name == "Michael Mann"));
        let mann = filmographies(&conn, &[10]).unwrap();
        assert!(mann.contains_key(&(Kind::Movie, 1)) && mann.contains_key(&(Kind::Movie, 2)));
    }

    #[test]
    fn a_tmdb_list_round_trips_and_is_replaced_by_a_newer_fetch() {
        let conn = db();
        assert!(tmdb_list(&conn, TRENDING_KEY).unwrap().is_none());
        store_tmdb_list(
            &conn,
            TRENDING_KEY,
            &[(Kind::Movie, 1), (Kind::Series, 2)],
            5,
        )
        .unwrap();
        store_tmdb_list(&conn, TRENDING_KEY, &[(Kind::Series, 3)], 9).unwrap();
        let (items, at) = tmdb_list(&conn, TRENDING_KEY).unwrap().unwrap();
        assert_eq!(items, vec![(Kind::Series, 3)]);
        assert_eq!(at, 9);
        assert_eq!(also_liked_key(Kind::Series, 1399), "recs:series:1399");
    }

    #[test]
    fn the_stamp_moves_when_anything_the_feed_reads_moves() {
        let conn = db();
        movie(&conn, 1, "Heat", "heat", 3, 0);
        let f = LibraryFilter::default();
        let a = stamp(&conn, 1, &f).unwrap();
        assert_eq!(a, stamp(&conn, 1, &f).unwrap(), "nothing changed");
        conn.execute("INSERT INTO my_list VALUES (1, 'movie', 1, 5)", [])
            .unwrap();
        let b = stamp(&conn, 1, &f).unwrap();
        assert_ne!(a, b, "a list change");
        set_not_interested(&conn, 1, (Kind::Movie, 1), true, 6).unwrap();
        let c = stamp(&conn, 1, &f).unwrap();
        assert_ne!(b, c, "a dismissal");
        conn.execute(
            "INSERT INTO watch_progress VALUES (1, 'movie', 1, 5, 100, 0, 7)",
            [],
        )
        .unwrap();
        assert_ne!(c, stamp(&conn, 1, &f).unwrap(), "something watched");
        let strict = LibraryFilter {
            english_only: true,
            ..f
        };
        assert_ne!(
            stamp(&conn, 1, &f).unwrap(),
            stamp(&conn, 1, &strict).unwrap(),
            "a filter"
        );
    }
}
