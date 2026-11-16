//! The seek preview: pictures of what has played, and what a moment was
//! (docs/PLAYER.md H24–H25).
//!
//! Frames come from the player itself as it plays, not from a second stream: a
//! provider line commonly allows one connection. So a picture exists only for a moment
//! that has been on screen, which is exactly the promise (H25: never of time ahead).

use aurora_core::markers::{self, Chapter, MarkerKind, SkipMarker};
use aurora_video::{Frame, Report, Status};
use base64::Engine as _;
use serde::Serialize;

use super::resolve::What;

/// Playing steadily with a picture this long before anything is grabbed (H24).
pub const STEADY_SECS: f64 = 3.0;
/// Frames kept for one title.
pub const KEEP: usize = 90;
/// Frames are never closer together than this.
pub const MIN_SPACING_SECS: f64 = 5.0;
/// The bubble's picture width, pixels.
pub const MAX_WIDTH: u32 = 320;
/// A jump in position bigger than this, beyond what the clock explains, is a seek.
const SEEK_JUMP_SECS: f64 = 2.0;

/// `SeekPreview` in `shared/ipc.ts`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeekPreview {
    pub position_secs: f64,
    pub image: Option<String>,
    pub image_at_secs: Option<f64>,
    pub pending: bool,
    pub label: Option<String>,
    pub clock_unix: Option<i64>,
    pub programme: Option<String>,
}

#[derive(Debug, Clone)]
struct Shot {
    at: f64,
    url: String,
}

#[derive(Debug, Default)]
pub struct Frames {
    title: Option<What>,
    shots: Vec<Shot>,
    steady_since: Option<f64>,
    last: Option<(f64, f64)>,
    spacing: f64,
}

/// How far apart frames are for this report: the title's length shared out (H24).
fn spacing(r: &Report) -> f64 {
    let span = match r.timeshift {
        Some(w) if r.is_live => w.live_secs - w.start_secs,
        _ if r.duration_secs > 0.0 => r.duration_secs,
        _ => 0.0,
    };
    (span / KEEP as f64).max(MIN_SPACING_SECS)
}

impl Frames {
    /// Look at the player; `true` when a frame should be grabbed now.
    pub fn observe(&mut self, title: Option<What>, r: &Report, now: f64) -> bool {
        if title != self.title {
            // H24: never offered for another title.
            *self = Self {
                title,
                ..Self::default()
            };
        }
        if title.is_none() || r.status != Status::Playing || !r.has_picture {
            self.steady_since = None;
            self.last = None;
            return false;
        }
        let pos = r.position_secs;
        if let Some((then, was)) = self.last {
            let expected = was + (now - then) * r.speed.max(0.0);
            if (pos - expected).abs() > SEEK_JUMP_SECS {
                self.steady_since = None; // H24: a seek restarts the wait.
            }
        }
        self.last = Some((now, pos));
        let since = *self.steady_since.get_or_insert(now);
        if now - since < STEADY_SECS {
            return false;
        }
        self.spacing = spacing(r);
        let half = self.spacing / 2.0;
        !self.shots.iter().any(|s| (s.at - pos).abs() < half)
    }

    /// Keep a picture taken at `at`. The frames furthest from `now_at` go first (H24).
    pub fn keep(&mut self, title: Option<What>, at: f64, url: String, now_at: f64) {
        if title != self.title || title.is_none() {
            return; // The title changed while the picture was being taken.
        }
        self.shots.push(Shot { at, url });
        while self.shots.len() > KEEP {
            let furthest = self
                .shots
                .iter()
                .enumerate()
                .max_by(|a, b| {
                    (a.1.at - now_at)
                        .abs()
                        .total_cmp(&(b.1.at - now_at).abs())
                })
                .map(|(i, _)| i)
                .expect("not empty");
            self.shots.remove(furthest);
        }
    }

    /// The picture for `position` of `title`: the nearest kept near its moment.
    pub fn nearest(&self, title: Option<What>, position: f64) -> Option<(f64, &str)> {
        if title != self.title || title.is_none() {
            return None;
        }
        let near = self.spacing.max(MIN_SPACING_SECS);
        self.shots
            .iter()
            .filter(|s| (s.at - position).abs() <= near)
            .min_by(|a, b| (a.at - position).abs().total_cmp(&(b.at - position).abs()))
            .map(|s| (s.at, s.url.as_str()))
    }

    pub fn len(&self) -> usize {
        self.shots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shots.is_empty()
    }
}

