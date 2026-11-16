//! The sleep timer (README §13, docs/PLAYER.md H19–H20).
//!
//! Pure: it is given the time and what is known about what is playing, and keeps the
//! deadline. The app's loop asks [`SleepTimer::due`] and carries out the action.

use serde::{Deserialize, Serialize};

/// The warning shows for the last minute (H20).
pub const WARNING_SECS: i64 = 60;
/// The longest a timer can be set or pushed out to: half a day.
pub const MAX_MINUTES: u32 = 12 * 60;

/// `SleepAction` in `shared/ipc.ts`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SleepAction {
    #[default]
    StopPlayback,
    QuitApp,
    /// Only ever reached by being chosen by name (H20).
    ShutDownPc,
}

/// `SleepAfter` in `shared/ipc.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SleepAfter {
    EndOfProgramme,
    EndOfEpisode,
    /// Untagged, so it is `{ "minutes": 30 }` on the wire; serde wants it last.
    #[serde(untagged)]
    Minutes { minutes: u32 },
}

/// `SleepDeadline` in `shared/ipc.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SleepDeadline {
    pub at_unix: i64,
    pub action: SleepAction,
    pub after: SleepAfter,
}

/// `SleepState` in `shared/ipc.ts`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SleepState {
    pub armed: Option<SleepDeadline>,
    pub remaining_secs: Option<i64>,
    pub warning: bool,
}

/// What is known about what is playing, to turn "end of …" into a time.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Playing {
    /// When the programme on now ends, from the guide (live).
    pub programme_ends: Option<i64>,
    /// Seconds of the episode or film left at normal speed, from its duration.
    pub remaining_secs: Option<f64>,
    pub speed: f64,
}

#[derive(Debug, Default)]
pub struct SleepTimer {
    armed: Option<SleepDeadline>,
}

impl SleepTimer {
    pub fn state(&self, now: i64) -> SleepState {
        match self.armed {
            None => SleepState::default(),
            Some(d) => {
                let left = (d.at_unix - now).max(0);
                SleepState {
                    armed: Some(d),
                    remaining_secs: Some(left),
                    warning: left <= WARNING_SECS,
                }
            }
        }
    }

    /// Arm it (H19). Refuses, in words, when there is nothing to go on or the time
    /// asked for has already passed.
    pub fn set(
        &mut self,
        after: SleepAfter,
        action: SleepAction,
        now: i64,
        playing: Playing,
    ) -> Result<SleepState, String> {
        let at = match after {
            SleepAfter::Minutes { minutes } => {
                if minutes == 0 || minutes > MAX_MINUTES {
                    return Err(format!(
                        "A sleep timer runs for 1 to {MAX_MINUTES} minutes, not {minutes}."
                    ));
                }
                now + i64::from(minutes) * 60
            }
            SleepAfter::EndOfProgramme => playing.programme_ends.ok_or_else(|| {
                "The guide has no listing for what is on, so Aurora cannot tell when it \
                 ends."
                    .to_string()
            })?,
            SleepAfter::EndOfEpisode => {
                let left = playing.remaining_secs.filter(|s| s.is_finite()).ok_or_else(|| {
                    "This stream does not say how long it is, so Aurora cannot tell when it \
                     ends."
                        .to_string()
                })?;
                let speed = if playing.speed.is_finite() && playing.speed > 0.0 {
                    playing.speed
                } else {
                    1.0
                };
                now + (left / speed).ceil() as i64
            }
        };
        if at <= now {
            return Err("That has already ended.".into());
        }
        self.armed = Some(SleepDeadline {
            at_unix: at,
            action,
            after,
        });
        Ok(self.state(now))
    }

    pub fn cancel(&mut self, now: i64) -> SleepState {
        self.armed = None;
        self.state(now)
    }

    /// "Keep watching": push the deadline back (H20).
    pub fn extend(&mut self, minutes: u32, now: i64) -> Result<SleepState, String> {
        let Some(d) = self.armed.as_mut() else {
            return Err("No sleep timer is set.".into());
        };
        if minutes == 0 || minutes > MAX_MINUTES {
            return Err(format!("It can be pushed back 1 to {MAX_MINUTES} minutes."));
        }
        // From now if it had already run out unseen, so extending always gives time.
        d.at_unix = d.at_unix.max(now) + i64::from(minutes) * 60;
        Ok(self.state(now))
    }

