//! The local half of sync: turning capture on and off, and reading what it captured.
//!
//! Capture itself is in SQL (migration 18, `schema::SYNC_CAPTURE`): triggers record every
//! change to synced data in `sync_outbox`, in the transaction that made it. This module
//! is the small surface the sync engine (`aurora-sync`) drives on top of that:
//!
//! - [`enable`] turns capture on and enqueues everything that already exists, so the
//!   first push uploads the device's whole personal state, not just what changed after.
//! - [`outbox`] and [`acknowledge`] are the push loop's read and commit.
//! - [`applying`] suspends capture while a pulled change is written, so it is not echoed
//!   straight back as a local edit.
//! - [`observe_clock`] folds a remote clock into this device's, which is what makes the
//!   clock *hybrid logical* rather than wall time.
//!
//! See `docs/APPLE_PORT_AND_SYNC.md` §3.6.

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::Result;

const ENABLED: &str = "enabled";
const APPLYING: &str = "applying";
const HLC: &str = "hlc";

/// One record changed here and not yet pushed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRow {
    pub sync_key: String,
    pub kind: String,
    /// JSON naming the record in device-independent terms.
    pub ident: String,
    /// The hybrid logical clock of its latest local change.
    pub hlc: i64,
    pub attempts: i64,
}

pub fn get_state(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM sync_state WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

pub fn set_state(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_state (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn clear_state(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM sync_state WHERE key = ?1", [key])?;
    Ok(())
}

pub fn is_enabled(conn: &Connection) -> Result<bool> {
    Ok(get_state(conn, ENABLED)?.is_some())
}

/// Turn capture on and enqueue every synced record that already exists.
///
/// Idempotent: enabling twice enqueues the snapshot again, which coalesces onto the rows
/// already there. Returns how many records are now waiting.
pub fn enable(conn: &mut Connection) -> Result<usize> {
    let tx = conn.transaction()?;
    set_state(&tx, ENABLED, "1")?;
    tx.execute_batch(SNAPSHOT)?;
    let waiting: i64 = tx.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get(0))?;
    tx.commit()?;
    Ok(usize::try_from(waiting).unwrap_or(0))
}

/// Turn capture off and forget everything sync knew, keeping the local data itself.
///
/// What "sign out" and "remove this device" come down to locally. The clock is kept: it
/// only ever moves forward, and a device that signs in again must not reissue old values.
pub fn disable(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM sync_state WHERE key <> ?1", [HLC])?;
    tx.execute("DELETE FROM sync_outbox", [])?;
    tx.execute("DELETE FROM sync_shadow", [])?;
    tx.execute("DELETE FROM sync_pending", [])?;
    tx.commit()?;
    Ok(())
}

/// Up to `limit` pending records, oldest change first.
pub fn outbox(conn: &Connection, limit: usize) -> Result<Vec<OutboxRow>> {
    let mut stmt = conn.prepare(
        "SELECT sync_key, kind, ident, hlc, attempts FROM sync_outbox
          ORDER BY hlc LIMIT ?1",
    )?;
    let rows = stmt.query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |r| {
        Ok(OutboxRow {
            sync_key: r.get(0)?,
            kind: r.get(1)?,
            ident: r.get(2)?,
            hlc: r.get(3)?,
            attempts: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn outbox_len(conn: &Connection) -> Result<usize> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get(0))?;
    Ok(usize::try_from(n).unwrap_or(0))
}

/// Remove a pushed record from the outbox -- unless it changed again since it was read.
///
/// The push reads a row, uploads it, then acknowledges. A change in between re-stamps the
/// row with a newer clock; deleting it regardless would lose that change until the record
/// happened to be edited again. Returns whether the row was removed.
pub fn acknowledge(conn: &Connection, sync_key: &str, hlc: i64) -> Result<bool> {
    Ok(conn.execute(
        "DELETE FROM sync_outbox WHERE sync_key = ?1 AND hlc = ?2",
        params![sync_key, hlc],
    )? > 0)
}

/// Count a failed push of this record, so one that can never be sent is visible.
pub fn record_attempt(conn: &Connection, sync_key: &str) -> Result<()> {
    conn.execute(
        "UPDATE sync_outbox SET attempts = attempts + 1 WHERE sync_key = ?1",
        [sync_key],
    )?;
    Ok(())
}

/// Run `f` with capture suspended, for writing changes pulled from another device.
///
/// The flag lives in `sync_state` rather than in a connection-local setting because the
/// triggers can only see tables in their own schema. It is set and cleared inside the
/// caller's transaction, so nothing else ever observes it: commit and rollback both leave
/// it gone.
pub fn applying<T>(
    tx: &rusqlite::Transaction<'_>,
    f: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T>,
) -> Result<T> {
    set_state(tx, APPLYING, "1")?;
    let out = f(tx);
    clear_state(tx, APPLYING)?;
    out
}

/// This device's current clock value.
pub fn clock(conn: &Connection) -> Result<i64> {
    Ok(get_state(conn, HLC)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0))
}

/// Fold a clock value seen on another device's record into this device's clock.
///
/// After this, every local change is stamped later than anything already received, which
/// is the property that makes "last writer wins" mean the writer who saw the other's
/// change, not the one whose wall clock happened to be ahead.
pub fn observe_clock(conn: &Connection, remote: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_state (key, value) VALUES ('hlc', ?1)
         ON CONFLICT (key) DO UPDATE
            SET value = MAX(CAST(sync_state.value AS INTEGER), CAST(excluded.value AS INTEGER))",
        [remote],
    )?;
    Ok(())
}

/// Every synced record that exists, in the same `(kind, ident)` shape the triggers write.
///
/// The idents here must match the triggers' exactly, or a record enqueued by the snapshot
/// and the same record enqueued by an edit would be two different keys.
/// `snapshot_and_triggers_agree` checks every kind.
const SNAPSHOT: &str = r#"
INSERT INTO sync_enqueue
SELECT 'provider', json_object('p', sync_id) FROM providers;

INSERT INTO sync_enqueue
SELECT 'profile', json_object('pf', 'pf:' || sync_id) FROM profiles;

INSERT INTO sync_enqueue VALUES ('parental', json_object());

INSERT INTO sync_enqueue
SELECT 'parental_lock', json_object('kind', l.kind,
    'value', CASE l.kind
        WHEN 'channel' THEN (SELECT key FROM sync_item_keys
                              WHERE kind = 'channel' AND id = CAST(l.value AS INTEGER))
        ELSE l.value END)
  FROM parental_locks l;

INSERT INTO sync_enqueue
SELECT 'progress', json_object(
    'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = w.profile_id),
    'item', (SELECT key FROM sync_item_keys WHERE kind = w.item_kind AND id = w.item_id))
  FROM watch_progress w WHERE w.item_kind <> 'recording';

INSERT INTO sync_enqueue
SELECT 'favorite', json_object(
    'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = f.profile_id),
    'list', f.list_name,
    'item', (SELECT key FROM sync_item_keys WHERE kind = f.item_kind AND id = f.item_id))
  FROM favorites f;

INSERT INTO sync_enqueue
SELECT 'my_list', json_object(
    'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = m.profile_id),
    'item', (SELECT key FROM sync_item_keys WHERE kind = m.item_kind AND id = m.item_id))
  FROM my_list m;

INSERT INTO sync_enqueue
SELECT 'not_interested', json_object(
    'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = n.profile_id),
    'item', (SELECT key FROM sync_item_keys WHERE kind = n.item_kind AND id = n.item_id))
  FROM not_interested n;

INSERT INTO sync_enqueue
SELECT 'series_prefs', json_object(
    'pf',   (SELECT 'pf:' || sync_id FROM profiles WHERE id = s.profile_id),
    'item', (SELECT key FROM sync_item_keys WHERE kind = 'series' AND id = s.series_id))
  FROM series_prefs s;

INSERT INTO sync_enqueue
SELECT 'override', json_object(
    'item', (SELECT key FROM sync_item_keys WHERE kind = 'channel' AND id = c.id))
  FROM channels c
 WHERE c.custom_name IS NOT NULL OR c.custom_number IS NOT NULL
    OR c.custom_logo IS NOT NULL OR c.custom_group  IS NOT NULL OR c.hidden <> 0;

INSERT INTO sync_enqueue
SELECT 'override', json_object(
    'item', (SELECT key FROM sync_item_keys WHERE kind = 'movie' AND id = m.id))
  FROM movies m WHERE m.custom_title IS NOT NULL OR m.hidden <> 0;

INSERT INTO sync_enqueue
SELECT 'override', json_object(
    'item', (SELECT key FROM sync_item_keys WHERE kind = 'series' AND id = s.id))
  FROM series s WHERE s.custom_title IS NOT NULL OR s.hidden <> 0;

INSERT INTO sync_enqueue
SELECT 'rule', json_object('id', id) FROM rules;

INSERT INTO sync_enqueue
SELECT 'epg_map', json_object(
    'item', (SELECT key FROM sync_item_keys WHERE kind = 'channel' AND id = e.channel_id))
  FROM epg_manual_map e;

INSERT INTO sync_enqueue
SELECT 'recording_rule', json_object('id', sync_id) FROM recording_rules;

INSERT INTO sync_enqueue
SELECT 'reminder', json_object(
    'item',  (SELECT key FROM sync_item_keys WHERE kind = 'channel' AND id = r.channel_id),
    'start', r.start,
    'title', r.title)
  FROM reminders r;

INSERT INTO sync_enqueue
SELECT 'mosaic', json_object('name', name) FROM mosaic_layouts;

INSERT INTO sync_enqueue
SELECT 'setting', json_object(
    'scope', CASE s.scope WHEN 'global' THEN 'global'
             ELSE (SELECT 'pf:' || sync_id FROM profiles WHERE id = CAST(s.scope AS INTEGER))
             END,
    'key', s.key)
  FROM settings s;
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{lists, settings};

    fn db() -> Connection {
        let conn = crate::open_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
                  VALUES (1,'Line','xtream','http://panel.example',0);
             INSERT INTO channels (id,provider_id,provider_key,name,match_key,last_seen_at)
                  VALUES (10,1,'501','BBC One','bbcone',0),
                         (11,1,'502','BBC Two','bbctwo',0);
             INSERT INTO movies (id,provider_id,provider_key,title,match_key,url,last_seen_at)
                  VALUES (20,1,'9001','Heat','heat','http://x/heat.mkv',0);
             INSERT INTO series (id,provider_id,provider_key,title,match_key,last_seen_at)
                  VALUES (30,1,'77','Lost','lost',0);
             INSERT INTO episodes (id,series_id,season,episode,url)
                  VALUES (40,30,1,2,'http://x/lost.mkv');",
        )
        .unwrap();
        conn
    }

    fn sync_id(conn: &Connection, table: &str, id: i64) -> String {
        conn.query_row(
            &format!("SELECT sync_id FROM {table} WHERE id = ?1"),
            [id],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn keys(conn: &Connection) -> Vec<String> {
        let mut k: Vec<String> = outbox(conn, 10_000)
            .unwrap()
            .into_iter()
            .map(|r| r.sync_key)
            .collect();
        k.sort();
        k
    }

    #[test]
    fn existing_and_new_rows_get_a_sync_id_of_their_own() {
        let conn = db();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
                  VALUES (2,'Other','m3u','http://m3u.example',0)",
            [],
        )
        .unwrap();
        let a = sync_id(&conn, "providers", 1);
        let b = sync_id(&conn, "providers", 2);
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
        // The default profile from migration 5 got one in the backfill.
        assert_eq!(sync_id(&conn, "profiles", 1).len(), 32);
    }

    #[test]
    fn nothing_is_captured_until_sync_is_turned_on() {
        let conn = db();
        lists::toggle_favorite(&conn, 1, 10, None, 100).unwrap();
        settings::set(&conn, "filter.english_only", &true).unwrap();
        assert_eq!(outbox_len(&conn).unwrap(), 0);
    }

    #[test]
    fn an_edit_is_captured_with_a_device_independent_ident() {
        let mut conn = db();
        enable(&mut conn).unwrap();
        conn.execute("DELETE FROM sync_outbox", []).unwrap();

        lists::toggle_favorite(&conn, 1, 10, None, 100).unwrap();

        let rows = outbox(&conn, 10).unwrap();
        assert_eq!(rows.len(), 1);
        let ident: serde_json::Value = serde_json::from_str(&rows[0].ident).unwrap();
        let prov = sync_id(&conn, "providers", 1);
        let pf = sync_id(&conn, "profiles", 1);
        assert_eq!(ident["item"], format!("ch:{prov}:501"));
        assert_eq!(ident["pf"], format!("pf:{pf}"));
        assert_eq!(ident["list"], "Favorites");
        assert_eq!(rows[0].kind, "favorite");
        assert_eq!(rows[0].sync_key, format!("favorite:{}", rows[0].ident));
    }

    #[test]
    fn repeated_edits_to_one_record_coalesce_and_the_clock_moves_forward() {
        let mut conn = db();
        enable(&mut conn).unwrap();
        conn.execute("DELETE FROM sync_outbox", []).unwrap();

        for pos in 0..50 {
            conn.execute(
                "INSERT INTO watch_progress
                   (profile_id,item_kind,item_id,position_secs,duration_secs,completed,updated_at)
                 VALUES (1,'movie',20,?1,7000,0,?1)
                 ON CONFLICT (profile_id,item_kind,item_id)
                 DO UPDATE SET position_secs = excluded.position_secs",
                [pos],
            )
            .unwrap();
        }
        let rows = outbox(&conn, 10).unwrap();
        assert_eq!(
            rows.len(),
            1,
            "fifty saves of one film are one pending record"
        );
        let first = rows[0].hlc;

        conn.execute("UPDATE watch_progress SET position_secs = 99", [])
            .unwrap();
        let later = outbox(&conn, 10).unwrap()[0].hlc;
        assert!(later > first, "{later} should be after {first}");
        assert_eq!(clock(&conn).unwrap(), later);
    }

    #[test]
    fn episode_keys_name_the_series_and_the_place_in_it() {
        let mut conn = db();
        enable(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO watch_progress VALUES (1,'episode',40,10,100,0,1)",
            [],
        )
        .unwrap();
        let prov = sync_id(&conn, "providers", 1);
        let row = outbox(&conn, 100)
            .unwrap()
            .into_iter()
            .find(|r| r.kind == "progress")
            .unwrap();
        let ident: serde_json::Value = serde_json::from_str(&row.ident).unwrap();
        assert_eq!(ident["item"], format!("ep:sr:{prov}:77:s1e2"));
    }

    #[test]
    fn recording_progress_stays_on_this_pc() {
        let mut conn = db();
        enable(&mut conn).unwrap();
        conn.execute("DELETE FROM sync_outbox", []).unwrap();
        conn.execute(
            "INSERT INTO watch_progress VALUES (1,'recording',5,10,100,0,1)",
            [],
        )
        .unwrap();
        assert_eq!(outbox_len(&conn).unwrap(), 0);
    }

    #[test]
    fn a_catalogue_refresh_enqueues_nothing_but_a_playlist_edit_does() {
        let mut conn = db();
        enable(&mut conn).unwrap();
        conn.execute("DELETE FROM sync_outbox", []).unwrap();

        // What the importer's upsert does: provider columns only.
        conn.execute(
            "UPDATE channels SET name = 'BBC One HD', last_seen_at = 5 WHERE id = 10",
            [],
        )
        .unwrap();
        // An update that names a user column but leaves it as it was.
        conn.execute("UPDATE channels SET hidden = hidden WHERE id = 10", [])
            .unwrap();
        assert_eq!(outbox_len(&conn).unwrap(), 0);

        conn.execute("UPDATE channels SET custom_name = 'One' WHERE id = 10", [])
            .unwrap();
        conn.execute("UPDATE movies SET hidden = 1 WHERE id = 20", [])
            .unwrap();
        let kinds: Vec<_> = outbox(&conn, 10)
            .unwrap()
            .into_iter()
            .map(|r| r.kind)
            .collect();
        assert_eq!(kinds, ["override", "override"]);
    }

    #[test]
    fn a_change_applied_from_another_device_is_not_echoed_back() {
        let mut conn = db();
        enable(&mut conn).unwrap();
        conn.execute("DELETE FROM sync_outbox", []).unwrap();

        let tx = conn.transaction().unwrap();
        applying(&tx, |tx| {
            lists::toggle_favorite(tx, 1, 10, None, 100)?;
            Ok(())
        })
        .unwrap();
        tx.commit().unwrap();

        assert_eq!(outbox_len(&conn).unwrap(), 0);
        assert_eq!(get_state(&conn, APPLYING).unwrap(), None);

        // And capture is back on afterwards.
        lists::toggle_favorite(&conn, 1, 11, None, 100).unwrap();
        assert_eq!(outbox_len(&conn).unwrap(), 1);
    }

    #[test]
    fn a_removal_is_captured_so_it_can_become_a_tombstone() {
        let mut conn = db();
        lists::toggle_my_list(&conn, 1, lists::ItemKind::Movie, 20, 1).unwrap();
        enable(&mut conn).unwrap();
        conn.execute("DELETE FROM sync_outbox", []).unwrap();

        lists::toggle_my_list(&conn, 1, lists::ItemKind::Movie, 20, 2).unwrap();
        let rows = outbox(&conn, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "my_list");
    }

    #[test]
    fn deleting_a_profile_enqueues_the_profile_not_each_of_its_rows() {
        let mut conn = db();
        conn.execute(
            "INSERT INTO profiles (id,name,created_at) VALUES (2,'Kid',0)",
            [],
        )
        .unwrap();
        lists::toggle_favorite(&conn, 2, 10, None, 1).unwrap();
        enable(&mut conn).unwrap();
        conn.execute("DELETE FROM sync_outbox", []).unwrap();

        conn.execute("DELETE FROM profiles WHERE id = 2", [])
            .unwrap();
        let kinds: Vec<_> = outbox(&conn, 10)
            .unwrap()
            .into_iter()
            .map(|r| r.kind)
            .collect();
        assert_eq!(kinds, ["profile"]);
    }

    #[test]
    fn profile_scoped_settings_are_named_by_the_profiles_sync_key() {
        let mut conn = db();
        enable(&mut conn).unwrap();
        conn.execute("DELETE FROM sync_outbox", []).unwrap();
        settings::set_in(
            &conn,
            &settings::profile_scope(1),
            "filter.english_only",
            &true,
        )
        .unwrap();
        let row = &outbox(&conn, 10).unwrap()[0];
        let ident: serde_json::Value = serde_json::from_str(&row.ident).unwrap();
        assert_eq!(
            ident["scope"],
            format!("pf:{}", sync_id(&conn, "profiles", 1))
        );
        assert_eq!(ident["key"], "filter.english_only");
    }

    /// Populate one row of every synced kind.
    fn one_of_everything(conn: &Connection) {
        conn.execute_batch(
            "UPDATE parental SET hide_adult = 0 WHERE id = 1;
             INSERT INTO parental_locks VALUES ('channel','10'), ('category','Adult');
             INSERT INTO watch_progress VALUES (1,'movie',20,10,100,0,1);
             INSERT INTO watch_progress VALUES (1,'episode',40,10,100,0,1);
             INSERT INTO favorites (profile_id,list_name,item_kind,item_id,added_at)
                  VALUES (1,'Sport','channel',10,1), (1,'Liked','movie',20,1);
             INSERT INTO my_list VALUES (1,'series',30,1);
             INSERT INTO not_interested VALUES (1,'movie',20,1);
             INSERT INTO series_prefs (profile_id,series_id,always_skip_intro,updated_at)
                  VALUES (1,30,1,1);
             UPDATE channels SET custom_number = 7 WHERE id = 11;
             UPDATE series SET custom_title = 'LOST' WHERE id = 30;
             INSERT INTO rules (id,definition) VALUES ('r-1','{}');
             INSERT INTO epg_manual_map VALUES (10,'bbc1.uk',1);
             INSERT INTO recording_rules (title,title_key,created_at) VALUES ('News','news',1);
             INSERT INTO reminders (channel_id,title,start,created_at)
                  VALUES (10,'Match',5000,1);
             INSERT INTO mosaic_layouts (name,layout,channels,created_at)
                  VALUES ('Quad','2x2','[10,null]',1);
             INSERT INTO settings VALUES ('global','filter.english_only','true');
             INSERT INTO settings VALUES ('1','gemini.enabled','false');",
        )
        .unwrap();
    }

    #[test]
    fn snapshot_and_triggers_agree() {
        // Captured edit by edit...
        let mut live = db();
        enable(&mut live).unwrap();
        one_of_everything(&live);

        // ...and captured all at once by turning sync on afterwards. The sync ids are
        // random, so copy the live database's across first.
        let mut later = db();
        for (table, id) in [("providers", 1), ("profiles", 1)] {
            later
                .execute(
                    &format!("UPDATE {table} SET sync_id = ?1 WHERE id = ?2"),
                    params![sync_id(&live, table, id), id],
                )
                .unwrap();
        }
        one_of_everything(&later);
        let rr: String = live
            .query_row("SELECT sync_id FROM recording_rules", [], |r| r.get(0))
            .unwrap();
        later
            .execute("UPDATE recording_rules SET sync_id = ?1", [rr])
            .unwrap();
        enable(&mut later).unwrap();

        let live_keys = keys(&live);
        assert_eq!(live_keys, keys(&later));

        let mut kinds: Vec<String> = outbox(&live, 1000)
            .unwrap()
            .into_iter()
            .map(|r| r.kind)
            .collect();
        kinds.sort();
        kinds.dedup();
        assert_eq!(
            kinds,
            [
                "epg_map",
                "favorite",
                "mosaic",
                "my_list",
                "not_interested",
                "override",
                "parental",
                "parental_lock",
                "profile",
                "progress",
                "provider",
                "recording_rule",
                "reminder",
                "rule",
                "series_prefs",
                "setting",
            ],
            "a synced kind is missing from the snapshot or the triggers"
        );
    }

    #[test]
    fn acknowledging_a_record_that_changed_again_keeps_it() {
        let mut conn = db();
        enable(&mut conn).unwrap();
        conn.execute("DELETE FROM sync_outbox", []).unwrap();
        lists::toggle_favorite(&conn, 1, 10, None, 1).unwrap();
        let read = outbox(&conn, 10).unwrap().remove(0);

        // Changed while the push was in flight.
        lists::toggle_favorite(&conn, 1, 10, None, 2).unwrap();
        assert!(!acknowledge(&conn, &read.sync_key, read.hlc).unwrap());
        assert_eq!(outbox_len(&conn).unwrap(), 1);

        let again = outbox(&conn, 10).unwrap().remove(0);
        assert!(acknowledge(&conn, &again.sync_key, again.hlc).unwrap());
        assert_eq!(outbox_len(&conn).unwrap(), 0);
    }

    #[test]
    fn a_remote_clock_ahead_of_ours_pulls_ours_forward() {
        let mut conn = db();
        enable(&mut conn).unwrap();
        let far_ahead = clock(&conn).unwrap() + (1 << 40);
        observe_clock(&conn, far_ahead).unwrap();
        lists::toggle_favorite(&conn, 1, 10, None, 1).unwrap();
        let hlc = outbox(&conn, 100)
            .unwrap()
            .into_iter()
            .find(|r| r.kind == "favorite")
            .unwrap()
            .hlc;
        assert!(hlc > far_ahead);

        // And one behind leaves it alone.
        observe_clock(&conn, 5).unwrap();
        assert!(clock(&conn).unwrap() > far_ahead);
    }

    #[test]
    fn disabling_forgets_sync_state_but_keeps_the_clock_and_the_data() {
        let mut conn = db();
        enable(&mut conn).unwrap();
        lists::toggle_favorite(&conn, 1, 10, None, 1).unwrap();
        let before = clock(&conn).unwrap();
        disable(&mut conn).unwrap();

        assert!(!is_enabled(&conn).unwrap());
        assert_eq!(outbox_len(&conn).unwrap(), 0);
        assert_eq!(clock(&conn).unwrap(), before);
        let favs: i64 = conn
            .query_row("SELECT COUNT(*) FROM favorites", [], |r| r.get(0))
            .unwrap();
        assert_eq!(favs, 1);
    }

    #[test]
    fn sync_lookups_use_the_primary_keys() {
        // A progress save runs these on every write. A scan of a 117k-film table per
        // save would be the whole cost of playback bookkeeping.
        let conn = db();
        let plan: Vec<String> = conn
            .prepare(
                "EXPLAIN QUERY PLAN
                 SELECT key FROM sync_item_keys WHERE kind = 'movie' AND id = 20",
            )
            .unwrap()
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for step in &plan {
            assert!(
                !step.starts_with("SCAN"),
                "item key lookup scans a table: {plan:?}"
            );
        }
    }
}
