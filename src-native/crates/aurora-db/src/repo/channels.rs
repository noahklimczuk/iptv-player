//! Channel persistence and refresh reconciliation.

use aurora_core::model::PlaylistEntry;
use aurora_core::title;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelRow {
    pub id: i64,
    pub name: String,
    pub number: Option<u32>,
    pub logo: Option<String>,
    pub group: Option<String>,
    pub tvg_id: Option<String>,
    pub epg_channel_id: Option<String>,
    pub quality: Option<String>,
    pub hidden: bool,
    pub is_radio: bool,
    pub has_catchup: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ChannelFilter {
    pub group: Option<String>,
    pub include_hidden: bool,
    pub radio_only: bool,
    pub limit: Option<u32>,
    pub offset: u32,
}

/// Insert or update a batch of channels for one provider.
///
/// README §4.6: a refresh must never discard user data. `custom_name`, `custom_number`,
/// `custom_logo`, `custom_group`, `hidden`, and `sort_order` are deliberately absent from the
/// UPDATE list — they are the user's, not the provider's.
pub fn upsert_batch(
    conn: &mut Connection,
    provider_id: i64,
    entries: &[(String, &PlaylistEntry)],
    now: i64,
) -> Result<usize> {
    let tx = conn.transaction()?;
    let mut written = 0usize;
    {
        let mut stmt = tx.prepare(
            r#"
INSERT INTO channels (
    provider_id, provider_key, name, match_key, number, logo, group_title, tvg_id,
    shift_minutes, quality, language, country, is_radio,
    catchup_mode, catchup_source, catchup_days, last_seen_at
) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
ON CONFLICT (provider_id, provider_key) DO UPDATE SET
    name           = excluded.name,
    match_key      = excluded.match_key,
    number         = excluded.number,
    logo           = excluded.logo,
    group_title    = excluded.group_title,
    tvg_id         = excluded.tvg_id,
    shift_minutes  = excluded.shift_minutes,
    quality        = excluded.quality,
    language       = excluded.language,
    country        = excluded.country,
    is_radio       = excluded.is_radio,
    catchup_mode   = excluded.catchup_mode,
    catchup_source = excluded.catchup_source,
    catchup_days   = excluded.catchup_days,
    last_seen_at   = excluded.last_seen_at
"#,
        )?;

        for (key, e) in entries {
            stmt.execute(params![
                provider_id,
                key,
                e.name,
                title::match_key(&e.name),
                e.number,
                e.logo,
                e.group,
                e.tvg_id,
                e.shift_minutes,
                title::detect_quality(&e.name),
                e.language,
                e.country,
                e.is_radio as i32,
                e.catchup.as_ref().map(|c| c.mode.clone()),
                e.catchup.as_ref().and_then(|c| c.source.clone()),
                e.catchup.as_ref().map(|c| c.days).unwrap_or(0),
                now,
            ])?;
            written += 1;
        }
    }
    tx.commit()?;
    Ok(written)
}

/// Channels not seen in the latest refresh. README §4.6 keeps them visible with a badge
/// rather than deleting them out from under the user's favorites.
pub fn stale(conn: &Connection, provider_id: i64, before: i64) -> Result<Vec<i64>> {
    let mut stmt = conn
        .prepare("SELECT id FROM channels WHERE provider_id = ?1 AND last_seen_at < ?2")?;
    let rows = stmt
        .query_map(params![provider_id, before], |r| r.get(0))?
        .collect::<std::result::Result<Vec<i64>, _>>()?;
    Ok(rows)
}

pub fn list(conn: &Connection, filter: &ChannelFilter) -> Result<Vec<ChannelRow>> {
    let mut sql = String::from(
        r#"
SELECT id,
       COALESCE(custom_name, name),
       COALESCE(custom_number, number),
       COALESCE(custom_logo, logo),
       COALESCE(custom_group, group_title),
       tvg_id, epg_channel_id, quality, hidden, is_radio, catchup_days
FROM channels WHERE 1=1
"#,
    );
    if !filter.include_hidden {
        sql.push_str(" AND hidden = 0");
    }
    if filter.radio_only {
        sql.push_str(" AND is_radio = 1");
    } else {
        sql.push_str(" AND is_radio = 0");
    }
    if filter.group.is_some() {
        sql.push_str(" AND COALESCE(custom_group, group_title) = :group");
    }
    sql.push_str(" ORDER BY COALESCE(custom_number, number, 999999), sort_order, name");
    if let Some(limit) = filter.limit {
        sql.push_str(&format!(" LIMIT {limit} OFFSET {}", filter.offset));
    }

    let mut stmt = conn.prepare(&sql)?;
    let map = |r: &rusqlite::Row<'_>| -> rusqlite::Result<ChannelRow> {
        Ok(ChannelRow {
            id: r.get(0)?,
            name: r.get(1)?,
            number: r.get::<_, Option<i64>>(2)?.map(|n| n as u32),
            logo: r.get(3)?,
            group: r.get(4)?,
            tvg_id: r.get(5)?,
            epg_channel_id: r.get(6)?,
            quality: r.get(7)?,
            hidden: r.get::<_, i64>(8)? != 0,
            is_radio: r.get::<_, i64>(9)? != 0,
            has_catchup: r.get::<_, i64>(10)? > 0,
        })
    };

    let rows = match &filter.group {
        Some(g) => stmt
            .query_map(rusqlite::named_params! { ":group": g }, map)?
            .collect::<std::result::Result<Vec<_>, _>>()?,
        None => stmt
            .query_map([], map)?
            .collect::<std::result::Result<Vec<_>, _>>()?,
    };
    Ok(rows)
}

