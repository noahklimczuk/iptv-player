//! What to do when a stream fails (docs/PLAYER.md H5–H9, README §6.1 and §7.14).
//!
//! Pure: it is told what happened and when, and answers with what to do next. The
//! session carries the answer out. That keeps every rule here testable with a clock
//! the test controls, and keeps the rules in one place.

use aurora_core::neterr::{ErrorCode, NetFailure};

/// How long to wait before each round of retries (H7).
pub const BACKOFF_SECS: [f64; 3] = [1.0, 3.0, 7.0];

/// At most this many sources are tried in one round, however many a channel lists
/// (H6). A channel with forty dead mirrors is not tried forty times before the
/// viewer hears about it.
pub const MAX_SOURCES_PER_ROUND: usize = 5;

/// A film is tried this many times in all before Aurora says it will not play (H8).
pub const FILM_ATTEMPTS: u32 = 3;

/// Playing this long with a picture forgives earlier failures (H7, H8).
pub const HEALTHY_SECS: f64 = 10.0;

/// What the session should do next.
#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    /// Load this candidate now, from this position (a film reloads where it died).
    Load { candidate: usize, start_secs: Option<f64> },
    /// Wait until this time, then [`Recovery::due`] says what to load.
    Wait { until: f64 },
    /// Nothing more to try: report the failure.
    GiveUp,
}

#[derive(Debug, Clone)]
pub struct Recovery {
    live: bool,
    sources: usize,
    /// The candidate playing, or last tried.
    at: usize,
    /// Sources tried in the current round.
    tried: usize,
    /// Rounds of backoff used.
    rounds: usize,
    /// Film loads that failed.
    film_failures: u32,
    /// Where a film should restart after its stream died.
    resume_at: Option<f64>,
    /// When playback was last seen healthy and continuously so.
    healthy_since: Option<f64>,
    /// Set while waiting out a backoff: what to load when it ends.
    pending: Option<(f64, usize)>,
}

impl Recovery {
    /// `sources` is the number of candidates; at least one.
    pub fn new(live: bool, sources: usize) -> Self {
        Self {
            live,
            sources: sources.max(1),
            at: 0,
            tried: 0,
            rounds: 0,
            film_failures: 0,
            resume_at: None,
            healthy_since: None,
            pending: None,
        }
    }

    /// The candidate in use.
    pub fn at(&self) -> usize {
        self.at
    }

    /// Whether a backoff is being waited out (H12: the page says "reconnecting").
    pub fn waiting(&self) -> bool {
        self.pending.is_some()
    }

    /// Whether anything has failed since playback was last healthy: the page shows
    /// "reconnecting" while this is true and the stream is not yet back.
    pub fn recovering(&self) -> bool {
        self.pending.is_some() || self.tried > 0 || self.rounds > 0 || self.film_failures > 0
    }

    /// Playback is fine: it is playing, with a picture where there is one. Called on
    /// every look; forgives earlier failures once it has lasted (H7, H8).
    pub fn playing(&mut self, now: f64) {
        let since = *self.healthy_since.get_or_insert(now);
        if now - since >= HEALTHY_SECS {
            self.tried = 0;
            self.rounds = 0;
            self.film_failures = 0;
            self.resume_at = None;
        }
    }

    /// Playback is not healthy right now (loading, buffering, paused): the next
    /// healthy stretch starts again from zero.
    pub fn interrupted(&mut self) {
        self.healthy_since = None;
    }

    /// The candidate in use failed. `died_at` is where a film was when its stream
    /// died, if it had got going.
    pub fn failed(&mut self, now: f64, failure: &NetFailure, died_at: Option<f64>) -> Next {
        self.healthy_since = None;
        self.pending = None;
        if self.live {
            self.live_failed(now, failure)
        } else {
            self.film_failed(now, failure, died_at)
        }
    }

    fn live_failed(&mut self, now: f64, failure: &NetFailure) -> Next {
        self.tried += 1;
        let per_round = self.sources.min(MAX_SOURCES_PER_ROUND);
        let next = (self.at + 1) % self.sources;
        if self.tried < per_round {
            // H6: another source straight away.
            self.at = next;
            return Next::Load {
                candidate: next,
                start_secs: None,
            };
        }
        // Every source in this round has failed. A failure no retry fixes (a request
        // the provider will never accept, a codec mpv cannot play) is not retried.
        if !worth_retrying(failure) || self.rounds >= BACKOFF_SECS.len() {
            return Next::GiveUp;
        }
        let until = now + BACKOFF_SECS[self.rounds];
        self.rounds += 1;
        self.tried = 0;
        self.pending = Some((until, next));
        Next::Wait { until }
    }

    fn film_failed(&mut self, now: f64, failure: &NetFailure, died_at: Option<f64>) -> Next {
        if let Some(t) = died_at.filter(|t| t.is_finite() && *t > 0.0) {
            self.resume_at = Some(t);
        }
        self.film_failures += 1;
        // H8: a 404 is asked for once; nothing else is asked for more than three times.
        if failure.code == ErrorCode::NotFound
            || !worth_retrying(failure)
            || self.film_failures >= FILM_ATTEMPTS
        {
            return Next::GiveUp;
        }
        let wait = BACKOFF_SECS[(self.film_failures as usize - 1).min(BACKOFF_SECS.len() - 1)];
        let until = now + wait;
        self.pending = Some((until, 0));
        Next::Wait { until }
    }

    /// When a backoff has run out, what to load.
    pub fn due(&mut self, now: f64) -> Option<Next> {
        let (until, candidate) = self.pending?;
        if now < until {
            return None;
        }
        self.pending = None;
        self.at = candidate;
        Some(Next::Load {
            candidate,
            start_secs: if self.live { None } else { self.resume_at },
        })
    }
}

