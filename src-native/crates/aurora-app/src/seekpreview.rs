//! The seek preview: what hovering the scrub bar shows (README §7.6).
//!
//! Asked for as "add a seek preview to the player", and settled as *frames and time*:
//! a picture wherever one can be had without a second connection to the provider, and
//! always the time, the chapter or intro/credits marker, and on live TV the programme
//! that was on at that moment.
//!
//! Where the pictures come from:
//!
//! * **What has already played.** The heartbeat grabs a small frame from the player
//!   every `interval` seconds of playback ([`Frames::observe`]), so going back through
//!   a film or the live rewind buffer shows what was there.
//! * **Any moment of a recording**, which is a file on this disk: a silent second
//!   player opens it and decodes the frame asked for ([`Frames::recording_frame`]).
//!
//! A film's unplayed future would need a second stream from the provider, and the
//! lines this app is used with allow one. So there the bubble is time and context only.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use aurora_player::{Frame, FrameSource, PlayerBackend, PlayerState, PlayerStatus};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// How wide the preview pictures are, in pixels. The bubble draws them at 160 CSS px,
/// so this is sharp at 200% scaling without being a full frame.
pub const THUMB_WIDTH: u32 = 320;
/// Frames kept for one item at most. A frame is ~20-40 kB as PNG, so this bounds the
/// store at a few tens of megabytes on a long session.
const MAX_FRAMES: usize = 600;
/// Never grab more often than this, whatever the interval says: a grab copies a frame
/// back from the GPU under the player lock.
const MIN_GAP: Duration = Duration::from_millis(1500);

/// What is playing, as far as the store is concerned: frames from one item are never
/// shown on another.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Key {
    Item { kind: String, id: i64 },
    Channel(i64),
}

fn key_of(state: &PlayerState) -> Option<Key> {
    if state.is_live {
        return state.channel_id.map(Key::Channel);
    }
    let kind = serde_json::to_value(state.item_kind?)
        .ok()?
        .as_str()?
        .to_string();
    Some(Key::Item {
        kind,
        id: state.item_id?,
    })
}

/// Seconds between kept frames: one every ~1/400th of a film, at least every five
/// seconds; every ten on live TV, whose buffer is an hour or two at most.
pub fn interval_for(state: &PlayerState) -> f64 {
    if state.is_live || state.duration_secs <= 0.0 {
        10.0
    } else {
        (state.duration_secs / 400.0).max(5.0)
    }
}

#[derive(Default)]
struct Store {
    key: Option<Key>,
    interval: f64,
    /// Bucket index -> (the second it was taken at, a `data:` URL).
    frames: BTreeMap<i64, (f64, String)>,
    last_grab: Option<Instant>,
}

/// The second player on a recording, kept open while that recording is the one playing.
struct Reader {
    recording_id: i64,
    source: Box<dyn FrameSource>,
}

#[derive(Default)]
pub struct Frames {
    store: Mutex<Store>,
    reader: Mutex<Option<Reader>>,
}

impl Frames {
    /// The heartbeat's half: keep a frame of what is on screen, when this stretch of the
    /// item has none yet.
    pub fn observe(&self, player: &Mutex<Box<dyn PlayerBackend>>) {
        let mut player = player.lock();
        let state = player.state();
        self.release_reader_unless(
            (state.item_kind == Some(aurora_player::state::MediaKind::Recording))
                .then_some(state.item_id)
                .flatten(),
        );
        let Some(key) = key_of(&state) else { return };
        let position = state.position_secs;
        {
            let mut store = self.store.lock();
            if store.key.as_ref() != Some(&key) {
                *store = Store {
                    key: Some(key),
                    interval: interval_for(&state),
                    ..Store::default()
                };
            }
            if state.status != PlayerStatus::Playing || !position.is_finite() || position < 0.0 {
                return;
            }
            let bucket = (position / store.interval).floor() as i64;
            if store.frames.contains_key(&bucket) {
                return;
            }
            if store.last_grab.is_some_and(|t| t.elapsed() < MIN_GAP) {
                return;
            }
            store.last_grab = Some(Instant::now());
        }
        let Some(frame) = player.grab_frame(THUMB_WIDTH) else {
            return;
        };
        drop(player);
        let Some(url) = data_url(&frame) else { return };
        let mut store = self.store.lock();
        let bucket = (position / store.interval).floor() as i64;
        store.frames.insert(bucket, (position, url));
        trim(&mut store.frames, bucket);
    }

    /// A kept frame near `position`, for what is playing now.
    pub fn kept(&self, state: &PlayerState, position: f64) -> Option<(f64, String)> {
        let store = self.store.lock();
        if store.key.is_none() || store.key != key_of(state) {
            return None;
        }
        let bucket = (position / store.interval).floor() as i64;
        // This stretch, else its neighbours, but nothing further: a picture from a
        // minute away would be a wrong answer, not an approximate one.
        [bucket, bucket - 1, bucket + 1]
            .iter()
            .filter_map(|b| store.frames.get(b))
            .min_by(|a, b| (a.0 - position).abs().total_cmp(&(b.0 - position).abs()))
            .filter(|(at, _)| (at - position).abs() <= store.interval * 1.5)
            .cloned()
    }

