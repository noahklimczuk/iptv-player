//! FTS5-backed unified search (README §10).

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub kind: String,
    pub ref_id: i64,
    pub title: String,
    pub subtitle: Option<String>,
}

pub fn index(
    conn: &mut Connection,
    rows: &[(String, i64, String, Option<String>)],
) -> Result<usize> {
    let tx = conn.transaction()?;
    let mut n = 0;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO search_index (title, subtitle, kind, ref_id) VALUES (?1,?2,?3,?4)",
        )?;
        for (kind, ref_id, title, subtitle) in rows {
            stmt.execute(params![title, subtitle, kind, ref_id])?;
            n += 1;
        }
    }
    tx.commit()?;
    Ok(n)
}

pub fn clear_kind(conn: &Connection, kind: &str) -> Result<usize> {
    Ok(conn.execute("DELETE FROM search_index WHERE kind = ?1", params![kind])?)
}

/// Escape user input so FTS5 treats it as a literal prefix query rather than syntax.
/// Without this, typing `"` or `*` or `AND` throws a parse error at the user.
fn to_fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|t| {
            t.chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
        })
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\"*"))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" AND "))
    }
}

pub fn query(conn: &Connection, text: &str, limit: u32) -> Result<Vec<SearchHit>> {
    let Some(q) = to_fts_query(text) else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(
        "SELECT kind, ref_id, title, subtitle FROM search_index
         WHERE search_index MATCH ?1 ORDER BY rank LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![q, limit], |r| {
            Ok(SearchHit {
                kind: r.get(0)?,
                ref_id: r.get(1)?,
                title: r.get(2)?,
                subtitle: r.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// What the command palette shows, grouped the way it renders (README §10).
///
/// The FTS index only carries the library — channels, movies and series — because
/// those change on sync. Programmes and people are queried live: an EPG row is only
/// interesting relative to the current time, and reindexing a week of listings on
/// every refresh would be stale within the hour.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    pub channels: Vec<SearchHit>,
    pub on_now: Vec<SearchHit>,
    pub upcoming: Vec<SearchHit>,
    pub movies: Vec<SearchHit>,
    pub series: Vec<SearchHit>,
    pub people: Vec<SearchHit>,
}

/// Escape a LIKE pattern so `%` and `_` typed by the user match themselves.
fn to_like(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let escaped = trimmed
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    Some(format!("%{escaped}%"))
}

pub fn grouped(conn: &Connection, text: &str, now: i64, limit: u32) -> Result<SearchResults> {
    let mut out = SearchResults::default();

    for hit in query(conn, text, limit)? {
        match hit.kind.as_str() {
            "channel" => out.channels.push(hit),
            "movie" => out.movies.push(hit),
            "series" => out.series.push(hit),
            _ => {}
        }
    }

    let Some(like) = to_like(text) else {
        return Ok(out);
    };

    // Programmes still in the guide, on the channels this install actually carries.
    // `ref_id` is the channel, not the programme: what a viewer wants from a search
    // hit is to watch the thing, and that needs the channel.
    let mut stmt = conn.prepare(
        "SELECT c.id, COALESCE(c.custom_name, c.name), p.title, p.start, p.stop
         FROM epg_programmes p
         JOIN channels c ON c.epg_channel_id = p.channel_id
         WHERE p.title LIKE ?1 ESCAPE '\\' AND p.stop > ?2 AND c.hidden = 0
         ORDER BY p.start LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(params![like, now, limit], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (channel_id, channel_name, title, start, _stop) in rows {
        let hit = SearchHit {
            kind: "programme".into(),
            ref_id: channel_id,
            title,
            subtitle: Some(channel_name),
        };
        if start <= now {
            out.on_now.push(hit);
        } else {
            out.upcoming.push(hit);
        }
    }

    // Cast and crew, but only those attached to something in the library.
    let mut stmt = conn.prepare(
        "SELECT pe.tmdb_id, pe.name FROM people pe
         WHERE pe.name LIKE ?1 ESCAPE '\\'
           AND EXISTS (SELECT 1 FROM credits cr WHERE cr.person_id = pe.tmdb_id)
         ORDER BY pe.name LIMIT ?2",
    )?;
    out.people = stmt
        .query_map(params![like, limit], |r| {
            Ok(SearchHit {
                kind: "person".into(),
                ref_id: r.get(0)?,
                title: r.get(1)?,
                subtitle: None,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> Connection {
        let mut conn = crate::open_memory().unwrap();
        index(
            &mut conn,
            &[
                ("movie".into(), 1, "The Matrix".into(), Some("1999".into())),
                ("movie".into(), 2, "Matrix Reloaded".into(), None),
                ("channel".into(), 3, "BBC One".into(), None),
                ("series".into(), 4, "Breaking Bad".into(), None),
                ("movie".into(), 5, "Amélie".into(), None),
            ],
        )
        .unwrap();
        conn
    }

    #[test]
    fn finds_by_prefix() {
        let hits = query(&seeded(), "matr", 10).unwrap();
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn multiple_terms_are_anded() {
        let hits = query(&seeded(), "matrix reloaded", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].ref_id, 2);
    }

    #[test]
    fn search_is_diacritic_insensitive() {
        // The FTS table is built with remove_diacritics=2.
        let hits = query(&seeded(), "amelie", 10).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn results_span_kinds() {
        let conn = seeded();
        assert_eq!(query(&conn, "bbc", 10).unwrap()[0].kind, "channel");
        assert_eq!(query(&conn, "breaking", 10).unwrap()[0].kind, "series");
    }

    #[test]
    fn fts_syntax_in_user_input_does_not_error() {
        let conn = seeded();
        // Each of these is valid FTS5 syntax that would otherwise throw.
        for nasty in ["\"", "*", "AND", "matrix OR", "NEAR(", "^", "a\"b*"] {
            let r = query(&conn, nasty, 10);
            assert!(r.is_ok(), "query {nasty:?} errored: {:?}", r.err());
        }
    }

    #[test]
    fn empty_and_punctuation_only_queries_return_nothing() {
        let conn = seeded();
        assert!(query(&conn, "", 10).unwrap().is_empty());
        assert!(query(&conn, "   ", 10).unwrap().is_empty());
        assert!(query(&conn, "!!!", 10).unwrap().is_empty());
    }

    #[test]
    fn clearing_one_kind_leaves_the_others() {
        let mut conn = seeded();
        clear_kind(&conn, "movie").unwrap();
        assert!(query(&conn, "matrix", 10).unwrap().is_empty());
        assert_eq!(query(&conn, "bbc", 10).unwrap().len(), 1);
        index(&mut conn, &[("movie".into(), 9, "The Matrix".into(), None)]).unwrap();
        assert_eq!(query(&conn, "matrix", 10).unwrap().len(), 1);
    }

    #[test]
    fn limit_is_respected() {
        let conn = seeded();
        assert_eq!(query(&conn, "matr", 1).unwrap().len(), 1);
    }

    const NOW: i64 = 1_760_000_000;

    /// A library with an EPG and a cast, so every group the palette renders has a
    /// source behind it.
    fn seeded_grouped() -> Connection {
        let conn = seeded();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO channels (id, provider_id, provider_key, name, match_key,
                                   epg_channel_id, last_seen_at)
             VALUES (3, 1, 'c1', 'BBC One', 'bbcone', 'bbcone.uk', 0)",
            [],
        )
        .unwrap();
        // Hidden channels are not offered, so neither are their listings.
        conn.execute(
            "INSERT INTO channels (id, provider_id, provider_key, name, match_key,
                                   epg_channel_id, hidden, last_seen_at)
             VALUES (4, 1, 'c2', 'BBC Two', 'bbctwo', 'bbctwo.uk', 1, 0)",
            [],
        )
        .unwrap();
        for (ch, title, start, stop) in [
            ("bbcone.uk", "Matrix Night", NOW - 600, NOW + 600),
            ("bbcone.uk", "Matrix Night Encore", NOW + 3600, NOW + 7200),
            ("bbcone.uk", "Matrix Night Repeat", NOW - 7200, NOW - 3600),
            ("bbctwo.uk", "Matrix Night Elsewhere", NOW - 600, NOW + 600),
        ] {
            conn.execute(
                "INSERT INTO epg_programmes (channel_id, start, stop, title)
                 VALUES (?1, ?2, ?3, ?4)",
                params![ch, start, stop, title],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO people (tmdb_id, name) VALUES (7, 'Keanu Reeves'), (8, 'Nobody Attached')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO credits (item_kind, item_id, person_id, role, is_cast, ord)
             VALUES ('movie', 1, 7, 'Neo', 1, 0)",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn results_arrive_in_the_groups_the_palette_renders() {
        let r = grouped(&seeded_grouped(), "matrix", NOW, 10).unwrap();
        assert_eq!(r.movies.len(), 2);
        assert!(r.channels.is_empty());
        assert!(r.series.is_empty());
        // One on air, one later today, and the one that already finished is gone.
        assert_eq!(r.on_now.len(), 1);
        assert_eq!(r.on_now[0].title, "Matrix Night");
        assert_eq!(r.upcoming.len(), 1);
        assert_eq!(r.upcoming[0].title, "Matrix Night Encore");
    }

    #[test]
    fn a_programme_hit_points_at_its_channel_so_it_can_be_watched() {
        let r = grouped(&seeded_grouped(), "matrix", NOW, 10).unwrap();
        assert_eq!(r.on_now[0].ref_id, 3);
        assert_eq!(r.on_now[0].subtitle.as_deref(), Some("BBC One"));
        assert_eq!(r.on_now[0].kind, "programme");
    }

    #[test]
    fn listings_on_a_hidden_channel_are_not_offered() {
        let r = grouped(&seeded_grouped(), "matrix", NOW, 10).unwrap();
        let all: Vec<&str> = r
            .on_now
            .iter()
            .chain(r.upcoming.iter())
            .map(|h| h.title.as_str())
            .collect();
        assert!(!all.contains(&"Matrix Night Elsewhere"), "{all:?}");
    }

    #[test]
    fn only_people_attached_to_something_are_offered() {
        let r = grouped(&seeded_grouped(), "keanu", NOW, 10).unwrap();
        assert_eq!(r.people.len(), 1);
        assert_eq!(r.people[0].ref_id, 7);
        // Indexed but credited to nothing: offering them leads nowhere.
        assert!(grouped(&seeded_grouped(), "nobody", NOW, 10)
            .unwrap()
            .people
            .is_empty());
    }

    #[test]
    fn like_wildcards_typed_by_a_user_are_literal() {
        let conn = seeded_grouped();
        // Without escaping, "%" matches every programme there is.
        let r = grouped(&conn, "%", NOW, 10).unwrap();
        assert!(r.on_now.is_empty(), "{:?}", r.on_now);
        assert!(r.upcoming.is_empty());
    }

    #[test]
    fn an_empty_query_is_empty_in_every_group() {
        let r = grouped(&seeded_grouped(), "   ", NOW, 10).unwrap();
        assert_eq!(r, SearchResults::default());
    }
}
