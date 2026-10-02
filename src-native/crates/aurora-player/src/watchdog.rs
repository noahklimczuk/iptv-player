//! Noticing that playback has stopped being playback.
//!
//! mpv's own timeouts cover the case it can see: `network-timeout` fires when a read
//! blocks for too long. What it cannot see is a provider that behaves — accepts the
//! connection, answers 200, and then sends nothing, or sends just enough to keep the
//! socket alive. There is no error for that, so mpv sits there, and so did this player:
//! `Loading` for ever on a stream that never started, or `Playing` with a position that
//! had not moved in a minute. Both read to the viewer as "the app is broken", and
//! neither reached `playback::tick`, which rolls over to the next source only when it
//! sees `Error`.
//!
//! So the symptom is given a name here. The decision is pure — it takes the clock as an
//! argument rather than reading one — because the interesting cases are all about time
//! passing, and a test that has to wait twenty seconds to find out is a test nobody
//! runs. `mpv.rs` is `#![cfg(windows)]` and cannot be tested on CI's Linux runner at
//! all; this module compiles everywhere.

/// What the watchdog thinks of the state it was just shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing to do. Either playback is progressing or it is legitimately not trying.
    Healthy,
    /// A load that never became a picture. The stream was asked for and never arrived.
    NeverStarted,
    /// Playback started and then stopped progressing: buffering that does not end, or a
    /// playhead that has not moved.
    Stalled,
}

impl Verdict {
    /// The text handed to `NetFailure::classify`, for the two verdicts that are failures.
    ///
    /// Phrased as a cause rather than a symptom, because this is what ends up in the log
    /// beside whatever mpv itself said.
    pub fn reason(self) -> Option<&'static str> {
        match self {
            Verdict::Healthy => None,
            Verdict::NeverStarted => Some("the stream did not start playing: timed out"),
            Verdict::Stalled => Some("the stream stopped sending data: timed out"),
        }
    }
}

/// The states the watchdog has an opinion about, so it does not depend on `PlayerStatus`
/// and cannot be confused by a status it has not been taught.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// A load is in flight and no picture has arrived yet.
    Starting,
    /// Playing, or meant to be.
    Running,
    /// Stalled on the network by mpv's own account (`paused-for-cache`).
    Rebuffering,
    /// Deliberately not progressing: idle, paused, or already failed. Never a fault.
    Resting,
}

/// How long a load may take before it is treated as never having started.
///
/// Longer than mpv's `network-timeout` of 12s on purpose: when mpv *can* see the
/// failure it should be the one to report it, with its own description, and this is the
/// backstop for when it cannot. Long enough to cover a slow handshake on a weak
/// connection, short enough that nobody stares at a spinner wondering.
pub const DEFAULT_START_TIMEOUT_SECS: f64 = 20.0;

/// How long playback may fail to progress before it is treated as dead.
///
/// A live stream on a congested line rebuffers for several seconds routinely, and
/// rolling over to another source for that would be worse than the stutter. Twelve
/// seconds is past anything that recovers on its own.
pub const DEFAULT_STALL_TIMEOUT_SECS: f64 = 12.0;

/// How far the playhead must move to count as progress.
///
/// Not zero: `time-pos` on a live stream arrives as a float derived from the stream's
/// own timestamps, and two consecutive reads of a healthy stream can differ in the last
/// place while meaning the same moment.
const PROGRESS_EPSILON_SECS: f64 = 0.05;

/// Watches for playback that has stopped happening without anybody saying so.
#[derive(Debug, Clone)]
pub struct Watchdog {
    start_timeout: f64,
    stall_timeout: f64,
    /// The phase being timed, and the moment its clock started: the last time something
    /// good happened, which is either entering this phase or making progress inside it.
    /// `None` before the first observation and after a reset.
    mark: Option<(Phase, f64)>,
    last_position: f64,
}

impl Default for Watchdog {
    fn default() -> Self {
        Self::new(DEFAULT_START_TIMEOUT_SECS, DEFAULT_STALL_TIMEOUT_SECS)
    }
}

