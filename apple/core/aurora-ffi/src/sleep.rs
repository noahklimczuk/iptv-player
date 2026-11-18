//! The sleep timer (README §13, docs/PLAYER.md H19–H20).
//!
//! Mirrors `aurora-app/src/play/sleep.rs`, which lives in the Windows host rather than a
//! shared crate, so it is repeated here and its constants are checked against the
//! original (`mirrors.rs`). One difference, stated plainly: Windows can stop playback,
//! quit the app or shut the PC down. On Apple the timer only ever stops playback: an
//! iPhone app must not quit itself, and neither platform lets an app power the device
//! off. So there is no action to choose and none is stored.
//!
//! Pure: given the time and what is known about what is playing, it keeps the deadline.
//! The player asks [`SleepTimer::due`] and stops when it says so.

use aurora_db::repo::{channels, epg};
use aurora_db::rusqlite::Connection;

use crate::error::{AuroraError, Result};
use crate::{now_unix, AuroraCore};

/// The warning shows for the last minute (H20).
pub const WARNING_SECS: i64 = 60;
/// The longest a timer can be set or pushed out to: half a day.
pub const MAX_MINUTES: u32 = 12 * 60;

/// When the timer should go off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SleepAfter {
    /// When the programme on now ends, from the guide (live).
    EndOfProgramme,
    /// When the film or episode ends, from its duration and the speed.
    EndOfEpisode,
    Minutes { minutes: u32 },
}

/// What the player shows about the timer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, uniffi::Record)]
pub struct SleepState {
    /// Unix seconds it goes off at, when one is set.
    pub at_unix: Option<i64>,
    /// What it was set for, so the menu can show the choice.
    pub after: Option<SleepAfter>,
    pub remaining_secs: Option<i64>,
    /// In the last minute: show the warning with Keep watching and Cancel.
    pub warning: bool,
}

/// What is playing, to turn "end of …" into a time.
#[derive(Debug, Clone, Copy, Default, PartialEq, uniffi::Record)]
pub struct SleepPlaying {
    /// The live channel on screen, whose guide says when the programme ends.
    pub channel_id: Option<i64>,
    /// Seconds of the film or episode left at normal speed, when its length is known.
    pub remaining_secs: Option<f64>,
    pub speed: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Deadline {
    at_unix: i64,
    after: SleepAfter,
}

#[derive(Debug, Default)]
pub(crate) struct SleepTimer {
    armed: Option<Deadline>,
}

/// `Playing` in the Windows timer: the guide already consulted.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Playing {
    programme_ends: Option<i64>,
    remaining_secs: Option<f64>,
    speed: f64,
}

impl SleepTimer {
    fn state(&self, now: i64) -> SleepState {
        match self.armed {
            None => SleepState::default(),
            Some(d) => {
                let left = (d.at_unix - now).max(0);
                SleepState {
                    at_unix: Some(d.at_unix),
                    after: Some(d.after),
                    remaining_secs: Some(left),
                    warning: left <= WARNING_SECS,
                }
            }
        }
    }

    /// Arm it (H19). Refuses, in words, when there is nothing to go on or the time asked
    /// for has already passed.
    fn set(&mut self, after: SleepAfter, now: i64, playing: Playing) -> std::result::Result<SleepState, String> {
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
                "The guide has no listing for what is on, so Aurora cannot tell when it ends."
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
        self.armed = Some(Deadline { at_unix: at, after });
        Ok(self.state(now))
    }

    fn cancel(&mut self, now: i64) -> SleepState {
        self.armed = None;
        self.state(now)
    }

    /// "Keep watching": push the deadline back (H20).
    fn extend(&mut self, minutes: u32, now: i64) -> std::result::Result<SleepState, String> {
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

    /// Whether the deadline has come, once. Disarms it.
    fn due(&mut self, now: i64) -> bool {
        match self.armed {
            Some(d) if now >= d.at_unix => {
                self.armed = None;
                true
            }
            _ => false,
        }
    }
}

/// The programme on `channel_id` at `at`, as `aurora-app/src/play/guide.rs` `on_at`
/// finds it: a channel the provider never matched to a listing has none.
fn programme_ends(db: &Connection, channel_id: i64, at: i64) -> Result<Option<i64>> {
    let Some(ch) = channels::get(db, channel_id)? else {
        return Ok(None);
    };
    let Some(id) = ch.epg_channel_id.filter(|id| !id.trim().is_empty()) else {
        return Ok(None);
    };
    Ok(epg::now_next(db, &id, at)?.0.map(|p| p.stop))
}

#[uniffi::export]
impl AuroraCore {
    pub fn sleep_state(&self) -> SleepState {
        self.sleep.lock().state(now_unix())
    }

