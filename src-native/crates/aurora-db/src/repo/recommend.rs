//! The rows the recommender works from.
//!
//! Split from `aurora_core::recommend` on purpose: the algorithm is pure and has
//! twenty unit tests that never open a database, and everything SQLite-shaped lives
//! here. What this module has to get right is which rows to load, because the library
//! it is asked about holds 117,508 films and 28,528 shows and reading all of them for
//! a rail that shows twenty is not a plan.

use rusqlite::{params, Connection};

use aurora_core::recommend::{Candidate, Kind, Watched};

use crate::Result;

/// How many watched items feed the taste profile.
///
/// Enough to describe a viewer, few enough that the decay has made the tail
/// irrelevant anyway: at a thirty-day half-life, the two hundredth most recent thing
/// somebody watched is contributing rounding error.
const HISTORY_LIMIT: u32 = 200;

/// Everything the viewer has watched enough of to say something, newest first.
///
/// An episode is credited to its series, because "you watched three episodes of The
/// Wire" is a fact about The Wire and not about three separate items — and because
/// the recommender can only recommend a series, never an episode.
pub fn history(conn: &Connection, profile_id: i64) -> Result<Vec<Watched>> {
    let mut out = Vec::new();

    let mut films = conn.prepare(
        "SELECT m.id, m.title, m.genres, m.year, m.rating, m.group_title,
                p.position_secs, p.duration_secs, p.completed, p.updated_at,
                EXISTS (SELECT 1 FROM favorites f
                        WHERE f.profile_id = p.profile_id
                          AND f.item_kind = 'movie' AND f.item_id = m.id)
         FROM watch_progress p
         JOIN movies m ON m.id = p.item_id
         WHERE p.profile_id = ?1 AND p.item_kind = 'movie'
         ORDER BY p.updated_at DESC
         LIMIT ?2",
    )?;
    let rows = films.query_map(params![profile_id, HISTORY_LIMIT], |r| {
        Ok(Watched {
            kind: Kind::Movie,
            id: r.get(0)?,
            title: r.get(1)?,
            genres: parse_genres(r.get::<_, Option<String>>(2)?),
            year: r.get(3)?,
            rating: r.get(4)?,
            category: r.get(5)?,
            fraction: fraction(r.get(6)?, r.get(7)?, r.get(8)?),
            updated_at: r.get(9)?,
            favourite: r.get(10)?,
        })
    })?;
    for row in rows {
        out.push(row?);
    }

    // Episodes fold into their series: the most recent watch stands for the show, and
    // the furthest-through episode decides how much of it counts. Somebody who
    // finished one episode of six has still committed to the show.
    let mut shows = conn.prepare(
        "SELECT s.id, s.title, s.genres, s.year, s.rating, s.group_title,
                MAX(CASE WHEN p.duration_secs > 0
                         THEN CAST(p.position_secs AS REAL) / p.duration_secs
                         ELSE 0 END) AS best,
                MAX(p.completed) AS ever_completed,
                MAX(p.updated_at) AS last_watched,
                EXISTS (SELECT 1 FROM favorites f
                        WHERE f.profile_id = ?1
                          AND f.item_kind = 'series' AND f.item_id = s.id)
         FROM watch_progress p
         JOIN episodes e ON e.id = p.item_id
         JOIN series   s ON s.id = e.series_id
         WHERE p.profile_id = ?1 AND p.item_kind = 'episode'
         GROUP BY s.id
         ORDER BY last_watched DESC
         LIMIT ?2",
    )?;
    let rows = shows.query_map(params![profile_id, HISTORY_LIMIT], |r| {
        let best: f64 = r.get(6)?;
        let completed: i64 = r.get(7)?;
        Ok(Watched {
            kind: Kind::Series,
            id: r.get(0)?,
            title: r.get(1)?,
            genres: parse_genres(r.get::<_, Option<String>>(2)?),
            year: r.get(3)?,
            rating: r.get(4)?,
            category: r.get(5)?,
            fraction: if completed != 0 {
                1.0
            } else {
                best.clamp(0.0, 1.0) as f32
            },
            updated_at: r.get(8)?,
            favourite: r.get(9)?,
        })
    })?;
    for row in rows {
        out.push(row?);
    }

    out.sort_by_key(|w| std::cmp::Reverse(w.updated_at));
    out.truncate(HISTORY_LIMIT as usize);
    Ok(out)
}