/// What a moment is called: a skip marker's kind, or the chapter it falls in (H25).
pub fn label(at: f64, chapters: &[Chapter], marks: &[SkipMarker]) -> Option<String> {
    if let Some(m) = markers::active_marker(marks, at) {
        return Some(
            match m.kind {
                MarkerKind::Intro => "Intro",
                MarkerKind::Recap => "Recap",
                MarkerKind::Credits => "Credits",
            }
            .to_string(),
        );
    }
    chapters
        .iter()
        .filter(|c| c.start_secs <= at)
        .max_by(|a, b| a.start_secs.total_cmp(&b.start_secs))
        .and_then(|c| c.title.clone())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// On live TV, the wall-clock moment `at` was, given the buffer (H25).
pub fn clock_at(r: &Report, at: f64, now_unix: i64) -> Option<i64> {
    let w = r.timeshift.filter(|_| r.is_live)?;
    let behind = (w.live_secs - at).max(0.0);
    Some(now_unix - behind.round() as i64)
}

/// A frame as a PNG `data:` URL.
pub fn data_url(frame: &Frame) -> Option<String> {
    let mut png = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png, frame.width, frame.height);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().ok()?;
        w.write_image_data(&frame.rgb).ok()?;
        w.finish().ok()?;
    }
    let mut url = String::from("data:image/png;base64,");
    url.reserve(png.len() * 4 / 3 + 4);
    base64::engine::general_purpose::STANDARD.encode_string(&png, &mut url);
    Some(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::markers::MarkerSource;
    use aurora_core::timeshift::Window;

    const FILM: Option<What> = Some(What::Movie(1));
    const OTHER: Option<What> = Some(What::Movie(2));

    fn playing(pos: f64) -> Report {
        Report {
            status: Status::Playing,
            has_picture: true,
            position_secs: pos,
            duration_secs: 5400.0,
            speed: 1.0,
            ..Report::default()
        }
    }

    /// Play from `from` for `secs`, a tick a second; the positions grabbed.
    fn run(f: &mut Frames, title: Option<What>, from: f64, t0: f64, secs: u32) -> Vec<f64> {
        let mut grabbed = Vec::new();
        for i in 0..=secs {
            let (t, pos) = (t0 + f64::from(i), from + f64::from(i));
            if f.observe(title, &playing(pos), t) {
                f.keep(title, pos, format!("img{pos}"), pos);
                grabbed.push(pos);
            }
        }
        grabbed
    }

    #[test]
    fn h24_nothing_is_grabbed_until_playback_is_steady() {
        let mut f = Frames::default();
        assert!(!f.observe(FILM, &playing(0.0), 0.0));
        assert!(!f.observe(FILM, &playing(2.0), 2.0));
        assert!(f.observe(FILM, &playing(3.0), 3.0));
        let mut paused = playing(4.0);
        paused.status = Status::Paused;
        assert!(!f.observe(FILM, &paused, 4.0));
        let mut dark = playing(4.0);
        dark.has_picture = false;
        assert!(!f.observe(FILM, &dark, 5.0));
    }

    #[test]
    fn h24_a_seek_restarts_the_wait() {
        let mut f = Frames::default();
        run(&mut f, FILM, 0.0, 0.0, 10);
        assert!(!f.observe(FILM, &playing(2000.0), 11.0), "grabbed straight after a seek");
        assert!(!f.observe(FILM, &playing(2001.0), 12.0));
        assert!(f.observe(FILM, &playing(2003.0), 14.0));
    }

    #[test]
    fn h24_frames_are_spaced_by_the_length_of_the_title() {
        let mut f = Frames::default();
        let grabbed = run(&mut f, FILM, 0.0, 0.0, 300);
        let gap = 5400.0 / KEEP as f64;
        for w in grabbed.windows(2) {
            assert!(w[1] - w[0] >= gap / 2.0, "{grabbed:?}");
        }
        assert!(grabbed.len() >= 4, "{grabbed:?}");
    }

    #[test]
    fn h24_never_offered_for_another_title() {
        let mut f = Frames::default();
        run(&mut f, FILM, 0.0, 0.0, 30);
        assert!(f.nearest(FILM, 10.0).is_some());
        assert!(f.nearest(OTHER, 10.0).is_none());
        f.observe(OTHER, &playing(0.0), 40.0);
        assert!(f.nearest(FILM, 10.0).is_none(), "the old title's frames survived");
        // A grab that finishes after the title changed is thrown away.
        f.keep(FILM, 12.0, "late".into(), 12.0);
        assert!(f.is_empty());
    }

    #[test]
    fn h24_kept_near_their_moment_and_the_furthest_dropped_first() {
        let mut f = Frames::default();
        f.observe(FILM, &playing(0.0), 0.0);
        for i in 0..(KEEP + 10) {
            f.keep(FILM, i as f64 * 60.0, format!("{i}"), 0.0);
        }
        assert_eq!(f.len(), KEEP);
        assert!(f.nearest(FILM, 0.0).is_some());
        assert!(f.nearest(FILM, (KEEP + 9) as f64 * 60.0).is_none());
        // Nothing within reach of a moment: no picture rather than a wrong one.
        assert!(f.nearest(FILM, 30.0 + 60.0 * 3.0).is_none());
    }

    #[test]
    fn h25_the_label_is_the_marker_or_the_chapter() {
        let chapters = vec![
            Chapter { title: Some("Cold open".into()), start_secs: 0.0 },
            Chapter { title: Some("Act One".into()), start_secs: 300.0 },
        ];
        let marks = vec![SkipMarker::new(MarkerKind::Intro, 60.0, 120.0, MarkerSource::Chapters)];
        assert_eq!(label(90.0, &chapters, &marks).as_deref(), Some("Intro"));
        assert_eq!(label(30.0, &chapters, &marks).as_deref(), Some("Cold open"));
        assert_eq!(label(400.0, &chapters, &marks).as_deref(), Some("Act One"));
        assert_eq!(label(10.0, &[], &[]), None);
    }

    #[test]
    fn h25_live_tv_says_what_time_it_was() {
        let mut r = playing(1000.0);
        r.is_live = true;
        r.timeshift = Some(Window { start_secs: 100.0, position_secs: 1000.0, live_secs: 1000.0 });
        assert_eq!(clock_at(&r, 700.0, 1_760_000_000), Some(1_760_000_000 - 300));
        r.timeshift = None;
        assert_eq!(clock_at(&r, 700.0, 1_760_000_000), None);
    }

    #[test]
    fn a_frame_becomes_a_png_data_url() {
        let frame = Frame { width: 2, height: 1, rgb: vec![255, 0, 0, 0, 0, 255] };
        let url = data_url(&frame).unwrap();
        assert!(url.starts_with("data:image/png;base64,iVBOR"), "{url}");
    }
}