    /// The deadline, once, when it has come. Disarms it.
    pub fn due(&mut self, now: i64) -> Option<SleepDeadline> {
        let d = self.armed?;
        if now < d.at_unix {
            return None;
        }
        self.armed = None;
        Some(d)
    }

    pub fn armed(&self) -> bool {
        self.armed.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_760_000_000;

    fn mins(minutes: u32) -> SleepAfter {
        SleepAfter::Minutes { minutes }
    }

    #[test]
    fn h19_minutes_from_now() {
        let mut t = SleepTimer::default();
        for m in [15, 30, 45, 60, 90] {
            let s = t.set(mins(m), SleepAction::StopPlayback, NOW, Playing::default()).unwrap();
            assert_eq!(s.remaining_secs, Some(i64::from(m) * 60));
            assert!(!s.warning);
        }
        assert!(t.set(mins(0), SleepAction::StopPlayback, NOW, Playing::default()).is_err());
    }

    #[test]
    fn h19_end_of_programme_needs_the_guide() {
        let mut t = SleepTimer::default();
        assert!(t
            .set(SleepAfter::EndOfProgramme, SleepAction::StopPlayback, NOW, Playing::default())
            .is_err());
        assert!(!t.armed(), "nothing to go on armed something");
        let on = Playing {
            programme_ends: Some(NOW + 1200),
            ..Playing::default()
        };
        let s = t.set(SleepAfter::EndOfProgramme, SleepAction::QuitApp, NOW, on).unwrap();
        assert_eq!(s.armed.unwrap().at_unix, NOW + 1200);
    }

    #[test]
    fn h19_end_of_episode_from_its_duration_and_speed() {
        let mut t = SleepTimer::default();
        let p = Playing {
            remaining_secs: Some(600.0),
            speed: 2.0,
            ..Playing::default()
        };
        let s = t.set(SleepAfter::EndOfEpisode, SleepAction::StopPlayback, NOW, p).unwrap();
        assert_eq!(s.armed.unwrap().at_unix, NOW + 300);
        assert!(t
            .set(SleepAfter::EndOfEpisode, SleepAction::StopPlayback, NOW, Playing::default())
            .is_err());
    }

    #[test]
    fn h19_a_time_already_past_is_not_a_deadline() {
        let mut t = SleepTimer::default();
        let ended = Playing {
            programme_ends: Some(NOW - 5),
            ..Playing::default()
        };
        assert!(t
            .set(SleepAfter::EndOfProgramme, SleepAction::StopPlayback, NOW, ended)
            .is_err());
        assert!(!t.armed());
    }

    #[test]
    fn h20_warns_in_the_last_minute_extends_and_cancels() {
        let mut t = SleepTimer::default();
        t.set(mins(15), SleepAction::StopPlayback, NOW, Playing::default()).unwrap();
        assert!(t.state(NOW + 15 * 60 - 61).warning == false);
        assert!(t.state(NOW + 15 * 60 - 60).warning);
        let s = t.extend(15, NOW + 15 * 60 - 30).unwrap();
        assert_eq!(s.armed.unwrap().at_unix, NOW + 30 * 60);
        assert!(!s.warning);
        assert_eq!(t.cancel(NOW).armed, None);
        assert!(t.extend(5, NOW).is_err());
    }

    #[test]
    fn it_fires_once() {
        let mut t = SleepTimer::default();
        t.set(mins(15), SleepAction::ShutDownPc, NOW, Playing::default()).unwrap();
        assert_eq!(t.due(NOW + 899), None);
        assert_eq!(t.due(NOW + 900).unwrap().action, SleepAction::ShutDownPc);
        assert_eq!(t.due(NOW + 901), None);
    }

    #[test]
    fn the_wire_shapes_match_the_contract() {
        let after: SleepAfter = serde_json::from_str(r#"{"minutes":30}"#).unwrap();
        assert_eq!(after, mins(30));
        let after: SleepAfter = serde_json::from_str(r#""endOfProgramme""#).unwrap();
        assert_eq!(after, SleepAfter::EndOfProgramme);
        assert_eq!(serde_json::to_value(mins(45)).unwrap(), serde_json::json!({"minutes": 45}));
        assert_eq!(
            serde_json::to_value(SleepAction::ShutDownPc).unwrap(),
            serde_json::json!("shutDownPc")
        );
        // Shutdown is never what an absent action means.
        assert_eq!(SleepAction::default(), SleepAction::StopPlayback);
    }
}
