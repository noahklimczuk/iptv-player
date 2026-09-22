//! Watch progress, continue-watching, and Up Next.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// README §6.3: movies count as watched at 92%, episodes at 95%.
pub const MOVIE_COMPLETE_RATIO: f32 = 0.92;
pub const EPISODE_COMPLETE_RATIO: f32 = 0.95;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    Movie,
    Episode,
    Channel,
    Recording,
}

impl ItemKind {
    fn as_str(self) -> &'static str {
        match self {
            ItemKind::Movie => "movie",
            ItemKind::Episode => "episode",
            ItemKind::Channel => "channel",
            ItemKind::Recording => "recording",
        }
    }

    fn complete_ratio(self) -> f32 {
        match self {
            ItemKind::Episode => EPISODE_COMPLETE_RATIO,
            _ => MOVIE_COMPLETE_RATIO,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub item_kind: ItemKind,
    pub item_id: i64,
    pub position_secs: i64,
    pub duration_secs: i64,
    pub completed: bool,
    pub updated_at: i64,
}

impl Progress {
    pub fn percent(&self) -> f32 {
        if self.duration_secs <= 0 {
            0.0
        } else {
            (self.position_secs as f32 / self.duration_secs as f32).clamp(0.0, 1.0)
        }
    }
}

/// Save a position. Completion is derived here rather than trusted from the caller so the
/// rule stays in one place.
pub fn save(
    conn: &Connection,
    profile_id: i64,
    kind: ItemKind,
    item_id: i64,
    position_secs: i64,
    duration_secs: i64,
    now: i64,
) -> Result<bool> {
    let completed =
        duration_secs > 0 && (position_secs as f32 / duration_secs as f32) >= kind.complete_ratio();

    conn.execute(
        r#"INSERT INTO watch_progress
           (profile_id, item_kind, item_id, position_secs, duration_secs, completed, updated_at)
           VALUES (?1,?2,?3,?4,?5,?6,?7)
           ON CONFLICT (profile_id, item_kind, item_id) DO UPDATE SET
             position_secs = excluded.position_secs,
             duration_secs = excluded.duration_secs,
             -- once complete, stay complete unless the user restarts from the beginning
             completed     = CASE WHEN excluded.position_secs < 60 THEN 0
                                  ELSE watch_progress.completed | excluded.completed END,
             updated_at    = excluded.updated_at"#,
        params![
            profile_id,
            kind.as_str(),
            item_id,
            position_secs,
            duration_secs,
            completed as i32,
            now
        ],
    )?;
    Ok(completed)
}

pub fn get(
    conn: &Connection,
    profile_id: i64,
    kind: ItemKind,
    item_id: i64,
) -> Result<Option<Progress>> {
    Ok(conn
        .query_row(
            "SELECT position_secs, duration_secs, completed, updated_at
             FROM watch_progress WHERE profile_id = ?1 AND item_kind = ?2 AND item_id = ?3",
            params![profile_id, kind.as_str(), item_id],
            |r| {
                Ok(Progress {
                    item_kind: kind,
                    item_id,
                    position_secs: r.get(0)?,
                    duration_secs: r.get(1)?,
                    completed: r.get::<_, i64>(2)? != 0,
                    updated_at: r.get(3)?,
                })
            },
        )
        .optional()?)
}

/// The Continue Watching rail: started, not finished, most recent first.
pub fn continue_watching(conn: &Connection, profile_id: i64, limit: u32) -> Result<Vec<Progress>> {
    let mut stmt = conn.prepare(
        "SELECT item_kind, item_id, position_secs, duration_secs, completed, updated_at
         FROM watch_progress
         WHERE profile_id = ?1 AND completed = 0 AND position_secs > 60
           AND item_kind IN ('movie','episode')
         ORDER BY updated_at DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![profile_id, limit], |r| {
            let kind: String = r.get(0)?;
            Ok(Progress {
                item_kind: match kind.as_str() {
                    "episode" => ItemKind::Episode,
                    "channel" => ItemKind::Channel,
                    "recording" => ItemKind::Recording,
                    _ => ItemKind::Movie,
                },
                item_id: r.get(1)?,
                position_secs: r.get(2)?,
                duration_secs: r.get(3)?,
                completed: r.get::<_, i64>(4)? != 0,
                updated_at: r.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// README §9: the next unwatched episode of a show the user is partway through.
pub fn next_episode(conn: &Connection, profile_id: i64, series_id: i64) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            r#"SELECT e.id FROM episodes e
               LEFT JOIN watch_progress w
                 ON w.item_id = e.id AND w.item_kind = 'episode' AND w.profile_id = ?1
               WHERE e.series_id = ?2 AND COALESCE(w.completed, 0) = 0
               ORDER BY e.season, e.episode LIMIT 1"#,
            params![profile_id, series_id],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn mark_watched(
    conn: &Connection,
    profile_id: i64,
    kind: ItemKind,
    item_id: i64,
    watched: bool,
    now: i64,
) -> Result<()> {
    conn.execute(
        r#"INSERT INTO watch_progress
           (profile_id, item_kind, item_id, position_secs, duration_secs, completed, updated_at)
           VALUES (?1,?2,?3,0,0,?4,?5)
           ON CONFLICT (profile_id, item_kind, item_id) DO UPDATE SET
             completed = excluded.completed, updated_at = excluded.updated_at"#,
        params![profile_id, kind.as_str(), item_id, watched as i32, now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO profiles (id, name, created_at) VALUES (1,'Me',0)",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn saves_and_reads_back_progress() {
        let conn = db();
        save(&conn, 1, ItemKind::Movie, 7, 600, 6000, 100).unwrap();
        let p = get(&conn, 1, ItemKind::Movie, 7).unwrap().unwrap();
        assert_eq!(p.position_secs, 600);
        assert!(!p.completed);
        assert!((p.percent() - 0.1).abs() < 0.001);
    }

    #[test]
    fn movies_complete_at_92_percent() {
        let conn = db();
        assert!(!save(&conn, 1, ItemKind::Movie, 1, 9100, 10_000, 0).unwrap());
        assert!(save(&conn, 1, ItemKind::Movie, 2, 9200, 10_000, 0).unwrap());
    }

    #[test]
    fn episodes_complete_at_95_percent() {
        let conn = db();
        assert!(!save(&conn, 1, ItemKind::Episode, 1, 9400, 10_000, 0).unwrap());
        assert!(save(&conn, 1, ItemKind::Episode, 2, 9500, 10_000, 0).unwrap());
    }

    #[test]
    fn completion_is_sticky_but_restarting_clears_it() {
        let conn = db();
        save(&conn, 1, ItemKind::Movie, 1, 9500, 10_000, 0).unwrap();
        assert!(
            get(&conn, 1, ItemKind::Movie, 1)
                .unwrap()
                .unwrap()
                .completed
        );

        // Scrubbing back mid-film should not un-complete it.
        save(&conn, 1, ItemKind::Movie, 1, 5000, 10_000, 1).unwrap();
        assert!(
            get(&conn, 1, ItemKind::Movie, 1)
                .unwrap()
                .unwrap()
                .completed
        );

        // Starting over from the top should.
        save(&conn, 1, ItemKind::Movie, 1, 10, 10_000, 2).unwrap();
        assert!(
            !get(&conn, 1, ItemKind::Movie, 1)
                .unwrap()
                .unwrap()
                .completed
        );
    }

    #[test]
    fn continue_watching_excludes_finished_and_barely_started() {
        let conn = db();
        save(&conn, 1, ItemKind::Movie, 1, 3000, 10_000, 10).unwrap(); // in progress
        save(&conn, 1, ItemKind::Movie, 2, 9900, 10_000, 20).unwrap(); // finished
        save(&conn, 1, ItemKind::Movie, 3, 5, 10_000, 30).unwrap(); // just opened
        save(&conn, 1, ItemKind::Channel, 4, 500, 0, 40).unwrap(); // live TV

        let rows = continue_watching(&conn, 1, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].item_id, 1);
    }

    #[test]
    fn continue_watching_is_most_recent_first() {
        let conn = db();
        save(&conn, 1, ItemKind::Movie, 1, 3000, 10_000, 10).unwrap();
        save(&conn, 1, ItemKind::Movie, 2, 3000, 10_000, 99).unwrap();
        let rows = continue_watching(&conn, 1, 10).unwrap();
        assert_eq!(rows[0].item_id, 2);
    }

    #[test]
    fn progress_is_per_profile() {
        let conn = db();
        conn.execute(
            "INSERT INTO profiles (id,name,created_at) VALUES (2,'Kid',0)",
            [],
        )
        .unwrap();
        save(&conn, 1, ItemKind::Movie, 1, 3000, 10_000, 0).unwrap();
        assert!(get(&conn, 2, ItemKind::Movie, 1).unwrap().is_none());
    }

    #[test]
    fn up_next_returns_the_first_unwatched_episode() {
        let conn = db();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO series (id,provider_id,provider_key,title,match_key,last_seen_at)
             VALUES (1,1,'s','Show','show',0)",
            [],
        )
        .unwrap();
        for (id, s, e) in [(10, 1, 1), (11, 1, 2), (12, 2, 1)] {
            conn.execute(
                "INSERT INTO episodes (id,series_id,season,episode,url) VALUES (?1,1,?2,?3,'u')",
                params![id, s, e],
            )
            .unwrap();
        }

        assert_eq!(next_episode(&conn, 1, 1).unwrap(), Some(10));
        mark_watched(&conn, 1, ItemKind::Episode, 10, true, 0).unwrap();
        assert_eq!(next_episode(&conn, 1, 1).unwrap(), Some(11));
        mark_watched(&conn, 1, ItemKind::Episode, 11, true, 0).unwrap();
        assert_eq!(next_episode(&conn, 1, 1).unwrap(), Some(12));
    }

    #[test]
    fn mark_unwatched_reopens_an_episode() {
        let conn = db();
        mark_watched(&conn, 1, ItemKind::Episode, 5, true, 0).unwrap();
        assert!(
            get(&conn, 1, ItemKind::Episode, 5)
                .unwrap()
                .unwrap()
                .completed
        );
        mark_watched(&conn, 1, ItemKind::Episode, 5, false, 1).unwrap();
        assert!(
            !get(&conn, 1, ItemKind::Episode, 5)
                .unwrap()
                .unwrap()
                .completed
        );
    }

    #[test]
    fn zero_duration_never_divides_by_zero() {
        let conn = db();
        save(&conn, 1, ItemKind::Channel, 1, 500, 0, 0).unwrap();
        let p = get(&conn, 1, ItemKind::Channel, 1).unwrap().unwrap();
        assert_eq!(p.percent(), 0.0);
    }
}
