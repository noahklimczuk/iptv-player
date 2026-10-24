//! The sleep timer (README §13, "Everyday use").
//!
//! "15/30/45/60/90 min, 'end of programme', 'end of episode', with a 60-second warning
//! and an optional PC-shutdown action."
//!
//! ## Why the deadline lives here and not in the UI
//!
//! A `setTimeout` in the front end would be the short way to write this and the wrong
//! place to keep it. The timer's whole purpose is to act on somebody who has fallen
//! asleep, which is exactly when nothing is interacting with the window — and a renderer
//! that has been backgrounded, throttled, or reloaded has lost its timer while the stream
//! plays on all night. The host already keeps time for the DVR for the same reason, and
//! this rides the player heartbeat that is running anyway.
//!
//! ## Why "end of programme" and "end of episode" are resolved once, to an instant
//!
//! Both are stored as a Unix deadline, computed when the timer is set, rather than
//! re-derived every tick. Re-deriving sounds more correct and is worse: the guide moves
//! as the EPG refreshes, so a programme whose stop time shifted would drag the deadline
//! with it, and a viewer who asked to sleep at the end of *this* programme would be woken
//! by a schedule change. An instant the viewer agreed to is a promise; a rule that keeps
//! recomputing is not.
//!
//! The cost is that it cannot tell what it cannot know when asked, which is why
//! [`Deadline::resolve`] returns `None` on a channel with no guide data — the caller then
//! says so instead of inventing an hour.

use serde::{Deserialize, Serialize};

/// How long before the deadline the viewer is warned, so a wrong setting can be undone.
///
/// README §13 asks for sixty seconds. It is a property of the timer rather than of the
/// screen that draws the warning: the UI reads `warning` off the state rather than
/// comparing against a number of its own, so the two cannot disagree about when the
/// warning is due.
pub const WARNING_SECS: i64 = 60;

/// What the timer does when it comes due.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SleepAction {
    /// Stop playback and leave the app up. The default, and the only one that cannot
    /// lose somebody's work.
    StopPlayback,
    /// Close Aurora.
    QuitApp,
    /// Shut the machine down.
    ///
    /// Deliberately a separate variant rather than a flag on the others, so that nothing
    /// can reach it by accident: the one path to a shutdown is a viewer choosing this
    /// value by name.
    ShutDownPc,
}

impl SleepAction {
    pub fn as_str(self) -> &'static str {
        match self {
            SleepAction::StopPlayback => "stopPlayback",
            SleepAction::QuitApp => "quitApp",
            SleepAction::ShutDownPc => "shutDownPc",
        }
    }
}

/// What the viewer asked for, before it became an instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SleepAfter {
    /// A flat number of minutes: README §13's 15/30/45/60/90.
    Minutes(u32),
    /// When the programme now on this channel ends.
    EndOfProgramme,
    /// When the episode now playing ends.
    EndOfEpisode,
}

/// The longest timer that can be set.
///
/// Six hours. Past this it is not a sleep timer, and a mis-typed number of minutes should
/// not arm something that fires next week — the field is a `u32` and arrives over IPC.
pub const MAX_MINUTES: u32 = 6 * 60;
/// The shortest. A one-minute timer would fire inside its own warning window.
pub const MIN_MINUTES: u32 = 2;

/// An armed timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deadline {
    /// When it fires, in Unix seconds.
    pub at_unix: i64,
    pub action: SleepAction,
    /// What was asked for, kept so the UI can say "end of programme" rather than a time.
    pub after: SleepAfter,
}

/// What a caller has to tell the timer about the thing playing, to resolve a deadline.
///
/// A plain struct of facts rather than a handle to the player and the database, so
/// resolving is a pure function that can be tested without either. The caller is the
/// command layer, which has both.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Playing {
    /// When the programme now on air ends, in Unix seconds, when the guide knows.
    pub programme_ends_unix: Option<i64>,
    /// Seconds left of the file playing, when its duration is known.
    pub remaining_secs: Option<f64>,
}

