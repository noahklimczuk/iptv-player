//! The engine: one thread that owns mpv, and the handle everything else holds.
//!
//! Rules this file exists to keep (docs/PLAYER.md):
//!
//! - **E1** mpv is touched by the engine thread only. Callers queue a [`Msg`] and wake
//!   it; commands are applied in the order they were sent.
//! - **E2** Every load is a new playlist entry, and mpv tags its file events with the
//!   entry. Anything about another entry — the file this one replaced finishing, a
//!   property of it changing late — is dropped instead of being reported as the new one.
//! - **E3** Only `load` waits, and only for mpv to accept the file.
//! - A caller's change is reflected in what it gets back at once (the volume it set is
//!   the volume it reads), and the engine does not publish an older picture over it
//!   while the change is still queued.

use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use aurora_core::markers::Chapter;
use aurora_core::neterr::NetFailure;
use aurora_core::timeshift::{Budget, Reading};

use crate::driver::{Driver, EndReason, Event, Node, Waker};
use crate::frame::Frame;
use crate::model::{Aspect, Report, Stats, Status, Track, TrackKind};
use crate::request::{Request, Setting};
use crate::stall::{Limits, Phase, Watch};

/// How often position, the rewind window and the stall watch are looked at.
pub const TICK: Duration = Duration::from_millis(250);
/// How often the stats are read; they are several property reads, and change slowly.
const STATS_EVERY: Duration = Duration::from_secs(1);
/// How long `load` waits for mpv to accept a file before calling the engine gone.
const LOAD_ACK: Duration = Duration::from_secs(15);
/// Events handled before the queue is looked at again, so a flood of property changes
/// cannot hold a command back.
const EVENT_BATCH: usize = 256;
/// Lines of mpv's log kept to explain a failure.
const LOG_LINES: usize = 24;
/// "Back to live" lands this far behind the newest data, not on it: asking mpv for the
/// very last byte it has is how a live picture freezes waiting for the next one.
pub const LIVE_MARGIN_SECS: f64 = 1.0;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VideoError {
    #[error("there is nothing to play: the address is empty")]
    EmptyUrl,
    #[error("the video engine refused it: {0}")]
    Refused(String),
    #[error("the video engine has stopped")]
    Gone,
}

/// Where to move the playhead.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seek {
    /// To a position: seconds into a film, or a moment in a live stream's timebase.
    To(f64),
    /// By an offset from where it is.
    By(f64),
    /// To the live edge of a buffered channel.
    Live,
}

/// Where a seek would land, given what is playing, or `None` when it cannot move.
///
/// E19: a film is held to its length. E20: a live stream with no buffer cannot move,
/// and one with a buffer is held inside it.
pub fn seek_target(r: &Report, seek: Seek) -> Option<f64> {
    if !matches!(
        r.status,
        Status::Buffering | Status::Playing | Status::Paused
    ) {
        return None;
    }
    let from = r.position_secs;
    let t = if r.is_live {
        let w = r.timeshift?;
        match seek {
            Seek::To(t) => w.clamp(t),
            Seek::By(d) => w.clamp(from + d),
            Seek::Live => (w.live_secs - LIVE_MARGIN_SECS).max(w.start_secs),
        }
    } else {
        let wanted = match seek {
            Seek::To(t) => t,
            Seek::By(d) => from + d,
            Seek::Live => return None,
        };
        let end = if r.duration_secs > 0.0 {
            r.duration_secs
        } else {
            f64::INFINITY
        };
        wanted.max(0.0).min(end)
    };
    t.is_finite().then_some(t)
}

pub type Listener = Box<dyn Fn(&Report) + Send + Sync>;

enum Msg {
    Load(Request, mpsc::Sender<Result<(), VideoError>>),
    Stop,
    Pause(bool),
    Seek(Seek),
    Volume(u32),
    Mute(bool),
    Speed(f64),
    Audio(Option<i64>),
    Subtitle(Option<i64>),
    Aspect(Aspect),
    Grab(u32, mpsc::Sender<Option<Frame>>),
    Quit,
}

#[derive(Default)]
struct Published {
    report: Report,
    chapters: Vec<Chapter>,
    /// Changes sent and not yet applied. While any are, the engine does not publish:
    /// what it has is older than what the caller already reads.
    queued: usize,
}

