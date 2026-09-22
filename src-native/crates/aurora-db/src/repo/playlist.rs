//! The playlist editor's read and write side (README §7.3).
//!
//! One shape for all three lists, because the operations are the same whether the row
//! is a channel, a film or a show: rename it, move it, hide it, put it back. Channels
//! additionally have a number.
//!
//! Every edit writes a `custom_*` column and never the provider's own, so a refresh
//! brings new content without discarding what the viewer decided (README §4.6). "Reset"
//! clears the overrides rather than re-fetching anything.

use rusqlite::{params, params_from_iter, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::repo::filtering::Kind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistRow {
    pub id: i64,
    /// What it is called now — the override if there is one.
    pub name: String,
    /// What the provider calls it, shown when the two differ.
    pub provider_name: String,
    pub number: Option<u32>,
    pub group: Option<String>,
    pub quality: Option<String>,
    pub lang: Option<String>,
    pub hidden: bool,
    /// True when any `custom_*` column is set, so the editor can offer a reset.
    pub edited: bool,
    /// How many other copies of this title exist, collapsed or not.
    pub duplicates: u32,
    pub provider: Option<String>,
}

/// Which rows to show. The editor is the one place hidden entries must be visible,
/// otherwise hiding something is a one-way door.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Show {
    #[default]
    All,
    Visible,
    Hidden,
}

#[derive(Debug, Clone)]
pub struct Query {
    pub kind: Kind,
    pub text: Option<String>,
    pub group: Option<String>,
    pub show: Show,
    /// Only rows that have another copy, so "find the duplicates" is one click.
    pub duplicates_only: bool,
    pub limit: u32,
    pub offset: u32,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            kind: Kind::Live,
            text: None,
            group: None,
            show: Show::All,
            duplicates_only: false,
            limit: 200,
            offset: 0,
        }
    }
}

struct Columns {
    name: &'static str,
    provider_name: &'static str,
    number: &'static str,
    group: &'static str,
    edited: &'static str,
}

fn columns(kind: Kind) -> Columns {
    match kind {
        Kind::Live => Columns {
            name: "COALESCE(t.custom_name, t.name)",
            provider_name: "t.name",
            number: "COALESCE(t.custom_number, t.number)",
            group: "COALESCE(t.custom_group, t.group_title)",
            edited: "(t.custom_name IS NOT NULL OR t.custom_number IS NOT NULL
                      OR t.custom_group IS NOT NULL OR t.custom_logo IS NOT NULL)",
        },
        _ => Columns {
            name: "COALESCE(t.custom_title, t.title)",
            provider_name: "t.title",
            number: "NULL",
            group: "t.group_title",
            edited: "(t.custom_title IS NOT NULL)",
        },
    }
}

/// The year half of a duplicate group. Channels have no year to disagree about.
fn year_match(kind: Kind) -> &'static str {
    match kind {
        Kind::Live => "",
        _ => " AND COALESCE(d.year,0) = COALESCE(t.year,0)",
    }
}

/// Build the WHERE clause and its bound values, so the count and the page agree.
fn conditions(q: &Query) -> (String, Vec<String>) {
    let c = columns(q.kind);
    let mut sql = String::from(" WHERE 1=1");
    let mut args: Vec<String> = Vec::new();

    match q.show {
        Show::All => {}
        Show::Visible => sql.push_str(" AND t.hidden = 0"),
        Show::Hidden => sql.push_str(" AND t.hidden = 1"),
    }
    if let Some(text) = q.text.as_ref().filter(|t| !t.trim().is_empty()) {
        // Escaped, so a typed % is a percent sign and not the whole playlist.
        let like = format!(
            "%{}%",
            text.trim()
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        sql.push_str(&format!(
            " AND ({} LIKE ? ESCAPE '\\' OR {} LIKE ? ESCAPE '\\')",
            c.name, c.provider_name
        ));
        args.push(like.clone());
        args.push(like);
    }
    if let Some(group) = q.group.as_ref().filter(|g| !g.is_empty()) {
        sql.push_str(&format!(" AND {} = ?", c.group));
        args.push(group.clone());
    }
    if q.duplicates_only {
        sql.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM {t} d WHERE d.match_key = t.match_key{year}
                          AND d.id != t.id)",
            t = q.kind.table(),
            year = year_match(q.kind),
        ));
    }
    (sql, args)
}

