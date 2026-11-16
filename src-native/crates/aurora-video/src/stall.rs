//! Noticing that playback has quietly stopped (E25–E28).
//!
//! mpv reports what it can see: a refused connection, a read that timed out. It cannot
//! see a provider that answers and then sends nothing, or just enough to keep the socket
//! open. That shows up only as time passing: a file that never starts, or a playhead
//! that does not move. This module watches the clock for those.
//!
//! It takes the time as an argument, so every case is a test that runs instantly.

/// What the playhead is supposed to be doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Nothing is expected to move: idle, paused, failed. Never a stall.
    Resting,
    /// A file was asked for and has not started.
    Opening,
    /// Waiting for data mid-file.
    Buffering,
    /// It should be moving.
    Playing,
}

/// Why it was judged stuck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stuck {
    NeverStarted,
    StoppedBuffering,
    Frozen,
}

impl Stuck {
    /// Phrased so the error classifier files it as a stream that went quiet.
    pub fn describe(self) -> &'static str {
        match self {
            Self::NeverStarted => "the stream stopped sending data before it started",
            Self::StoppedBuffering => "the stream stopped sending data while buffering",
            Self::Frozen => "the stream stopped sending data: the picture stopped moving",
        }
    }
}

/// How long each phase may go without progress.
pub const OPEN_LIMIT_SECS: f64 = 25.0;
pub const BUFFER_LIMIT_SECS: f64 = 25.0;
pub const FROZEN_LIMIT_SECS: f64 = 15.0;

/// Movement smaller than this is timestamp noise, not progress.
pub const JITTER_SECS: f64 = 0.2;

/// How long each phase may go without progress before it is called stuck.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub open_secs: f64,
    pub buffer_secs: f64,
    pub frozen_secs: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            open_secs: OPEN_LIMIT_SECS,
            buffer_secs: BUFFER_LIMIT_SECS,
            frozen_secs: FROZEN_LIMIT_SECS,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Watch {
    limits: Limits,
    phase: Phase,
    /// When progress was last seen, or the phase began.
    since: f64,
    position: Option<f64>,
    judged: bool,
}

impl Default for Watch {
    fn default() -> Self {
        Self {
            limits: Limits::default(),
            phase: Phase::Resting,
            since: 0.0,
            position: None,
            judged: false,
        }
    }
}

impl Watch {
    pub fn with_limits(limits: Limits) -> Self {
        Self {
            limits,
            ..Self::default()
        }
    }

    /// A new file: whatever was known about the last one is irrelevant.
    pub fn reset(&mut self, now: f64) {
        *self = Self {
            since: now,
            ..Self::with_limits(self.limits)
        };
    }

    /// The viewer moved the playhead. Time spent before it says nothing about after.
    pub fn seeked(&mut self, now: f64, position: Option<f64>) {
        self.since = now;
        self.position = position;
        self.judged = false;
    }

    /// Look at the state now. Returns a verdict the first time the limit is passed, and
    /// not again until something changes (E28).
    pub fn check(&mut self, now: f64, phase: Phase, position: Option<f64>) -> Option<Stuck> {
        if phase != self.phase {
            // Between Playing and Buffering the clock keeps running: a stream that
            // flips between the two every second without the playhead moving is just
            // as stuck, and restarting the clock on each flip would never say so.
            let running = |p| matches!(p, Phase::Playing | Phase::Buffering);
            if !(running(phase) && running(self.phase)) {
                self.since = now;
                self.position = position;
                self.judged = false;
            }
            self.phase = phase;
            return None;
        }
        let moved = match (self.position, position) {
            (Some(a), Some(b)) => (b - a).abs() > JITTER_SECS,
            (None, Some(_)) => true,
            _ => false,
        };
        if moved {
            self.position = position;
            self.since = now;
            self.judged = false;
            return None;
        }
        let (limit, verdict) = match phase {
            Phase::Resting => return None,
            Phase::Opening => (self.limits.open_secs, Stuck::NeverStarted),
            Phase::Buffering => (self.limits.buffer_secs, Stuck::StoppedBuffering),
            Phase::Playing => (self.limits.frozen_secs, Stuck::Frozen),
        };
        if self.judged || now - self.since < limit {
            return None;
        }
        self.judged = true;
        Some(verdict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Phase::*;

    fn run(
        w: &mut Watch,
        from: f64,
        to: f64,
        phase: Phase,
        pos: impl Fn(f64) -> Option<f64>,
    ) -> Vec<(f64, Stuck)> {
        let mut out = Vec::new();
        let mut t = from;
        while t <= to {
            if let Some(v) = w.check(t, phase, pos(t)) {
                out.push((t, v));
            }
            t += 0.5;
        }
        out
    }

    #[test]
    fn a_file_that_never_starts_is_caught() {
        let mut w = Watch::default();
        w.reset(0.0);
        let v = run(&mut w, 0.0, 40.0, Opening, |_| None);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].1, Stuck::NeverStarted);
        assert!(v[0].0 >= OPEN_LIMIT_SECS);
    }

