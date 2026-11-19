//! Settings → Privacy (README §15): clear watch history, and how long to keep it.
//!
//! The README promised both ("history retention, clear history") and there was neither.
//! A viewer could take one title at a time off Continue Watching and that was all: the
//! rest of what they had watched stayed for good, kept feeding recommendations, and
//! synced to every device in the household.
//!
//! Both act on `watch_progress`, which is the history: Continue Watching, the ticks on
//! episodes, "Because you watched" and the Gemini rail are all read from it. Its sync
//! triggers carry the deletions to the household's other devices, so clearing here
//! clears there.

use aurora_db::repo::progress::{self, WatchHistory, MAX_HISTORY_DAYS};
use aurora_db::rusqlite::Connection;
use serde::Deserialize;
use tauri::State;

use crate::error::{AppError, Result};
use crate::now_unix;
use crate::services::Services;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryArgs {
    pub profile_id: i64,
}

/// How long history is kept, and how much this profile has.
#[tauri::command(async)]
pub fn privacy_history(services: State<'_, Services>, args: HistoryArgs) -> Result<WatchHistory> {
    Ok(progress::watch_history(
        &services.db.lock(),
        args.profile_id,
    )?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryDaysArgs {
    /// `None` keeps history for good.
    pub days: Option<u32>,
}

/// Choose how long history is kept, and apply it now rather than at the next sweep.
///
/// Returns how many entries the new window removed, so Settings can say so.
#[tauri::command(async)]
pub fn privacy_set_history_days(
    services: State<'_, Services>,
    args: HistoryDaysArgs,
) -> Result<usize> {
    crate::profiles::require_settings(&services.db.lock())?;
    let db = services.db.lock();
    set_history_days(&db, args.days, now_unix())
}

/// What the command does, without Tauri in the way.
pub(crate) fn set_history_days(db: &Connection, days: Option<u32>, now: i64) -> Result<usize> {
    if let Some(d) = days {
        if !(1..=MAX_HISTORY_DAYS).contains(&d) {
            return Err(AppError::Other(format!(
                "History can be kept for 1 to {MAX_HISTORY_DAYS} days, or for good."
            )));
        }
    }
    progress::set_history_days(db, days)?;
    Ok(progress::prune_history(db, now)?)
}

/// Forget everything one profile has watched.
///
/// Also drops the cached Gemini suggestions: they are a reading of that history, and a
/// rail still drawn from it after the viewer cleared it would be the history surviving
/// under another name.
#[tauri::command(async)]
pub fn privacy_clear_history(services: State<'_, Services>, args: HistoryArgs) -> Result<usize> {
    crate::profiles::require_settings(&services.db.lock())?;
    let db = services.db.lock();
    let removed = progress::clear_history(&db, args.profile_id)?;
    crate::gemini::forget_cached(&db);
    Ok(removed)
}

/// Apply the retention window. The refresh sweep runs this two minutes after launch and
/// every half hour after, so a device left on for weeks keeps to it as well as one
/// restarted daily.
pub fn prune(services: &Services, now: i64) {
    match progress::prune_history(&services.db.lock(), now) {
        Ok(0) => {}
        Ok(n) => tracing::info!("forgot {n} watch history entries past the retention window"),
        Err(e) => tracing::warn!("could not apply the watch history window: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = aurora_db::open_memory().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO profiles (id, name, created_at) VALUES (1,'Me',0)",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn choosing_a_window_applies_it_at_once_and_refuses_nonsense() {
        let conn = db();
        let day = 86_400;
        let now = 400 * day;
        for (id, ago) in [(1, 200), (2, 10)] {
            progress::save(
                &conn,
                1,
                progress::ItemKind::Movie,
                id,
                60,
                600,
                now - ago * day,
            )
            .unwrap();
        }

        assert!(set_history_days(&conn, Some(0), now).is_err());
        assert!(set_history_days(&conn, Some(MAX_HISTORY_DAYS + 1), now).is_err());
        assert_eq!(progress::watch_history(&conn, 1).unwrap().items, 2);

        assert_eq!(set_history_days(&conn, Some(90), now).unwrap(), 1);
        assert_eq!(
            progress::watch_history(&conn, 1).unwrap(),
            WatchHistory {
                days: Some(90),
                items: 1
            }
        );
        assert_eq!(set_history_days(&conn, None, now).unwrap(), 0);
        assert_eq!(progress::watch_history(&conn, 1).unwrap().days, None);
    }
}