pub fn list(conn: &Connection, q: &Query) -> Result<Vec<PlaylistRow>> {
    let c = columns(q.kind);
    let (where_sql, args) = conditions(q);
    let table = q.kind.table();
    let order = match q.kind {
        Kind::Live => "COALESCE(t.custom_number, t.number, 999999), t.sort_order, t.name",
        _ => "COALESCE(t.custom_title, t.title) COLLATE NOCASE",
    };
    let sql = format!(
        "SELECT t.id, {name}, {provider_name}, {number}, {group}, t.quality, t.lang_code,
                t.hidden, {edited},
                (SELECT count(*) - 1 FROM {table} d
                  WHERE d.match_key = t.match_key{year}),
                p.name
         FROM {table} t
         LEFT JOIN providers p ON p.id = t.provider_id
         {where_sql}
         ORDER BY {order} LIMIT ? OFFSET ?",
        name = c.name,
        provider_name = c.provider_name,
        number = c.number,
        group = c.group,
        edited = c.edited,
        year = year_match(q.kind),
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut bound: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    for a in args {
        bound.push(Box::new(a));
    }
    bound.push(Box::new(q.limit));
    bound.push(Box::new(q.offset));

    let rows = stmt
        .query_map(params_from_iter(bound.iter()), |r| {
            Ok(PlaylistRow {
                id: r.get(0)?,
                name: r.get(1)?,
                provider_name: r.get(2)?,
                number: r.get::<_, Option<i64>>(3)?.map(|n| n as u32),
                group: r.get(4)?,
                quality: r.get(5)?,
                lang: r.get(6)?,
                hidden: r.get::<_, i64>(7)? != 0,
                edited: r.get::<_, i64>(8)? != 0,
                duplicates: r.get::<_, i64>(9)?.max(0) as u32,
                provider: r.get(10)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// How many rows the same query matches, for the editor's paging and its header.
pub fn count(conn: &Connection, q: &Query) -> Result<i64> {
    let (where_sql, args) = conditions(q);
    let sql = format!("SELECT count(*) FROM {} t {where_sql}", q.kind.table());
    let mut stmt = conn.prepare(&sql)?;
    let mut bound: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    for a in args {
        bound.push(Box::new(a));
    }
    Ok(stmt.query_row(params_from_iter(bound.iter()), |r| r.get(0))?)
}

/// The groups present in one list, with their sizes.
pub fn groups(conn: &Connection, kind: Kind) -> Result<Vec<(String, i64)>> {
    let expr = match kind {
        Kind::Live => "COALESCE(custom_group, group_title)",
        _ => "group_title",
    };
    let sql = format!(
        "SELECT {expr} AS g, count(*) FROM {} WHERE g IS NOT NULL AND g != ''
         GROUP BY g ORDER BY g",
        kind.table()
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// One edit. An absent field is left alone; an empty string or a zero clears the
/// override and the provider's own value shows through again.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    pub name: Option<String>,
    pub number: Option<u32>,
    pub group: Option<String>,
    pub hidden: Option<bool>,
}

fn blank_to_none(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

pub fn update(conn: &Connection, kind: Kind, id: i64, patch: &Patch) -> Result<()> {
    let table = kind.table();
    if let Some(name) = &patch.name {
        let column = if matches!(kind, Kind::Live) {
            "custom_name"
        } else {
            "custom_title"
        };
        conn.execute(
            &format!("UPDATE {table} SET {column} = ?2 WHERE id = ?1"),
            params![id, blank_to_none(name)],
        )?;
    }
    if let Some(number) = patch.number {
        // Channel 0 does not exist, so it is how the editor says "back to the
        // provider's number".
        let value = if number == 0 { None } else { Some(number) };
        if matches!(kind, Kind::Live) {
            conn.execute(
                "UPDATE channels SET custom_number = ?2 WHERE id = ?1",
                params![id, value],
            )?;
        }
    }
    if let Some(group) = &patch.group {
        let column = if matches!(kind, Kind::Live) {
            "custom_group"
        } else {
            "group_title"
        };
        conn.execute(
            &format!("UPDATE {table} SET {column} = ?2 WHERE id = ?1"),
            params![id, blank_to_none(group)],
        )?;
    }
    if let Some(hidden) = patch.hidden {
        conn.execute(
            &format!("UPDATE {table} SET hidden = ?2 WHERE id = ?1"),
            params![id, hidden as i64],
        )?;
    }
    Ok(())
}

/// Hide or show a whole selection at once — the point of a multi-select on a playlist
/// with ten thousand rows in it.
pub fn set_hidden_many(
    conn: &mut Connection,
    kind: Kind,
    ids: &[i64],
    hidden: bool,
) -> Result<usize> {
    if ids.is_empty() {
        return Ok(0);
    }
    let table = kind.table();
    let tx = conn.transaction()?;
    let mut n = 0usize;
    {
        let mut stmt = tx.prepare(&format!("UPDATE {table} SET hidden = ?2 WHERE id = ?1"))?;
        for id in ids {
            n += stmt.execute(params![id, hidden as i64])?;
        }
    }
    tx.commit()?;
    Ok(n)
}

/// Hide every row the current query matches, however many pages of it there are.
pub fn hide_matching(conn: &Connection, q: &Query, hidden: bool) -> Result<usize> {
    let (where_sql, args) = conditions(q);
    let sql = format!(
        "UPDATE {table} SET hidden = ? WHERE id IN
           (SELECT t.id FROM {table} t {where_sql})",
        table = q.kind.table()
    );
    let mut bound: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(hidden as i64)];
    for a in args {
        bound.push(Box::new(a));
    }
    Ok(conn.execute(&sql, params_from_iter(bound.iter()))?)
}

/// Put a selection back the way the provider sends it, overrides and all.
pub fn reset(conn: &mut Connection, kind: Kind, ids: &[i64]) -> Result<usize> {
    if ids.is_empty() {
        return Ok(0);
    }
    let sql = match kind {
        Kind::Live => {
            "UPDATE channels SET custom_name = NULL, custom_number = NULL,
                    custom_group = NULL, custom_logo = NULL, hidden = 0 WHERE id = ?1"
        }
        Kind::Movies => "UPDATE movies SET custom_title = NULL, hidden = 0 WHERE id = ?1",
        Kind::Series => "UPDATE series SET custom_title = NULL, hidden = 0 WHERE id = ?1",
    };
    let tx = conn.transaction()?;
    let mut n = 0usize;
    {
        let mut stmt = tx.prepare(sql)?;
        for id in ids {
            n += stmt.execute(params![id])?;
        }
    }
    tx.commit()?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> Connection {
        let mut conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'Provider One','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        let rows = [
            ("CNN HD", "News", 101, "HD"),
            ("CNN FHD", "News", 102, "FHD"),
            ("Sky Sports", "Sports", 401, "HD"),
            ("FR | TF1", "France", 501, "HD"),
        ];
        for (i, (name, group, number, quality)) in rows.iter().enumerate() {
            conn.execute(
                "INSERT INTO channels (provider_id, provider_key, name, match_key, number,
                                       group_title, quality, last_seen_at)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, 0)",
                params![
                    format!("c{i}"),
                    name,
                    aurora_core::title::match_key(name),
                    number,
                    group,
                    quality
                ],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO movies (provider_id, provider_key, title, match_key, url, last_seen_at)
             VALUES (1,'m0','The Matrix','thematrix','http://x',0),
                    (1,'m1','Amelie','amelie','http://x',0)",
            [],
        )
        .unwrap();
        crate::repo::filtering::reclassify(&mut conn).unwrap();
        conn
    }

    fn q(kind: Kind) -> Query {
        Query {
            kind,
            ..Default::default()
        }
    }

    #[test]
    fn the_editor_sees_hidden_rows_so_hiding_is_reversible() {
        let conn = seeded();
        update(
            &conn,
            Kind::Live,
            1,
            &Patch {
                hidden: Some(true),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(list(&conn, &q(Kind::Live)).unwrap().len(), 4);
        let visible = Query {
            show: Show::Visible,
            ..q(Kind::Live)
        };
        assert_eq!(list(&conn, &visible).unwrap().len(), 3);
        let hidden = Query {
            show: Show::Hidden,
            ..q(Kind::Live)
        };
        let only = list(&conn, &hidden).unwrap();
        assert_eq!(only.len(), 1);
        assert!(only[0].hidden);
    }

    #[test]
    fn the_count_agrees_with_the_page() {
        let conn = seeded();
        let query = Query {
            limit: 2,
            ..q(Kind::Live)
        };
        assert_eq!(list(&conn, &query).unwrap().len(), 2);
        assert_eq!(
            count(&conn, &query).unwrap(),
            4,
            "the count is of matches, not of the page"
        );
    }

    #[test]
    fn renaming_leaves_the_provider_name_visible_underneath() {
        let conn = seeded();
        update(
            &conn,
            Kind::Live,
            1,
            &Patch {
                name: Some("CNN".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let row = &list(&conn, &q(Kind::Live)).unwrap()[0];
        assert_eq!(row.name, "CNN");
        assert_eq!(row.provider_name, "CNN HD");
        assert!(row.edited);
    }

    #[test]
    fn an_empty_value_puts_the_providers_own_back() {
        let conn = seeded();
        let patch = |name: &str| Patch {
            name: Some(name.into()),
            ..Default::default()
        };
        update(&conn, Kind::Live, 1, &patch("Renamed")).unwrap();
        update(&conn, Kind::Live, 1, &patch("   ")).unwrap();
        let row = &list(&conn, &q(Kind::Live)).unwrap()[0];
        assert_eq!(row.name, "CNN HD");
        assert!(!row.edited);
    }

    #[test]
    fn renumbering_reorders_the_list_and_zero_undoes_it() {
        let conn = seeded();
        update(
            &conn,
            Kind::Live,
            3,
            &Patch {
                number: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        let names: Vec<String> = list(&conn, &q(Kind::Live))
            .unwrap()
            .iter()
            .map(|r| r.name.clone())
            .collect();
        assert_eq!(names[0], "Sky Sports");

        update(
            &conn,
            Kind::Live,
            3,
            &Patch {
                number: Some(0),
                ..Default::default()
            },
        )
        .unwrap();
        let row = list(&conn, &q(Kind::Live))
            .unwrap()
            .into_iter()
            .find(|r| r.id == 3)
            .unwrap();
        assert_eq!(row.number, Some(401));
    }

    #[test]
    fn search_matches_both_names_and_treats_a_percent_literally() {
        let conn = seeded();
        update(
            &conn,
            Kind::Live,
            1,
            &Patch {
                name: Some("Cable News".into()),
                ..Default::default()
            },
        )
        .unwrap();

        let by_custom = Query {
            text: Some("cable".into()),
            ..q(Kind::Live)
        };
        assert_eq!(list(&conn, &by_custom).unwrap().len(), 1);
        // The provider's name still finds it, which is how you undo a rename you regret.
        let by_provider = Query {
            text: Some("CNN HD".into()),
            ..q(Kind::Live)
        };
        assert_eq!(list(&conn, &by_provider).unwrap().len(), 1);

        let wildcard = Query {
            text: Some("%".into()),
            ..q(Kind::Live)
        };
        assert_eq!(list(&conn, &wildcard).unwrap().len(), 0);
    }

    #[test]
    fn a_group_filter_narrows_to_that_group() {
        let conn = seeded();
        let news = Query {
            group: Some("News".into()),
            ..q(Kind::Live)
        };
        assert_eq!(list(&conn, &news).unwrap().len(), 2);
        assert_eq!(
            groups(&conn, Kind::Live).unwrap(),
            vec![
                ("France".to_string(), 1),
                ("News".to_string(), 2),
                ("Sports".to_string(), 1)
            ]
        );
    }

    #[test]
    fn moving_a_channel_to_another_group_moves_it_in_the_filter_too() {
        let conn = seeded();
        update(
            &conn,
            Kind::Live,
            3,
            &Patch {
                group: Some("News".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let news = Query {
            group: Some("News".into()),
            ..q(Kind::Live)
        };
        assert_eq!(list(&conn, &news).unwrap().len(), 3);
    }

    #[test]
    fn a_duplicate_count_rides_along_with_each_row() {
        let conn = seeded();
        let rows = list(&conn, &q(Kind::Live)).unwrap();
        let cnn: Vec<u32> = rows
            .iter()
            .filter(|r| r.name.starts_with("CNN"))
            .map(|r| r.duplicates)
            .collect();
        assert_eq!(cnn, vec![1, 1], "each CNN has one other copy");
        let sky = rows.iter().find(|r| r.name == "Sky Sports").unwrap();
        assert_eq!(sky.duplicates, 0);

        let only_dupes = Query {
            duplicates_only: true,
            ..q(Kind::Live)
        };
        assert_eq!(list(&conn, &only_dupes).unwrap().len(), 2);
    }

    #[test]
    fn a_selection_can_be_hidden_and_shown_in_one_go() {
        let mut conn = seeded();
        assert_eq!(
            set_hidden_many(&mut conn, Kind::Live, &[1, 2, 3], true).unwrap(),
            3
        );
        let visible = Query {
            show: Show::Visible,
            ..q(Kind::Live)
        };
        assert_eq!(list(&conn, &visible).unwrap().len(), 1);

        set_hidden_many(&mut conn, Kind::Live, &[1, 2], false).unwrap();
        assert_eq!(list(&conn, &visible).unwrap().len(), 3);
        assert_eq!(
            set_hidden_many(&mut conn, Kind::Live, &[], true).unwrap(),
            0
        );
    }

    #[test]
    fn everything_a_search_matches_can_be_hidden_at_once() {
        let conn = seeded();
        // The point of this on a ten-thousand-row playlist: hide a whole country
        // without paging through it.
        let french = Query {
            text: Some("FR |".into()),
            ..q(Kind::Live)
        };
        assert_eq!(hide_matching(&conn, &french, true).unwrap(), 1);
        let visible = Query {
            show: Show::Visible,
            ..q(Kind::Live)
        };
        assert_eq!(list(&conn, &visible).unwrap().len(), 3);
    }

    #[test]
    fn reset_undoes_every_edit_including_hiding() {
        let mut conn = seeded();
        update(
            &conn,
            Kind::Live,
            1,
            &Patch {
                name: Some("Renamed".into()),
                number: Some(7),
                group: Some("Mine".into()),
                hidden: Some(true),
            },
        )
        .unwrap();
        assert_eq!(reset(&mut conn, Kind::Live, &[1]).unwrap(), 1);

        let row = list(&conn, &q(Kind::Live))
            .unwrap()
            .into_iter()
            .find(|r| r.id == 1)
            .unwrap();
        assert_eq!(row.name, "CNN HD");
        assert_eq!(row.number, Some(101));
        assert_eq!(row.group.as_deref(), Some("News"));
        assert!(!row.hidden);
        assert!(!row.edited);
    }

    #[test]
    fn movies_edit_the_same_way_but_have_no_number() {
        let conn = seeded();
        update(
            &conn,
            Kind::Movies,
            1,
            &Patch {
                name: Some("Matrix, The".into()),
                number: Some(5),
                hidden: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        // Films sort by title, so find it rather than assuming where it landed.
        let row = list(&conn, &q(Kind::Movies))
            .unwrap()
            .into_iter()
            .find(|r| r.id == 1)
            .unwrap();
        assert_eq!(row.name, "Matrix, The");
        assert_eq!(row.provider_name, "The Matrix");
        assert!(row.hidden);
        // A number on a film means nothing, and asking for one is quietly ignored
        // rather than being an error the UI has to handle.
        assert_eq!(row.number, None);
    }

    #[test]
    fn an_edit_survives_a_provider_refresh() {
        let mut conn = seeded();
        update(
            &conn,
            Kind::Live,
            1,
            &Patch {
                name: Some("Mine".into()),
                hidden: Some(true),
                ..Default::default()
            },
        )
        .unwrap();

        // What a refresh does to a channel it has seen before (README §4.6).
        conn.execute(
            "UPDATE channels SET name = 'CNN HD NEW', last_seen_at = 99 WHERE id = 1",
            [],
        )
        .unwrap();
        crate::repo::filtering::reclassify(&mut conn).unwrap();

        let row = list(&conn, &q(Kind::Live))
            .unwrap()
            .into_iter()
            .find(|r| r.id == 1)
            .unwrap();
        assert_eq!(row.name, "Mine");
        assert_eq!(row.provider_name, "CNN HD NEW");
        assert!(row.hidden, "a refresh must not unhide what the viewer hid");
    }

    #[test]
    fn a_row_carries_what_the_filters_judge_it_by() {
        let conn = seeded();
        let rows = list(&conn, &q(Kind::Live)).unwrap();
        let tf1 = rows.iter().find(|r| r.name.contains("TF1")).unwrap();
        assert_eq!(tf1.lang.as_deref(), Some("fr"));
        assert_eq!(tf1.quality.as_deref(), Some("HD"));
        assert_eq!(tf1.provider.as_deref(), Some("Provider One"));
    }
}