pub fn groups(conn: &Connection) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(custom_group, group_title) AS g, count(*)
         FROM channels WHERE hidden = 0 AND g IS NOT NULL
         GROUP BY g ORDER BY g",
    )?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn set_hidden(conn: &Connection, channel_id: i64, hidden: bool) -> Result<()> {
    conn.execute(
        "UPDATE channels SET hidden = ?2 WHERE id = ?1",
        params![channel_id, hidden as i32],
    )?;
    Ok(())
}

pub fn rename(conn: &Connection, channel_id: i64, name: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE channels SET custom_name = ?2 WHERE id = ?1",
        params![channel_id, name],
    )?;
    Ok(())
}

pub fn renumber(conn: &Connection, channel_id: i64, number: Option<u32>) -> Result<()> {
    conn.execute(
        "UPDATE channels SET custom_number = ?2 WHERE id = ?1",
        params![channel_id, number],
    )?;
    Ok(())
}

pub fn set_epg_mapping(
    conn: &Connection,
    channel_id: i64,
    epg_channel_id: Option<&str>,
    method: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE channels SET epg_channel_id = ?2, epg_match_method = ?3 WHERE id = ?1",
        params![channel_id, epg_channel_id, method],
    )?;
    Ok(())
}

pub fn find_by_number(conn: &Connection, number: u32) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT id FROM channels WHERE COALESCE(custom_number, number) = ?1 AND hidden = 0
             LIMIT 1",
            params![number],
            |r| r.get(0),
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::model::{Catchup, PlaylistEntry};

    fn provider(conn: &Connection) -> i64 {
        conn.execute(
            "INSERT INTO providers (name, kind, base_url, created_at)
             VALUES ('P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn entry(name: &str, number: Option<u32>) -> PlaylistEntry {
        let mut e = PlaylistEntry::new(name, "https://example.com/s.ts");
        e.number = number;
        e.group = Some("News".into());
        e
    }

    #[test]
    fn inserts_and_lists_channels() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let a = entry("CNN HD", Some(202));
        let b = entry("BBC One", Some(101));
        upsert_batch(&mut conn, p, &[("k1".into(), &a), ("k2".into(), &b)], 100).unwrap();

        let rows = list(&conn, &ChannelFilter::default()).unwrap();
        assert_eq!(rows.len(), 2);
        // Ordered by channel number.
        assert_eq!(rows[0].name, "BBC One");
        assert_eq!(rows[0].number, Some(101));
        assert_eq!(rows[1].quality.as_deref(), Some("HD"));
    }

    #[test]
    fn refresh_updates_provider_fields_but_keeps_user_edits() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let first = entry("CNN", Some(202));
        upsert_batch(&mut conn, p, &[("k1".into(), &first)], 100).unwrap();

        let id = list(&conn, &ChannelFilter::default()).unwrap()[0].id;
        rename(&conn, id, Some("My CNN")).unwrap();
        renumber(&conn, id, Some(5)).unwrap();
        set_hidden(&conn, id, true).unwrap();

        // Provider renames the channel on the next refresh.
        let second = entry("CNN International", Some(303));
        upsert_batch(&mut conn, p, &[("k1".into(), &second)], 200).unwrap();

        let rows = list(
            &conn,
            &ChannelFilter {
                include_hidden: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1, "should update in place, not duplicate");
        assert_eq!(rows[0].name, "My CNN", "user rename must survive refresh");
        assert_eq!(rows[0].number, Some(5), "user number must survive refresh");
        assert!(rows[0].hidden, "hidden flag must survive refresh");
    }

    #[test]
    fn hidden_channels_are_excluded_by_default() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let e = entry("X", Some(1));
        upsert_batch(&mut conn, p, &[("k".into(), &e)], 0).unwrap();
        let id = list(&conn, &ChannelFilter::default()).unwrap()[0].id;
        set_hidden(&conn, id, true).unwrap();

        assert!(list(&conn, &ChannelFilter::default()).unwrap().is_empty());
        assert_eq!(
            list(
                &conn,
                &ChannelFilter {
                    include_hidden: true,
                    ..Default::default()
                }
            )
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    fn stale_channels_are_reported_not_deleted() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let a = entry("Keep", Some(1));
        let b = entry("Drop", Some(2));
        upsert_batch(&mut conn, p, &[("k1".into(), &a), ("k2".into(), &b)], 100).unwrap();
        // Second refresh only sees one of them.
        upsert_batch(&mut conn, p, &[("k1".into(), &a)], 200).unwrap();

        let gone = stale(&conn, p, 200).unwrap();
        assert_eq!(gone.len(), 1);
        assert_eq!(list(&conn, &ChannelFilter::default()).unwrap().len(), 2);
    }

    #[test]
    fn filters_by_group_and_finds_by_number() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let mut sport = entry("Sky Sports", Some(401));
        sport.group = Some("Sports".into());
        let news = entry("CNN", Some(202));
        upsert_batch(&mut conn, p, &[("a".into(), &sport), ("b".into(), &news)], 0).unwrap();

        let rows = list(
            &conn,
            &ChannelFilter {
                group: Some("Sports".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Sky Sports");

        assert!(find_by_number(&conn, 202).unwrap().is_some());
        assert!(find_by_number(&conn, 999).unwrap().is_none());
    }

    #[test]
    fn catchup_metadata_round_trips() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let mut e = entry("Archive Ch", Some(1));
        e.catchup = Some(Catchup {
            mode: "shift".into(),
            source: None,
            days: 7,
        });
        upsert_batch(&mut conn, p, &[("k".into(), &e)], 0).unwrap();
        assert!(list(&conn, &ChannelFilter::default()).unwrap()[0].has_catchup);
    }

    #[test]
    fn groups_are_counted() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let a = entry("A", Some(1));
        let b = entry("B", Some(2));
        upsert_batch(&mut conn, p, &[("a".into(), &a), ("b".into(), &b)], 0).unwrap();
        let g = groups(&conn).unwrap();
        assert_eq!(g, vec![("News".to_string(), 2)]);
    }

    #[test]
    fn radio_channels_are_a_separate_view() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let mut radio = entry("Radio 1", Some(700));
        radio.is_radio = true;
        let tv = entry("BBC One", Some(101));
        upsert_batch(&mut conn, p, &[("r".into(), &radio), ("t".into(), &tv)], 0).unwrap();

        assert_eq!(list(&conn, &ChannelFilter::default()).unwrap().len(), 1);
        let radios = list(
            &conn,
            &ChannelFilter {
                radio_only: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(radios.len(), 1);
        assert_eq!(radios[0].name, "Radio 1");
    }
}
