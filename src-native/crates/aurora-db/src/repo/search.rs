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
        .map(|t| t.chars().filter(|c| c.is_alphanumeric()).collect::<String>())
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
}