impl Deadline {
    /// Turn what the viewer asked for into an instant, or `None` when it cannot be known.
    ///
    /// `None` is not an error and not a guess. A channel with no guide data cannot answer
    /// "end of programme", and a live stream reporting no duration cannot answer "end of
    /// episode" — and inventing an hour for either would arm a timer the viewer did not
    /// ask for. The caller turns this into a sentence rather than a default.
    pub fn resolve(
        after: SleepAfter,
        action: SleepAction,
        now: i64,
        playing: &Playing,
    ) -> Option<Self> {
        let at_unix = match after {
            SleepAfter::Minutes(m) => {
                let minutes = m.clamp(MIN_MINUTES, MAX_MINUTES);
                now + i64::from(minutes) * 60
            }
            SleepAfter::EndOfProgramme => {
                let ends = playing.programme_ends_unix?;
                // A programme that has already finished -- the guide is stale, or the
                // tune happened in the gap between two airings -- is not a deadline. It
                // would fire immediately, which to a viewer is the timer being broken
                // rather than the guide being old.
                if ends <= now {
                    return None;
                }
                ends
            }
            SleepAfter::EndOfEpisode => {
                let remaining = playing.remaining_secs?;
                if !remaining.is_finite() || remaining <= 0.0 {
                    return None;
                }
                now + remaining.round() as i64
            }
        };
        Some(Self {
            at_unix,
            action,
            after,
        })
    }

    /// Seconds until it fires, never below zero.
    pub fn remaining(&self, now: i64) -> i64 {
        (self.at_unix - now).max(0)
    }

    /// Whether it is due.
    pub fn due(&self, now: i64) -> bool {
        now >= self.at_unix
    }

    /// Whether the viewer should be warned now.
    ///
    /// True for the whole last minute rather than once at the sixty-second mark: the
    /// warning is a thing on screen the viewer can cancel, not an event, and a tick that
    /// landed either side of one instant would miss it.
    pub fn warning(&self, now: i64) -> bool {
        let left = self.remaining(now);
        left > 0 && left <= WARNING_SECS
    }

    /// Push the deadline back, which is what the warning's "Keep watching" does.
    pub fn extended(self, minutes: u32) -> Self {
        let minutes = minutes.clamp(MIN_MINUTES, MAX_MINUTES);
        Self {
            at_unix: self.at_unix + i64::from(minutes) * 60,
            // No longer the thing that was asked for: a programme's end, pushed back
            // twenty minutes, is twenty minutes.
            after: SleepAfter::Minutes(minutes),
            ..self
        }
    }
}

/// What the UI draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SleepState {
    pub armed: Option<Deadline>,
    /// Seconds left, or `null` when nothing is armed. Derived here so the screen does not
    /// subtract clocks — the host's `now` is the one the deadline was set against.
    pub remaining_secs: Option<i64>,
    /// Whether the 60-second warning is up.
    pub warning: bool,
}