/// Everything the viewer has any history with, which is everything to leave out.
///
/// Half-watched counts: Continue Watching already has a rail of its own, and putting
/// the same title in both is a waste of the only row anybody reads.
pub fn already_seen(conn: &Connection, profile_id: i64) -> Result<Vec<(Kind, i64)>> {
    let mut out = Vec::new();

    let mut films = conn.prepare(
        "SELECT item_id FROM watch_progress
         WHERE profile_id = ?1 AND item_kind = 'movie'",
    )?;
    for id in films.query_map(params![profile_id], |r| r.get::<_, i64>(0))? {
        out.push((Kind::Movie, id?));
    }

    let mut shows = conn.prepare(
        "SELECT DISTINCT e.series_id FROM watch_progress p
         JOIN episodes e ON e.id = p.item_id
         WHERE p.profile_id = ?1 AND p.item_kind = 'episode'",
    )?;
    for id in shows.query_map(params![profile_id], |r| r.get::<_, i64>(0))? {
        out.push((Kind::Series, id?));
    }

    Ok(out)
}

/// The pool to rank, drawn towards what this viewer actually watches.
///
/// The naive version of this — "the best-rated N rows" — is what it started as, and
/// it does not work on a real subscription. 117,508 films and 28,528 shows cannot all
/// be loaded to pick twenty, so something has to be sliced off; but a panel that
/// publishes no ratings (which is all of them until TMDB enrichment has run) makes
/// that slice effectively arbitrary, and an arbitrary slice of 146,000 rows need not
/// contain a single title from the shelves this viewer watches. Measured against a
/// real panel: three films watched off "NL ✪ FILMS [SUB]" (7,578 titles) and not one
/// of them was in the pool.
///
/// So retrieval is directed by the taste rather than independent of it: each shelf and
/// genre the viewer actually watches contributes its own best rows, and a smaller
/// general pool is added so there is somewhere to discover from. Deduplicated by
/// `(kind, id)`, because a title can arrive through both.
///
/// `hidden` rows are excluded throughout: the viewer has said they do not want to see
/// them, and a recommendation is the most annoying possible place for one to return.
pub fn candidates(
    conn: &Connection,
    categories: &[String],
    genres: &[String],
    per_bucket: u32,
    explore: u32,
) -> Result<Vec<Candidate>> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    for (table, kind) in [("movies", Kind::Movie), ("series", Kind::Series)] {
        // Each shelf the viewer watches. `group_title` is compared case-insensitively
        // because the taste profile lowercases everything it stores.
        for category in categories {
            let sql = format!(
                "{} WHERE hidden = 0 AND LOWER(group_title) = ?1 {ORDER} LIMIT ?2",
                select(table),
                ORDER = ORDER_BY,
            );
            collect(
                conn,
                &sql,
                (category, per_bucket),
                kind,
                &mut seen,
                &mut out,
            )?;
        }

        // Each genre they watch. The stored form is a JSON array, so this is the same
        // `LIKE` the browse filter uses.
        for genre in genres {
            let sql = format!(
                "{} WHERE hidden = 0 AND LOWER(genres) LIKE '%\"' || ?1 || '\"%' {ORDER} LIMIT ?2",
                select(table),
                ORDER = ORDER_BY,
            );
            collect(conn, &sql, (genre, per_bucket), kind, &mut seen, &mut out)?;
        }

        // And something to discover from, so a viewer is not walled into what they
        // have already told us. Newest first rather than best-rated: without TMDB
        // there are no ratings to sort by, and "what arrived most recently" is at
        // least a real ordering.
        let sql = format!(
            "{} WHERE hidden = 0
               AND ( (genres IS NOT NULL AND genres != '' AND genres != '[]')
                     OR (group_title IS NOT NULL AND group_title != '') )
             {ORDER} LIMIT ?1",
            select(table),
            ORDER = ORDER_BY,
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![explore], |r| read(kind, r))?;
        for row in rows {
            let c = row?;
            if seen.insert((kind, c.id)) {
                out.push(c);
            }
        }
    }
    Ok(out)
}

/// Best-rated first where a rating exists, newest first otherwise.
const ORDER_BY: &str = "ORDER BY COALESCE(rating, 0) DESC, COALESCE(added_at, 0) DESC, id";

fn select(table: &str) -> String {
    format!("SELECT id, title, genres, group_title, year, rating, added_at FROM {table}")
}

