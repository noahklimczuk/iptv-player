//! Recording schedule, series rules, and the recordings library (README §7.7).
//!
//! The scheduling *decisions* — padding, conflicts, which rule matches what — live in
//! `aurora_core::dvr`. This module is the storage around them: it persists what was
//! decided, answers "what is due now", and enforces the disk quota.

use aurora_core::dvr::{self, RecordingState, SeriesRule, Slot};
use aurora_core::model::Programme;
use aurora_core::title;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{DbError, Result};

fn state_str(state: RecordingState) -> &'static str {
    match state {
        RecordingState::Scheduled => "scheduled",
        RecordingState::Recording => "recording",
        RecordingState::Completed => "completed",
        RecordingState::Failed => "failed",
        RecordingState::Skipped => "skipped",
    }
}

fn parse_state(s: &str) -> RecordingState {
    match s {
        "recording" => RecordingState::Recording,
        "completed" => RecordingState::Completed,
        "failed" => RecordingState::Failed,
        "skipped" => RecordingState::Skipped,
        _ => RecordingState::Scheduled,
    }
}

/// A scheduled or finished recording, as the library page shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recording {
    pub id: i64,
    pub channel_id: i64,
    pub rule_id: Option<i64>,
    pub title: String,
    pub sub_title: Option<String>,
    pub season: Option<u16>,
    pub episode: Option<u16>,
    /// Airtime as the guide gave it, without padding.
    pub air_start: i64,
    pub air_stop: i64,
    /// What the recorder opens and closes on: airtime plus padding.
    pub start: i64,
    pub stop: i64,
    pub state: RecordingState,
    pub reason: Option<String>,
    pub priority: i32,
    pub file_path: Option<String>,
    pub bytes: i64,
    pub duration_secs: i64,
    pub keep: bool,
    pub watched: bool,
}

/// Everything needed to put one airing on the schedule.
#[derive(Debug, Clone, Default)]
pub struct NewRecording<'a> {
    pub channel_id: i64,
    pub rule_id: Option<i64>,
    pub programme_id: Option<i64>,
    pub title: &'a str,
    pub sub_title: Option<&'a str>,
    pub description: Option<&'a str>,
    pub season: Option<u16>,
    pub episode: Option<u16>,
    pub air_start: i64,
    pub air_stop: i64,
    pub pre_padding_secs: i64,
    pub post_padding_secs: i64,
    pub priority: i32,
}

impl<'a> NewRecording<'a> {
    /// A manual "record this" from the guide, with the default padding.
    pub fn from_programme(channel_id: i64, programme: &'a Programme) -> Self {
        Self {
            channel_id,
            rule_id: None,
            programme_id: None,
            title: &programme.title,
            sub_title: programme.sub_title.as_deref(),
            description: programme.description.as_deref(),
            season: programme.season,
            episode: programme.episode,
            air_start: programme.start,
            air_stop: programme.stop,
            pre_padding_secs: dvr::DEFAULT_PRE_PADDING_SECS,
            post_padding_secs: dvr::DEFAULT_POST_PADDING_SECS,
            priority: 0,
        }
    }
}

fn row_to_recording(r: &rusqlite::Row<'_>) -> rusqlite::Result<Recording> {
    let state: String = r.get("state")?;
    Ok(Recording {
        id: r.get("id")?,
        channel_id: r.get("channel_id")?,
        rule_id: r.get("rule_id")?,
        title: r.get("title")?,
        sub_title: r.get("sub_title")?,
        season: r.get("season")?,
        episode: r.get("episode")?,
        air_start: r.get("air_start")?,
        air_stop: r.get("air_stop")?,
        start: r.get("start")?,
        stop: r.get("stop")?,
        state: parse_state(&state),
        reason: r.get("reason")?,
        priority: r.get("priority")?,
        file_path: r.get("file_path")?,
        bytes: r.get("bytes")?,
        duration_secs: r.get("duration_secs")?,
        keep: r.get::<_, i64>("keep")? != 0,
        watched: r.get::<_, i64>("watched")? != 0,
    })
}

const SELECT: &str = "SELECT id, channel_id, rule_id, title, sub_title, season, episode,
                             air_start, air_stop, start, stop, state, reason, priority,
                             file_path, bytes, duration_secs, keep, watched
                      FROM recordings";

