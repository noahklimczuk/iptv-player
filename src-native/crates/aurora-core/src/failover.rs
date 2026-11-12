//! What to do about a live stream that has stopped (README §7.14, §23 item 4).
//!
//! A channel has several URLs and a provider hands out streams that 404 on a Tuesday
//! and stall on a Wednesday, so a stream that dies is not the end of the channel. The
//! rules for what happens next are small, and every one of them was learned from a
//! viewer complaint rather than designed:
//!
//! * **Another source is tried at once.** It has not failed yet; making somebody wait
//!   out a backoff before trying a URL that is probably fine is a worse picture for no
//!   reason.
//! * **The last source is retried, after waiting.** A stream that stopped was working a
//!   second ago, which is the one thing a stall tells you that a failed tune does not —
//!   so on a channel with a single source, which is ordinary on a real panel, the answer
//!   is to ask again rather than to give up. But not in the same breath: a provider that
//!   has just dropped the connection is not ready a quarter-second later.
//! * **Both are bounded.** A total outage must not spin through every URL a provider
//!   owns for ever. A stopped picture with an error on it is more honest than an endless
//!   reconnect.
//! * **Retries are consecutive, not lifetime.** An evening's viewing accumulates stalls,
//!   and arriving at the one that mattered with nothing left to try is the bug that
//!   `Attempt::recovered` exists to prevent. The caller clears the counters the moment
//!   playback is healthy.
//!
//! ## Why this is a module and not a method
//!
//! It was a method, on `Playback`, reachable only through a `Mutex<Option<LiveSession>>`
//! and a player backend — and the multi-view tiles, which are live streams by any
//! definition, therefore had none of it. A tile took the first URL of the several its
//! channel had, never tried another, and when its stream died `Mosaic::tick` wrote an
//! error string and did nothing else. Nine tiles against a provider that hiccups is the
//! same complaint that produced `MAX_RETRIES`, one layer up.
//!
//! The same asymmetry had already happened once inside `Playback`: a film had retries
//! from the start and a channel did not, because the two paths were written at different
//! times in different places. So the policy lives here, pure, with the numbers and the
//! reasoning in one place, and both callers are thin shells over `next`.

/// How many times one failing session may roll to another source before giving up.
///
/// Bounded because a provider having a total outage would otherwise spin through every
/// URL it owns for ever.
pub const MAX_ROLLOVERS: u32 = 6;

/// How many times a live stream's *current* source is reloaded before rolling over.
///
/// Two, not three. A live stream has no position to resume from, so a retry costs the
/// viewer the same second or two a rollover does — but it is far likelier to work,
/// because the stream that just stopped is overwhelmingly the stream that will be fine
/// in a moment. Rolling over remains what happens when retrying does not.
pub const MAX_RETRIES: u32 = 2;

/// Seconds to wait before each reload, indexed by how many have been tried.
///
/// Short, because live has a cost a film does not: every second spent waiting is a
/// second of the programme gone for good. Long enough that a provider which has just
/// dropped the connection is not hammered, short enough that a recovered stream looks
/// like a stutter rather than an outage.
pub const BACKOFF_SECS: [i64; MAX_RETRIES as usize] = [1, 3];

/// Where a failing session has got to.
///
/// Deliberately plain data. The caller owns the URLs, the player and the clock; this
/// carries only what the decision depends on, which is what makes the decision testable
/// without any of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Attempt {
    /// Index into the caller's source list of the one currently loaded.
    pub current: usize,
    /// How many sources the channel has.
    pub sources: usize,
    /// How many sources have been given up on during this session.
    pub rollovers: u32,
    /// Consecutive reloads of the *current* source. Reset by the caller the moment
    /// playback is healthy, which is what makes the allowance three in a row rather
    /// than three in an evening.
    pub retries: u32,
    /// When the current failure was first seen, so the backoff is measured against it.
    /// `None` while playing, and cleared again before each retry.
    pub failed_at: Option<i64>,
}

impl Attempt {
    /// Whether this is the first heartbeat to see the current failure.
    ///
    /// The caller records a failure against the source exactly when this is true.
    /// `next` is asked on every heartbeat — four times a second — and answers `Wait`
    /// for the whole of the backoff, so a caller that recorded on every call would
    /// turn one stall into a dozen-odd failures. `fail_count` is not a log: it is what
    /// `crate::sources::Health::cooldown_secs` multiplies by `COOLDOWN_STEP_SECS`, so
    /// over-counting pins a healthy source at the maximum cooldown and the ordering
    /// meant to favour the stream that works reads a number many times the truth.
    pub fn first_sight(&self) -> bool {
        self.failed_at.is_none()
    }
}

/// What the caller should do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Note the time and let the error stand. Nothing is loaded.
    ///
    /// The first sight of a failure does not reload at once, deliberately: pretending
    /// otherwise by holding the old state would be showing a picture that is not there.
    StartClock,
    /// The backoff has not elapsed yet. Do nothing at all.
    Wait,
    /// Load the source at this index, counting a rollover and resetting the retries.
    ///
    /// The next source has not failed yet and must get its own allowance rather than
    /// inheriting the exhausted count of the one before it.
    RollTo(usize),
    /// Reload the current source, counting a retry.
    Retry,
    /// Out of rollovers. Leave the session as it is — a later deliberate tune re-reads
    /// the source order, by which time the failures recorded will have moved the good
    /// one to the front.
    Stop,
    /// No other source and no retries left. Drop the session rather than looping over
    /// the same dead list.
    Abandon,
}

