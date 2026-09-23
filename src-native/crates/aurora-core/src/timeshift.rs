//! Pause live TV (README §7.6).
//!
//! Two things live here: the *budget* — how much history is kept — and the *window*,
//! which is the part of it the viewer can currently reach and where the playhead sits
//! inside it. Neither opens a socket or touches a disk.
//!
//! Aurora keeps no ring buffer of its own; the buffer is mpv's own on-disk demuxer
//! cache, and docs/DECISIONS.md D21 says why. What that leaves is arithmetic, and the
//! OSD is only honest if the arithmetic is: a scrub bar offering half an hour of rewind
//! on a buffer holding seven minutes is worse than no scrub bar at all.

use serde::{Deserialize, Serialize};

/// Default buffer, README §7.6: "1 GB / 30 min".
pub const DEFAULT_BYTES: u64 = 1024 * 1024 * 1024;
/// Ceiling, README §7.6: "up to 10 GB".
pub const MAX_BYTES: u64 = 10 * 1024 * 1024 * 1024;
/// Below this there is nothing to pause into — a 20 Mb/s feed fills 64 MiB in half a
/// minute — so a smaller number is taken as a mistake rather than honoured.
pub const MIN_BYTES: u64 = 64 * 1024 * 1024;

pub const DEFAULT_SECS: u32 = 30 * 60;
pub const MIN_SECS: u32 = 60;
/// Six hours. Not a storage limit — the byte cap is that — but the point past which a
/// "pause live TV" buffer is really a recording, and recordings are §7.7's job.
pub const MAX_SECS: u32 = 6 * 60 * 60;

/// How close to the newest buffered moment still counts as watching live.
///
/// A live stream is always a few seconds behind its own edge — that is what the network
/// buffer is — so equality is the wrong test and "Live" has to mean "near enough".
pub const LIVE_EDGE_SECS: f64 = 5.0;

/// How much of a live stream to keep for rewinding.
///
/// Both caps are real and the tighter one wins. Bytes are what the disk actually
/// spends; minutes are what a person means. A gigabyte is hours of a radio stream,
/// eighteen minutes of an 8 Mb/s HD feed and seven of a 20 Mb/s 4K one, so neither
/// number on its own describes the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Budget {
    pub bytes: u64,
    pub secs: u32,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            bytes: DEFAULT_BYTES,
            secs: DEFAULT_SECS,
        }
    }
}

impl Budget {
    /// Build a budget from numbers that came from outside, held to the documented range.
    ///
    /// Clamped rather than rejected: these arrive from a settings screen and from a
    /// database that an older build may have written, and neither is a reason to refuse
    /// to buffer.
    pub fn clamped(bytes: u64, secs: u32) -> Self {
        Self {
            bytes: bytes.clamp(MIN_BYTES, MAX_BYTES),
            secs: secs.clamp(MIN_SECS, MAX_SECS),
        }
    }

    /// How many seconds the byte cap holds at this bitrate, or `None` when the bitrate
    /// is not known yet.
    ///
    /// `None` is not zero: a stream whose bitrate has not been observed must not have
    /// its window reported as empty, so the caller falls back to the other bounds.
    pub fn bytes_as_secs(&self, bitrate_bps: u64) -> Option<f64> {
        if bitrate_bps == 0 {
            return None;
        }
        Some((self.bytes * 8) as f64 / bitrate_bps as f64)
    }

    /// What the viewer can reach, from what the player can actually report.
    ///
    /// Three bounds, and the latest of them wins: the minutes cap, what the byte cap
    /// can hold at the observed bitrate, and how long this channel has been on. The
    /// last one is the one people notice — ten seconds after a zap there is ten seconds
    /// of rewind, whatever the settings say.
    pub fn window(&self, reading: &Reading) -> Window {
        let live = reading.live_secs.max(reading.position_secs);
        let mut start = live - f64::from(self.secs);
        if let Some(secs) = self.bytes_as_secs(reading.bitrate_bps) {
            start = start.max(live - secs);
        }
        let start = start.max(reading.tuned_at_secs).min(reading.position_secs);
        Window {
            start_secs: start,
            position_secs: reading.position_secs,
            live_secs: live,
        }
    }
}

/// What the player knows about a live stream in flight.
///
/// Deliberately the properties a backend can read cheaply and often: mpv's `time-pos`,
/// `demuxer-cache-time`, and the bitrates it already reports for the stats overlay.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Reading {
    /// The playhead, in the stream's own timebase.
    pub position_secs: f64,
    /// The newest buffered moment — the live edge.
    pub live_secs: f64,
    /// The playhead when this channel was tuned. Nothing before it was ever buffered,
    /// and a live stream's timebase does not have to start at zero.
    pub tuned_at_secs: f64,
    /// Observed bits per second, video and audio together. Zero when not known yet.
    pub bitrate_bps: u64,
}

