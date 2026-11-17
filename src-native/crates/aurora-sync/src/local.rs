//! Reading records out of this device's tables and writing them back.
//!
//! A record names things by sync key (`pf:…`, `mv:{provider}:{stream}`); the tables use
//! row ids. This module is the edge where one becomes the other (§3.3). One `read` and one
//! `write` per kind, each naming its columns once, so what a record carries for a kind is
//! visible in one place.
//!
//! A record that refers to something this device has not imported -- a favourite of a
//! channel from a provider not refreshed here yet -- cannot be written. [`write`] says
//! [`Applied::Parked`] and the engine keeps it in `sync_pending` until the next import.
//! It is never dropped: "a refresh never destroys user data" applies to sync too.

use rusqlite::types::Value as Sql;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::error::{Result, SyncError};
use crate::record::{Fields, Record};

/// Every kind this build understands. A record of any other kind came from a newer app
/// and is left alone rather than misapplied.
pub const KINDS: &[&str] = &[
    "provider",
    "profile",
    "parental",
    "parental_lock",
    "progress",
    "favorite",
    "my_list",
    "not_interested",
    "series_prefs",
    "override",
    "rule",
    "epg_map",
    "recording_rule",
    "reminder",
    "mosaic",
    "setting",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    Done,
    /// Refers to something not imported here yet. Retry after the next import.
    Parked,
}

const PROVIDER: &[&str] = &[
    "name",
    "kind",
    "base_url",
    "username",
    "user_agent",
    "referrer",
    "enabled",
    "max_connections",
    "sort_order",
];
const PROFILE: &[&str] = &[
    "name",
    "avatar",
    "is_kids",
    "pin_hash",
    "max_rating",
    "allow_unrated",
    "daily_limit_min",
    "sort_order",
];
const PARENTAL: &[&str] = &["master_pin", "hide_adult", "lock_settings"];
const PROGRESS: &[&str] = &["position_secs", "duration_secs", "completed", "updated_at"];
const SERIES_PREFS: &[&str] = &[
    "always_skip_intro",
    "always_skip_recap",
    "autoplay_next",
    "updated_at",
];
const CHANNEL_OVERRIDE: &[&str] = &[
    "custom_name",
    "custom_number",
    "custom_logo",
    "custom_group",
    "hidden",
];
const TITLE_OVERRIDE: &[&str] = &["custom_title", "hidden"];
const RULE: &[&str] = &["enabled", "definition", "sort_order"];
const RECORDING_RULE: &[&str] = &[
    "title",
    "title_key",
    "new_only",
    "weekdays",
    "around_local_minute",
    "time_slack_secs",
    "pre_padding_secs",
    "post_padding_secs",
    "keep_episodes",
    "priority",
    "enabled",
    "created_at",
];

// ---------------------------------------------------------------------------------------
// Values

fn to_json(v: Sql) -> Value {
    match v {
        Sql::Null => Value::Null,
        Sql::Integer(i) => json!(i),
        Sql::Real(f) => json!(f),
        Sql::Text(s) => Value::String(s),
        // Nothing synced is a blob; this keeps the conversion total.
        Sql::Blob(b) => Value::String(String::from_utf8_lossy(&b).into_owned()),
    }
}

fn to_sql(v: Option<&Value>) -> Sql {
    match v {
        None | Some(Value::Null) => Sql::Null,
        Some(Value::Bool(b)) => Sql::Integer(i64::from(*b)),
        Some(Value::Number(n)) => match n.as_i64() {
            Some(i) => Sql::Integer(i),
            None => Sql::Real(n.as_f64().unwrap_or_default()),
        },
        Some(Value::String(s)) => Sql::Text(s.clone()),
        Some(other) => Sql::Text(other.to_string()),
    }
}

fn str_of<'a>(ident: &'a Value, field: &str) -> Result<&'a str> {
    ident
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| SyncError::Malformed(format!("ident has no {field}: {ident}")))
}

/// Read `columns` of the one row `sql` selects, as fields.
fn select_fields(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
    columns: &[&str],
) -> Result<Option<Fields>> {
    Ok(conn
        .query_row(sql, params, |r| {
            let mut f = Fields::new();
            for (i, c) in columns.iter().enumerate() {
                f.insert((*c).to_string(), to_json(r.get::<_, Sql>(i)?));
            }
            Ok(f)
        })
        .optional()?)
}