    /// Arm the timer, replacing any set before. `playing` is what is on screen; the
    /// guide is read here for "end of programme", as `aurora-app` `playing_now` does.
    pub fn sleep_set(&self, after: SleepAfter, playing: SleepPlaying) -> Result<SleepState> {
        let now = now_unix();
        let programme_ends = match playing.channel_id {
            Some(ch) if after == SleepAfter::EndOfProgramme => {
                programme_ends(&self.db.lock(), ch, now)?
            }
            _ => None,
        };
        let playing = Playing {
            programme_ends,
            remaining_secs: playing.remaining_secs,
            speed: playing.speed,
        };
        self.sleep.lock().set(after, now, playing).map_err(AuroraError::other)
    }

    pub fn sleep_cancel(&self) -> SleepState {
        self.sleep.lock().cancel(now_unix())
    }

    pub fn sleep_extend(&self, minutes: u32) -> Result<SleepState> {
        self.sleep.lock().extend(minutes, now_unix()).map_err(AuroraError::other)
    }

    /// True once, when the deadline has come: stop playback then.
    pub fn sleep_due(&self) -> bool {
        self.sleep.lock().due(now_unix())
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
            let s = t.set(mins(m), NOW, Playing::default()).unwrap();
            assert_eq!(s.remaining_secs, Some(i64::from(m) * 60));
            assert!(!s.warning);
        }
        assert!(t.set(mins(0), NOW, Playing::default()).is_err());
    }

    #[test]
    fn h19_end_of_programme_needs_the_guide() {
        let mut t = SleepTimer::default();
        assert!(t.set(SleepAfter::EndOfProgramme, NOW, Playing::default()).is_err());
        assert!(t.armed.is_none(), "nothing to go on armed something");
        let on = Playing { programme_ends: Some(NOW + 1200), ..Playing::default() };
        let s = t.set(SleepAfter::EndOfProgramme, NOW, on).unwrap();
        assert_eq!(s.at_unix, Some(NOW + 1200));
        assert_eq!(s.after, Some(SleepAfter::EndOfProgramme));
    }

    #[test]
    fn h19_end_of_episode_from_its_duration_and_speed() {
        let mut t = SleepTimer::default();
        let p = Playing { remaining_secs: Some(600.0), speed: 2.0, ..Playing::default() };
        let s = t.set(SleepAfter::EndOfEpisode, NOW, p).unwrap();
        assert_eq!(s.at_unix, Some(NOW + 300));
        assert!(t.set(SleepAfter::EndOfEpisode, NOW, Playing::default()).is_err());
    }

    #[test]
    fn h19_a_time_already_past_is_not_a_deadline() {
        let mut t = SleepTimer::default();
        let ended = Playing { programme_ends: Some(NOW - 5), ..Playing::default() };
        assert!(t.set(SleepAfter::EndOfProgramme, NOW, ended).is_err());
        assert!(t.armed.is_none());
    }

    #[test]
    fn h20_warns_in_the_last_minute_extends_and_cancels() {
        let mut t = SleepTimer::default();
        t.set(mins(15), NOW, Playing::default()).unwrap();
        assert!(!t.state(NOW + 15 * 60 - 61).warning);
        assert!(t.state(NOW + 15 * 60 - 60).warning);
        let s = t.extend(15, NOW + 15 * 60 - 30).unwrap();
        assert_eq!(s.at_unix, Some(NOW + 30 * 60));
        assert!(!s.warning);
        assert_eq!(t.cancel(NOW).at_unix, None);
        assert!(t.extend(5, NOW).is_err());
    }

    #[test]
    fn it_fires_once() {
        let mut t = SleepTimer::default();
        t.set(mins(15), NOW, Playing::default()).unwrap();
        assert!(!t.due(NOW + 899));
        assert!(t.due(NOW + 900));
        assert!(!t.due(NOW + 901));
    }

    #[test]
    fn end_of_programme_reads_the_guide_and_a_channel_without_one_is_refused() {
        let (core, _dir) = crate::testing::core();
        // A channel id the library does not have: nothing to read, refused in words.
        let err = core
            .sleep_set(
                SleepAfter::EndOfProgramme,
                SleepPlaying { channel_id: Some(999), remaining_secs: None, speed: 1.0 },
            )
            .unwrap_err();
        assert!(err.to_string().contains("guide"), "{err}");
        assert_eq!(core.sleep_state(), SleepState::default());
        // Minutes need nothing from the guide, and go off once.
        let s = core.sleep_set(mins(30), SleepPlaying::default()).unwrap();
        assert_eq!(s.remaining_secs, Some(1800));
        assert!(!core.sleep_due());
        assert_eq!(core.sleep_cancel().at_unix, None);
    }
}