/// Decide what to do about a session whose stream has stopped.
///
/// Pure, and asked on every heartbeat: `Wait` is the common answer and means the caller
/// should do nothing, not that anything is wrong.
pub fn next(attempt: Attempt, now: i64) -> Next {
    if attempt.rollovers >= MAX_ROLLOVERS {
        return Next::Stop;
    }

    // Another URL to try. Taken at once rather than after a backoff, because it has not
    // failed yet -- this is the case a channel with several sources hits, and waiting
    // would cost a picture that was available.
    let next_index = attempt.current + 1;
    if next_index < attempt.sources {
        return Next::RollTo(next_index);
    }

    // Nothing to roll to, so retrying this one is worth it -- which it was not a moment
    // ago, while a different source was available to roll to instantly.
    if attempt.retries >= MAX_RETRIES {
        return Next::Abandon;
    }
    let Some(since) = attempt.failed_at else {
        return Next::StartClock;
    };
    // `now` can be behind `since` if the clock went backwards, which must not read as a
    // backoff that has elapsed by a huge margin -- nor freeze the stream out for ever.
    // Treated as no time passed, so the next heartbeat with a sane clock decides.
    let waited = now.saturating_sub(since);
    if waited < 0 || waited < BACKOFF_SECS[attempt.retries as usize] {
        return Next::Wait;
    }
    Next::Retry
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_760_000_000;

    /// A channel with somewhere to go goes there, without waiting.
    #[test]
    fn another_source_is_tried_at_once() {
        let a = Attempt {
            current: 0,
            sources: 3,
            ..Default::default()
        };
        assert_eq!(next(a, NOW), Next::RollTo(1));
    }

    /// And keeps going, one source at a time.
    #[test]
    fn rollovers_walk_the_list_in_order() {
        for current in 0..4 {
            let a = Attempt {
                current,
                sources: 5,
                rollovers: current as u32,
                ..Default::default()
            };
            assert_eq!(next(a, NOW), Next::RollTo(current + 1));
        }
    }

    /// The case the viewer actually hits: one source, which must be retried rather than
    /// given up on.
    #[test]
    fn the_last_source_starts_a_clock_then_retries() {
        let first = Attempt {
            current: 0,
            sources: 1,
            ..Default::default()
        };
        assert_eq!(next(first, NOW), Next::StartClock);

        let waiting = Attempt {
            failed_at: Some(NOW),
            ..first
        };
        assert_eq!(
            next(waiting, NOW),
            Next::Wait,
            "a provider that just dropped the connection was asked again in the same breath"
        );
        assert_eq!(next(waiting, NOW + BACKOFF_SECS[0] - 1), Next::Wait);
        assert_eq!(next(waiting, NOW + BACKOFF_SECS[0]), Next::Retry);
    }

    /// The second retry waits longer than the first.
    #[test]
    fn the_backoff_lengthens() {
        let a = Attempt {
            current: 0,
            sources: 1,
            retries: 1,
            failed_at: Some(NOW),
            ..Default::default()
        };
        assert_eq!(next(a, NOW + BACKOFF_SECS[0]), Next::Wait);
        assert_eq!(next(a, NOW + BACKOFF_SECS[1]), Next::Retry);
    }

    /// Retrying the only source is bounded.
    #[test]
    fn a_dead_channel_is_abandoned_rather_than_retried_for_ever() {
        let a = Attempt {
            current: 0,
            sources: 1,
            retries: MAX_RETRIES,
            failed_at: Some(NOW),
            ..Default::default()
        };
        assert_eq!(next(a, NOW + 600), Next::Abandon);
    }

    /// So is walking a long source list.
    #[test]
    fn rolling_over_is_bounded_even_with_a_long_list() {
        let a = Attempt {
            current: 3,
            sources: 50,
            rollovers: MAX_ROLLOVERS,
            ..Default::default()
        };
        assert_eq!(
            next(a, NOW),
            Next::Stop,
            "a provider outage would otherwise spin through every URL it owns"
        );
    }

    /// `Stop` and `Abandon` are different answers and the difference matters: one leaves
    /// the session for the next tune to re-read, the other says there is nothing left to
    /// re-read.
    #[test]
    fn running_out_of_rollovers_is_not_the_same_as_running_out_of_sources() {
        let out_of_rollovers = Attempt {
            current: 2,
            sources: 9,
            rollovers: MAX_ROLLOVERS,
            ..Default::default()
        };
        let out_of_sources = Attempt {
            current: 0,
            sources: 1,
            retries: MAX_RETRIES,
            ..Default::default()
        };
        assert_eq!(next(out_of_rollovers, NOW), Next::Stop);
        assert_eq!(next(out_of_sources, NOW), Next::Abandon);
    }

    /// One stall is one failure however many heartbeats it spans.
    #[test]
    fn only_the_first_heartbeat_of_a_failure_blames_the_source() {
        let fresh = Attempt {
            current: 0,
            sources: 1,
            ..Default::default()
        };
        assert!(fresh.first_sight());
        assert!(!Attempt {
            failed_at: Some(NOW),
            ..fresh
        }
        .first_sight());
    }

    /// A clock that went backwards must not read as a backoff long elapsed.
    #[test]
    fn a_clock_that_went_backwards_waits_rather_than_reloading() {
        let a = Attempt {
            current: 0,
            sources: 1,
            failed_at: Some(NOW),
            ..Default::default()
        };
        assert_eq!(next(a, NOW - 10_000), Next::Wait);
    }

    /// A session with no sources at all has nothing to retry and nothing to roll to.
    #[test]
    fn a_session_with_no_sources_is_abandoned() {
        let a = Attempt {
            current: 0,
            sources: 0,
            ..Default::default()
        };
        assert_eq!(next(a, NOW), Next::StartClock);
        assert_eq!(
            next(
                Attempt {
                    retries: MAX_RETRIES,
                    ..a
                },
                NOW
            ),
            Next::Abandon
        );
    }
}