fn read(kind: Kind, r: &rusqlite::Row<'_>) -> rusqlite::Result<Candidate> {
    Ok(Candidate {
        kind: Some(kind),
        id: r.get(0)?,
        title: r.get(1)?,
        genres: parse_genres(r.get::<_, Option<String>>(2)?),
        category: r.get(3)?,
        year: r.get(4)?,
        rating: r.get(5)?,
        added_at: r.get(6)?,
    })
}

fn collect(
    conn: &Connection,
    sql: &str,
    args: (&String, u32),
    kind: Kind,
    seen: &mut std::collections::HashSet<(Kind, i64)>,
    out: &mut Vec<Candidate>,
) -> Result<()> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![args.0, args.1], |r| read(kind, r))?;
    for row in rows {
        let c = row?;
        if seen.insert((kind, c.id)) {
            out.push(c);
        }
    }
    Ok(())
}

/// How much of something was watched, as a fraction.
///
/// `completed` wins outright: a viewer who finished a film and then started it again
/// is at 2% of it, and that is not what they think of it.
fn fraction(position: i64, duration: i64, completed: i64) -> f32 {
    if completed != 0 {
        return 1.0;
    }
    if duration <= 0 {
        return 0.0;
    }
    (position as f32 / duration as f32).clamp(0.0, 1.0)
}