impl SleepState {
    pub fn of(armed: Option<Deadline>, now: i64) -> Self {
        Self {
            remaining_secs: armed.map(|d| d.remaining(now)),
            warning: armed.is_some_and(|d| d.warning(now)),
            armed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000;

    fn playing() -> Playing {
        Playing {
            programme_ends_unix: Some(NOW + 1_800),
            remaining_secs: Some(900.0),
        }
    }

    #[test]
    fn minutes_become_an_instant() {
        let d = Deadline::resolve(
            SleepAfter::Minutes(45),
            SleepAction::StopPlayback,
            NOW,
            &playing(),
        )
        .unwrap();
        assert_eq!(d.at_unix, NOW + 45 * 60);
        assert_eq!(d.remaining(NOW), 2_700);
        assert!(!d.due(NOW));
        assert!(d.due(NOW + 2_700));
    }

    /// README §13's five choices, all of them arming something.
    #[test]
    fn every_offered_duration_arms() {
        for m in [15, 30, 45, 60, 90] {
            let d = Deadline::resolve(
                SleepAfter::Minutes(m),
                SleepAction::StopPlayback,
                NOW,
                &playing(),
            )
            .expect("a documented duration must arm");
            assert_eq!(d.remaining(NOW), i64::from(m) * 60);
        }
    }

    /// A number from outside is held to the range rather than refused.
    #[test]
    fn an_absurd_number_of_minutes_is_clamped() {
        let huge = Deadline::resolve(
            SleepAfter::Minutes(u32::MAX),
            SleepAction::StopPlayback,
            NOW,
            &playing(),
        )
        .unwrap();
        assert_eq!(huge.remaining(NOW), i64::from(MAX_MINUTES) * 60);

        let tiny = Deadline::resolve(
            SleepAfter::Minutes(0),
            SleepAction::StopPlayback,
            NOW,
            &playing(),
        )
        .unwrap();
        assert_eq!(tiny.remaining(NOW), i64::from(MIN_MINUTES) * 60);
    }

    #[test]
    fn end_of_programme_follows_the_guide() {
        let d = Deadline::resolve(
            SleepAfter::EndOfProgramme,
            SleepAction::StopPlayback,
            NOW,
            &playing(),
        )
        .unwrap();
        assert_eq!(d.at_unix, NOW + 1_800);
    }

    /// The two cases that cannot be answered, and must not be guessed at.
    #[test]
    fn nothing_to_go_on_arms_nothing() {
        let blank = Playing::default();
        assert!(Deadline::resolve(
            SleepAfter::EndOfProgramme,
            SleepAction::StopPlayback,
            NOW,
            &blank
        )
        .is_none());
        assert!(Deadline::resolve(
            SleepAfter::EndOfEpisode,
            SleepAction::StopPlayback,
            NOW,
            &blank
        )
        .is_none());
        // ...while a flat duration needs nothing from the player at all, which is why it
        // is the one that always works.
        assert!(Deadline::resolve(
            SleepAfter::Minutes(30),
            SleepAction::StopPlayback,
            NOW,
            &blank
        )
        .is_some());
    }

    /// A stale guide must not arm a timer that fires at once.
    #[test]
    fn a_programme_that_already_ended_is_not_a_deadline() {
        let stale = Playing {
            programme_ends_unix: Some(NOW - 60),
            ..Playing::default()
        };
        assert!(Deadline::resolve(
            SleepAfter::EndOfProgramme,
            SleepAction::StopPlayback,
            NOW,
            &stale
        )
        .is_none());
    }

    #[test]
    fn an_episode_with_no_time_left_is_not_a_deadline() {
        for remaining in [0.0, -5.0, f64::NAN, f64::INFINITY] {
            let p = Playing {
                remaining_secs: Some(remaining),
                ..Playing::default()
            };
            assert!(
                Deadline::resolve(SleepAfter::EndOfEpisode, SleepAction::StopPlayback, NOW, &p)
                    .is_none(),
                "{remaining} armed a timer"
            );
        }
    }

    /// The warning is up for the whole last minute, not at one instant.
    #[test]
    fn the_warning_covers_the_last_minute() {
        let d = Deadline::resolve(
            SleepAfter::Minutes(15),
            SleepAction::StopPlayback,
            NOW,
            &playing(),
        )
        .unwrap();
        assert!(!d.warning(NOW), "not warning fifteen minutes out");
        assert!(!d.warning(d.at_unix - WARNING_SECS - 1));
        assert!(d.warning(d.at_unix - WARNING_SECS));
        assert!(d.warning(d.at_unix - 1), "still warning a second before");
        assert!(!d.warning(d.at_unix), "due is not warning");
        assert!(!d.warning(d.at_unix + 10), "past due is not warning");
    }

    /// "Keep watching" moves the deadline and stops claiming to be what it was.
    #[test]
    fn extending_pushes_it_back_and_forgets_the_programme() {
        let d = Deadline::resolve(
            SleepAfter::EndOfProgramme,
            SleepAction::StopPlayback,
            NOW,
            &playing(),
        )
        .unwrap();
        let later = d.extended(30);
        assert_eq!(later.at_unix, d.at_unix + 1_800);
        assert_eq!(
            later.after,
            SleepAfter::Minutes(30),
            "a programme's end pushed back half an hour is half an hour"
        );
        assert_eq!(later.action, d.action);
    }

    #[test]
    fn the_state_the_ui_reads_is_derived_from_one_clock() {
        let d = Deadline::resolve(
            SleepAfter::Minutes(15),
            SleepAction::StopPlayback,
            NOW,
            &playing(),
        )
        .unwrap();
        let s = SleepState::of(Some(d), NOW);
        assert_eq!(s.remaining_secs, Some(900));
        assert!(!s.warning);

        let nearly = SleepState::of(Some(d), d.at_unix - 10);
        assert_eq!(nearly.remaining_secs, Some(10));
        assert!(nearly.warning);

        let nothing = SleepState::of(None, NOW);
        assert_eq!(nothing.remaining_secs, None);
        assert!(!nothing.warning);
        assert!(nothing.armed.is_none());
    }

    /// The shutdown action has to be asked for by name.
    ///
    /// Not a flag on another variant, so there is no value of any other field that can
    /// turn "stop the video" into "turn the computer off".
    #[test]
    fn a_shutdown_is_its_own_choice() {
        assert_eq!(SleepAction::StopPlayback.as_str(), "stopPlayback");
        assert_eq!(SleepAction::ShutDownPc.as_str(), "shutDownPc");
        assert_ne!(SleepAction::StopPlayback, SleepAction::ShutDownPc);
    }
}