impl Watchdog {
    pub fn new(start_timeout: f64, stall_timeout: f64) -> Self {
        Self {
            start_timeout,
            stall_timeout,
            mark: None,
            last_position: 0.0,
        }
    }

    /// Forget everything. Called when a new URL is loaded: the previous stream's
    /// position has nothing to do with this one's, and a load that follows a stall must
    /// not inherit its clock.
    pub fn reset(&mut self) {
        self.mark = None;
        self.last_position = 0.0;
    }

    /// Show the watchdog where things stand. `now_secs` is any monotonic clock.
    ///
    /// Returns a failure at most once per window: the verdict restarts the clock, so a
    /// caller that keeps polling a dead stream gets one answer now and the next only
    /// after another full window.
    pub fn observe(&mut self, now_secs: f64, phase: Phase, position_secs: f64) -> Verdict {
        // Progress means the playhead moved. `Starting` has no position to move yet —
        // arriving at `Running` *is* its progress — so there only the clock counts.
        let advanced = position_secs > self.last_position + PROGRESS_EPSILON_SECS;
        if position_secs > self.last_position {
            self.last_position = position_secs;
        }

        // Nothing is being attempted, so nothing can be late. This also covers the
        // already-failed case: a stream that has reported an error does not need a
        // second opinion, and the app layer is busy rolling over to another source.
        if phase == Phase::Resting {
            self.mark = None;
            return Verdict::Healthy;
        }

        let (timeout, verdict) = match phase {
            Phase::Starting => (self.start_timeout, Verdict::NeverStarted),
            _ => (self.stall_timeout, Verdict::Stalled),
        };

        // A phase it was not in before starts its own clock. A load that gets as far as
        // rebuffering *did* start, and is judged as a stall from then on rather than
        // being failed by the clock the load was running against.
        let entered_phase = !matches!(self.mark, Some((was, _)) if was == phase);
        if entered_phase || (phase == Phase::Running && advanced) {
            self.mark = Some((phase, now_secs));
            return Verdict::Healthy;
        }

        // The clock runs from the last good moment rather than from when the absence of
        // one was first noticed: "it has not moved for twelve seconds" is the question
        // being asked, and a heartbeat polling every 250 ms must answer it the same way
        // as one polling every two seconds.
        let Some((_, since)) = self.mark else {
            return Verdict::Healthy;
        };
        if now_secs - since < timeout {
            return Verdict::Healthy;
        }
        self.mark = Some((phase, now_secs));
        verdict
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream that is asked for and never arrives. mpv reports nothing at all in this
    /// case — no error, no event — so without this the player sits on `Loading` and the
    /// app layer never learns there is another source worth trying.
    #[test]
    fn a_load_that_never_starts_is_a_failure() {
        let mut w = Watchdog::new(20.0, 12.0);
        assert_eq!(w.observe(0.0, Phase::Starting, 0.0), Verdict::Healthy);
        assert_eq!(w.observe(19.9, Phase::Starting, 0.0), Verdict::Healthy);
        assert_eq!(w.observe(20.0, Phase::Starting, 0.0), Verdict::NeverStarted);
    }

    /// The one that looks most like a working app and is not: mpv says `Playing`, the
    /// picture is frozen, and the position has not moved for a quarter of a minute.
    #[test]
    fn a_playhead_that_stops_moving_is_a_failure() {
        let mut w = Watchdog::new(20.0, 12.0);
        assert_eq!(w.observe(0.0, Phase::Running, 100.0), Verdict::Healthy);
        assert_eq!(w.observe(5.0, Phase::Running, 100.0), Verdict::Healthy);
        assert_eq!(w.observe(12.0, Phase::Running, 100.0), Verdict::Stalled);
    }

    /// Rebuffering is normal and recovery is normal, so neither is reported.
    #[test]
    fn rebuffering_that_ends_is_not_a_failure() {
        let mut w = Watchdog::new(20.0, 12.0);
        assert_eq!(w.observe(0.0, Phase::Running, 10.0), Verdict::Healthy);
        for t in 1..=11 {
            assert_eq!(
                w.observe(t as f64, Phase::Rebuffering, 10.0),
                Verdict::Healthy,
                "a stall of {t}s should be tolerated"
            );
        }
        // It came back, and the playhead moved.
        assert_eq!(w.observe(12.0, Phase::Running, 12.0), Verdict::Healthy);
        // And the clock started again, rather than carrying the stall's eleven seconds.
        assert_eq!(w.observe(23.0, Phase::Running, 12.5), Verdict::Healthy);
    }

    /// A stall that follows a slow load is judged as a stall, not finished off by the
    /// load's clock. The stream did start; what happened next is a different fault.
    #[test]
    fn changing_phase_restarts_the_clock() {
        let mut w = Watchdog::new(20.0, 12.0);
        assert_eq!(w.observe(0.0, Phase::Starting, 0.0), Verdict::Healthy);
        assert_eq!(w.observe(15.0, Phase::Starting, 0.0), Verdict::Healthy);
        // Fifteen seconds in, it starts buffering. That is not 15s of a stall.
        assert_eq!(w.observe(16.0, Phase::Rebuffering, 0.0), Verdict::Healthy);
        assert_eq!(w.observe(21.0, Phase::Rebuffering, 0.0), Verdict::Healthy);
        assert_eq!(w.observe(28.0, Phase::Rebuffering, 0.0), Verdict::Stalled);
    }

    /// Being paused is not being broken, and neither is being idle. Without this, pausing
    /// a film for a phone call would roll the player over to another source.
    #[test]
    fn resting_is_never_a_failure() {
        let mut w = Watchdog::new(20.0, 12.0);
        for t in 0..60 {
            assert_eq!(w.observe(t as f64, Phase::Resting, 500.0), Verdict::Healthy);
        }
    }

    /// One verdict per window. The heartbeat polls four times a second; a dead stream
    /// must not produce four failures a second for the app layer to act on.
    #[test]
    fn a_verdict_is_given_once_and_not_repeated() {
        let mut w = Watchdog::new(20.0, 12.0);
        w.observe(0.0, Phase::Running, 10.0);
        assert_eq!(w.observe(12.0, Phase::Running, 10.0), Verdict::Stalled);
        assert_eq!(w.observe(12.25, Phase::Running, 10.0), Verdict::Healthy);
        assert_eq!(w.observe(13.0, Phase::Running, 10.0), Verdict::Healthy);
        // And the next full window without progress is reported again.
        assert_eq!(w.observe(24.25, Phase::Running, 10.0), Verdict::Stalled);
    }

    /// A reload clears the clock and the remembered position. Without the position going
    /// too, a film resumed at 10s after a channel at 3000s would look frozen.
    #[test]
    fn a_reset_forgets_the_previous_stream() {
        let mut w = Watchdog::new(20.0, 12.0);
        w.observe(0.0, Phase::Running, 3000.0);
        w.reset();
        assert_eq!(w.observe(100.0, Phase::Running, 10.0), Verdict::Healthy);
        assert_eq!(w.observe(105.0, Phase::Running, 15.0), Verdict::Healthy);
    }

    /// The jitter in a live stream's own timestamps is not progress, and is not a stall
    /// either until it stops entirely.
    #[test]
    fn timestamp_jitter_does_not_count_as_progress() {
        let mut w = Watchdog::new(20.0, 12.0);
        w.observe(0.0, Phase::Running, 500.0);
        // Moving by less than the epsilon, repeatedly, is a frozen picture.
        assert_eq!(w.observe(6.0, Phase::Running, 500.01), Verdict::Healthy);
        assert_eq!(w.observe(12.0, Phase::Running, 500.02), Verdict::Stalled);
    }

    #[test]
    fn only_failures_carry_a_reason() {
        assert!(Verdict::Healthy.reason().is_none());
        assert!(Verdict::NeverStarted
            .reason()
            .unwrap()
            .contains("not start"));
        assert!(Verdict::Stalled
            .reason()
            .unwrap()
            .contains("stopped sending"));
    }
}