    /// Any moment of a recording, decoded from its file.
    ///
    /// `Err(Busy)` when another frame is being decoded right now: the bubble asks again
    /// as the pointer settles, and queueing every position it passed over would answer
    /// them long after the pointer had moved on.
    pub fn recording_frame(
        &self,
        state: &PlayerState,
        recording_id: i64,
        path: &std::path::Path,
        position: f64,
    ) -> Result<Option<(f64, String)>, Busy> {
        let interval = interval_for(state);
        // Snapped to the bucket, so hovering back and forth reuses frames.
        let at = ((position / interval).floor() * interval).max(0.0);
        if let Some(hit) = self.kept(state, at) {
            if (hit.0 - at).abs() < 0.5 {
                return Ok(Some(hit));
            }
        }
        let Some(mut reader) = self.reader.try_lock() else {
            return Err(Busy);
        };
        if reader.as_ref().map(|r| r.recording_id) != Some(recording_id) {
            *reader = aurora_player::open_file_frames(path).map(|source| Reader {
                recording_id,
                source,
            });
        }
        let Some(r) = reader.as_mut() else {
            return Ok(None);
        };
        let Some(frame) = r.source.frame_at(at, THUMB_WIDTH) else {
            return Ok(None);
        };
        drop(reader);
        let Some(url) = data_url(&frame) else {
            return Ok(None);
        };
        let mut store = self.store.lock();
        if store.key.is_some() && store.key == key_of(state) {
            let bucket = (at / store.interval).floor() as i64;
            store.frames.insert(bucket, (at, url.clone()));
            trim(&mut store.frames, bucket);
        }
        Ok(Some((at, url)))
    }

    /// Close the recording reader once the recording is no longer what is playing.
    pub fn release_reader_unless(&self, recording_id: Option<i64>) {
        if let Some(mut reader) = self.reader.try_lock() {
            if reader.as_ref().map(|r| r.recording_id) != recording_id {
                *reader = None;
            }
        }
    }
}

#[derive(Debug)]
pub struct Busy;

/// Over the cap, drop the frames furthest from where the viewer is.
fn trim(frames: &mut BTreeMap<i64, (f64, String)>, around: i64) {
    while frames.len() > MAX_FRAMES {
        let first = *frames.keys().next().expect("non-empty");
        let last = *frames.keys().next_back().expect("non-empty");
        let drop = if around - first >= last - around {
            first
        } else {
            last
        };
        frames.remove(&drop);
    }
}

/// A frame as a `data:image/png` URL, ready for an `<img>`.
pub fn data_url(frame: &Frame) -> Option<String> {
    use base64::Engine as _;
    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, frame.width, frame.height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&frame.rgb).ok()?;
    }
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png_bytes)
    ))
}