/// Put an airing on the schedule, applying padding.
///
/// Returns `Ok(None)` when this airing is already scheduled — rule expansion re-runs on
/// every EPG refresh, and re-scheduling the same episode each time would be a bug.
pub fn schedule(conn: &Connection, rec: &NewRecording<'_>, now: i64) -> Result<Option<i64>> {
    let (start, stop) = dvr::with_padding(
        rec.air_start,
        rec.air_stop,
        rec.pre_padding_secs,
        rec.post_padding_secs,
    );
    let changed = conn.execute(
        "INSERT OR IGNORE INTO recordings
           (channel_id, rule_id, programme_id, title, sub_title, description, season, episode,
            air_start, air_stop, start, stop, priority, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        params![
            rec.channel_id,
            rec.rule_id,
            rec.programme_id,
            rec.title,
            rec.sub_title,
            rec.description,
            rec.season,
            rec.episode,
            rec.air_start,
            rec.air_stop,
            start,
            stop,
            rec.priority,
            now
        ],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    Ok(Some(conn.last_insert_rowid()))
}

/// Drop a recording that hasn't started yet. Finished ones are deleted, not cancelled.
pub fn cancel(conn: &Connection, id: i64) -> Result<bool> {
    let state: Option<String> = conn
        .query_row("SELECT state FROM recordings WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    let Some(state) = state else {
        return Ok(false);
    };
    if state == "completed" {
        return Err(DbError::Rejected(
            "recording already finished — delete it instead".into(),
        ));
    }
    conn.execute("DELETE FROM recordings WHERE id = ?1", [id])?;
    Ok(true)
}

/// Remove a recording and hand back the file the caller must unlink.
///
/// Deleting the row and the file can't be one atomic step, so the order matters: the row
/// goes first, and a file left behind by a crash is found by the orphan sweep. The other
/// order would lose the recording while the row still claimed it existed.
pub fn delete(conn: &Connection, id: i64) -> Result<Option<String>> {
    let path: Option<Option<String>> = conn
        .query_row(
            "SELECT file_path FROM recordings WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(path) = path else {
        return Ok(None);
    };
    conn.execute("DELETE FROM recordings WHERE id = ?1", [id])?;
    Ok(path)
}

/// Move a recording through its lifecycle. `reason` explains a failure or a skip.
pub fn set_state(
    conn: &Connection,
    id: i64,
    state: RecordingState,
    reason: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE recordings SET state = ?2, reason = ?3 WHERE id = ?1",
        params![id, state_str(state), reason],
    )?;
    Ok(())
}

/// Record where the finished file landed and how big it is.
pub fn set_file(
    conn: &Connection,
    id: i64,
    file_path: &str,
    bytes: i64,
    duration_secs: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE recordings SET file_path = ?2, bytes = ?3, duration_secs = ?4 WHERE id = ?1",
        params![id, file_path, bytes, duration_secs],
    )?;
    Ok(())
}

pub fn set_keep(conn: &Connection, id: i64, keep: bool) -> Result<()> {
    conn.execute(
        "UPDATE recordings SET keep = ?2 WHERE id = ?1",
        params![id, keep as i64],
    )?;
    Ok(())
}