/// The reachable span of a buffered live stream, and the playhead within it.
///
/// Three timestamps rather than the derived readouts, so the host and the OSD cannot
/// disagree about what is on screen. Everything else is a method.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    /// The oldest moment still held.
    pub start_secs: f64,
    pub position_secs: f64,
    /// The live edge.
    pub live_secs: f64,
}

impl Window {
    /// How far behind live the playhead is. Zero when watching live.
    pub fn delay_secs(&self) -> f64 {
        (self.live_secs - self.position_secs).max(0.0)
    }

    /// How much rewind is left before the buffer runs out.
    pub fn rewindable_secs(&self) -> f64 {
        (self.position_secs - self.start_secs).max(0.0)
    }

    /// The whole reachable span.
    pub fn span_secs(&self) -> f64 {
        (self.live_secs - self.start_secs).max(0.0)
    }

    /// Where the playhead sits in the span: 0.0 at the oldest moment, 1.0 at live.
    pub fn ratio(&self) -> f64 {
        let span = self.span_secs();
        if span <= 0.0 {
            return 1.0;
        }
        (self.rewindable_secs() / span).clamp(0.0, 1.0)
    }

    pub fn is_at_live(&self) -> bool {
        self.delay_secs() <= LIVE_EDGE_SECS
    }

    /// Hold an absolute seek target inside the buffer.
    ///
    /// Forward past live is the one that matters: nothing beyond the live edge exists
    /// yet, and asking mpv for it on a live stream is how a picture ends up frozen.
    pub fn clamp(&self, target_secs: f64) -> f64 {
        target_secs.clamp(self.start_secs, self.live_secs)
    }

