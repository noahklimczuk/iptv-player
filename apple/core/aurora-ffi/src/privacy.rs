//! Settings → Privacy (README §15): clear watch history, and how long to keep it.
//! Windows' `aurora-app/src/privacy.rs`, on the same `aurora_db::repo::progress` calls.
//!
//! Both act on `watch_progress`, which is the history: Continue Watching, the ticks on
//! episodes, "Because you watched" and the Gemini rail are all read from it. Its sync
//! triggers carry the deletions to the household's other devices, so clearing here
//! clears there.

use aurora_db::repo::progress::{self, MAX_HISTORY_DAYS};
use aurora_db::rusqlite::Connection;

use crate::error::{AuroraError, Result};
use crate::parental::require_settings;
use crate::{now_unix, AuroraCore};

/// How long history is kept, and how much this profile has.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WatchHistory {
    /// `None` keeps everything.
    pub days: Option<u32>,
    /// Titles and channels this profile has a position or a "watched" for.
    pub items: u64,
}

/// What `privacy_set_history_days` does, without the lock in the way.
pub(crate) fn set_history_days(db: &Connection, days: Option<u32>, now: i64) -> Result<u64> {
    if let Some(d) = days {
        if !(1..=MAX_HISTORY_DAYS).contains(&d) {
            return Err(AuroraError::other(format!(
                "History can be kept for 1 to {MAX_HISTORY_DAYS} days, or for good."
            )));
        }
    }
    progress::set_history_days(db, days)?;
    Ok(progress::prune_history(db, now)? as u64)
}

#[uniffi::export]
impl AuroraCore {
    /// How long history is kept, and how much this profile has (`privacy.history`).
    pub fn privacy_history(&self, profile_id: i64) -> Result<WatchHistory> {
        let h = progress::watch_history(&self.db.lock(), profile_id)?;
        Ok(WatchHistory {
            days: h.days,
            items: h.items.max(0) as u64,
        })
    }

    /// Choose how long history is kept, and apply it now rather than at the next sweep.
    /// Returns how many entries the new window removed, so Settings can say so.
    pub fn privacy_set_history_days(&self, days: Option<u32>) -> Result<u64> {
        let db = self.db.lock();
        require_settings(&db)?;
        set_history_days(&db, days, now_unix())
    }

    /// Forget everything one profile has watched. Also drops the cached Gemini
    /// suggestions: they are a reading of that history, and a rail still drawn from it
    /// would be the history surviving under another name.
    pub fn privacy_clear_history(&self, profile_id: i64) -> Result<u64> {
        let db = self.db.lock();
        require_settings(&db)?;
        let removed = progress::clear_history(&db, profile_id)?;
        crate::gemini::forget_cached(&db);
        Ok(removed as u64)
    }

    /// Apply the retention window. Swift runs this two minutes after launch and every
    /// half hour after, as Windows' refresh sweep does, so a device left on for weeks
    /// keeps to it as well as one restarted daily.
    pub fn privacy_prune(&self) {
        match progress::prune_history(&self.db.lock(), now_unix()) {
            Ok(0) => {}
            Ok(n) => tracing::info!("forgot {n} watch history entries past the retention window"),
            Err(e) => tracing::warn!("could not apply the watch history window: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choosing_a_window_applies_it_at_once_and_refuses_nonsense() {
        let conn = aurora_db::open_memory().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO profiles (id, name, created_at) VALUES (1,'Me',0)",
            [],
        )
        .unwrap();
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
        let h = progress::watch_history(&conn, 1).unwrap();
        assert_eq!((h.days, h.items), (Some(90), 1));
        assert_eq!(set_history_days(&conn, None, now).unwrap(), 0);
        assert_eq!(progress::watch_history(&conn, 1).unwrap().days, None);
    }
}
