//! Which stream to try next for a channel (README §7.14, §23 item 4).
//!
//! One logical channel can have several URLs. Picking between them is not just
//! "lowest priority first": a source that failed a minute ago is a bad bet right now,
//! and a source that failed last week is not.
//!
//! The rule that matters is that **no source is ever banned**. A provider having a bad
//! ten minutes must not permanently demote the stream the viewer actually wants, and a
//! channel whose every source is currently sick must still produce something to try —
//! a list that filtered out unhealthy sources could return nothing at all, which is a
//! worse outcome than trying one that might work.
//!
//! So failure buys a cooling-off period, proportional to how many failures in a row and
//! capped, after which the source returns to its place in line. Anything that has
//! worked since it last failed is treated as healthy immediately.

/// Each consecutive failure adds this much cooling-off.
pub const COOLDOWN_STEP_SECS: i64 = 60;
/// However bad it gets, a source is retried at least this often.
pub const MAX_COOLDOWN_SECS: i64 = 6 * COOLDOWN_STEP_SECS;

/// What is known about one source's recent behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Health {
    /// The provider's or the viewer's preferred order. Lower is better.
    pub priority: i64,
    pub fail_count: u32,
    pub last_ok_at: Option<i64>,
    pub last_fail_at: Option<i64>,
}

impl Health {
    /// True when the source has worked since the last time it failed.
    ///
    /// Without this a source that failed once, was retried, and worked would carry its
    /// failure for ever — the count alone cannot tell the difference between "broken"
    /// and "was briefly broken".
    pub fn recovered(&self) -> bool {
        match (self.last_ok_at, self.last_fail_at) {
            (Some(ok), Some(fail)) => ok >= fail,
            (Some(_), None) => true,
            _ => false,
        }
    }

    /// How long this source stays out of favour after its latest failure.
    pub fn cooldown_secs(&self) -> i64 {
        if self.recovered() || self.fail_count == 0 {
            return 0;
        }
        (i64::from(self.fail_count) * COOLDOWN_STEP_SECS).min(MAX_COOLDOWN_SECS)
    }

    /// Whether the cooling-off period is still running at `now`.
    pub fn cooling_down(&self, now: i64) -> bool {
        let Some(failed_at) = self.last_fail_at else {
            return false;
        };
        if self.recovered() {
            return false;
        }
        // A clock that went backwards should not freeze a source out for ever.
        now >= failed_at && now - failed_at < self.cooldown_secs()
    }
}

/// Sort key for one source, smallest first.
///
/// Cooling-down sources sort last rather than being removed, so the list always offers
/// every URL the channel has.
pub fn rank(health: &Health, now: i64) -> (u8, i64, u32) {
    (
        u8::from(health.cooling_down(now)),
        health.priority,
        health.fail_count,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_760_000_000;

    fn fresh(priority: i64) -> Health {
        Health {
            priority,
            ..Default::default()
        }
    }

    fn failed(priority: i64, fails: u32, ago: i64) -> Health {
        Health {
            priority,
            fail_count: fails,
            last_fail_at: Some(NOW - ago),
            last_ok_at: None,
        }
    }

    #[test]
    fn priority_decides_between_healthy_sources() {
        assert!(rank(&fresh(0), NOW) < rank(&fresh(1), NOW));
        assert!(rank(&fresh(1), NOW) < rank(&fresh(2), NOW));
    }

    #[test]
    fn a_source_that_just_failed_goes_last_even_if_it_is_preferred() {
        let preferred_but_broken = failed(0, 1, 5);
        let backup = fresh(9);
        assert!(rank(&backup, NOW) < rank(&preferred_but_broken, NOW));
    }

    #[test]
    fn a_failure_is_never_permanent() {
        // The whole point: one bad night must not sink the best stream for ever.
        let long_ago = failed(0, 3, 24 * 3600);
        assert!(!long_ago.cooling_down(NOW));
        assert!(rank(&long_ago, NOW) < rank(&fresh(9), NOW));
    }

    #[test]
    fn repeated_failures_wait_longer_but_not_for_ever() {
        assert_eq!(failed(0, 1, 0).cooldown_secs(), COOLDOWN_STEP_SECS);
        assert_eq!(failed(0, 3, 0).cooldown_secs(), 3 * COOLDOWN_STEP_SECS);
        assert_eq!(failed(0, 99, 0).cooldown_secs(), MAX_COOLDOWN_SECS);
        assert!(!failed(0, 99, MAX_COOLDOWN_SECS + 1).cooling_down(NOW));
    }

    #[test]
    fn working_once_clears_the_record() {
        let recovered = Health {
            priority: 0,
            fail_count: 4,
            last_fail_at: Some(NOW - 10),
            last_ok_at: Some(NOW - 5),
        };
        assert!(recovered.recovered());
        assert_eq!(recovered.cooldown_secs(), 0);
        assert!(!recovered.cooling_down(NOW));
        assert!(rank(&recovered, NOW) < rank(&fresh(1), NOW));
    }

    #[test]
    fn failing_again_after_recovering_starts_a_new_cooldown() {
        let broken_again = Health {
            priority: 0,
            fail_count: 1,
            last_ok_at: Some(NOW - 600),
            last_fail_at: Some(NOW - 5),
        };
        assert!(!broken_again.recovered());
        assert!(broken_again.cooling_down(NOW));
    }

    #[test]
    fn every_source_cooling_down_still_leaves_an_order() {
        // A channel whose sources are all sick must still offer something to try:
        // ranking sorts them, it never filters them out.
        let mut all = [failed(2, 1, 1), failed(0, 1, 1), failed(1, 1, 1)];
        all.sort_by_key(|h| rank(h, NOW));
        assert_eq!(all.map(|h| h.priority), [0, 1, 2]);
    }

    #[test]
    fn a_clock_that_went_backwards_does_not_freeze_a_source_out() {
        // A failure timestamped in the future would otherwise cool down for ever.
        let from_the_future = failed(0, 1, -3600);
        assert!(!from_the_future.cooling_down(NOW));
    }
}