/// What hovering the scrub bar at `positionSecs` shows. See the module comment.
#[tauri::command(async)]
pub fn player_seek_preview(
    services: tauri::State<'_, crate::services::Services>,
    args: SeekPreviewArgs,
) -> crate::error::Result<SeekPreview> {
    use aurora_db::repo::{channels, dvr, epg, markers};
    use aurora_player::state::MediaKind;

    let state = services.playback.state();
    let position = args.position_secs;
    let mut out = SeekPreview {
        position_secs: position,
        ..SeekPreview::default()
    };
    if state.status == PlayerStatus::Idle || !position.is_finite() {
        return Ok(out);
    }

    let picture = match (state.item_kind, state.item_id) {
        (Some(MediaKind::Recording), Some(id)) => {
            let path = {
                let db = services.read();
                dvr::get(&db, id)?.and_then(|r| r.file_path)
            };
            match path {
                Some(path) => match services.frames.recording_frame(
                    &state,
                    id,
                    std::path::Path::new(&path),
                    position,
                ) {
                    Ok(hit) => hit,
                    Err(Busy) => {
                        out.pending = true;
                        services.frames.kept(&state, position)
                    }
                },
                None => None,
            }
        }
        _ => services.frames.kept(&state, position),
    };
    if let Some((at, url)) = picture {
        out.image_at_secs = Some(at);
        out.image = Some(url);
    }

    let chapters = services.player.lock().chapters();
    let skip = match (state.item_kind, state.item_id) {
        (Some(MediaKind::Episode), Some(id)) => {
            let duration = (state.duration_secs > 0.0).then_some(state.duration_secs);
            let db = services.read();
            markers::resolve(&db, id, markers::DEFAULT_MIN_SAMPLES, duration).unwrap_or_default()
        }
        _ => Vec::new(),
    };
    out.label = label_at(position, &skip, &chapters);

    // Live TV: when that was, and what was on. The buffer's right end is now.
    if state.is_live {
        if let Some(window) = &state.timeshift {
            let behind = (window.live_secs - position).max(0.0);
            let clock = crate::now_unix() - behind.round() as i64;
            out.clock_unix = Some(clock);
            if let Some(channel_id) = state.channel_id {
                let db = services.read();
                if let Some(epg_id) = channels::get(&db, channel_id)?.and_then(|c| c.epg_channel_id)
                {
                    let (now, _) = epg::now_next(&db, &epg_id, clock)?;
                    out.programme = now.map(|p| p.title);
                }
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeekPreviewArgs {
    pub position_secs: f64,
}

#[derive(Debug, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SeekPreview {
    pub position_secs: f64,
    /// A picture of that moment, or of one within a few seconds of it.
    pub image: Option<String>,
    /// The second the picture was actually taken at.
    pub image_at_secs: Option<f64>,
    /// A picture is being made and asking again shortly will find it.
    pub pending: bool,
    /// What that moment is: a chapter's title, or "Intro", "Recap", "Credits".
    pub label: Option<String>,
    /// Live TV: the wall-clock time of that moment, Unix seconds.
    pub clock_unix: Option<i64>,
    /// Live TV: the programme on at that moment.
    pub programme: Option<String>,
}

/// The label for a moment: a skip marker if one covers it, else the chapter it is in.
pub fn label_at(
    position: f64,
    markers: &[aurora_core::markers::SkipMarker],
    chapters: &[aurora_core::markers::Chapter],
) -> Option<String> {
    if let Some(m) = markers
        .iter()
        .find(|m| position >= m.start_secs && position < m.end_secs)
    {
        let word = m.kind.as_str();
        let mut chars = word.chars();
        return chars
            .next()
            .map(|c| c.to_uppercase().collect::<String>() + chars.as_str());
    }
    chapters
        .iter()
        .filter(|c| c.start_secs <= position)
        .max_by(|a, b| a.start_secs.total_cmp(&b.start_secs))
        .and_then(|c| c.title.clone())
        .filter(|t| !t.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::markers::{Chapter, MarkerKind, MarkerSource, SkipMarker};

    fn film(duration: f64) -> PlayerState {
        PlayerState {
            duration_secs: duration,
            item_kind: Some(aurora_player::state::MediaKind::Movie),
            item_id: Some(7),
            ..Default::default()
        }
    }

    #[test]
    fn frames_are_spaced_by_the_length_of_what_is_playing() {
        assert_eq!(
            interval_for(&film(600.0)),
            5.0,
            "never closer than five seconds"
        );
        assert_eq!(interval_for(&film(8000.0)), 20.0);
        let live = PlayerState {
            is_live: true,
            ..Default::default()
        };
        assert_eq!(interval_for(&live), 10.0);
    }

    #[test]
    fn a_kept_frame_is_offered_near_its_moment_and_never_for_another_item() {
        let frames = Frames::default();
        let state = film(1000.0);
        {
            let mut s = frames.store.lock();
            s.key = key_of(&state);
            s.interval = 5.0;
            s.frames.insert(20, (101.0, "data:a".into()));
        }
        assert_eq!(frames.kept(&state, 102.0).unwrap().1, "data:a");
        assert_eq!(
            frames.kept(&state, 106.0).unwrap().1,
            "data:a",
            "the next stretch over"
        );
        assert!(
            frames.kept(&state, 130.0).is_none(),
            "too far away to be honest"
        );
        let other = PlayerState {
            item_id: Some(8),
            ..state
        };
        assert!(frames.kept(&other, 102.0).is_none());
    }

    #[test]
    fn the_store_drops_what_is_furthest_from_the_viewer() {
        let mut map: BTreeMap<i64, (f64, String)> = (0..(MAX_FRAMES as i64 + 3))
            .map(|b| (b, (b as f64, String::new())))
            .collect();
        trim(&mut map, 0);
        assert_eq!(map.len(), MAX_FRAMES);
        assert!(map.contains_key(&0));
        assert!(!map.contains_key(&(MAX_FRAMES as i64 + 2)));
    }

    #[test]
    fn a_frame_becomes_a_png_data_url() {
        let frame = Frame {
            width: 2,
            height: 1,
            rgb: vec![255, 0, 0, 0, 0, 255],
        };
        let url = data_url(&frame).unwrap();
        assert!(url.starts_with("data:image/png;base64,iVBOR"), "{url}");
    }

    #[test]
    fn a_marker_names_its_stretch_before_a_chapter_does() {
        let markers = [SkipMarker {
            kind: MarkerKind::Intro,
            start_secs: 30.0,
            end_secs: 90.0,
            source: MarkerSource::Chapters,
        }];
        let chapters = [
            Chapter {
                title: Some("Cold open".into()),
                start_secs: 0.0,
            },
            Chapter {
                title: Some("Act one".into()),
                start_secs: 90.0,
            },
        ];
        assert_eq!(
            label_at(10.0, &markers, &chapters).as_deref(),
            Some("Cold open")
        );
        assert_eq!(
            label_at(45.0, &markers, &chapters).as_deref(),
            Some("Intro")
        );
        assert_eq!(
            label_at(400.0, &markers, &chapters).as_deref(),
            Some("Act one")
        );
        assert_eq!(label_at(400.0, &[], &[]), None);
    }
}