    /// Hold a relative seek inside the buffer, in the same terms the caller asked in:
    /// the returned offset is what is left of `delta` once the ends are respected.
    pub fn clamp_relative(&self, delta_secs: f64) -> f64 {
        self.clamp(self.position_secs + delta_secs) - self.position_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1080p IPTV, roughly.
    const HD: u64 = 8_000_000;
    /// What a 4K feed costs, and the case where the byte cap starts to bite.
    const UHD: u64 = 20_000_000;

    fn reading(position: f64, live: f64, bitrate: u64) -> Reading {
        Reading {
            position_secs: position,
            live_secs: live,
            tuned_at_secs: 0.0,
            bitrate_bps: bitrate,
        }
    }

    #[test]
    fn the_default_budget_is_the_one_the_spec_names() {
        let b = Budget::default();
        assert_eq!(b.bytes, 1024 * 1024 * 1024, "1 GB");
        assert_eq!(b.secs, 1800, "30 minutes");
    }

    #[test]
    fn numbers_from_outside_are_held_to_the_documented_range() {
        // A settings screen asking for 40 GB, and one asking for none at all.
        assert_eq!(Budget::clamped(40 * MAX_BYTES, 600).bytes, MAX_BYTES);
        assert_eq!(Budget::clamped(1, 600).bytes, MIN_BYTES);
        assert_eq!(Budget::clamped(DEFAULT_BYTES, 0).secs, MIN_SECS);
        assert_eq!(Budget::clamped(DEFAULT_BYTES, 99 * 3600).secs, MAX_SECS);
        // Anything already inside it is left exactly alone.
        let b = Budget::clamped(2 * 1024 * 1024 * 1024, 1200);
        assert_eq!((b.bytes, b.secs), (2 * 1024 * 1024 * 1024, 1200));
    }

    #[test]
    fn a_gigabyte_is_minutes_of_hd_and_hours_of_radio() {
        let b = Budget::default();
        let hd = b.bytes_as_secs(HD).unwrap();
        assert!(
            (1000.0..1150.0).contains(&hd),
            "1 GB of 8 Mb/s is about eighteen minutes, got {hd}"
        );
        let uhd = b.bytes_as_secs(UHD).unwrap();
        assert!(
            (400.0..460.0).contains(&uhd),
            "1 GB of 20 Mb/s is about seven minutes, got {uhd}"
        );
        let radio = b.bytes_as_secs(128_000).unwrap();
        assert!(
            radio > 6.0 * 3600.0,
            "1 GB of 128 kb/s is hours, got {radio}"
        );
    }

    #[test]
    fn an_unobserved_bitrate_is_not_an_empty_buffer() {
        assert_eq!(Budget::default().bytes_as_secs(0), None);
        // …and the window falls back to the other bounds rather than collapsing.
        let w = Budget::default().window(&reading(3600.0, 3600.0, 0));
        assert_eq!(w.span_secs(), 1800.0, "the minutes cap still applies");
    }

    #[test]
    fn the_byte_cap_wins_over_the_minutes_cap_on_a_fat_stream() {
        // Half an hour of a 20 Mb/s feed is 4.5 GB, which does not fit in the default
        // gigabyte, so the window has to be the seven minutes the bytes hold — not the
        // thirty the setting names.
        let w = Budget::default().window(&reading(7200.0, 7200.0, UHD));
        let span = w.span_secs();
        assert!(
            (400.0..460.0).contains(&span),
            "expected the byte cap to bound the window, got {span}"
        );
    }

    #[test]
    fn the_minutes_cap_wins_over_the_byte_cap_on_a_thin_stream() {
        let w = Budget::default().window(&reading(7200.0, 7200.0, 128_000));
        assert_eq!(w.span_secs(), 1800.0);
    }

    #[test]
    fn just_after_a_zap_the_window_is_as_long_as_the_channel_has_been_on() {
        // Ten seconds in, nothing older than the tune exists, whatever the caps allow.
        let w = Budget::default().window(&Reading {
            position_secs: 10.0,
            live_secs: 10.0,
            tuned_at_secs: 0.0,
            bitrate_bps: 128_000,
        });
        assert_eq!(w.span_secs(), 10.0);
        assert_eq!(w.rewindable_secs(), 10.0);
    }

    #[test]
    fn a_stream_whose_clock_does_not_start_at_zero_is_handled() {
        // A live TS carries whatever PTS the provider is up to; the window must be
        // measured from the tune, not from zero.
        let w = Budget::default().window(&Reading {
            position_secs: 90_060.0,
            live_secs: 90_060.0,
            tuned_at_secs: 90_000.0,
            bitrate_bps: 128_000,
        });
        assert_eq!(w.start_secs, 90_000.0);
        assert_eq!(w.span_secs(), 60.0);
    }

    #[test]
    fn the_window_reports_delay_rewind_and_position() {
        let w = Window {
            start_secs: 100.0,
            position_secs: 400.0,
            live_secs: 700.0,
        };
        assert_eq!(w.delay_secs(), 300.0);
        assert_eq!(w.rewindable_secs(), 300.0);
        assert_eq!(w.span_secs(), 600.0);
        assert_eq!(w.ratio(), 0.5);
        assert!(!w.is_at_live());
    }

    #[test]
    fn live_means_near_enough_to_the_edge_not_exactly_on_it() {
        let at_edge = Window {
            start_secs: 0.0,
            position_secs: 597.0,
            live_secs: 600.0,
        };
        assert!(
            at_edge.is_at_live(),
            "three seconds behind is what watching live looks like"
        );
        let behind = Window {
            start_secs: 0.0,
            position_secs: 580.0,
            live_secs: 600.0,
        };
        assert!(!behind.is_at_live());
    }

    #[test]
    fn a_window_with_nothing_in_it_reads_as_live() {
        // The moment a channel is tuned, before a single second is buffered.
        let w = Budget::default().window(&reading(0.0, 0.0, 0));
        assert_eq!(w.span_secs(), 0.0);
        assert_eq!(w.ratio(), 1.0, "no span means the playhead is at the edge");
        assert!(w.is_at_live());
    }

    #[test]
    fn seeking_is_held_inside_the_buffer_at_both_ends() {
        let w = Window {
            start_secs: 100.0,
            position_secs: 400.0,
            live_secs: 700.0,
        };
        assert_eq!(w.clamp(500.0), 500.0, "inside is left alone");
        assert_eq!(w.clamp(9_000.0), 700.0, "nothing past live exists yet");
        assert_eq!(w.clamp(0.0), 100.0, "nor anything before the oldest byte");
    }

    #[test]
    fn a_relative_seek_comes_back_as_what_is_left_of_it() {
        let w = Window {
            start_secs: 100.0,
            position_secs: 400.0,
            live_secs: 700.0,
        };
        assert_eq!(w.clamp_relative(-60.0), -60.0);
        assert_eq!(
            w.clamp_relative(-9_000.0),
            -300.0,
            "rewinding past the start stops at the start"
        );
        assert_eq!(
            w.clamp_relative(9_000.0),
            300.0,
            "fast-forwarding lands on live, not past it"
        );
    }

    #[test]
    fn the_position_is_never_outside_the_window_it_is_in() {
        // Whatever the caps say, the start cannot be reported as later than the
        // playhead — a scrub bar with the handle off the left edge is a bug on screen.
        let w = Budget::clamped(MIN_BYTES, MIN_SECS).window(&Reading {
            position_secs: 5.0,
            live_secs: 900.0,
            tuned_at_secs: 0.0,
            bitrate_bps: UHD,
        });
        assert!(w.start_secs <= w.position_secs);
        assert_eq!(w.ratio(), 0.0);
    }

    #[test]
    fn a_window_survives_a_round_trip_through_json() {
        let w = Window {
            start_secs: 1.5,
            position_secs: 2.5,
            live_secs: 3.5,
        };
        let json = serde_json::to_string(&w).unwrap();
        assert!(json.contains("startSecs"), "the UI reads camelCase: {json}");
        assert_eq!(serde_json::from_str::<Window>(&json).unwrap(), w);
    }
}
