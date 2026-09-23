//! A channel's stream URLs, and what happened last time each was tried.
//!
//! `channel_sources` has carried `fail_count`, `last_ok_at` and `last_fail_at` since
//! the first migration and nothing ever wrote them, so the ordering they were meant to
//! drive never happened. This is the half that records outcomes; the policy that reads
//! them is `aurora_core::sources`.

use aurora_core::sources::{rank, Health};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: i64,
    pub url: String,
    pub priority: i64,
    pub quality: Option<String>,
    pub fail_count: u32,
    pub last_ok_at: Option<i64>,
    pub last_fail_at: Option<i64>,
}

impl Source {
    pub fn health(&self) -> Health {
        Health {
            priority: self.priority,
            fail_count: self.fail_count,
            last_ok_at: self.last_ok_at,
            last_fail_at: self.last_fail_at,
        }
    }
}

/// Every URL for a channel, best bet first.
///
/// Never filtered: a channel whose sources have all failed recently still has to offer
/// something to try (`aurora_core::sources`).
pub fn for_channel(conn: &Connection, channel_id: i64, now: i64) -> Result<Vec<Source>> {
    let mut stmt = conn.prepare(
        "SELECT id, url, priority, quality, fail_count, last_ok_at, last_fail_at
         FROM channel_sources WHERE channel_id = ?1",
    )?;
    let mut rows = stmt
        .query_map(params![channel_id], |r| {
            Ok(Source {
                id: r.get(0)?,
                url: r.get(1)?,
                priority: r.get(2)?,
                quality: r.get(3)?,
                fail_count: r.get::<_, i64>(4)?.max(0) as u32,
                last_ok_at: r.get(5)?,
                last_fail_at: r.get(6)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    // The id breaks ties, so the same library always produces the same order.
    rows.sort_by_key(|s| (rank(&s.health(), now), s.id));
    Ok(rows)
}

/// This source just played. Clearing the count is what stops one bad night from
/// following a good stream around for ever.
pub fn record_ok(conn: &Connection, source_id: i64, now: i64) -> Result<()> {
    conn.execute(
        "UPDATE channel_sources SET last_ok_at = ?2, fail_count = 0 WHERE id = ?1",
        params![source_id, now],
    )?;
    Ok(())
}

/// This source would not play, or stopped playing.
pub fn record_failure(conn: &Connection, source_id: i64, now: i64) -> Result<()> {
    conn.execute(
        "UPDATE channel_sources SET last_fail_at = ?2, fail_count = fail_count + 1
         WHERE id = ?1",
        params![source_id, now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_760_000_000;

    fn seeded() -> Connection {
        let conn = crate::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO channels (id, provider_id, provider_key, name, match_key, last_seen_at)
             VALUES (1, 1, 'c1', 'BBC One', 'bbcone', 0)",
            [],
        )
        .unwrap();
        for (i, priority) in [0i64, 1, 2].iter().enumerate() {
            conn.execute(
                "INSERT INTO channel_sources (channel_id, url, priority)
                 VALUES (1, ?1, ?2)",
                params![format!("http://example.com/{i}.ts"), priority],
            )
            .unwrap();
        }
        conn
    }

    fn ids(conn: &Connection) -> Vec<i64> {
        for_channel(conn, 1, NOW)
            .unwrap()
            .iter()
            .map(|s| s.id)
            .collect()
    }

    #[test]
    fn untouched_sources_come_back_in_priority_order() {
        assert_eq!(ids(&seeded()), vec![1, 2, 3]);
    }

    #[test]
    fn a_failure_is_recorded_and_moves_the_source_down() {
        let conn = seeded();
        record_failure(&conn, 1, NOW).unwrap();
        assert_eq!(ids(&conn), vec![2, 3, 1]);

        let first = for_channel(&conn, 1, NOW)
            .unwrap()
            .into_iter()
            .find(|s| s.id == 1)
            .unwrap();
        assert_eq!(first.fail_count, 1);
        assert_eq!(first.last_fail_at, Some(NOW));
    }

    #[test]
    fn success_restores_a_demoted_source_immediately() {
        let conn = seeded();
        record_failure(&conn, 1, NOW).unwrap();
        assert_eq!(ids(&conn)[0], 2);

        record_ok(&conn, 1, NOW + 1).unwrap();
        assert_eq!(
            ids(&conn),
            vec![1, 2, 3],
            "it worked, so it is preferred again"
        );
        let first = for_channel(&conn, 1, NOW)
            .unwrap()
            .into_iter()
            .find(|s| s.id == 1)
            .unwrap();
        assert_eq!(first.fail_count, 0);
    }

    #[test]
    fn a_source_returns_to_its_place_once_the_cooldown_passes() {
        let conn = seeded();
        record_failure(&conn, 1, NOW).unwrap();
        assert_eq!(ids(&conn)[0], 2);

        let later = NOW + aurora_core::sources::MAX_COOLDOWN_SECS + 1;
        assert_eq!(
            for_channel(&conn, 1, later).unwrap().first().map(|s| s.id),
            Some(1),
            "a failure buys a cooling-off period, not a ban"
        );
    }

    #[test]
    fn every_source_failing_still_yields_every_source() {
        let conn = seeded();
        for id in [1, 2, 3] {
            record_failure(&conn, id, NOW).unwrap();
        }
        // Returning nothing here would turn "the provider is having a moment" into
        // "this channel does not exist".
        assert_eq!(ids(&conn), vec![1, 2, 3]);
    }

    #[test]
    fn a_channel_with_no_sources_is_empty_not_an_error() {
        let conn = seeded();
        assert!(for_channel(&conn, 999, NOW).unwrap().is_empty());
    }

    #[test]
    fn recording_against_a_source_that_is_gone_is_not_an_error() {
        // A refresh can remove a source while it is playing.
        let conn = seeded();
        assert!(record_failure(&conn, 4242, NOW).is_ok());
        assert!(record_ok(&conn, 4242, NOW).is_ok());
    }
}