fn values(fields: &Fields, columns: &[&str]) -> Vec<Sql> {
    columns.iter().map(|c| to_sql(fields.get(*c))).collect()
}

fn set_clause(columns: &[&str], first_param: usize) -> String {
    columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{c} = ?{}", i + first_param))
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------------------
// Keys <-> local ids

fn provider_id(conn: &Connection, sync_id: &str) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT id FROM providers WHERE sync_id = ?1",
            [sync_id],
            |r| r.get(0),
        )
        .optional()?)
}

fn profile_id(conn: &Connection, pf: &str) -> Result<Option<i64>> {
    let Some(sync_id) = pf.strip_prefix("pf:") else {
        return Ok(None);
    };
    Ok(conn
        .query_row(
            "SELECT id FROM profiles WHERE sync_id = ?1",
            [sync_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// The local kind and id of a catalogue item, if it has been imported here.
pub fn item(conn: &Connection, key: &str) -> Result<Option<(&'static str, i64)>> {
    if let Some(rest) = key.strip_prefix("ep:sr:") {
        let Some((provider, rest)) = rest.split_once(':') else {
            return Ok(None);
        };
        let Some((series_key, place)) = rest.rsplit_once(':') else {
            return Ok(None);
        };
        let Some((season, episode)) = place
            .strip_prefix('s')
            .and_then(|p| p.split_once('e'))
            .and_then(|(s, e)| Some((s.parse::<i64>().ok()?, e.parse::<i64>().ok()?)))
        else {
            return Ok(None);
        };
        let id = conn
            .query_row(
                "SELECT e.id FROM episodes e
                   JOIN series s    ON s.id = e.series_id
                   JOIN providers p ON p.id = s.provider_id
                  WHERE p.sync_id = ?1 AND s.provider_key = ?2
                    AND e.season = ?3 AND e.episode = ?4",
                params![provider, series_key, season, episode],
                |r| r.get(0),
            )
            .optional()?;
        return Ok(id.map(|id| ("episode", id)));
    }
    let (kind, table, rest) = if let Some(r) = key.strip_prefix("ch:") {
        ("channel", "channels", r)
    } else if let Some(r) = key.strip_prefix("mv:") {
        ("movie", "movies", r)
    } else if let Some(r) = key.strip_prefix("sr:") {
        ("series", "series", r)
    } else {
        return Ok(None);
    };
    let Some((provider, provider_key)) = rest.split_once(':') else {
        return Ok(None);
    };
    let id = conn
        .query_row(
            &format!(
                "SELECT t.id FROM {table} t JOIN providers p ON p.id = t.provider_id
                  WHERE p.sync_id = ?1 AND t.provider_key = ?2"
            ),
            params![provider, provider_key],
            |r| r.get(0),
        )
        .optional()?;
    Ok(id.map(|id| (kind, id)))
}

/// The sync key of a local catalogue item.
pub fn item_key(conn: &Connection, kind: &str, id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT key FROM sync_item_keys WHERE kind = ?1 AND id = ?2",
            params![kind, id],
            |r| r.get(0),
        )
        .optional()?)
}

/// The `settings.scope` a record's scope names: `global`, or a local profile id.
fn setting_scope(conn: &Connection, scope: &str) -> Result<Option<String>> {
    if scope == "global" {
        return Ok(Some("global".into()));
    }
    Ok(profile_id(conn, scope)?.map(|id| id.to_string()))
}

/// Resolve `(profile, item)` idents, the shape most per-profile kinds share.
fn profile_and_item(conn: &Connection, ident: &Value) -> Result<Option<(i64, &'static str, i64)>> {
    let Some(pf) = profile_id(conn, str_of(ident, "pf")?)? else {
        return Ok(None);
    };
    let Some((kind, id)) = item(conn, str_of(ident, "item")?)? else {
        return Ok(None);
    };
    Ok(Some((pf, kind, id)))
}

// ---------------------------------------------------------------------------------------
// Read

/// The fields of the local row a record names, or `None` when there is no such row.
pub fn read(conn: &Connection, kind: &str, ident: &Value) -> Result<Option<Fields>> {
    match kind {
        "provider" => select_fields(
            conn,
            &format!(
                "SELECT {} FROM providers WHERE sync_id = ?1",
                PROVIDER.join(", ")
            ),
            [str_of(ident, "p")?],
            PROVIDER,
        ),
        "profile" => {
            let Some(id) = profile_id(conn, str_of(ident, "pf")?)? else {
                return Ok(None);
            };
            select_fields(
                conn,
                &format!("SELECT {} FROM profiles WHERE id = ?1", PROFILE.join(", ")),
                [id],
                PROFILE,
            )
        }
        "parental" => select_fields(
            conn,
            &format!("SELECT {} FROM parental WHERE id = 1", PARENTAL.join(", ")),
            [],
            PARENTAL,
        ),
        "parental_lock" => {
            let Some(value) = lock_value(conn, ident)? else {
                return Ok(None);
            };
            select_fields(
                conn,
                "SELECT 1 FROM parental_locks WHERE kind = ?1 AND value = ?2",
                params![str_of(ident, "kind")?, value],
                &[],
            )
        }
        "progress" => {
            let Some((pf, k, id)) = profile_and_item(conn, ident)? else {
                return Ok(None);
            };
            select_fields(
                conn,
                &format!(
                    "SELECT {} FROM watch_progress
                      WHERE profile_id = ?1 AND item_kind = ?2 AND item_id = ?3",
                    PROGRESS.join(", ")
                ),
                params![pf, k, id],
                PROGRESS,
            )
        }
        "favorite" => {
            let Some((pf, k, id)) = profile_and_item(conn, ident)? else {
                return Ok(None);
            };
            select_fields(
                conn,
                "SELECT sort_order, added_at FROM favorites
                  WHERE profile_id = ?1 AND list_name = ?2 AND item_kind = ?3 AND item_id = ?4",
                params![pf, str_of(ident, "list")?, k, id],
                &["sort_order", "added_at"],
            )
        }
        "my_list" | "not_interested" => {
            let Some((pf, k, id)) = profile_and_item(conn, ident)? else {
                return Ok(None);
            };
            select_fields(
                conn,
                &format!(
                    "SELECT added_at FROM {kind}
                      WHERE profile_id = ?1 AND item_kind = ?2 AND item_id = ?3"
                ),
                params![pf, k, id],
                &["added_at"],
            )
        }
        "series_prefs" => {
            let Some((pf, _, id)) = profile_and_item(conn, ident)? else {
                return Ok(None);
            };
            select_fields(
                conn,
                &format!(
                    "SELECT {} FROM series_prefs WHERE profile_id = ?1 AND series_id = ?2",
                    SERIES_PREFS.join(", ")
                ),
                params![pf, id],
                SERIES_PREFS,
            )
        }
        "override" => {
            let Some((k, id)) = item(conn, str_of(ident, "item")?)? else {
                return Ok(None);
            };
            let (table, cols) = override_table(k)?;
            select_fields(
                conn,
                &format!("SELECT {} FROM {table} WHERE id = ?1", cols.join(", ")),
                [id],
                cols,
            )
        }
        "rule" => select_fields(
            conn,
            &format!("SELECT {} FROM rules WHERE id = ?1", RULE.join(", ")),
            [str_of(ident, "id")?],
            RULE,
        ),
        "epg_map" => {
            let Some((_, id)) = item(conn, str_of(ident, "item")?)? else {
                return Ok(None);
            };
            select_fields(
                conn,
                "SELECT epg_channel_id, created_at FROM epg_manual_map WHERE channel_id = ?1",
                [id],
                &["epg_channel_id", "created_at"],
            )
        }
        "recording_rule" => {
            let row: Option<(Fields, Option<i64>)> = conn
                .query_row(
                    &format!(
                        "SELECT {}, channel_id FROM recording_rules WHERE sync_id = ?1",
                        RECORDING_RULE.join(", ")
                    ),
                    [str_of(ident, "id")?],
                    |r| {
                        let mut f = Fields::new();
                        for (i, c) in RECORDING_RULE.iter().enumerate() {
                            f.insert((*c).to_string(), to_json(r.get::<_, Sql>(i)?));
                        }
                        Ok((f, r.get(RECORDING_RULE.len())?))
                    },
                )
                .optional()?;
            let Some((mut f, channel)) = row else {
                return Ok(None);
            };
            let channel = match channel {
                Some(id) => item_key(conn, "channel", id)?.map_or(Value::Null, Value::String),
                None => Value::Null,
            };
            f.insert("channel".into(), channel);
            Ok(Some(f))
        }
        "reminder" => {
            let Some((_, id)) = item(conn, str_of(ident, "item")?)? else {
                return Ok(None);
            };
            select_fields(
                conn,
                "SELECT lead_secs, created_at FROM reminders
                  WHERE channel_id = ?1 AND start = ?2 AND title = ?3",
                params![
                    id,
                    ident.get("start").and_then(Value::as_i64),
                    str_of(ident, "title")?
                ],
                &["lead_secs", "created_at"],
            )
        }
        "mosaic" => {
            let row: Option<(String, String, i64)> = conn
                .query_row(
                    "SELECT layout, channels, created_at FROM mosaic_layouts WHERE name = ?1",
                    [str_of(ident, "name")?],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let Some((layout, channels, created_at)) = row else {
                return Ok(None);
            };
            let ids: Vec<Option<i64>> = serde_json::from_str(&channels).unwrap_or_default();
            let mut keys = Vec::with_capacity(ids.len());
            for id in ids {
                keys.push(match id {
                    Some(id) => item_key(conn, "channel", id)?.map_or(Value::Null, Value::String),
                    None => Value::Null,
                });
            }
            let mut f = Fields::new();
            f.insert("layout".into(), Value::String(layout));
            f.insert("channels".into(), Value::Array(keys));
            f.insert("created_at".into(), json!(created_at));
            Ok(Some(f))
        }
        "setting" => {
            let Some(scope) = setting_scope(conn, str_of(ident, "scope")?)? else {
                return Ok(None);
            };
            select_fields(
                conn,
                "SELECT value FROM settings WHERE scope = ?1 AND key = ?2",
                params![scope, str_of(ident, "key")?],
                &["value"],
            )
        }
        other => Err(SyncError::Malformed(format!("unknown kind {other}"))),
    }
}

fn override_table(kind: &str) -> Result<(&'static str, &'static [&'static str])> {
    match kind {
        "channel" => Ok(("channels", CHANNEL_OVERRIDE)),
        "movie" => Ok(("movies", TITLE_OVERRIDE)),
        "series" => Ok(("series", TITLE_OVERRIDE)),
        other => Err(SyncError::Malformed(format!("no overrides for {other}"))),
    }
}

/// The stored `parental_locks.value`: a local channel id for a channel lock.
fn lock_value(conn: &Connection, ident: &Value) -> Result<Option<String>> {
    let value = str_of(ident, "value")?;
    if str_of(ident, "kind")? == "channel" {
        return Ok(item(conn, value)?.map(|(_, id)| id.to_string()));
    }
    Ok(Some(value.to_string()))
}

// ---------------------------------------------------------------------------------------
// Write

/// Make the local row match a record: upsert its fields, or delete it for a tombstone.
pub fn write(
    conn: &Connection,
    kind: &str,
    ident: &Value,
    fields: Option<&Fields>,
    now: i64,
) -> Result<Applied> {
    use Applied::{Done, Parked};
    match kind {
        "provider" => {
            let sync_id = str_of(ident, "p")?;
            let existing = provider_id(conn, sync_id)?;
            match (fields, existing) {
                (None, Some(id)) => {
                    conn.execute("DELETE FROM providers WHERE id = ?1", [id])?;
                }
                (None, None) => {}
                (Some(f), Some(id)) => {
                    let mut v = values(f, PROVIDER);
                    v.push(Sql::Integer(id));
                    conn.execute(
                        &format!(
                            "UPDATE providers SET {} WHERE id = ?{}",
                            set_clause(PROVIDER, 1),
                            PROVIDER.len() + 1
                        ),
                        rusqlite::params_from_iter(v),
                    )?;
                }
                (Some(f), None) => {
                    let mut v = values(f, PROVIDER);
                    v.push(Sql::Text(sync_id.into()));
                    v.push(Sql::Integer(now));
                    conn.execute(
                        &format!(
                            "INSERT INTO providers ({}, sync_id, created_at) VALUES ({})",
                            PROVIDER.join(", "),
                            placeholders(PROVIDER.len() + 2)
                        ),
                        rusqlite::params_from_iter(v),
                    )?;
                }
            }
            Ok(Done)
        }
        "profile" => {
            let pf = str_of(ident, "pf")?;
            let existing = profile_id(conn, pf)?;
            match (fields, existing) {
                (None, Some(id)) => {
                    // The last profile stays: there would be nothing to sign in to, and
                    // every per-profile row would cascade away with it.
                    let count: i64 =
                        conn.query_row("SELECT COUNT(*) FROM profiles", [], |r| r.get(0))?;
                    if count > 1 {
                        conn.execute("DELETE FROM profiles WHERE id = ?1", [id])?;
                    }
                }
                (None, None) => {}
                (Some(f), Some(id)) => {
                    let mut v = values(f, PROFILE);
                    v.push(Sql::Integer(id));
                    conn.execute(
                        &format!(
                            "UPDATE profiles SET {} WHERE id = ?{}",
                            set_clause(PROFILE, 1),
                            PROFILE.len() + 1
                        ),
                        rusqlite::params_from_iter(v),
                    )?;
                }
                (Some(f), None) => {
                    let sync_id = pf.trim_start_matches("pf:");
                    let mut v = values(f, PROFILE);
                    v.push(Sql::Text(sync_id.into()));
                    v.push(Sql::Integer(now));
                    conn.execute(
                        &format!(
                            "INSERT INTO profiles ({}, sync_id, created_at) VALUES ({})",
                            PROFILE.join(", "),
                            placeholders(PROFILE.len() + 2)
                        ),
                        rusqlite::params_from_iter(v),
                    )?;
                }
            }
            Ok(Done)
        }
        "parental" => {
            if let Some(f) = fields {
                conn.execute(
                    &format!(
                        "UPDATE parental SET {}, updated_at = ?{} WHERE id = 1",
                        set_clause(PARENTAL, 1),
                        PARENTAL.len() + 1
                    ),
                    rusqlite::params_from_iter(
                        values(f, PARENTAL).into_iter().chain([Sql::Integer(now)]),
                    ),
                )?;
            }
            Ok(Done)
        }
        "parental_lock" => {
            let Some(value) = lock_value(conn, ident)? else {
                return Ok(if fields.is_some() { Parked } else { Done });
            };
            let lock_kind = str_of(ident, "kind")?;
            if fields.is_some() {
                conn.execute(
                    "INSERT OR IGNORE INTO parental_locks (kind, value) VALUES (?1, ?2)",
                    params![lock_kind, value],
                )?;
            } else {
                conn.execute(
                    "DELETE FROM parental_locks WHERE kind = ?1 AND value = ?2",
                    params![lock_kind, value],
                )?;
            }
            Ok(Done)
        }
        "progress" => per_profile_item(conn, ident, fields, |conn, (pf, k, id), f| {
            match f {
                Some(f) => {
                    let mut v = vec![Sql::Integer(pf), Sql::Text(k.into()), Sql::Integer(id)];
                    v.extend(values(f, PROGRESS));
                    conn.execute(
                        "INSERT INTO watch_progress
                           (profile_id, item_kind, item_id,
                            position_secs, duration_secs, completed, updated_at)
                         VALUES (?1, ?2, ?3, COALESCE(?4, 0), COALESCE(?5, 0),
                                 COALESCE(?6, 0), COALESCE(?7, 0))
                         ON CONFLICT (profile_id, item_kind, item_id) DO UPDATE SET
                            position_secs = excluded.position_secs,
                            duration_secs = excluded.duration_secs,
                            completed     = excluded.completed,
                            updated_at    = excluded.updated_at",
                        rusqlite::params_from_iter(v),
                    )?;
                }
                None => {
                    conn.execute(
                        "DELETE FROM watch_progress
                          WHERE profile_id = ?1 AND item_kind = ?2 AND item_id = ?3",
                        params![pf, k, id],
                    )?;
                }
            }
            Ok(())
        }),
        "favorite" => {
            let list = str_of(ident, "list")?.to_string();
            per_profile_item(conn, ident, fields, |conn, (pf, k, id), f| {
                match f {
                    Some(f) => {
                        conn.execute(
                            "INSERT INTO favorites
                               (profile_id, list_name, item_kind, item_id, sort_order, added_at)
                             VALUES (?1, ?2, ?3, ?4, COALESCE(?5, 0), COALESCE(?6, ?7))
                             ON CONFLICT (profile_id, list_name, item_kind, item_id)
                             DO UPDATE SET sort_order = excluded.sort_order,
                                           added_at   = excluded.added_at",
                            params![
                                pf,
                                list,
                                k,
                                id,
                                to_sql(f.get("sort_order")),
                                to_sql(f.get("added_at")),
                                now
                            ],
                        )?;
                    }
                    None => {
                        conn.execute(
                            "DELETE FROM favorites WHERE profile_id = ?1 AND list_name = ?2
                                AND item_kind = ?3 AND item_id = ?4",
                            params![pf, list, k, id],
                        )?;
                    }
                }
                Ok(())
            })
        }
        "my_list" | "not_interested" => {
            per_profile_item(conn, ident, fields, |conn, (pf, k, id), f| {
                match f {
                    Some(f) => {
                        conn.execute(
                            &format!(
                                "INSERT INTO {kind} (profile_id, item_kind, item_id, added_at)
                                 VALUES (?1, ?2, ?3, COALESCE(?4, ?5))
                                 ON CONFLICT (profile_id, item_kind, item_id)
                                 DO UPDATE SET added_at = excluded.added_at"
                            ),
                            params![pf, k, id, to_sql(f.get("added_at")), now],
                        )?;
                    }
                    None => {
                        conn.execute(
                            &format!(
                                "DELETE FROM {kind}
                                  WHERE profile_id = ?1 AND item_kind = ?2 AND item_id = ?3"
                            ),
                            params![pf, k, id],
                        )?;
                    }
                }
                Ok(())
            })
        }
        "series_prefs" => per_profile_item(conn, ident, fields, |conn, (pf, _, id), f| {
            match f {
                Some(f) => {
                    let mut v = vec![Sql::Integer(pf), Sql::Integer(id)];
                    v.extend(values(f, SERIES_PREFS));
                    conn.execute(
                        "INSERT INTO series_prefs
                           (profile_id, series_id, always_skip_intro, always_skip_recap,
                            autoplay_next, updated_at)
                         VALUES (?1, ?2, COALESCE(?3, 0), COALESCE(?4, 0),
                                 COALESCE(?5, 1), COALESCE(?6, 0))
                         ON CONFLICT (profile_id, series_id) DO UPDATE SET
                            always_skip_intro = excluded.always_skip_intro,
                            always_skip_recap = excluded.always_skip_recap,
                            autoplay_next     = excluded.autoplay_next,
                            updated_at        = excluded.updated_at",
                        rusqlite::params_from_iter(v),
                    )?;
                }
                None => {
                    conn.execute(
                        "DELETE FROM series_prefs WHERE profile_id = ?1 AND series_id = ?2",
                        params![pf, id],
                    )?;
                }
            }
            Ok(())
        }),
        "override" => {
            let Some((k, id)) = item(conn, str_of(ident, "item")?)? else {
                return Ok(Parked);
            };
            // Overrides are columns on a catalogue row; there is nothing to delete.
            if let Some(f) = fields {
                let (table, cols) = override_table(k)?;
                let mut v = values(f, cols);
                // `hidden` is NOT NULL.
                if let Some(i) = cols.iter().position(|c| *c == "hidden") {
                    if v[i] == Sql::Null {
                        v[i] = Sql::Integer(0);
                    }
                }
                v.push(Sql::Integer(id));
                conn.execute(
                    &format!(
                        "UPDATE {table} SET {} WHERE id = ?{}",
                        set_clause(cols, 1),
                        cols.len() + 1
                    ),
                    rusqlite::params_from_iter(v),
                )?;
            }
            Ok(Done)
        }
        "rule" => {
            let id = str_of(ident, "id")?;
            match fields {
                Some(f) => {
                    let mut v = vec![Sql::Text(id.into())];
                    v.extend(values(f, RULE));
                    conn.execute(
                        "INSERT INTO rules (id, enabled, definition, sort_order)
                         VALUES (?1, COALESCE(?2, 1), COALESCE(?3, '{}'), COALESCE(?4, 0))
                         ON CONFLICT (id) DO UPDATE SET
                            enabled = excluded.enabled, definition = excluded.definition,
                            sort_order = excluded.sort_order",
                        rusqlite::params_from_iter(v),
                    )?;
                }
                None => {
                    conn.execute("DELETE FROM rules WHERE id = ?1", [id])?;
                }
            }
            Ok(Done)
        }
        "epg_map" => {
            let Some((_, id)) = item(conn, str_of(ident, "item")?)? else {
                return Ok(if fields.is_some() { Parked } else { Done });
            };
            match fields {
                Some(f) => {
                    conn.execute(
                        "INSERT INTO epg_manual_map (channel_id, epg_channel_id, created_at)
                         VALUES (?1, ?2, COALESCE(?3, ?4))
                         ON CONFLICT (channel_id) DO UPDATE SET
                            epg_channel_id = excluded.epg_channel_id,
                            created_at     = excluded.created_at",
                        params![
                            id,
                            to_sql(f.get("epg_channel_id")),
                            to_sql(f.get("created_at")),
                            now
                        ],
                    )?;
                }
                None => {
                    conn.execute("DELETE FROM epg_manual_map WHERE channel_id = ?1", [id])?;
                }
            }
            Ok(Done)
        }
        "recording_rule" => {
            let sync_id = str_of(ident, "id")?;
            let Some(f) = fields else {
                conn.execute("DELETE FROM recording_rules WHERE sync_id = ?1", [sync_id])?;
                return Ok(Done);
            };
            let channel = match f.get("channel").and_then(Value::as_str) {
                Some(key) => match item(conn, key)? {
                    Some((_, id)) => Sql::Integer(id),
                    None => return Ok(Parked),
                },
                None => Sql::Null,
            };
            let mut v = values(f, RECORDING_RULE);
            v.push(channel);
            v.push(Sql::Text(sync_id.into()));
            let n = RECORDING_RULE.len();
            let updated = conn.execute(
                &format!(
                    "UPDATE recording_rules SET {}, channel_id = ?{} WHERE sync_id = ?{}",
                    set_clause(RECORDING_RULE, 1),
                    n + 1,
                    n + 2
                ),
                rusqlite::params_from_iter(v.clone()),
            )?;
            if updated == 0 {
                conn.execute(
                    &format!(
                        "INSERT INTO recording_rules ({}, channel_id, sync_id) VALUES ({})",
                        RECORDING_RULE.join(", "),
                        placeholders(n + 2)
                    ),
                    rusqlite::params_from_iter(v),
                )?;
            }
            Ok(Done)
        }
        "reminder" => {
            let Some((_, id)) = item(conn, str_of(ident, "item")?)? else {
                return Ok(if fields.is_some() { Parked } else { Done });
            };
            let start = ident.get("start").and_then(Value::as_i64);
            let title = str_of(ident, "title")?;
            match fields {
                Some(f) => {
                    conn.execute(
                        "INSERT INTO reminders (channel_id, title, start, lead_secs, created_at)
                         VALUES (?1, ?2, ?3, COALESCE(?4, 120), COALESCE(?5, ?6))
                         ON CONFLICT (channel_id, start, title) DO UPDATE SET
                            lead_secs = excluded.lead_secs",
                        params![
                            id,
                            title,
                            start,
                            to_sql(f.get("lead_secs")),
                            to_sql(f.get("created_at")),
                            now
                        ],
                    )?;
                }
                None => {
                    conn.execute(
                        "DELETE FROM reminders WHERE channel_id = ?1 AND start = ?2 AND title = ?3",
                        params![id, start, title],
                    )?;
                }
            }
            Ok(Done)
        }
        "mosaic" => {
            let name = str_of(ident, "name")?;
            let Some(f) = fields else {
                conn.execute("DELETE FROM mosaic_layouts WHERE name = ?1", [name])?;
                return Ok(Done);
            };
            let mut ids: Vec<Option<i64>> = Vec::new();
            for key in f
                .get("channels")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                match key.as_str() {
                    Some(k) => match item(conn, k)? {
                        Some((_, id)) => ids.push(Some(id)),
                        None => return Ok(Parked),
                    },
                    None => ids.push(None),
                }
            }
            conn.execute(
                "INSERT INTO mosaic_layouts (name, layout, channels, created_at)
                 VALUES (?1, ?2, ?3, COALESCE(?4, ?5))
                 ON CONFLICT (name) DO UPDATE SET
                    layout = excluded.layout, channels = excluded.channels,
                    created_at = excluded.created_at",
                params![
                    name,
                    to_sql(f.get("layout")),
                    serde_json::to_string(&ids)?,
                    to_sql(f.get("created_at")),
                    now
                ],
            )?;
            Ok(Done)
        }
        "setting" => {
            let Some(scope) = setting_scope(conn, str_of(ident, "scope")?)? else {
                return Ok(if fields.is_some() { Parked } else { Done });
            };
            let key = str_of(ident, "key")?;
            match fields.and_then(|f| f.get("value")).filter(|v| !v.is_null()) {
                Some(v) => {
                    conn.execute(
                        "INSERT INTO settings (scope, key, value) VALUES (?1, ?2, ?3)
                         ON CONFLICT (scope, key) DO UPDATE SET value = excluded.value",
                        params![scope, key, to_sql(Some(v))],
                    )?;
                }
                None => {
                    conn.execute(
                        "DELETE FROM settings WHERE scope = ?1 AND key = ?2",
                        params![scope, key],
                    )?;
                }
            }
            Ok(Done)
        }
        other => Err(SyncError::Malformed(format!("unknown kind {other}"))),
    }
}

fn placeholders(n: usize) -> String {
    (1..=n)
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Shared shape of the per-profile, per-item kinds: resolve both, park if either is not
/// here, and treat removing something that is not here as already done.
fn per_profile_item(
    conn: &Connection,
    ident: &Value,
    fields: Option<&Fields>,
    f: impl FnOnce(&Connection, (i64, &'static str, i64), Option<&Fields>) -> Result<()>,
) -> Result<Applied> {
    match profile_and_item(conn, ident)? {
        Some(ids) => {
            f(conn, ids, fields)?;
            Ok(Applied::Done)
        }
        None if fields.is_some() => Ok(Applied::Parked),
        None => Ok(Applied::Done),
    }
}

// ---------------------------------------------------------------------------------------
// Identity adoption

/// The same provider added on two devices before either synced has two UUIDs. When a
/// remote provider or profile arrives whose id this device does not know, and a local one
/// that has never been synced matches it, the local one takes the remote id -- so both
/// devices' favourites and progress end up naming the same thing (§3.8).
///
/// Providers match on kind, base URL (case and trailing slash ignored) and username;
/// profiles on name. Returns the replaced local id, if any.
pub fn adopt(conn: &Connection, remote: &Record) -> Result<Option<String>> {
    if !remote.alive.value {
        return Ok(None);
    }
    let field = |name: &str| remote.fields.get(name).map(|f| f.value.clone());
    match remote.kind.as_str() {
        "provider" => {
            let sync_id = str_of(&remote.ident, "p")?;
            if provider_id(conn, sync_id)?.is_some() {
                return Ok(None);
            }
            let url = field("base_url")
                .and_then(|v| v.as_str().map(normalise_url))
                .unwrap_or_default();
            let candidates: Vec<(i64, String, String, Option<String>, String)> = conn
                .prepare("SELECT id, sync_id, kind, username, base_url FROM providers")?
                .query_map([], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })?
                .collect::<std::result::Result<_, _>>()?;
            for (id, local_sync, kind, user, base) in candidates {
                if Some(Value::String(kind)) == field("kind")
                    && normalise_url(&base) == url
                    && user.map_or(Value::Null, Value::String)
                        == field("username").unwrap_or(Value::Null)
                    && !ever_synced(conn, "provider", &json!({ "p": local_sync }))?
                {
                    conn.execute(
                        "UPDATE providers SET sync_id = ?1 WHERE id = ?2",
                        params![sync_id, id],
                    )?;
                    return Ok(Some(local_sync));
                }
            }
            Ok(None)
        }
        "profile" => {
            let pf = str_of(&remote.ident, "pf")?;
            if profile_id(conn, pf)?.is_some() {
                return Ok(None);
            }
            let Some(name) = field("name").and_then(|v| v.as_str().map(str::to_string)) else {
                return Ok(None);
            };
            let candidates: Vec<(i64, String)> = conn
                .prepare(
                    "SELECT id, sync_id FROM profiles
                      WHERE lower(trim(name)) = lower(trim(?1)) ORDER BY id",
                )?
                .query_map([&name], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<_, _>>()?;
            for (id, local_sync) in candidates {
                if !ever_synced(
                    conn,
                    "profile",
                    &json!({ "pf": format!("pf:{local_sync}") }),
                )? {
                    conn.execute(
                        "UPDATE profiles SET sync_id = ?1 WHERE id = ?2",
                        params![pf.trim_start_matches("pf:"), id],
                    )?;
                    return Ok(Some(local_sync));
                }
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn normalise_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_ascii_lowercase()
}

/// Whether this device has ever merged a record for this identity.
fn ever_synced(conn: &Connection, kind: &str, ident: &Value) -> Result<bool> {
    // Built with SQLite's own json_object so the key matches the triggers' byte for byte.
    let (k, v) = ident
        .as_object()
        .and_then(|o| o.iter().next())
        .ok_or_else(|| SyncError::Malformed(ident.to_string()))?;
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM sync_shadow WHERE sync_key = ?1 || ':' || json_object(?2, ?3)",
            params![kind, k, v.as_str().unwrap_or_default()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}