    #[test]
    fn a_playhead_that_stops_moving_is_caught() {
        let mut w = Watch::default();
        let v = run(&mut w, 0.0, 10.0, Playing, Some);
        assert!(v.is_empty());
        let v = run(&mut w, 10.5, 40.0, Playing, |_| Some(10.0));
        assert_eq!(v.iter().map(|x| x.1).collect::<Vec<_>>(), [Stuck::Frozen]);
    }

    #[test]
    fn resting_is_never_a_stall() {
        let mut w = Watch::default();
        assert!(run(&mut w, 0.0, 600.0, Resting, |_| Some(5.0)).is_empty());
    }

    #[test]
    fn rebuffering_that_ends_is_not_a_stall() {
        let mut w = Watch::default();
        run(&mut w, 0.0, 10.0, Playing, Some);
        assert!(run(&mut w, 10.5, 20.0, Buffering, |_| Some(10.0)).is_empty());
        assert!(run(&mut w, 20.5, 40.0, Playing, Some).is_empty());
    }

    #[test]
    fn buffering_that_never_ends_is_a_stall() {
        let mut w = Watch::default();
        let v = run(&mut w, 0.0, 60.0, Buffering, |_| Some(3.0));
        assert_eq!(
            v.iter().map(|x| x.1).collect::<Vec<_>>(),
            [Stuck::StoppedBuffering]
        );
    }

    #[test]
    fn a_long_rewind_is_movement_not_a_stall() {
        let mut w = Watch::default();
        run(&mut w, 0.0, 10.0, Playing, |t| Some(1000.0 + t));
        // Jumped back ten minutes, then plays on from there.
        assert!(run(&mut w, 10.5, 40.0, Playing, |t| Some(400.0 + t)).is_empty());
    }

    #[test]
    fn a_rewind_that_leads_nowhere_is_still_caught() {
        let mut w = Watch::default();
        run(&mut w, 0.0, 10.0, Playing, |t| Some(1000.0 + t));
        let v = run(&mut w, 10.5, 40.0, Playing, |_| Some(400.0));
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn a_seek_starts_the_clock_again() {
        let mut w = Watch::default();
        run(&mut w, 0.0, 10.0, Playing, |_| Some(5.0));
        w.seeked(10.0, Some(5.0));
        assert!(run(&mut w, 10.5, 24.0, Playing, |_| Some(5.0)).is_empty());
    }

    #[test]
    fn a_verdict_is_given_once() {
        let mut w = Watch::default();
        let v = run(&mut w, 0.0, 300.0, Playing, |_| Some(1.0));
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn a_reset_forgets_the_old_file() {
        let mut w = Watch::default();
        run(&mut w, 0.0, 100.0, Playing, |_| Some(1.0));
        w.reset(100.0);
        assert!(run(&mut w, 100.0, 110.0, Opening, |_| None).is_empty());
    }

    #[test]
    fn jitter_is_not_progress() {
        let mut w = Watch::default();
        let v = run(&mut w, 0.0, 40.0, Playing, |t| {
            Some(10.0 + (t * 7.0).sin() * 0.05)
        });
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn flipping_between_buffering_and_playing_without_moving_is_caught() {
        let mut w = Watch::default();
        let mut caught = 0;
        let mut t = 0.0;
        while t < 60.0 {
            let phase = if (t as u32) & 1 == 0 {
                Playing
            } else {
                Buffering
            };
            if w.check(t, phase, Some(7.0)).is_some() {
                caught += 1;
            }
            t += 0.5;
        }
        assert_eq!(caught, 1);
    }

    #[test]
    fn slow_motion_is_still_progress() {
        let mut w = Watch::default();
        assert!(run(&mut w, 0.0, 120.0, Playing, |t| Some(t * 0.25)).is_empty());
    }
}