/// Whether trying the same thing again could ever help.
fn worth_retrying(f: &NetFailure) -> bool {
    !matches!(
        f.code,
        ErrorCode::BadRequest | ErrorCode::UnsupportedCodec | ErrorCode::DrmProtected
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(code: ErrorCode) -> NetFailure {
        NetFailure {
            code,
            message: "m".into(),
            cause: "c".into(),
            actions: vec![],
            retryable: true,
        }
    }

    fn dropped() -> NetFailure {
        failure(ErrorCode::Dropped)
    }

    #[test]
    fn h6_a_failed_source_rolls_straight_to_the_next() {
        let mut r = Recovery::new(true, 3);
        assert_eq!(
            r.failed(0.0, &dropped(), None),
            Next::Load { candidate: 1, start_secs: None }
        );
        assert_eq!(
            r.failed(0.1, &dropped(), None),
            Next::Load { candidate: 2, start_secs: None }
        );
    }

    #[test]
    fn h6_rollovers_are_bounded_even_with_many_sources() {
        let mut r = Recovery::new(true, 40);
        let mut loads = 0;
        let mut now = 0.0;
        loop {
            match r.failed(now, &dropped(), None) {
                Next::Load { .. } => loads += 1,
                Next::Wait { until } => {
                    now = until;
                    assert!(matches!(r.due(now), Some(Next::Load { .. })));
                    loads += 1;
                }
                Next::GiveUp => break,
            }
            assert!(loads < 100, "never gave up");
        }
        assert_eq!(loads, (MAX_SOURCES_PER_ROUND * 4) - 1);
    }

    #[test]
    fn h7_a_single_source_channel_is_retried_with_backoff_then_given_up() {
        let mut r = Recovery::new(true, 1);
        let mut now = 100.0;
        for wait in BACKOFF_SECS {
            assert_eq!(r.failed(now, &dropped(), None), Next::Wait { until: now + wait });
            assert!(r.waiting());
            assert_eq!(r.due(now + wait - 0.01), None, "loaded before the backoff ran out");
            now += wait;
            assert_eq!(r.due(now), Some(Next::Load { candidate: 0, start_secs: None }));
        }
        assert_eq!(r.failed(now, &dropped(), None), Next::GiveUp);
    }

    #[test]
    fn h7_a_channel_that_comes_back_gets_its_retries_again() {
        let mut r = Recovery::new(true, 1);
        assert!(matches!(r.failed(0.0, &dropped(), None), Next::Wait { .. }));
        r.due(1.0);
        assert!(matches!(r.failed(1.0, &dropped(), None), Next::Wait { .. }));
        r.due(4.0);
        // Back, and healthy for long enough.
        r.playing(4.0);
        r.playing(4.0 + HEALTHY_SECS);
        assert!(!r.recovering());
        assert_eq!(r.failed(20.0, &dropped(), None), Next::Wait { until: 21.0 });
    }

    #[test]
    fn a_moment_of_playing_is_not_healthy_enough_to_forgive() {
        let mut r = Recovery::new(true, 1);
        r.failed(0.0, &dropped(), None);
        r.due(1.0);
        r.playing(1.0);
        r.interrupted();
        r.playing(1.0 + HEALTHY_SECS);
        assert_eq!(r.failed(12.0, &dropped(), None), Next::Wait { until: 15.0 });
    }

    #[test]
    fn h8_a_film_refused_every_time_is_tried_three_times_and_no_more() {
        let mut r = Recovery::new(false, 1);
        let refused = failure(ErrorCode::Refused);
        assert!(matches!(r.failed(0.0, &refused, None), Next::Wait { .. }));
        assert!(r.due(10.0).is_some());
        assert!(matches!(r.failed(10.0, &refused, None), Next::Wait { .. }));
        assert!(r.due(20.0).is_some());
        assert_eq!(r.failed(20.0, &refused, None), Next::GiveUp);
    }

    #[test]
    fn h8_a_404_is_asked_for_once() {
        let mut r = Recovery::new(false, 1);
        assert_eq!(r.failed(0.0, &failure(ErrorCode::NotFound), None), Next::GiveUp);
    }

    #[test]
    fn h8_a_film_whose_stream_dies_is_reloaded_where_it_died() {
        let mut r = Recovery::new(false, 1);
        assert_eq!(r.failed(0.0, &dropped(), Some(2400.0)), Next::Wait { until: 1.0 });
        assert_eq!(
            r.due(1.0),
            Some(Next::Load { candidate: 0, start_secs: Some(2400.0) })
        );
    }

    #[test]
    fn h8_healthy_playback_forgives_a_film_earlier_failures() {
        let mut r = Recovery::new(false, 1);
        for t in [0.0, 100.0] {
            r.failed(t, &dropped(), Some(t + 50.0));
            r.due(t + 10.0);
            r.playing(t + 10.0);
            r.playing(t + 10.0 + HEALTHY_SECS);
        }
        // Two failures, each followed by healthy playback: a third is not the last.
        assert!(matches!(r.failed(300.0, &dropped(), None), Next::Wait { .. }));
    }

    #[test]
    fn a_codec_mpv_cannot_play_is_not_retried() {
        let mut film = Recovery::new(false, 1);
        assert_eq!(
            film.failed(0.0, &failure(ErrorCode::UnsupportedCodec), None),
            Next::GiveUp
        );
        // A channel still tries its other sources: another may carry another codec.
        let mut channel = Recovery::new(true, 2);
        let codec = failure(ErrorCode::UnsupportedCodec);
        assert!(matches!(channel.failed(0.0, &codec, None), Next::Load { candidate: 1, .. }));
        assert_eq!(channel.failed(0.0, &codec, None), Next::GiveUp);
    }
}