/// Genres are stored as a JSON array. Anything else is treated as none rather than as
/// an error: a malformed row should cost that row its genres, not the whole rail.
fn parse_genres(raw: Option<String>) -> Vec<String> {
    raw.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|g| !g.trim().is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_memory;

    fn seeded() -> Connection {
        let conn = open_memory().unwrap();
        conn.execute_batch(
            r#"
            INSERT INTO providers (id, name, kind, base_url, created_at)
                VALUES (1, 'p', 'm3u', 'http://x', 0);
            -- Migration 5 already creates a default profile, and it may well be id 1.
            INSERT OR IGNORE INTO profiles (id, name, created_at) VALUES (1, 'Me', 0);

            INSERT INTO movies (id, provider_id, provider_key, title, match_key, url,
                                genres, year, rating, added_at, last_seen_at)
            VALUES (1, 1, 'a', 'Alien',  'alien',  'u', '["Sci-Fi","Horror"]', 1979, 8.1, 100, 0),
                   (2, 1, 'b', 'Heat',   'heat',   'u', '["Crime"]',           1995, 8.3, 200, 0),
                   (3, 1, 'c', 'Nothing','nothing','u', NULL,                  2000, 9.9, 300, 0),
                   (4, 1, 'd', 'Empty',  'empty',  'u', '[]',                  2000, 9.9, 300, 0);

            INSERT INTO series (id, provider_id, provider_key, title, match_key,
                                genres, year, rating, added_at, last_seen_at)
            VALUES (1, 1, 's1', 'The Wire', 'the wire', '["Crime","Drama"]', 2002, 9.3, 150, 0);

            INSERT INTO episodes (id, series_id, season, episode, title, url)
            VALUES (10, 1, 1, 1, 'The Target', 'u'),
                   (11, 1, 1, 2, 'The Detail', 'u');
            "#,
        )
        .unwrap();
        conn
    }

    #[test]
    fn a_watched_film_comes_back_with_everything_needed_to_score_it() {
        let conn = seeded();
        conn.execute(
            "INSERT INTO watch_progress (profile_id, item_kind, item_id, position_secs,
                                         duration_secs, completed, updated_at)
             VALUES (1, 'movie', 1, 3000, 6000, 0, 500)",
            [],
        )
        .unwrap();

        let history = history(&conn, 1).unwrap();
        assert_eq!(history.len(), 1);
        let item = &history[0];
        assert_eq!(item.title, "Alien");
        assert_eq!(item.genres, vec!["Sci-Fi", "Horror"]);
        assert_eq!(item.year, Some(1979));
        assert!((item.fraction - 0.5).abs() < 0.001, "{}", item.fraction);
        assert!(!item.favourite);
    }

    /// Finishing something and then starting it again must not read as 2% watched.
    #[test]
    fn completed_beats_whatever_the_position_says() {
        let conn = seeded();
        conn.execute(
            "INSERT INTO watch_progress (profile_id, item_kind, item_id, position_secs,
                                         duration_secs, completed, updated_at)
             VALUES (1, 'movie', 1, 120, 6000, 1, 500)",
            [],
        )
        .unwrap();
        assert_eq!(history(&conn, 1).unwrap()[0].fraction, 1.0);
    }

    #[test]
    fn a_favourite_is_reported_as_one() {
        let conn = seeded();
        conn.execute(
            "INSERT INTO watch_progress (profile_id, item_kind, item_id, position_secs,
                                         duration_secs, completed, updated_at)
             VALUES (1, 'movie', 2, 100, 6000, 0, 500)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO favorites (profile_id, list_name, item_kind, item_id, added_at)
             VALUES (1, 'Favorites', 'movie', 2, 0)",
            [],
        )
        .unwrap();
        assert!(history(&conn, 1).unwrap()[0].favourite);
    }

    /// Three episodes of one show is a fact about the show, not about three items —
    /// and the recommender can only ever recommend the show.
    #[test]
    fn episodes_are_credited_to_their_series() {
        let conn = seeded();
        conn.execute_batch(
            "INSERT INTO watch_progress (profile_id, item_kind, item_id, position_secs,
                                         duration_secs, completed, updated_at)
             VALUES (1, 'episode', 10, 3000, 3000, 1, 400),
                    (1, 'episode', 11,  600, 3000, 0, 900);",
        )
        .unwrap();

        let history = history(&conn, 1).unwrap();
        assert_eq!(history.len(), 1, "one show, not two episodes: {history:?}");
        let show = &history[0];
        assert_eq!(show.kind, Kind::Series);
        assert_eq!(show.id, 1);
        assert_eq!(show.title, "The Wire");
        assert_eq!(show.fraction, 1.0, "one episode was finished");
        assert_eq!(show.updated_at, 900, "the most recent watch stands for it");
    }

    #[test]
    fn history_is_newest_first_across_both_kinds() {
        let conn = seeded();
        conn.execute_batch(
            "INSERT INTO watch_progress (profile_id, item_kind, item_id, position_secs,
                                         duration_secs, completed, updated_at)
             VALUES (1, 'movie',    1, 6000, 6000, 1, 100),
                    (1, 'episode', 10, 3000, 3000, 1, 900);",
        )
        .unwrap();
        let history = history(&conn, 1).unwrap();
        assert_eq!(history[0].title, "The Wire");
        assert_eq!(history[1].title, "Alien");
    }

    #[test]
    fn another_profiles_viewing_is_not_this_ones() {
        let conn = seeded();
        conn.execute(
            "INSERT OR IGNORE INTO profiles (id, name, created_at) VALUES (2, 'Someone else', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO watch_progress (profile_id, item_kind, item_id, position_secs,
                                         duration_secs, completed, updated_at)
             VALUES (2, 'movie', 1, 6000, 6000, 1, 500)",
            [],
        )
        .unwrap();
        assert!(history(&conn, 1).unwrap().is_empty());
        assert!(already_seen(&conn, 1).unwrap().is_empty());
    }

    #[test]
    fn everything_with_any_history_counts_as_seen() {
        let conn = seeded();
        conn.execute_batch(
            "INSERT INTO watch_progress (profile_id, item_kind, item_id, position_secs,
                                         duration_secs, completed, updated_at)
             VALUES (1, 'movie',    1,  60, 6000, 0, 100),
                    (1, 'episode', 10, 100, 3000, 0, 200);",
        )
        .unwrap();
        let mut seen = already_seen(&conn, 1).unwrap();
        seen.sort();
        assert_eq!(seen, vec![(Kind::Movie, 1), (Kind::Series, 1)]);
    }

    /// The bug this covers: genres come from TMDB enrichment, which needs an API key.
    /// Demanding one meant that on a freshly imported library the pool was empty and
    /// the rail never appeared at all.
    #[test]
    fn a_row_with_only_a_provider_category_is_still_a_candidate() {
        let conn = seeded();
        conn.execute(
            "UPDATE movies SET genres = NULL, group_title = 'EN ✪ BOX OFFICE' WHERE id = 3",
            [],
        )
        .unwrap();
        let pool = candidates(&conn, &[], &[], 100, 100).unwrap();
        let shelved = pool.iter().find(|c| c.title == "Nothing").unwrap();
        assert!(shelved.genres.is_empty());
        assert_eq!(shelved.category.as_deref(), Some("EN ✪ BOX OFFICE"));
    }

    #[test]
    fn the_shelf_a_watched_film_came_off_is_reported_with_it() {
        let conn = seeded();
        conn.execute(
            "UPDATE movies SET group_title = 'EN ✪ BOX OFFICE' WHERE id = 1",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO watch_progress (profile_id, item_kind, item_id, position_secs,
                                         duration_secs, completed, updated_at)
             VALUES (1, 'movie', 1, 6000, 6000, 1, 500)",
            [],
        )
        .unwrap();
        assert_eq!(
            history(&conn, 1).unwrap()[0].category.as_deref(),
            Some("EN ✪ BOX OFFICE")
        );
    }

    #[test]
    fn candidates_skip_rows_with_nothing_to_go_on() {
        let conn = seeded();
        let pool = candidates(&conn, &[], &[], 100, 100).unwrap();
        let titles: Vec<_> = pool.iter().map(|c| c.title.as_str()).collect();
        assert!(titles.contains(&"Alien"));
        assert!(titles.contains(&"The Wire"));
        assert!(
            !titles.contains(&"Nothing") && !titles.contains(&"Empty"),
            "no genres and no category is nothing to go on: {titles:?}"
        );
    }

    #[test]
    fn a_hidden_row_is_never_recommended() {
        // The viewer has already said they do not want to see it, and a
        // recommendation is the most annoying possible place for it to come back.
        let conn = seeded();
        conn.execute("UPDATE movies SET hidden = 1 WHERE id = 1", [])
            .unwrap();
        let pool = candidates(&conn, &[], &[], 100, 100).unwrap();
        assert!(!pool.iter().any(|c| c.title == "Alien"));
    }

    #[test]
    fn the_explore_pool_is_capped_per_kind_and_takes_the_best_first() {
        let conn = seeded();
        let pool = candidates(&conn, &[], &[], 1, 1).unwrap();
        // One film and one show, and the film is the better-rated of the two eligible.
        assert_eq!(pool.len(), 2, "{pool:?}");
        assert_eq!(pool[0].title, "Heat", "8.3 outranks 8.1");
        assert_eq!(pool[1].title, "The Wire");
    }

    /// The bug this exists for, measured on a real panel: three films watched off a
    /// 7,578-title shelf, and not one of that shelf was in the pool — because with no
    /// ratings to order by, "the best N rows" is an arbitrary N.
    #[test]
    fn a_shelf_the_viewer_watches_is_fetched_even_when_the_general_pool_misses_it() {
        let conn = seeded();
        conn.execute(
            "UPDATE movies SET group_title = 'NL ✪ FILMS [SUB]', rating = NULL WHERE id = 1",
            [],
        )
        .unwrap();

        // An explore pool of one cannot reach it: `Heat` is better rated.
        let general = candidates(&conn, &[], &[], 0, 1).unwrap();
        assert!(!general.iter().any(|c| c.title == "Alien"), "{general:?}");

        // Naming the shelf does.
        let directed = candidates(&conn, &["nl ✪ films [sub]".into()], &[], 10, 1).unwrap();
        assert!(
            directed.iter().any(|c| c.title == "Alien"),
            "the watched shelf must be represented: {directed:?}"
        );
    }

    #[test]
    fn a_genre_the_viewer_watches_is_fetched_the_same_way() {
        let conn = seeded();
        conn.execute("UPDATE movies SET rating = NULL WHERE id = 1", [])
            .unwrap();
        let directed = candidates(&conn, &[], &["horror".into()], 10, 0).unwrap();
        assert!(
            directed.iter().any(|c| c.title == "Alien"),
            "Alien is tagged Horror: {directed:?}"
        );
    }

    #[test]
    fn a_title_reachable_two_ways_appears_once() {
        let conn = seeded();
        conn.execute(
            "UPDATE movies SET group_title = 'EN ✪ BOX OFFICE' WHERE id = 1",
            [],
        )
        .unwrap();
        let pool = candidates(
            &conn,
            &["en ✪ box office".into()],
            &["horror".into()],
            10,
            10,
        )
        .unwrap();
        let aliens = pool.iter().filter(|c| c.title == "Alien").count();
        assert_eq!(aliens, 1, "{pool:?}");
    }

    #[test]
    fn a_malformed_genre_column_costs_that_row_its_genres_and_nothing_else() {
        let conn = seeded();
        conn.execute("UPDATE movies SET genres = 'not json' WHERE id = 1", [])
            .unwrap();
        let pool = candidates(&conn, &[], &[], 100, 100).unwrap();
        let alien = pool.iter().find(|c| c.title == "Alien").unwrap();
        assert!(alien.genres.is_empty());
        assert!(pool.iter().any(|c| c.title == "Heat"), "the rest survives");
    }

    #[test]
    fn a_profile_that_has_watched_nothing_is_not_an_error() {
        let conn = seeded();
        assert!(history(&conn, 1).unwrap().is_empty());
        assert!(already_seen(&conn, 1).unwrap().is_empty());
    }
}