struct Shared {
    state: Mutex<Published>,
    listener: Mutex<Option<Listener>>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Published> {
        // A panic while holding this lock is a bug elsewhere; the data is plain values
        // and still the best there is, so carry on with it rather than poisoning every
        // later call.
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// The handle to a running engine. Cheap to call from any thread.
pub struct Engine {
    shared: Arc<Shared>,
    tx: mpsc::Sender<Msg>,
    waker: Waker,
    thread: Mutex<Option<JoinHandle<()>>>,
    renders_video: bool,
    name: String,
    version: Option<String>,
}

impl Engine {
    /// Start an engine on `driver`, on a thread of its own.
    pub fn start<D: Driver>(driver: D) -> Self {
        Self::start_with(driver, Limits::default())
    }

    /// As [`Engine::start`], with the stall limits given: tests use short ones.
    pub fn start_with<D: Driver>(mut driver: D, limits: Limits) -> Self {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            state: Mutex::new(Published::default()),
            listener: Mutex::new(None),
        });
        let waker = driver.waker();
        let renders_video = driver.renders_video();
        let (name, version) = driver.describe();
        for p in OBSERVED {
            if let Err(e) = driver.observe(p) {
                tracing::warn!(property = p, "mpv will not report this property: {e}");
            }
        }
        let thread_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("aurora-video".into())
            .spawn(move || Run::new(driver, thread_shared, rx, limits).run())
            .ok();
        if thread.is_none() {
            tracing::error!("the video engine's thread could not be started");
        }
        Self {
            shared,
            tx,
            waker,
            thread: Mutex::new(thread),
            renders_video,
            name,
            version,
        }
    }

    /// Queue a change, reflecting it in the published report at once.
    fn intend(&self, change: impl FnOnce(&mut Report), msg: Msg) -> Result<Report, VideoError> {
        let mut g = self.shared.lock();
        let before = g.report.clone();
        change(&mut g.report);
        g.queued += 1;
        let out = g.report.clone();
        drop(g);
        if self.tx.send(msg).is_err() {
            let mut g = self.shared.lock();
            g.queued -= 1;
            g.report = before;
            return Err(VideoError::Gone);
        }
        (self.waker)();
        Ok(out)
    }

    /// Play `request`, replacing whatever is playing. Returns once mpv has accepted it;
    /// whether it then plays is reported in the state.
    pub fn load(&self, request: Request) -> Result<Report, VideoError> {
        if request.url.trim().is_empty() {
            return Err(VideoError::EmptyUrl);
        }
        let live = request.live;
        let (reply, answer) = mpsc::channel();
        self.intend(
            |r| {
                *r = r.carried_over();
                r.status = Status::Loading;
                r.is_live = live;
            },
            Msg::Load(request, reply),
        )?;
        answer
            .recv_timeout(LOAD_ACK)
            .map_err(|_| VideoError::Gone)??;
        Ok(self.report())
    }

    pub fn stop(&self) -> Result<Report, VideoError> {
        self.intend(|r| *r = r.carried_over(), Msg::Stop)
    }

    pub fn set_paused(&self, paused: bool) -> Result<Report, VideoError> {
        self.intend(
            |r| match (r.status, paused) {
                // Loading too: pressing pause while it opens is honoured (mpv opens it
                // paused), and saying so is what lets the next press resume it.
                (Status::Loading | Status::Playing | Status::Buffering, true) => {
                    r.status = Status::Paused
                }
                (Status::Paused, false) => r.status = Status::Playing,
                _ => {}
            },
            Msg::Pause(paused),
        )
    }

    pub fn seek(&self, seek: Seek) -> Result<Report, VideoError> {
        self.intend(
            |r| {
                if let Some(t) = seek_target(r, seek) {
                    r.position_secs = t;
                    if let Some(w) = r.timeshift.as_mut() {
                        w.position_secs = t;
                    }
                }
            },
            Msg::Seek(seek),
        )
    }

    pub fn set_volume(&self, volume: u32) -> Result<Report, VideoError> {
        let v = volume.min(200);
        self.intend(|r| r.volume = v, Msg::Volume(v))
    }

    pub fn set_muted(&self, muted: bool) -> Result<Report, VideoError> {
        self.intend(|r| r.muted = muted, Msg::Mute(muted))
    }

    pub fn set_speed(&self, speed: f64) -> Result<Report, VideoError> {
        let s = if speed.is_finite() {
            speed.clamp(0.25, 4.0)
        } else {
            1.0
        };
        self.intend(|r| r.speed = s, Msg::Speed(s))
    }