pub fn set_watched(conn: &Connection, id: i64, watched: bool) -> Result<()> {
    conn.execute(
        "UPDATE recordings SET watched = ?2 WHERE id = ?1",
        params![id, watched as i64],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Recording>> {
    let sql = format!("{SELECT} WHERE id = ?1");
    Ok(conn.query_row(&sql, [id], row_to_recording).optional()?)
}

/// Everything in one state, soonest first. `None` lists the lot.
pub fn list(conn: &Connection, state: Option<RecordingState>) -> Result<Vec<Recording>> {
    let sql = match state {
        Some(_) => format!("{SELECT} WHERE state = ?1 ORDER BY start"),
        None => format!("{SELECT} ORDER BY start"),
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = match state {
        Some(s) => stmt.query_map([state_str(s)], row_to_recording)?,
        None => stmt.query_map([], row_to_recording)?,
    };
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Scheduled recordings whose padded start has arrived. This is what the recorder polls.
pub fn due(conn: &Connection, now: i64) -> Result<Vec<Recording>> {
    let sql = format!(
        "{SELECT} WHERE state = 'scheduled' AND start <= ?1 AND stop > ?1 ORDER BY priority DESC, start"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([now], row_to_recording)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Upcoming recordings as conflict-detection slots.
pub fn slots(conn: &Connection, from: i64) -> Result<Vec<Slot>> {
    let mut stmt = conn.prepare(
        "SELECT id, channel_id, start, stop, priority FROM recordings
         WHERE state IN ('scheduled','recording') AND stop > ?1
         ORDER BY start",
    )?;
    let rows = stmt.query_map([from], |r| {
        Ok(Slot {
            id: r.get(0)?,
            channel_id: r.get(1)?,
            start: r.get(2)?,
            stop: r.get(3)?,
            priority: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Windows where the schedule wants more streams than the subscription allows.
pub fn conflicts(
    conn: &Connection,
    from: i64,
    max_concurrent: usize,
) -> Result<Vec<dvr::Conflict>> {
    Ok(dvr::find_conflicts(&slots(conn, from)?, max_concurrent))
}

/// Mark the recordings that lose a conflict as skipped, and say which they were.
///
/// Run when the schedule changes, not at record time: a user who sees "will not record"
/// a day ahead can do something about it, one who finds out afterwards cannot.
pub fn resolve_conflicts(conn: &Connection, from: i64, max_concurrent: usize) -> Result<Vec<i64>> {
    let dropped = dvr::resolve_conflicts(&slots(conn, from)?, max_concurrent);
    for id in &dropped {
        set_state(
            conn,
            *id,
            RecordingState::Skipped,
            Some("too many recordings at once for this subscription"),
        )?;
    }
    Ok(dropped)
}

/// A standing series rule, as stored.
#[derive(Debug, Clone, Default)]
pub struct NewRule<'a> {
    pub title: &'a str,
    pub channel_id: Option<i64>,
    pub new_only: bool,
    /// 0 = Monday. Empty or `None` means any day.
    pub weekdays: Option<Vec<u8>>,
    pub around_local_minute: Option<u16>,
    pub time_slack_secs: Option<i64>,
    pub pre_padding_secs: Option<i64>,
    pub post_padding_secs: Option<i64>,
    pub keep_episodes: Option<u16>,
    pub priority: i32,
}

pub fn create_rule(conn: &Connection, rule: &NewRule<'_>, now: i64) -> Result<i64> {
    let weekdays = match &rule.weekdays {
        Some(days) if !days.is_empty() => Some(serde_json::to_string(days)?),
        _ => None,
    };
    conn.execute(
        "INSERT INTO recording_rules
           (title, title_key, channel_id, new_only, weekdays, around_local_minute,
            time_slack_secs, pre_padding_secs, post_padding_secs, keep_episodes, priority, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![
            rule.title,
            title::match_key(rule.title),
            rule.channel_id,
            rule.new_only as i64,
            weekdays,
            rule.around_local_minute,
            rule.time_slack_secs.unwrap_or(900),
            rule.pre_padding_secs.unwrap_or(dvr::DEFAULT_PRE_PADDING_SECS),
            rule.post_padding_secs.unwrap_or(dvr::DEFAULT_POST_PADDING_SECS),
            rule.keep_episodes,
            rule.priority,
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Delete a rule. Recordings it already scheduled stay — the user asked for those
/// episodes, and "stop recording this show" shouldn't silently drop tonight's.
pub fn delete_rule(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn.execute("DELETE FROM recording_rules WHERE id = ?1", [id])? > 0)
}

pub fn set_rule_enabled(conn: &Connection, id: i64, enabled: bool) -> Result<()> {
    conn.execute(
        "UPDATE recording_rules SET enabled = ?2 WHERE id = ?1",
        params![id, enabled as i64],
    )?;
    Ok(())
}

/// Enabled rules, in the form the matcher wants.
pub fn active_rules(conn: &Connection) -> Result<Vec<SeriesRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, title_key, channel_id, new_only, weekdays, around_local_minute,
                time_slack_secs, priority
         FROM recording_rules WHERE enabled = 1 ORDER BY priority DESC, id",
    )?;
    let rows = stmt.query_map([], |r| {
        let weekdays: Option<String> = r.get(4)?;
        Ok(SeriesRule {
            id: r.get(0)?,
            title_key: r.get(1)?,
            channel_id: r.get(2)?,
            new_only: r.get::<_, i64>(3)? != 0,
            // A rule whose weekday list is corrupt should match every day rather than
            // silently stop recording, so a parse failure falls back to `None`.
            weekdays: weekdays.and_then(|s| serde_json::from_str(&s).ok()),
            around_local_minute: r.get(5)?,
            time_slack_secs: r.get(6)?,
            priority: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Padding and priority for one rule, so expansion schedules with the rule's own values.
fn rule_padding(conn: &Connection, rule_id: i64) -> Result<(i64, i64, i32)> {
    Ok(conn.query_row(
        "SELECT pre_padding_secs, post_padding_secs, priority FROM recording_rules WHERE id = ?1",
        [rule_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?)
}

/// Run every enabled rule over a slice of guide entries and schedule what matches.
///
/// Idempotent: called after every EPG refresh, and the unique index absorbs airings that
/// are already on the schedule. Returns the ids of recordings actually added.
pub fn expand_rules(
    conn: &Connection,
    programmes: &[(Programme, i64)],
    utc_offset_secs: i32,
    now: i64,
) -> Result<Vec<i64>> {
    let rules = active_rules(conn)?;
    if rules.is_empty() {
        return Ok(Vec::new());
    }
    let matches = dvr::expand_rules(&rules, programmes, utc_offset_secs);
    let mut added = Vec::new();
    for (rule_id, index) in matches {
        let (programme, channel_id) = &programmes[index];
        // Don't schedule something that already aired.
        if programme.stop <= now {
            continue;
        }
        let (pre, post, priority) = rule_padding(conn, rule_id)?;
        let mut rec = NewRecording::from_programme(*channel_id, programme);
        rec.rule_id = Some(rule_id);
        rec.pre_padding_secs = pre;
        rec.post_padding_secs = post;
        rec.priority = priority;
        if let Some(id) = schedule(conn, &rec, now)? {
            added.push(id);
        }
    }
    Ok(added)
}

/// Bytes held by completed recordings.
pub fn total_bytes(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(bytes),0) FROM recordings WHERE state = 'completed'",
        [],
        |r| r.get(0),
    )?)
}

/// What to delete to get back under `quota_bytes`, oldest-first.
///
/// Watched recordings go before unwatched ones — deleting something the user hasn't seen
/// to make room is the worst thing a DVR can do. Anything flagged `keep` is never
/// returned, even if that means staying over quota; the caller surfaces that.
pub fn over_quota(conn: &Connection, quota_bytes: i64) -> Result<Vec<i64>> {
    let mut total = total_bytes(conn)?;
    if total <= quota_bytes {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT id, bytes FROM recordings
         WHERE state = 'completed' AND keep = 0
         ORDER BY watched DESC, start ASC",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    let mut prune = Vec::new();
    for row in rows {
        let (id, bytes) = row?;
        if total <= quota_bytes {
            break;
        }
        total -= bytes;
        prune.push(id);
    }
    Ok(prune)
}

/// Episodes of one rule beyond its `keep_episodes` limit, oldest first.
pub fn over_keep_limit(conn: &Connection, rule_id: i64) -> Result<Vec<i64>> {
    let keep: Option<i64> = conn.query_row(
        "SELECT keep_episodes FROM recording_rules WHERE id = ?1",
        [rule_id],
        |r| r.get(0),
    )?;
    let Some(keep) = keep else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(
        "SELECT id FROM recordings
         WHERE rule_id = ?1 AND state = 'completed' AND keep = 0
         ORDER BY start DESC LIMIT -1 OFFSET ?2",
    )?;
    let rows = stmt.query_map(params![rule_id, keep], |r| r.get::<_, i64>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// A "remind me" entry from the guide.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub id: i64,
    pub channel_id: i64,
    pub title: String,
    pub start: i64,
    pub lead_secs: i64,
}

/// Returns `Ok(None)` if this airing already has a reminder.
pub fn add_reminder(
    conn: &Connection,
    channel_id: i64,
    programme: &Programme,
    lead_secs: i64,
    now: i64,
) -> Result<Option<i64>> {
    let changed = conn.execute(
        "INSERT OR IGNORE INTO reminders (channel_id, title, start, lead_secs, created_at)
         VALUES (?1,?2,?3,?4,?5)",
        params![channel_id, programme.title, programme.start, lead_secs, now],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    Ok(Some(conn.last_insert_rowid()))
}

pub fn remove_reminder(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn.execute("DELETE FROM reminders WHERE id = ?1", [id])? > 0)
}

/// Reminders whose lead time has arrived, marking them fired so they show once.
pub fn fire_reminders(conn: &mut Connection, now: i64) -> Result<Vec<Reminder>> {
    let tx = conn.transaction()?;
    let due: Vec<Reminder> = {
        let mut stmt = tx.prepare(
            "SELECT id, channel_id, title, start, lead_secs FROM reminders
             WHERE fired = 0 AND start - lead_secs <= ?1 AND start > ?1
             ORDER BY start",
        )?;
        let rows = stmt.query_map([now], |r| {
            Ok(Reminder {
                id: r.get(0)?,
                channel_id: r.get(1)?,
                title: r.get(2)?,
                start: r.get(3)?,
                lead_secs: r.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for reminder in &due {
        tx.execute(
            "UPDATE reminders SET fired = 1 WHERE id = ?1",
            [reminder.id],
        )?;
    }
    tx.commit()?;
    Ok(due)
}

/// Drop reminders for programmes that have already started.
pub fn prune_reminders(conn: &Connection, now: i64) -> Result<usize> {
    Ok(conn.execute("DELETE FROM reminders WHERE start <= ?1", [now])?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        for id in 1..=3 {
            conn.execute(
                "INSERT INTO channels (id,provider_id,provider_key,name,match_key,last_seen_at)
                 VALUES (?1,1,?2,?3,?3,0)",
                params![id, format!("k{id}"), format!("ch{id}")],
            )
            .unwrap();
        }
        conn
    }

    fn programme(title: &str, start: i64) -> Programme {
        Programme {
            channel_id: "c".into(),
            start,
            stop: start + 1800,
            title: title.into(),
            sub_title: None,
            description: None,
            categories: vec![],
            season: None,
            episode: None,
            icon: None,
            rating: None,
            star_rating: None,
            is_new: true,
            is_live: false,
            is_premiere: false,
            credits: vec![],
        }
    }

    fn manual(channel_id: i64, title: &'static str, air_start: i64) -> NewRecording<'static> {
        NewRecording {
            channel_id,
            title,
            air_start,
            air_stop: air_start + 1800,
            pre_padding_secs: dvr::DEFAULT_PRE_PADDING_SECS,
            post_padding_secs: dvr::DEFAULT_POST_PADDING_SECS,
            ..Default::default()
        }
    }

    #[test]
    fn schedule_applies_padding_to_the_stored_window() {
        let conn = db();
        let id = schedule(&conn, &manual(1, "Show", 10_000), 0)
            .unwrap()
            .unwrap();
        let rec = get(&conn, id).unwrap().unwrap();
        // Airtime is preserved; only start/stop carry the padding.
        assert_eq!((rec.air_start, rec.air_stop), (10_000, 11_800));
        assert_eq!((rec.start, rec.stop), (9_940, 12_100));
        assert_eq!(rec.state, RecordingState::Scheduled);
    }

    #[test]
    fn scheduling_the_same_airing_twice_is_a_no_op() {
        let conn = db();
        assert!(schedule(&conn, &manual(1, "Show", 10_000), 0)
            .unwrap()
            .is_some());
        assert!(schedule(&conn, &manual(1, "Show", 10_000), 0)
            .unwrap()
            .is_none());
        assert_eq!(list(&conn, None).unwrap().len(), 1);
    }

    #[test]
    fn the_same_show_on_another_channel_is_a_different_recording() {
        let conn = db();
        schedule(&conn, &manual(1, "Show", 10_000), 0)
            .unwrap()
            .unwrap();
        schedule(&conn, &manual(2, "Show", 10_000), 0)
            .unwrap()
            .unwrap();
        assert_eq!(list(&conn, None).unwrap().len(), 2);
    }

    #[test]
    fn cancel_removes_a_scheduled_recording() {
        let conn = db();
        let id = schedule(&conn, &manual(1, "Show", 10_000), 0)
            .unwrap()
            .unwrap();
        assert!(cancel(&conn, id).unwrap());
        assert!(get(&conn, id).unwrap().is_none());
        // Cancelling something that isn't there is false, not an error.
        assert!(!cancel(&conn, id).unwrap());
    }

    #[test]
    fn cancel_refuses_a_finished_recording() {
        let conn = db();
        let id = schedule(&conn, &manual(1, "Show", 10_000), 0)
            .unwrap()
            .unwrap();
        set_state(&conn, id, RecordingState::Completed, None).unwrap();
        assert!(matches!(cancel(&conn, id), Err(DbError::Rejected(_))));
        assert!(get(&conn, id).unwrap().is_some());
    }

    #[test]
    fn delete_hands_back_the_file_to_unlink() {
        let conn = db();
        let id = schedule(&conn, &manual(1, "Show", 10_000), 0)
            .unwrap()
            .unwrap();
        set_file(&conn, id, r"C:\Recordings\show.ts", 1_024, 1_800).unwrap();
        assert_eq!(
            delete(&conn, id).unwrap().as_deref(),
            Some(r"C:\Recordings\show.ts")
        );
        assert!(get(&conn, id).unwrap().is_none());
    }

    #[test]
    fn deleting_a_recording_that_never_produced_a_file_is_not_an_error() {
        let conn = db();
        let id = schedule(&conn, &manual(1, "Show", 10_000), 0)
            .unwrap()
            .unwrap();
        // Scheduled-but-never-recorded: row exists, file doesn't.
        assert_eq!(delete(&conn, id).unwrap(), None);
        assert!(get(&conn, id).unwrap().is_none());
        // And a row that was never there is also None, not an error.
        assert_eq!(delete(&conn, 999).unwrap(), None);
    }

    #[test]
    fn due_returns_only_what_is_on_air_now() {
        let conn = db();
        let now = schedule(&conn, &manual(1, "Now", 10_000), 0)
            .unwrap()
            .unwrap();
        schedule(&conn, &manual(2, "Later", 50_000), 0)
            .unwrap()
            .unwrap();
        let on_air = due(&conn, 10_500).unwrap();
        assert_eq!(on_air.iter().map(|r| r.id).collect::<Vec<_>>(), vec![now]);
        // Past its padded stop, it is no longer due.
        assert!(due(&conn, 12_500).unwrap().is_empty());
    }

    #[test]
    fn due_skips_recordings_that_already_started() {
        let conn = db();
        let id = schedule(&conn, &manual(1, "Show", 10_000), 0)
            .unwrap()
            .unwrap();
        set_state(&conn, id, RecordingState::Recording, None).unwrap();
        assert!(due(&conn, 10_500).unwrap().is_empty());
    }

    #[test]
    fn conflicts_surface_when_the_connection_limit_is_exceeded() {
        let conn = db();
        for ch in 1..=3 {
            schedule(&conn, &manual(ch, "Show", 10_000), 0)
                .unwrap()
                .unwrap();
        }
        let found = conflicts(&conn, 0, 2).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].over_by, 1);
    }

    #[test]
    fn resolving_a_conflict_marks_the_loser_skipped_with_a_reason() {
        let conn = db();
        let low = schedule(&conn, &manual(1, "Low", 10_000), 0)
            .unwrap()
            .unwrap();
        let mut high = manual(2, "High", 10_000);
        high.priority = 10;
        let high = schedule(&conn, &high, 0).unwrap().unwrap();

        let dropped = resolve_conflicts(&conn, 0, 1).unwrap();
        assert_eq!(dropped, vec![low]);
        assert_eq!(
            get(&conn, low).unwrap().unwrap().state,
            RecordingState::Skipped
        );
        assert!(get(&conn, low).unwrap().unwrap().reason.is_some());
        assert_eq!(
            get(&conn, high).unwrap().unwrap().state,
            RecordingState::Scheduled
        );
    }

    #[test]
    fn rules_round_trip_through_storage() {
        let conn = db();
        let id = create_rule(
            &conn,
            &NewRule {
                title: "The Late Show",
                channel_id: Some(2),
                new_only: true,
                weekdays: Some(vec![0, 1, 2, 3, 4]),
                around_local_minute: Some(23 * 60),
                ..Default::default()
            },
            0,
        )
        .unwrap();

        let rules = active_rules(&conn).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].id, id);
        assert_eq!(rules[0].title_key, title::match_key("The Late Show"));
        assert_eq!(rules[0].channel_id, Some(2));
        assert!(rules[0].new_only);
        assert_eq!(rules[0].weekdays.as_deref(), Some(&[0u8, 1, 2, 3, 4][..]));
        assert_eq!(rules[0].around_local_minute, Some(1380));

        set_rule_enabled(&conn, id, false).unwrap();
        assert!(active_rules(&conn).unwrap().is_empty());
    }

    #[test]
    fn an_empty_weekday_list_stores_as_any_day() {
        let conn = db();
        create_rule(
            &conn,
            &NewRule {
                title: "Show",
                weekdays: Some(vec![]),
                ..Default::default()
            },
            0,
        )
        .unwrap();
        assert_eq!(active_rules(&conn).unwrap()[0].weekdays, None);
    }

    #[test]
    fn deleting_a_rule_keeps_the_episodes_it_already_scheduled() {
        let conn = db();
        let rule = create_rule(
            &conn,
            &NewRule {
                title: "Show",
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let programmes = vec![(programme("Show", 100_000), 1i64)];
        let added = expand_rules(&conn, &programmes, 0, 0).unwrap();
        assert_eq!(added.len(), 1);

        assert!(delete_rule(&conn, rule).unwrap());
        let rec = get(&conn, added[0]).unwrap().unwrap();
        assert_eq!(rec.state, RecordingState::Scheduled);
        // The link is severed, not the row.
        assert_eq!(rec.rule_id, None);
    }

    #[test]
    fn expansion_is_idempotent_across_epg_refreshes() {
        let conn = db();
        create_rule(
            &conn,
            &NewRule {
                title: "Show",
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let programmes = vec![
            (programme("Show", 100_000), 1i64),
            (programme("Something Else", 100_000), 2i64),
        ];
        assert_eq!(expand_rules(&conn, &programmes, 0, 0).unwrap().len(), 1);
        // Second refresh: same guide data, nothing new scheduled.
        assert!(expand_rules(&conn, &programmes, 0, 0).unwrap().is_empty());
        assert_eq!(list(&conn, None).unwrap().len(), 1);
    }

    #[test]
    fn expansion_uses_the_rules_own_padding_and_priority() {
        let conn = db();
        create_rule(
            &conn,
            &NewRule {
                title: "Show",
                pre_padding_secs: Some(300),
                post_padding_secs: Some(900),
                priority: 7,
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let programmes = vec![(programme("Show", 100_000), 1i64)];
        let id = expand_rules(&conn, &programmes, 0, 0).unwrap()[0];
        let rec = get(&conn, id).unwrap().unwrap();
        assert_eq!((rec.start, rec.stop), (99_700, 102_700));
        assert_eq!(rec.priority, 7);
    }

    #[test]
    fn expansion_ignores_programmes_that_already_aired() {
        let conn = db();
        create_rule(
            &conn,
            &NewRule {
                title: "Show",
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let programmes = vec![(programme("Show", 100_000), 1i64)];
        // `now` is past the programme's stop.
        assert!(expand_rules(&conn, &programmes, 0, 200_000)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn expansion_with_no_rules_does_no_work() {
        let conn = db();
        let programmes = vec![(programme("Show", 100_000), 1i64)];
        assert!(expand_rules(&conn, &programmes, 0, 0).unwrap().is_empty());
    }

    #[test]
    fn quota_pruning_takes_watched_recordings_before_unwatched_ones() {
        let conn = db();
        let mut ids = Vec::new();
        for (i, watched) in [false, true, false].into_iter().enumerate() {
            let id = schedule(&conn, &manual(1, "Show", 10_000 + i as i64 * 10_000), 0)
                .unwrap()
                .unwrap();
            set_state(&conn, id, RecordingState::Completed, None).unwrap();
            set_file(&conn, id, "f", 100, 1_800).unwrap();
            set_watched(&conn, id, watched).unwrap();
            ids.push(id);
        }
        assert_eq!(total_bytes(&conn).unwrap(), 300);
        assert!(over_quota(&conn, 300).unwrap().is_empty());
        // Room for two: the watched one goes even though it isn't the oldest.
        assert_eq!(over_quota(&conn, 200).unwrap(), vec![ids[1]]);
        // Room for one: then the oldest unwatched follows.
        assert_eq!(over_quota(&conn, 100).unwrap(), vec![ids[1], ids[0]]);
    }

    #[test]
    fn quota_pruning_never_returns_a_kept_recording() {
        let conn = db();
        let id = schedule(&conn, &manual(1, "Show", 10_000), 0)
            .unwrap()
            .unwrap();
        set_state(&conn, id, RecordingState::Completed, None).unwrap();
        set_file(&conn, id, "f", 500, 1_800).unwrap();
        set_keep(&conn, id, true).unwrap();
        // Over quota with nothing prunable: staying over beats deleting what was kept.
        assert!(over_quota(&conn, 100).unwrap().is_empty());
    }

    #[test]
    fn keep_episodes_prunes_the_oldest_beyond_the_limit() {
        let conn = db();
        let rule = create_rule(
            &conn,
            &NewRule {
                title: "Show",
                keep_episodes: Some(2),
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let mut ids = Vec::new();
        for i in 0..4 {
            let mut rec = manual(1, "Show", 10_000 + i * 10_000);
            rec.rule_id = Some(rule);
            let id = schedule(&conn, &rec, 0).unwrap().unwrap();
            set_state(&conn, id, RecordingState::Completed, None).unwrap();
            ids.push(id);
        }
        // Newest two stay; the two oldest are returned newest-first.
        assert_eq!(over_keep_limit(&conn, rule).unwrap(), vec![ids[1], ids[0]]);
    }

    #[test]
    fn a_rule_with_no_keep_limit_prunes_nothing() {
        let conn = db();
        let rule = create_rule(
            &conn,
            &NewRule {
                title: "Show",
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let mut rec = manual(1, "Show", 10_000);
        rec.rule_id = Some(rule);
        let id = schedule(&conn, &rec, 0).unwrap().unwrap();
        set_state(&conn, id, RecordingState::Completed, None).unwrap();
        assert!(over_keep_limit(&conn, rule).unwrap().is_empty());
    }

    #[test]
    fn reminders_fire_once_inside_their_lead_time() {
        let mut conn = db();
        let prog = programme("Show", 10_000);
        let id = add_reminder(&conn, 1, &prog, 120, 0).unwrap().unwrap();
        // Duplicate for the same airing is absorbed.
        assert!(add_reminder(&conn, 1, &prog, 120, 0).unwrap().is_none());

        assert!(fire_reminders(&mut conn, 9_000).unwrap().is_empty());
        let fired = fire_reminders(&mut conn, 9_900).unwrap();
        assert_eq!(fired.iter().map(|r| r.id).collect::<Vec<_>>(), vec![id]);
        // Already fired: it doesn't come back.
        assert!(fire_reminders(&mut conn, 9_950).unwrap().is_empty());
    }

    #[test]
    fn reminders_can_be_removed_and_pruned() {
        let mut conn = db();
        let id = add_reminder(&conn, 1, &programme("A", 10_000), 120, 0)
            .unwrap()
            .unwrap();
        assert!(remove_reminder(&conn, id).unwrap());
        assert!(!remove_reminder(&conn, id).unwrap());

        add_reminder(&conn, 1, &programme("B", 10_000), 120, 0)
            .unwrap()
            .unwrap();
        // A reminder for something that already started is no use to anyone.
        assert_eq!(prune_reminders(&conn, 20_000).unwrap(), 1);
        assert!(fire_reminders(&mut conn, 20_000).unwrap().is_empty());
    }

    #[test]
    fn deleting_a_channel_takes_its_recordings_and_reminders_with_it() {
        let conn = db();
        schedule(&conn, &manual(1, "Show", 10_000), 0)
            .unwrap()
            .unwrap();
        add_reminder(&conn, 1, &programme("Show", 10_000), 120, 0)
            .unwrap()
            .unwrap();
        conn.execute("DELETE FROM channels WHERE id = 1", [])
            .unwrap();
        assert!(list(&conn, None).unwrap().is_empty());
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM reminders", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn list_filters_by_state() {
        let conn = db();
        let a = schedule(&conn, &manual(1, "A", 10_000), 0)
            .unwrap()
            .unwrap();
        let b = schedule(&conn, &manual(2, "B", 20_000), 0)
            .unwrap()
            .unwrap();
        set_state(&conn, b, RecordingState::Completed, None).unwrap();
        assert_eq!(
            list(&conn, Some(RecordingState::Scheduled))
                .unwrap()
                .iter()
                .map(|r| r.id)
                .collect::<Vec<_>>(),
            vec![a]
        );
        assert_eq!(list(&conn, None).unwrap().len(), 2);
    }
}
