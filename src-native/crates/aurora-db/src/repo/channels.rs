//! Channel persistence and refresh reconciliation.

use aurora_core::model::PlaylistEntry;
use aurora_core::title;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// ISO 639-1, or nothing when the name never said (README §7.3).
    pub lang: Option<String>,
    /// Whether the profile the query was made for has this channel in Favourites.
    /// False when the query named no profile.
    pub favorite: bool,
    /// The provider no longer lists this channel: it was in the library before the last
    /// refresh and not in the refresh itself.
    ///
    /// Kept rather than deleted, because a favourite vanishing out from under somebody
    /// is worse than a row that does not tune (README §4.6) -- but until R-53 that was
    /// the whole of it. `stale` existed as a count in the refresh report and nothing
    /// else, while the wizard told the viewer in as many words that these channels had
    /// been "kept and flagged". Nothing flagged them, no query returned a staleness
    /// column, and nothing in the UI could have drawn one, so a dropped channel sat in
    /// the list looking exactly like a working one and the only sign was a tune that
    /// failed. Being told to look for a badge that does not exist is worse than being
    /// told nothing.
    pub stale: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ChannelFilter {
    pub group: Option<String>,
    pub include_hidden: bool,
    pub radio_only: bool,
    pub limit: Option<u32>,
    pub offset: u32,
    /// The library-wide filters (README §7.3). Default is "show everything", so a
    /// lookup that has to find a channel by id — playback, a recording, a favourite —
    /// is never affected by what the viewer chose to hide from a list.
    pub library: crate::repo::filtering::LibraryFilter,
    /// Whose favourites to report in [`ChannelRow::favorite`], and to filter by when
    /// `favorites_only` is set. Favourites are per profile (README §11), so a list
    /// that names no profile simply has none.
    pub profile_id: Option<i64>,
    /// Show only this profile's favourites. Does nothing without `profile_id`.
    pub favorites_only: bool,
    /// Which named list the favourites come from, or the default when absent
    /// (README §4). The `favorite` flag on every row answers for this list too, so a
    /// heart shown while a list is open means "in *this* list".
    pub favorites_list: Option<String>,
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

/// Channels not seen in the latest refresh, for the refresh report's count.
///
/// README §4.6 keeps them visible with a badge rather than deleting them out from under
/// the user's favourites. The badge is `ChannelRow::stale`, which `list` and `get`
/// compute the same way this does; this function stays because the count in the refresh
/// report is a different question from "is this row dropped" and is asked once per
/// refresh rather than once per row.
pub fn stale(conn: &Connection, provider_id: i64, before: i64) -> Result<Vec<i64>> {
    let mut stmt =
        conn.prepare("SELECT id FROM channels WHERE provider_id = ?1 AND last_seen_at < ?2")?;
    let rows = stmt
        .query_map(params![provider_id, before], |r| r.get(0))?
        .collect::<std::result::Result<Vec<i64>, _>>()?;
    Ok(rows)
}

pub fn list(conn: &Connection, filter: &ChannelFilter) -> Result<Vec<ChannelRow>> {
    // The favourites join is LEFT so an unfavourited channel is still a row, and it is
    // keyed by profile because favourites are per profile. A filter that names no
    // profile binds -1, which matches nothing, so `favorite` comes back false
    // throughout rather than leaking one profile's choices into another's list.
    let mut sql = String::from(
        r#"
SELECT channels.id,
       COALESCE(channels.custom_name, channels.name),
       COALESCE(channels.custom_number, channels.number),
       COALESCE(channels.custom_logo, channels.logo),
       COALESCE(channels.custom_group, channels.group_title),
       channels.tvg_id, channels.epg_channel_id, channels.quality, channels.hidden,
       channels.is_radio, channels.catchup_days, channels.lang_code,
       fav.item_id IS NOT NULL,
       -- Dropped by the provider: in the library before the last refresh of its own
       -- provider, and not in that refresh. Compared against `last_refresh_at` rather
       -- than against "now" because a library nobody has refreshed for a week has not
       -- lost anything -- and `LEFT JOIN`, so a row whose provider is somehow missing
       -- reads as present rather than as dropped.
       (prov.last_refresh_at IS NOT NULL
        AND channels.last_seen_at < prov.last_refresh_at)
FROM channels
LEFT JOIN favorites fav
       ON fav.item_id = channels.id
      AND fav.item_kind = 'channel'
      AND fav.list_name = :list
      AND fav.profile_id = :profile
LEFT JOIN providers prov ON prov.id = channels.provider_id
WHERE 1=1
"#,
    );
    if filter.favorites_only {
        sql.push_str(" AND fav.item_id IS NOT NULL");
    }
    if !filter.include_hidden {
        sql.push_str(" AND channels.hidden = 0");
    }
    if filter.radio_only {
        sql.push_str(" AND channels.is_radio = 1");
    } else {
        sql.push_str(" AND channels.is_radio = 0");
    }
    if filter.group.is_some() {
        sql.push_str(" AND COALESCE(channels.custom_group, channels.group_title) = :group");
    }
    sql.push_str(&filter.library.where_sql(crate::repo::filtering::Kind::Live));
    // Qualified: `favorites` has a `sort_order` of its own, and an unqualified one
    // is ambiguous the moment the join is there.
    //
    // And in the favourites view it is that one that leads. `favorites.sort_order` is
    // what "drag to reorder" writes, and ordering this list by channel number instead
    // meant a viewer could rearrange their favourites and see no change -- the column
    // was written, read by `lists::favorite_channel_ids`, and then overruled by the only
    // query the screen actually uses. Everywhere else the channel number leads, which is
    // what a channel list is for.
    if filter.favorites_only {
        sql.push_str(
            " ORDER BY fav.sort_order,
                       COALESCE(channels.custom_number, channels.number, 999999),
                       channels.name",
        );
    } else {
        sql.push_str(
            " ORDER BY COALESCE(channels.custom_number, channels.number, 999999),
                       channels.sort_order, channels.name",
        );
    }
    if let Some(limit) = filter.limit {
        sql.push_str(&format!(" LIMIT {limit} OFFSET {}", filter.offset));
    }

    let mut stmt = conn.prepare(&sql)?;
    let profile = filter.profile_id.unwrap_or(-1);
    // Bound rather than interpolated: a list name is whatever the viewer typed, and the
    // one thing a name must never be able to do is end the string it is sitting in.
    let list = filter
        .favorites_list
        .as_deref()
        .and_then(crate::repo::lists::clean_list_name)
        .unwrap_or_else(|| crate::repo::lists::FAVORITES.to_string());
    let rows = match &filter.group {
        Some(g) => stmt
            .query_map(
                rusqlite::named_params! { ":group": g, ":profile": profile, ":list": list },
                row_to_channel,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?,
        None => stmt
            .query_map(
                rusqlite::named_params! { ":profile": profile, ":list": list },
                row_to_channel,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?,
    };
    Ok(rows)
}

/// The column order every channel query shares, so [`list`] and [`get`] cannot drift.
fn row_to_channel(r: &rusqlite::Row<'_>) -> rusqlite::Result<ChannelRow> {
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
        lang: r.get(11)?,
        favorite: r.get::<_, i64>(12)? != 0,
        stale: r.get::<_, i64>(13)? != 0,
    })
}

/// The channel groups, with how many channels are in each — after the library filters.
///
/// Filtered here as well as in [`list`], because these counts are the sidebar: a group
/// offering 482 channels that opens onto 120 is two screens disagreeing about what the
/// library holds, and "English only" or collapsed duplicates is exactly the sort of thing
/// that makes them disagree by a factor of four.
pub fn groups(
    conn: &Connection,
    filter: &crate::repo::filtering::LibraryFilter,
) -> Result<Vec<(String, i64)>> {
    let visible = filter.where_sql(crate::repo::filtering::Kind::Live);
    // `is_radio = 0`, because that is what `list` beside this counts.
    //
    // This filtered on `hidden` and on the library filter and not on radio, while `list`
    // appends `AND channels.is_radio = 0` for every filter that is not `radio_only` --
    // so the sidebar counted radio stations and the grid next to it did not, and a group
    // made only of radio would have shown a count and opened onto nothing. Exactly the
    // disagreement this function's own doc comment above says it exists to prevent.
    //
    // Hardcoded rather than taken as an argument because there is one caller,
    // `channels_groups`, and the `channels_list` it is the sidebar for always passes
    // `radio_only: false`: no screen in the app lists radio on its own today. If one is
    // added, both queries have to learn it together -- which is the whole lesson of this
    // finding, and why the clause is spelled out here rather than left implied (R-88).
    let mut stmt = conn.prepare(&format!(
        "SELECT COALESCE(custom_group, group_title) AS g, count(*)
         FROM channels WHERE hidden = 0 AND is_radio = 0 AND g IS NOT NULL{visible}
         GROUP BY g ORDER BY g"
    ))?;
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

/// One channel by id, with none of [`list`]'s filters applied.
///
/// [`list`] paints a screen, so it hides what the viewer asked to hide and answers
/// `is_radio = 0` unless a caller says otherwise. Playback, a scheduled recording and
/// a now/next lookup are the opposite question — they already know which channel they
/// mean — and routing them through the list filter made two things wrong at once.
///
/// It was wrong: `ChannelFilter::default()` excludes radio, so a radio station could
/// not be tuned at all, and a recording on a channel the viewer had hidden failed with
/// "unknown channel". And it was slow: reading one channel's name sorted and
/// materialised the whole table — twenty-two thousand rows on a real subscription, per
/// zap, per recording start, and once per row of the Live TV list.
pub fn get(conn: &Connection, channel_id: i64) -> Result<Option<ChannelRow>> {
    Ok(conn
        .query_row(
            r#"
SELECT channels.id,
       -- Qualified throughout: `providers` has a `name` of its own, so the join below
       -- makes a bare one ambiguous.
       COALESCE(channels.custom_name, channels.name),
       COALESCE(channels.custom_number, channels.number),
       COALESCE(channels.custom_logo, channels.logo),
       COALESCE(channels.custom_group, channels.group_title),
       channels.tvg_id, channels.epg_channel_id, channels.quality, channels.hidden,
       channels.is_radio, channels.catchup_days, channels.lang_code,
       -- Favourites are per profile and this lookup has no profile to ask about.
       -- The callers are playback, recording and now/next, none of which care.
       0,
       (prov.last_refresh_at IS NOT NULL
        AND channels.last_seen_at < prov.last_refresh_at)
FROM channels
LEFT JOIN providers prov ON prov.id = channels.provider_id
WHERE channels.id = ?1
"#,
            params![channel_id],
            row_to_channel,
        )
        .optional()?)
}

pub fn find_by_number(conn: &Connection, number: u32) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            // Ordered, because two channels can hold one number and `LIMIT 1` without an
            // order returns whichever the plan reaches. A real playlist carries the same
            // feed several times — that is what `hide_duplicates` exists for — and the
            // numbers come from the provider, so a collision is ordinary rather than
            // corrupt. Typing 101 twice has to reach the same channel both times, so the
            // tie goes to the lowest id: the first one imported, which is stable across
            // restarts and across a re-import that adds more.
            "SELECT id FROM channels WHERE COALESCE(custom_number, number) = ?1 AND hidden = 0
             ORDER BY id LIMIT 1",
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

    fn profile(conn: &Connection) -> i64 {
        conn.query_row("SELECT id FROM profiles LIMIT 1", [], |r| r.get(0))
            .unwrap()
    }

    /// `Channel.favorite` was declared in `shared/ipc.ts`, filled in by the mock
    /// transport, and never set by the host — so the heart was never filled in the
    /// shipped app. `favoritesOnly` was accepted by the command and then ignored, so
    /// the Favorites button re-fetched the same list.
    #[test]
    fn favorites_are_reported_and_filterable() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let a = entry("CNN HD", Some(202));
        let b = entry("BBC One", Some(101));
        upsert_batch(&mut conn, p, &[("k1".into(), &a), ("k2".into(), &b)], 100).unwrap();
        let me = profile(&conn);

        let cnn = list(&conn, &ChannelFilter::default())
            .unwrap()
            .into_iter()
            .find(|c| c.name == "CNN HD")
            .unwrap()
            .id;
        crate::repo::lists::toggle_favorite(&conn, me, cnn, None, 0).unwrap();

        let mine = ChannelFilter {
            profile_id: Some(me),
            ..Default::default()
        };
        let rows = list(&conn, &mine).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().find(|c| c.id == cnn).unwrap().favorite);
        assert!(!rows.iter().find(|c| c.id != cnn).unwrap().favorite);

        let only = ChannelFilter {
            favorites_only: true,
            ..mine.clone()
        };
        let rows = list(&conn, &only).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, cnn);

        // Toggling off removes it again.
        crate::repo::lists::toggle_favorite(&conn, me, cnn, None, 0).unwrap();
        assert!(list(&conn, &only).unwrap().is_empty());
    }

    /// Favourites are per profile, and a list that names none must not show anybody's.
    #[test]
    fn one_profiles_favorites_do_not_appear_in_anothers() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let a = entry("CNN HD", Some(202));
        upsert_batch(&mut conn, p, &[("k1".into(), &a)], 100).unwrap();
        let me = profile(&conn);
        conn.execute(
            "INSERT INTO profiles (name, is_kids, created_at) VALUES ('Kid',1,0)",
            [],
        )
        .unwrap();
        let other = conn.last_insert_rowid();

        let cnn = list(&conn, &ChannelFilter::default()).unwrap()[0].id;
        crate::repo::lists::toggle_favorite(&conn, me, cnn, None, 0).unwrap();

        let theirs = ChannelFilter {
            profile_id: Some(other),
            ..Default::default()
        };
        assert!(!list(&conn, &theirs).unwrap()[0].favorite);
        assert!(list(
            &conn,
            &ChannelFilter {
                favorites_only: true,
                ..theirs
            }
        )
        .unwrap()
        .is_empty());

        // And with no profile named at all.
        assert!(!list(&conn, &ChannelFilter::default()).unwrap()[0].favorite);
    }

    /// The bug `get` exists for: a radio station could never be played.
    ///
    /// `ChannelFilter::default()` is `radio_only: false`, which the SQL turns into
    /// `AND is_radio = 0`, and every lookup-by-id went through it. So tuning a radio
    /// channel answered "unknown channel", and so did a scheduled recording on a
    /// channel the viewer had hidden from their list.
    #[test]
    fn get_finds_a_hidden_or_radio_channel_that_list_hides() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);

        let mut radio = entry("Jazz FM", Some(700));
        radio.is_radio = true;
        let normal = entry("BBC One", Some(101));
        upsert_batch(
            &mut conn,
            p,
            &[("radio".into(), &radio), ("normal".into(), &normal)],
            0,
        )
        .unwrap();

        let radio_id: i64 = conn
            .query_row("SELECT id FROM channels WHERE is_radio = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        let normal_id: i64 = conn
            .query_row("SELECT id FROM channels WHERE is_radio = 0", [], |r| {
                r.get(0)
            })
            .unwrap();
        set_hidden(&conn, normal_id, true).unwrap();

        // Neither is in a default list…
        let listed = list(&conn, &ChannelFilter::default()).unwrap();
        assert!(listed.is_empty(), "{listed:?}");

        // …and both are findable by id, which is what playback asks.
        let found = get(&conn, radio_id).unwrap().expect("the radio channel");
        assert_eq!(found.name, "Jazz FM");
        assert!(found.is_radio);

        let found = get(&conn, normal_id).unwrap().expect("the hidden channel");
        assert_eq!(found.name, "BBC One");
        assert!(found.hidden);

        assert!(get(&conn, 9999).unwrap().is_none());
    }

    /// `get` must stay a primary-key lookup rather than becoming a filtered list
    /// again: a zap did a full sort of every channel to read one name, and Live TV
    /// did that once per row.
    #[test]
    fn get_is_a_primary_key_lookup_not_a_filtered_list() {
        let conn = crate::open_memory().unwrap();
        let plan: Vec<String> = {
            let mut stmt = conn
                .prepare(
                    "EXPLAIN QUERY PLAN
                     SELECT channels.id FROM channels WHERE channels.id = ?1",
                )
                .unwrap();
            stmt.query_map([1i64], |r| r.get::<_, String>(3))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap()
        };
        let detail = plan.join("; ").to_ascii_lowercase();
        assert!(
            detail.contains("rowid") || detail.contains("using index") || detail.contains("search"),
            "the lookup stopped being indexed: {detail}"
        );
        assert!(
            !detail.contains("scan channels"),
            "the lookup went back to a table scan: {detail}"
        );
    }

    /// `get` and `list` read the same columns in the same order through one mapper;
    /// this is what says so out loud.
    #[test]
    fn get_and_list_agree_about_a_channel() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let mut e = entry("CNN HD", Some(202));
        e.tvg_id = Some("cnn.us".into());
        e.logo = Some("https://example.com/cnn.png".into());
        upsert_batch(&mut conn, p, &[("k1".into(), &e)], 100).unwrap();

        let listed = list(&conn, &ChannelFilter::default()).unwrap();
        let one = listed.first().expect("a channel");
        assert_eq!(&get(&conn, one.id).unwrap().unwrap(), one);
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

    /// The shape that put the badge on every channel in the library.
    ///
    /// `last_refresh_at` used to be written from a second call to the clock, after the
    /// import had finished, while the rows carry the stamp taken before it started. On a
    /// real panel those are minutes apart, so `last_seen_at < last_refresh_at` was true
    /// of *everything* and the whole channel list read "your provider no longer lists
    /// this channel" -- about channels the provider had just listed.
    ///
    /// The caller is what was wrong, and `aurora-app`'s `refresh_stamp.rs` is what holds
    /// it to the fix. This is the rule that makes the fix the right one: a refresh whose
    /// recorded instant is the one its rows carry drops nothing it saw.
    #[test]
    fn a_refresh_that_records_its_own_stamp_drops_nothing_it_saw() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let a = entry("One", Some(1));
        let b = entry("Two", Some(2));

        // A refresh that saw both, stamping them with the instant it began.
        let began = 1_000;
        upsert_batch(&mut conn, p, &[("k1".into(), &a), ("k2".into(), &b)], began).unwrap();

        // Recorded as the instant it *finished*, two minutes later. This is the bug.
        conn.execute(
            "UPDATE providers SET last_refresh_at = ?2 WHERE id = ?1",
            params![p, began + 120],
        )
        .unwrap();
        assert!(
            list(&conn, &ChannelFilter::default())
                .unwrap()
                .iter()
                .all(|r| r.stale),
            "the end-of-refresh stamp should mark the whole library dropped -- if this \
             fails the comparison has changed and this test is no longer about anything"
        );

        // Recorded as the instant it began, which is what the rows carry.
        conn.execute(
            "UPDATE providers SET last_refresh_at = ?2 WHERE id = ?1",
            params![p, began],
        )
        .unwrap();
        for row in list(&conn, &ChannelFilter::default()).unwrap() {
            assert!(
                !row.stale,
                "{} was dropped by the refresh that had just listed it",
                row.name
            );
        }
    }

    /// Migration 16 repairs a library that already has the wrong number stored.
    ///
    /// The code change fixes the next refresh. Without this, an install that has already
    /// refreshed goes on showing the badge on everything until somebody refreshes again.
    #[test]
    fn the_migration_repairs_a_library_already_in_the_bad_state() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let a = entry("One", Some(1));
        let b = entry("Two", Some(2));
        upsert_batch(&mut conn, p, &[("k1".into(), &a), ("k2".into(), &b)], 1_000).unwrap();
        conn.execute(
            "UPDATE providers SET last_refresh_at = 1_120 WHERE id = ?1",
            params![p],
        )
        .unwrap();
        assert!(
            list(&conn, &ChannelFilter::default())
                .unwrap()
                .iter()
                .all(|r| r.stale),
            "the fixture is not in the bad state"
        );

        // The migration's own statement, run against this library.
        let sql = crate::schema::MIGRATIONS
            .iter()
            .find(|m| m.version == 16)
            .expect("migration 16")
            .sql;
        conn.execute_batch(sql).unwrap();

        for row in list(&conn, &ChannelFilter::default()).unwrap() {
            assert!(!row.stale, "{} is still marked dropped", row.name);
        }
    }

    /// And it leaves a genuinely dropped channel dropped.
    ///
    /// The repair takes the newest stamp the provider's rows carry, so a channel the
    /// last refresh really did not see is still behind it.
    #[test]
    fn the_migration_keeps_a_real_drop_dropped() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let a = entry("Kept", Some(1));
        let b = entry("Gone", Some(2));
        // Both seen once...
        upsert_batch(&mut conn, p, &[("k1".into(), &a), ("k2".into(), &b)], 1_000).unwrap();
        // ...then a refresh that saw only one of them, recorded wrongly as its end.
        upsert_batch(&mut conn, p, &[("k1".into(), &a)], 2_000).unwrap();
        conn.execute(
            "UPDATE providers SET last_refresh_at = 2_120 WHERE id = ?1",
            params![p],
        )
        .unwrap();

        let sql = crate::schema::MIGRATIONS
            .iter()
            .find(|m| m.version == 16)
            .expect("migration 16")
            .sql;
        conn.execute_batch(sql).unwrap();

        let rows = list(&conn, &ChannelFilter::default()).unwrap();
        let kept = rows.iter().find(|r| r.name == "Kept").expect("Kept");
        let gone = rows.iter().find(|r| r.name == "Gone").expect("Gone");
        assert!(!kept.stale, "the channel the refresh saw is marked dropped");
        assert!(gone.stale, "the channel it did not see is no longer marked");
    }

    /// And the row says which one it is, which is the half that was missing.
    ///
    /// `stale` counted them for the refresh report and nothing marked the rows, while
    /// the setup wizard told the viewer they had been "kept and flagged". No query
    /// returned a staleness column, so a dropped channel was indistinguishable from a
    /// working one until a tune failed (R-53).
    #[test]
    fn a_dropped_channel_says_so_on_its_own_row() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let a = entry("Keep", Some(1));
        let b = entry("Drop", Some(2));
        upsert_batch(&mut conn, p, &[("k1".into(), &a), ("k2".into(), &b)], 100).unwrap();

        // Nothing has refreshed yet, so nothing is dropped: a library with no refresh
        // behind it has not lost anything.
        for row in list(&conn, &ChannelFilter::default()).unwrap() {
            assert!(!row.stale, "{} is dropped before any refresh", row.name);
        }

        // A refresh that sees only one of the two.
        upsert_batch(&mut conn, p, &[("k1".into(), &a)], 200).unwrap();
        conn.execute(
            "UPDATE providers SET last_refresh_at = 200 WHERE id = ?1",
            params![p],
        )
        .unwrap();

        let rows = list(&conn, &ChannelFilter::default()).unwrap();
        let keep = rows.iter().find(|r| r.name == "Keep").expect("Keep");
        let drop = rows.iter().find(|r| r.name == "Drop").expect("Drop");
        assert!(
            !keep.stale,
            "a channel the refresh listed is marked dropped"
        );
        assert!(
            drop.stale,
            "a channel the refresh did not list is not marked"
        );

        // `get` answers the same way, which is what keeps the two from drifting.
        assert!(get(&conn, drop.id).unwrap().expect("the row").stale);
        assert!(!get(&conn, keep.id).unwrap().expect("the row").stale);
    }

    #[test]
    fn filters_by_group_and_finds_by_number() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let mut sport = entry("Sky Sports", Some(401));
        sport.group = Some("Sports".into());
        let news = entry("CNN", Some(202));
        upsert_batch(
            &mut conn,
            p,
            &[("a".into(), &sport), ("b".into(), &news)],
            0,
        )
        .unwrap();

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

    /// Two channels on one number is ordinary: the numbers come from the provider and a
    /// real playlist carries the same feed more than once. Zapping to 101 has to reach
    /// the same channel every time, which `LIMIT 1` with no order did not guarantee.
    #[test]
    fn two_channels_on_one_number_resolve_the_same_way_every_time() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let east = entry("Regional East", Some(101));
        let west = entry("Regional West", Some(101));
        upsert_batch(
            &mut conn,
            p,
            &[("east".into(), &east), ("west".into(), &west)],
            0,
        )
        .unwrap();

        let rows = list(&conn, &ChannelFilter::default()).unwrap();
        let ids: Vec<i64> = rows.iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), 2, "both feeds should be stored");

        let picked = find_by_number(&conn, 101).unwrap().expect("one of them");
        assert_eq!(
            picked,
            *ids.iter().min().unwrap(),
            "the tie must go to the lowest id, which is the first imported"
        );
        // Repeated, because the defect was an answer that need not be the same twice.
        for _ in 0..5 {
            assert_eq!(find_by_number(&conn, 101).unwrap(), Some(picked));
        }
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
        let g = groups(&conn, &crate::repo::filtering::LibraryFilter::default()).unwrap();
        assert_eq!(g, vec![("News".to_string(), 2)]);
    }

    /// The count in the sidebar and the list it opens have to be the same library.
    ///
    /// They were two different questions: `list` was read through the library filter and
    /// `groups` was not, so "English only" left a group advertising channels it would not
    /// then show — by a factor of four on a subscription that carries every country.
    #[test]
    fn a_group_counts_what_the_filter_would_actually_show() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);
        let english = entry("BBC One", Some(1));
        let french = entry("TF1", Some(2));
        upsert_batch(
            &mut conn,
            p,
            &[("en".into(), &english), ("fr".into(), &french)],
            0,
        )
        .unwrap();
        // A playlist entry carries no language; the classifier writes one onto the row
        // after import (migration 8), which is what the filter reads.
        conn.execute(
            "UPDATE channels SET lang_code = CASE name WHEN 'BBC One' THEN 'en' ELSE 'fr' END",
            [],
        )
        .unwrap();

        let english_only = crate::repo::filtering::LibraryFilter {
            english_only: true,
            ..Default::default()
        };
        let counted = groups(&conn, &english_only).unwrap();
        let listed = list(
            &conn,
            &ChannelFilter {
                library: english_only,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            counted,
            vec![("News".to_string(), listed.len() as i64)],
            "the sidebar promised {counted:?} and the list has {} in it",
            listed.len()
        );
    }

    /// The sidebar's counts do not include channels the grid beside it hides (R-88).
    #[test]
    fn a_group_does_not_count_radio_the_list_will_not_show() {
        let mut conn = crate::open_memory().unwrap();
        let p = provider(&conn);

        // One group holding a TV channel and a radio station, and one holding only radio.
        let mut tv = entry("BBC One", Some(101));
        tv.group = Some("Mixed".into());
        let mut radio = entry("Radio 1", Some(700));
        radio.is_radio = true;
        radio.group = Some("Mixed".into());
        let mut radio_only = entry("Radio 2", Some(701));
        radio_only.is_radio = true;
        radio_only.group = Some("Stations".into());
        upsert_batch(
            &mut conn,
            p,
            &[
                ("t".into(), &tv),
                ("r1".into(), &radio),
                ("r2".into(), &radio_only),
            ],
            0,
        )
        .unwrap();

        let counted = groups(&conn, &Default::default()).unwrap();
        assert_eq!(
            counted,
            vec![("Mixed".to_string(), 1)],
            "a group of only radio must not be offered at all"
        );

        // And the promise holds against the list it is the sidebar for, which is the
        // property rather than the number.
        for (name, count) in counted {
            let listed = list(
                &conn,
                &ChannelFilter {
                    group: Some(name.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                listed.len() as i64,
                count,
                "{name} promised {count} and lists {}",
                listed.len()
            );
        }
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