    /// `None` turns audio off.
    pub fn set_audio_track(&self, id: Option<i64>) -> Result<Report, VideoError> {
        self.intend(|r| r.active_audio_track = id, Msg::Audio(id))
    }

    /// `None` turns subtitles off.
    pub fn set_subtitle_track(&self, id: Option<i64>) -> Result<Report, VideoError> {
        self.intend(|r| r.active_subtitle_track = id, Msg::Subtitle(id))
    }

    pub fn set_aspect(&self, aspect: Aspect) -> Result<Report, VideoError> {
        self.intend(|r| r.aspect = aspect, Msg::Aspect(aspect))
    }

    pub fn report(&self) -> Report {
        self.shared.lock().report.clone()
    }

    /// The chapters of what is loaded (E11). Empty after the next load.
    pub fn chapters(&self) -> Vec<Chapter> {
        self.shared.lock().chapters.clone()
    }

    /// A still of the picture now, at most `max_width` wide. `None` when there is no
    /// picture, or mpv could not take one in time.
    pub fn grab_frame(&self, max_width: u32) -> Option<Frame> {
        let (reply, answer) = mpsc::channel();
        self.tx.send(Msg::Grab(max_width, reply)).ok()?;
        (self.waker)();
        answer.recv_timeout(Duration::from_secs(3)).ok().flatten()
    }

    /// Called on the engine thread whenever the report changes. Keep it quick.
    pub fn on_change(&self, listener: Listener) {
        *self
            .shared
            .listener
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(listener);
    }

    pub fn renders_video(&self) -> bool {
        self.renders_video
    }

    /// The engine's name and version, for diagnostics.
    pub fn describe(&self) -> (String, Option<String>) {
        (self.name.clone(), self.version.clone())
    }

    /// Stop mpv and wait for the thread. Harmless to call twice (E4).
    pub fn shutdown(&self) {
        let handle = self.thread.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(handle) = handle {
            let _ = self.tx.send(Msg::Quit);
            (self.waker)();
            if handle.join().is_err() {
                tracing::error!("the video engine's thread panicked");
            }
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// What the engine asks mpv to report as it changes.
const OBSERVED: &[&str] = &[
    "pause",
    "volume",
    "mute",
    "speed",
    "time-pos",
    "duration",
    "paused-for-cache",
    "seeking",
    "eof-reached",
    "track-list",
    "aid",
    "sid",
    "chapter-list",
];

/// Properties that describe one file. Their changes are only believed once that file
/// has started (E2): until then they are the last file's.
const PER_FILE: &[&str] = &[
    "time-pos",
    "duration",
    "paused-for-cache",
    "seeking",
    "eof-reached",
    "track-list",
    "aid",
    "sid",
    "chapter-list",
];

/// Which file mpv is on, as far as this engine is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Current {
    Idle,
    /// Asked for; mpv has not said it started. `Some` when mpv told us the entry id.
    Waiting(Option<i64>),
    Started(i64),
}

/// The engine thread's own state.
struct Run<D: Driver> {
    d: D,
    shared: Arc<Shared>,
    rx: mpsc::Receiver<Msg>,
    clock: Instant,
    r: Report,
    chapters: Vec<Chapter>,
    current: Current,
    // About the current file.
    loaded: bool,
    restarted: bool,
    has_video: bool,
    paused: bool,
    cache_wait: bool,
    seeking: bool,
    eof: bool,
    position: Option<f64>,
    tuned_at: Option<f64>,
    budget: Option<Budget>,
    log: VecDeque<String>,
    watch: Watch,
    last_stats: Option<Instant>,
    dirty: bool,
    /// What the listener was last told. Compared separately from what is published,
    /// because a caller's own change is published the moment it is made, and the
    /// listener still has to hear about it.
    heard: Option<Report>,
}

impl<D: Driver> Run<D> {
    fn new(d: D, shared: Arc<Shared>, rx: mpsc::Receiver<Msg>, limits: Limits) -> Self {
        Self {
            d,
            shared,
            rx,
            clock: Instant::now(),
            r: Report::default(),
            chapters: Vec::new(),
            current: Current::Idle,
            loaded: false,
            restarted: false,
            has_video: false,
            paused: false,
            cache_wait: false,
            seeking: false,
            eof: false,
            position: None,
            tuned_at: None,
            budget: None,
            log: VecDeque::new(),
            watch: Watch::with_limits(limits),
            last_stats: None,
            dirty: false,
            heard: None,
        }
    }

    fn now(&self) -> f64 {
        self.clock.elapsed().as_secs_f64()
    }

    fn run(mut self) {
        let mut next_tick = Instant::now() + TICK;
        'outer: loop {
            while let Ok(msg) = self.rx.try_recv() {
                if !self.apply(msg) {
                    break 'outer;
                }
            }
            let wait = next_tick.saturating_duration_since(Instant::now());
            if let Some(ev) = self.d.wait(wait) {
                if !self.handle(ev) {
                    break;
                }
                for _ in 0..EVENT_BATCH {
                    match self.d.wait(Duration::ZERO) {
                        Some(ev) => {
                            if !self.handle(ev) {
                                break 'outer;
                            }
                        }
                        None => break,
                    }
                }
            }
            if Instant::now() >= next_tick {
                self.tick();
                next_tick = Instant::now() + TICK;
            }
            if self.dirty {
                self.publish();
            }
        }
        // Whatever is still queued will never be applied; say so to anyone waiting.
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Load(_, reply) => {
                    let _ = reply.send(Err(VideoError::Gone));
                }
                Msg::Grab(_, reply) => {
                    let _ = reply.send(None);
                }
                _ => {}
            }
        }
        let mut g = self.shared.lock();
        g.queued = 0;
        g.report = g.report.carried_over();
    }

    /// Apply one queued change. `false` means quit.
    fn apply(&mut self, msg: Msg) -> bool {
        // `intend` counted everything but a grab and the quit.
        let counted = !matches!(msg, Msg::Grab(..) | Msg::Quit);
        let keep_going = match msg {
            Msg::Quit => false,
            Msg::Load(req, reply) => {
                let _ = reply.send(self.load(req));
                true
            }
            Msg::Stop => {
                self.stop();
                true
            }
            Msg::Pause(p) => {
                self.set("pause", if p { "yes" } else { "no" });
                self.paused = p;
                true
            }
            Msg::Seek(seek) => {
                self.seek(seek);
                true
            }
            Msg::Volume(v) => {
                self.set("volume", &v.to_string());
                self.r.volume = v;
                true
            }
            Msg::Mute(m) => {
                self.set("mute", if m { "yes" } else { "no" });
                self.r.muted = m;
                true
            }
            Msg::Speed(s) => {
                self.set("speed", &format!("{s}"));
                self.r.speed = s;
                true
            }
            Msg::Audio(id) => {
                self.set("aid", &id.map_or("no".into(), |i| i.to_string()));
                self.r.active_audio_track = id;
                true
            }
            Msg::Subtitle(id) => {
                self.set("sid", &id.map_or("no".into(), |i| i.to_string()));
                self.r.active_subtitle_track = id;
                true
            }
            Msg::Aspect(a) => {
                for (name, value) in a.mpv() {
                    self.set(name, value);
                }
                self.r.aspect = a;
                true
            }
            Msg::Grab(max_width, reply) => {
                let _ = reply.send(self.grab(max_width));
                true
            }
        };
        if counted {
            let mut g = self.shared.lock();
            g.queued = g.queued.saturating_sub(1);
            drop(g);
            self.dirty = true;
        }
        keep_going
    }

    fn set(&mut self, name: &str, value: &str) {
        if let Err(e) = self.d.set(name, value) {
            tracing::warn!(property = name, value, "mpv refused a setting: {e}");
        }
    }

    /// Forget everything about the file that was playing.
    fn forget_file(&mut self) {
        self.loaded = false;
        self.restarted = false;
        self.has_video = false;
        self.cache_wait = false;
        self.seeking = false;
        self.eof = false;
        self.position = None;
        self.tuned_at = None;
        self.chapters.clear();
        self.shared.lock().chapters.clear();
        self.last_stats = None;
        self.r = self.r.carried_over();
    }

    fn load(&mut self, req: Request) -> Result<(), VideoError> {
        for setting in req.settings() {
            let outcome = match &setting {
                Setting::Text(name, value) => self.d.set(name, value),
                Setting::List(name, items) => self.d.set_list(name, items),
            };
            if let Err(e) = outcome {
                // `start=none` is how a file is told to start at the beginning; an mpv
                // that does not take the word takes a zero.
                if setting.name() == "start" {
                    self.set("start", "0");
                } else {
                    tracing::warn!(property = setting.name(), "mpv refused a setting: {e}");
                }
            }
        }
        self.set("pause", "no");
        self.paused = false;
        self.forget_file();
        self.r.is_live = req.live;
        self.r.status = Status::Loading;
        self.budget = req.rewind().map(|t| t.budget);
        let now = self.now();
        self.watch.reset(now);
        self.dirty = true;
        match self.d.command(&["loadfile", &req.url, "replace"]) {
            Ok(node) => {
                let entry = node.get("playlist_entry_id").and_then(Node::as_i64);
                self.current = Current::Waiting(entry);
                Ok(())
            }
            Err(e) => {
                self.fail(&format!("could not open the stream: {e}"));
                Err(VideoError::Refused(e))
            }
        }
    }

    fn stop(&mut self) {
        if self.current != Current::Idle {
            if let Err(e) = self.d.command(&["stop"]) {
                tracing::warn!("mpv would not stop: {e}");
            }
        }
        self.current = Current::Idle;
        self.budget = None;
        self.forget_file();
        self.dirty = true;
    }

    fn seek(&mut self, seek: Seek) {
        // From the freshest numbers there are: the published report may be a tick old.
        self.r.position_secs = self.position.unwrap_or(self.r.position_secs);
        self.r.status = self.status();
        self.refresh_window();
        let Some(t) = seek_target(&self.r, seek) else {
            return;
        };
        let target = format!("{t:.3}");
        if let Err(e) = self.d.command(&["seek", &target, "absolute"]) {
            tracing::warn!("mpv would not seek: {e}");
            return;
        }
        if seek == Seek::Live && self.paused {
            self.set("pause", "no");
            self.paused = false;
        }
        self.position = Some(t);
        self.r.position_secs = t;
        if let Some(w) = self.r.timeshift.as_mut() {
            w.position_secs = t;
        }
        self.eof = false;
        let now = self.now();
        self.watch.seeked(now, Some(t));
        self.dirty = true;
    }

    fn grab(&mut self, max_width: u32) -> Option<Frame> {
        if !self.r.has_picture {
            return None;
        }
        let node = self.d.command(&["screenshot-raw", "video"]).ok()?;
        let w = node.get("w")?.as_i64()?;
        let h = node.get("h")?.as_i64()?;
        let stride = node.get("stride")?.as_i64()?;
        if node.get("format").and_then(Node::as_str) != Some("bgr0") {
            return None;
        }
        let Node::Bytes(data) = node.get("data")? else {
            return None;
        };
        Frame::from_bgr0(
            u32::try_from(w).ok()?,
            u32::try_from(h).ok()?,
            usize::try_from(stride).ok()?,
            data,
            max_width,
        )
    }

    /// It cannot go on. Say why, and let go of the stream: on a line that allows one
    /// connection, a stuck stream left open is the reason the next one is refused.
    fn fail(&mut self, what: &str) {
        let recent: Vec<&str> = self.log.iter().map(String::as_str).collect();
        let text = if recent.is_empty() {
            what.to_string()
        } else {
            format!("{what} ({})", recent.join("; "))
        };
        let failure = NetFailure::classify(&text);
        tracing::warn!(code = ?failure.code, "playback failed: {}", without_addresses(&text));
        if self.current != Current::Idle {
            let _ = self.d.command(&["stop"]);
        }
        self.current = Current::Idle;
        self.forget_file();
        self.r.error = Some(failure);
        self.r.status = Status::Error;
        self.dirty = true;
    }

    /// Handle one event. `false` means mpv has gone.
    fn handle(&mut self, ev: Event) -> bool {
        match ev {
            Event::StartFile { entry } => {
                let ours = match self.current {
                    Current::Waiting(None) => true,
                    Current::Waiting(Some(id)) => id == entry,
                    _ => false,
                };
                if ours {
                    self.current = Current::Started(entry);
                    self.log.clear();
                    self.dirty = true;
                }
            }
            Event::FileLoaded => {
                if matches!(self.current, Current::Started(_)) {
                    self.loaded = true;
                    self.read_tracks();
                    self.read_chapters();
                    self.dirty = true;
                }
            }
            Event::PlaybackRestart => {
                if matches!(self.current, Current::Started(_)) && self.loaded {
                    self.restarted = true;
                    self.seeking = false;
                    if self.has_video {
                        self.r.has_picture = true;
                    }
                    self.dirty = true;
                }
            }
            Event::EndFile {
                entry,
                reason,
                error,
                next,
            } => {
                let ours = match self.current {
                    Current::Started(id) => id == entry,
                    Current::Waiting(Some(id)) => id == entry,
                    _ => false,
                };
                if ours {
                    self.ended(reason, error, next);
                }
            }
            Event::Property { name, value } => self.property(&name, value),
            Event::Log { prefix, text } => {
                // Into the app's log as well: a video output that fails while the sound
                // carries on is not a playback failure, so the ring below is never shown
                // for it, and the log is the only place it can be seen.
                tracing::warn!(target: "mpv", "{prefix}: {}", text.trim());
                if self.log.len() == LOG_LINES {
                    self.log.pop_front();
                }
                self.log.push_back(format!("{prefix}: {}", text.trim()));
            }
            Event::Shutdown => {
                self.fail("the video engine stopped");
                return false;
            }
        }
        true
    }

    fn ended(&mut self, reason: EndReason, error: Option<String>, next: Option<i64>) {
        match reason {
            EndReason::Redirect => {
                // A playlist file: mpv is opening what it pointed at. Still loading.
                self.current = Current::Waiting(next);
            }
            EndReason::Error => {
                let what = error.unwrap_or_else(|| "loading failed".into());
                self.fail(&what);
            }
            EndReason::Eof if self.r.is_live => {
                self.fail("the channel's stream ended: end of file");
            }
            EndReason::Eof => {
                // A film that played out with no keep-open: done, at its end.
                let end = self.r.duration_secs;
                self.current = Current::Idle;
                self.forget_file();
                self.r.position_secs = end;
                self.dirty = true;
            }
            EndReason::Stop | EndReason::Quit => {
                self.current = Current::Idle;
                self.forget_file();
                self.dirty = true;
            }
        }
    }

    fn property(&mut self, name: &str, value: Node) {
        if PER_FILE.contains(&name) && !matches!(self.current, Current::Started(_)) {
            return;
        }
        match name {
            "pause" => self.paused = value.as_bool().unwrap_or(self.paused),
            "volume" => {
                if let Some(v) = value.as_f64() {
                    self.r.volume = v.round().clamp(0.0, 200.0) as u32;
                }
            }
            "mute" => self.r.muted = value.as_bool().unwrap_or(self.r.muted),
            "speed" => {
                if let Some(s) = value.as_f64() {
                    self.r.speed = s;
                }
            }
            "time-pos" => {
                self.position = value.as_f64();
                if self.r.is_live && self.tuned_at.is_none() {
                    self.tuned_at = self.position;
                }
                // Published on the tick: it changes every frame.
                return;
            }
            "duration" => {
                self.r.duration_secs = if self.r.is_live {
                    0.0
                } else {
                    value.as_f64().filter(|d| *d > 0.0).unwrap_or(0.0)
                }
            }
            "paused-for-cache" => self.cache_wait = value.as_bool().unwrap_or(false),
            "seeking" => self.seeking = value.as_bool().unwrap_or(false),
            "eof-reached" => {
                self.eof = value.as_bool().unwrap_or(false);
                if self.eof && self.r.is_live {
                    self.fail("the channel's stream ended: end of file");
                    return;
                }
            }
            "track-list" => self.apply_tracks(&value),
            "aid" => self.r.active_audio_track = track_id(&value),
            "sid" => self.r.active_subtitle_track = track_id(&value),
            "chapter-list" => self.apply_chapters(&value),
            _ => return,
        }
        self.dirty = true;
    }

    fn read_tracks(&mut self) {
        if let Some(list) = self.d.get("track-list") {
            self.apply_tracks(&list);
        }
    }

    fn apply_tracks(&mut self, list: &Node) {
        let mut audio = Vec::new();
        let mut subs = Vec::new();
        let mut video = false;
        for t in list.as_array() {
            let Some(id) = t.get("id").and_then(Node::as_i64) else {
                continue;
            };
            let selected = t.get("selected").and_then(Node::as_bool).unwrap_or(false);
            let kind = match t.get("type").and_then(Node::as_str) {
                Some("audio") => TrackKind::Audio,
                Some("sub") => TrackKind::Subtitle,
                Some("video") => {
                    video |= selected;
                    continue;
                }
                _ => continue,
            };
            let track = Track {
                id,
                kind,
                title: t.get("title").and_then(Node::text),
                language: t.get("lang").and_then(Node::text),
                codec: t.get("codec").and_then(Node::text),
                channels: t
                    .get("demux-channels")
                    .or_else(|| t.get("audio-channels"))
                    .and_then(Node::text),
                default: t.get("default").and_then(Node::as_bool).unwrap_or(false),
            };
            match kind {
                TrackKind::Audio => {
                    if selected {
                        self.r.active_audio_track = Some(id);
                    }
                    audio.push(track);
                }
                TrackKind::Subtitle => {
                    if selected {
                        self.r.active_subtitle_track = Some(id);
                    }
                    subs.push(track);
                }
            }
        }
        if !subs
            .iter()
            .any(|t| Some(t.id) == self.r.active_subtitle_track)
        {
            self.r.active_subtitle_track = None;
        }
        if !audio
            .iter()
            .any(|t| Some(t.id) == self.r.active_audio_track)
        {
            self.r.active_audio_track = None;
        }
        self.r.audio_tracks = audio;
        self.r.subtitle_tracks = subs;
        self.has_video = video;
        // A picture is up once playback has restarted with a video track selected,
        // whichever of the two mpv reported first.
        if video && self.restarted {
            self.r.has_picture = true;
        }
        if !video {
            self.r.has_picture = false;
        }
    }

    fn read_chapters(&mut self) {
        if let Some(list) = self.d.get("chapter-list") {
            self.apply_chapters(&list);
        }
    }

    fn apply_chapters(&mut self, list: &Node) {
        self.chapters = list
            .as_array()
            .iter()
            .filter_map(|c| {
                Some(Chapter {
                    title: c.get("title").and_then(Node::text),
                    start_secs: c.get("time").and_then(Node::as_f64)?,
                })
            })
            .collect();
        self.shared.lock().chapters = self.chapters.clone();
    }

    fn status(&self) -> Status {
        if self.r.error.is_some() && self.current == Current::Idle {
            return Status::Error;
        }
        // Paused while it opens says Paused, so the page shows it paused and its next
        // press resumes. The stall watch does not go by this: see `opening`.
        if self.opening() {
            return if self.paused {
                Status::Paused
            } else {
                Status::Loading
            };
        }
        match self.current {
            Current::Idle => Status::Idle,
            Current::Waiting(_) => Status::Loading,
            Current::Started(_) => {
                if self.paused || self.eof {
                    Status::Paused
                } else if self.cache_wait || self.seeking {
                    Status::Buffering
                } else {
                    Status::Playing
                }
            }
        }
    }

    /// The file has been asked for and is not yet playing: no picture to pause on yet.
    fn opening(&self) -> bool {
        match self.current {
            Current::Idle => false,
            Current::Waiting(_) => true,
            Current::Started(_) => !self.loaded || !self.restarted,
        }
    }

    /// Recompute the rewind window from mpv's cache (E21–E24).
    fn refresh_window(&mut self) {
        let (Some(budget), true, Some(position)) = (self.budget, self.r.is_live, self.position)
        else {
            self.r.timeshift = None;
            return;
        };
        let cache = self.d.get("demuxer-cache-state");
        let cache_end = cache
            .as_ref()
            .and_then(|c| c.get("cache-end"))
            .and_then(Node::as_f64);
        let seekable_start = cache
            .as_ref()
            .and_then(|c| c.get("seekable-ranges"))
            .and_then(|r| r.as_array().first().cloned())
            .and_then(|r| r.get("start").and_then(Node::as_f64));
        let bitrate = ["video-bitrate", "audio-bitrate"]
            .iter()
            .filter_map(|p| self.d.get(p).and_then(|n| n.as_f64()))
            .sum::<f64>();
        let reading = Reading {
            position_secs: position,
            live_secs: cache_end.unwrap_or(position),
            tuned_at_secs: self.tuned_at.unwrap_or(position),
            bitrate_bps: bitrate.max(0.0) as u64,
        };
        let mut w = budget.window(&reading);
        if let Some(s) = seekable_start {
            // What mpv still holds is the hard limit, whatever the budget allows.
            w.start_secs = w.start_secs.max(s).min(w.position_secs);
        }
        self.r.timeshift = Some(w);
    }

    fn read_stats(&mut self) -> Stats {
        let get_f = |d: &mut D, p: &str| d.get(p).and_then(|n| n.as_f64());
        let get_s = |d: &mut D, p: &str| d.get(p).and_then(|n| n.text());
        let params = self.d.get("video-params");
        let resolution = params.as_ref().and_then(|p| {
            let w = p.get("w")?.as_i64()?;
            let h = p.get("h")?.as_i64()?;
            Some(format!("{w}x{h}"))
        });
        let cache = self.d.get("demuxer-cache-state");
        let bitrate = ["video-bitrate", "audio-bitrate"]
            .iter()
            .filter_map(|p| get_f(&mut self.d, p))
            .sum::<f64>();
        let drops = ["frame-drop-count", "decoder-frame-drop-count"]
            .iter()
            .filter_map(|p| self.d.get(p).and_then(|n| n.as_i64()))
            .map(|n| n.max(0) as u64)
            .sum();
        Stats {
            resolution,
            video_codec: get_s(&mut self.d, "video-format"),
            audio_codec: get_s(&mut self.d, "audio-codec-name"),
            fps: get_f(&mut self.d, "estimated-vf-fps")
                .or_else(|| get_f(&mut self.d, "container-fps")),
            bitrate_kbps: (bitrate > 0.0).then(|| (bitrate / 1000.0).round() as u64),
            dropped_frames: drops,
            buffer_secs: get_f(&mut self.d, "demuxer-cache-duration")
                .unwrap_or(0.0)
                .max(0.0),
            hw_decoder: get_s(&mut self.d, "hwdec-current").filter(|s| s != "no"),
            cache_bytes: cache
                .as_ref()
                .and_then(|c| c.get("total-bytes"))
                .and_then(Node::as_i64)
                .unwrap_or(0)
                .max(0) as u64,
            cache_file_bytes: cache
                .as_ref()
                .and_then(|c| c.get("file-cache-bytes"))
                .and_then(Node::as_i64)
                .map(|n| n.max(0) as u64),
        }
    }

    fn tick(&mut self) {
        let started = matches!(self.current, Current::Started(_));
        if started {
            if let Some(p) = self.position {
                if (p - self.r.position_secs).abs() > f64::EPSILON {
                    self.r.position_secs = p;
                    self.dirty = true;
                }
            }
            let before = self.r.timeshift;
            self.refresh_window();
            self.dirty |= before != self.r.timeshift;
            let due = self.last_stats.is_none_or(|t| t.elapsed() >= STATS_EVERY);
            if due && self.loaded {
                let stats = Some(self.read_stats());
                self.last_stats = Some(Instant::now());
                if stats != self.r.stats {
                    self.r.stats = stats;
                    self.dirty = true;
                }
            }
        }
        let status = self.status();
        // A stream paused before it has even opened is still being opened, and one that
        // never answers must still be given up (E25). Once the file is open, a pause is
        // a rest like any other (E27).
        let phase = match status {
            _ if self.opening() && !self.loaded => Phase::Opening,
            Status::Loading => Phase::Opening,
            Status::Buffering => Phase::Buffering,
            Status::Playing => Phase::Playing,
            Status::Idle | Status::Paused | Status::Error => Phase::Resting,
        };
        let now = self.now();
        if let Some(stuck) = self.watch.check(now, phase, self.position) {
            self.fail(stuck.describe());
        }
    }

    fn publish(&mut self) {
        self.dirty = false;
        self.r.status = self.status();
        let mut g = self.shared.lock();
        if g.queued > 0 {
            // A caller has changed something this thread has not applied yet; what it
            // reads already says so, and this would put the old value back.
            self.dirty = true;
            return;
        }
        if g.report != self.r {
            g.report = self.r.clone();
        }
        drop(g);
        if self.heard.as_ref() == Some(&self.r) {
            return;
        }
        let report = self.r.clone();
        self.heard = Some(report.clone());
        let listener = self
            .shared
            .listener
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(f) = listener.as_ref() {
            f(&report);
        }
    }
}

/// `aid` / `sid`: a track number, or `no` / `false` for none.
fn track_id(v: &Node) -> Option<i64> {
    match v {
        Node::Int(i) => Some(*i),
        Node::Str(s) => s.parse().ok(),
        _ => None,
    }
}

/// `text` with every address replaced, for the log. A provider's stream address carries
/// the viewer's username and password in its path, and the log is something people
/// attach to bug reports.
pub fn without_addresses(text: &str) -> String {
    text.split(' ')
        .map(|word| {
            if word.contains("://") {
                "<address>"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
