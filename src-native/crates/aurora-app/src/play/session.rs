//! What is playing, and keeping it playing (docs/PLAYER.md H1–H12).
//!
//! Rules this file keeps:
//!
//! - **H4** Every `play` and `stop` takes a new generation. A tune that finds the
//!   generation has moved on by the time it is ready to load was replaced, and neither
//!   loads nor publishes anything. Loads happen under one lock, so a slow tune that has
//!   already started loading finishes before the next one starts and is then replaced.
//! - **H9** A failure is acted on once per load, however many times it is looked at.
//! - **H10** Recovery only acts when no tune holds the lock.
//! - **H12** The page is told "reconnecting" while anything is being tried, and an
//!   error only once the session has given up.
//!
//! Lock order, where two are held: `tuning`, then the database. `inner` is only ever
//! held briefly, never across a call into the engine: the engine's thread calls back
//! into [`Session::engine_changed`], which takes it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use aurora_core::neterr::{ErrorAction, ErrorCode, NetFailure};
use aurora_db::repo::sources;
use aurora_db::rusqlite::Connection;
use aurora_ingest::credentials::CredentialStore;
use aurora_video::{Engine, PlayerState, Report, Request, Seek, Status, Timeshift, VideoError};
use parking_lot::{Condvar, Mutex, MutexGuard};

use super::recovery::{Next, Recovery};
use super::resolve::{self, Plan, What};
use super::timeshift;
use crate::dblock::TimedMutex;
use crate::error::{AppError, Result};

/// How often recovery looks at the engine when nothing wakes it sooner.
pub const TICK: Duration = Duration::from_millis(250);

type Sink = Box<dyn Fn(&PlayerState) + Send + Sync>;

pub struct Session {
    engine: Arc<Engine>,
    db: Arc<TimedMutex<Connection>>,
    credentials: Arc<dyn CredentialStore>,
    data_dir: PathBuf,
    origin: Instant,
    generation: AtomicU64,
    tuning: Mutex<()>,
    inner: Mutex<Inner>,
    sink: Mutex<Option<Sink>>,
    wake: Arc<(Mutex<bool>, Condvar)>,
}

#[derive(Default)]
struct Inner {
    current: Option<Current>,
    /// Set once the session has given up (H11).
    failed: Option<NetFailure>,
    /// Loads sent to the engine, ever.
    loads: u64,
    /// The load whose failure has been acted on (H9).
    handled: u64,
    /// The load recorded as having worked against its source (H5).
    proved: u64,
    /// Where the current file last was while playing: where a film resumes (H8).
    last_position: Option<f64>,
}

struct Current {
    plan: Plan,
    /// The start point the page asked for, before the plan adjusted it: what a repeat
    /// press is compared against.
    asked: Option<f64>,
    recovery: Recovery,
    timeshift: Option<Timeshift>,
}

impl Current {
    fn request(&self, candidate: usize, start_secs: Option<f64>) -> Request {
        let c = &self.plan.candidates[candidate.min(self.plan.candidates.len() - 1)];
        Request {
            url: c.url.clone(),
            headers: Vec::new(),
            user_agent: self.plan.user_agent.clone(),
            referrer: self.plan.referrer.clone(),
            start_secs,
            live: self.plan.live,
            cache_secs: self.plan.buffer_secs,
            timeshift: if self.plan.live {
                self.timeshift.clone()
            } else {
                None
            },
        }
    }

    fn source_id(&self) -> Option<i64> {
        self.plan
            .candidates
            .get(self.recovery.at())
            .and_then(|c| c.source_id)
    }
}

impl Session {
    pub fn new(
        engine: Arc<Engine>,
        db: Arc<TimedMutex<Connection>>,
        credentials: Arc<dyn CredentialStore>,
        data_dir: PathBuf,
    ) -> Arc<Self> {
        let session = Arc::new(Self {
            engine: engine.clone(),
            db,
            credentials,
            data_dir,
            origin: Instant::now(),
            generation: AtomicU64::new(0),
            tuning: Mutex::new(()),
            inner: Mutex::new(Inner::default()),
            sink: Mutex::new(None),
            wake: Arc::new((Mutex::new(false), Condvar::new())),
        });
        let weak: Weak<Self> = Arc::downgrade(&session);
        engine.on_change(Box::new(move |report| {
            if let Some(s) = weak.upgrade() {
                s.engine_changed(report);
            }
        }));
        session
    }

    /// Where every new state goes: the page, in the app.
    pub fn on_state(&self, sink: Sink) {
        *self.sink.lock() = Some(sink);
    }

    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    /// Seconds since the session started: the clock recovery runs on.
    pub fn now(&self) -> f64 {
        self.origin.elapsed().as_secs_f64()
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock()
    }

    /// What is playing: the plan, for the sleep timer, the seek preview and Record.
    pub fn plan(&self) -> Option<Plan> {
        self.inner().current.as_ref().map(|c| c.plan.clone())
    }

    /// The state as the page sees it now.
    pub fn state(&self) -> PlayerState {
        self.compose(self.engine.report())
    }

    fn compose(&self, report: Report) -> PlayerState {
        let i = self.inner();
        let Some(cur) = &i.current else {
            return PlayerState {
                report: Report {
                    error: None,
                    ..report
                },
                ..PlayerState::default()
            };
        };
        let label = &cur.plan.label;
        let mut s = PlayerState {
            title: label.title.clone(),
            subtitle: label.subtitle.clone(),
            channel_id: label.channel_id,
            item_kind: label.kind,
            item_id: label.item_id,
            reconnecting: false,
            report,
        };
        if let Some(f) = &i.failed {
            s.report.status = Status::Error;
            s.report.error = Some(f.clone());
            return s;
        }
        // The engine's error is the session's to act on; until it gives up, the page
        // is told it is being dealt with (H12).
        let engine_failed = s.report.status == Status::Error;
        let in_trouble = cur.recovery.recovering()
            && !matches!(s.report.status, Status::Playing | Status::Paused);
        if engine_failed || in_trouble {
            s.reconnecting = true;
            s.report.error = None;
            if matches!(s.report.status, Status::Error | Status::Idle) {
                s.report.status = Status::Buffering;
            }
        }
        s
    }

    fn publish(&self, state: &PlayerState) {
        if let Some(sink) = self.sink.lock().as_ref() {
            sink(state);
        }
    }

    /// Called on the engine's thread with every new report. Quick, and never calls
    /// back into the engine.
    fn engine_changed(&self, report: &Report) {
        let state = self.compose(report.clone());
        self.publish(&state);
        if report.status == Status::Error || report.status == Status::Playing {
            self.poke();
        }
    }

    /// Wake the recovery loop now rather than at its next tick.
    fn poke(&self) {
        let (flag, cv) = &*self.wake;
        *flag.lock() = true;
        cv.notify_one();
    }

    /// Run recovery on a thread of its own, for as long as the session lives.
    pub fn spawn_loop(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        let wake = self.wake.clone();
        let spawned = std::thread::Builder::new()
            .name("aurora-play".into())
            .spawn(move || loop {
                {
                    let (flag, cv) = &*wake;
                    let mut woken = flag.lock();
                    if !*woken {
                        cv.wait_for(&mut woken, TICK);
                    }
                    *woken = false;
                }
                let Some(session) = weak.upgrade() else {
                    return;
                };
                let now = session.now();
                crate::supervise::supervised("playback", || session.tick(now));
            });
        if let Err(e) = spawned {
            tracing::error!("playback recovery could not start: {e}");
        }
    }

    /// Play `what` (H1–H4). Returns once the engine has the file; whether it then
    /// plays arrives in later states.
    pub fn play(&self, what: What, start_secs: Option<f64>) -> Result<PlayerState> {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        // After taking a generation, not before: the repeat is still the latest request,
        // so a different tune resolving in the meantime has been replaced by it (H4).
        if let Some(state) = self.still_opening(what, start_secs) {
            tracing::info!(?what, "play: already opening this, the repeat is ignored");
            return Ok(state);
        }
        let planned = {
            let db = self.db.lock();
            resolve::plan(
                &db,
                self.credentials.as_ref(),
                what,
                start_secs,
                crate::now_unix(),
            )
        };
        let _tuning = self.tuning.lock();
        if self.generation.load(Ordering::SeqCst) != generation {
            return Err(AppError::Superseded);
        }
        let plan = match planned {
            Ok(plan) => plan,
            Err(e) => {
                // Whatever was on is not what the viewer now wants, and an error over
                // the old picture would read as the old picture failing.
                self.inner().current = None;
                self.inner().failed = None;
                let _ = self.engine.stop();
                self.publish(&self.state());
                return Err(e);
            }
        };
        let timeshift = if plan.live {
            let db = self.db.lock();
            timeshift::for_live(&db, &self.data_dir)?
        } else {
            None
        };
        let start = plan.start_secs;
        {
            let mut i = self.inner();
            i.failed = None;
            i.last_position = None;
            i.current = Some(Current {
                recovery: Recovery::new(plan.live, plan.candidates.len()),
                plan,
                asked: start_secs,
                timeshift,
            });
        }
        self.load(self.now(), 0, start);
        let state = self.state();
        self.publish(&state);
        Ok(state)
    }

    /// The same Play pressed again while the first press is still opening the stream.
    ///
    /// Reloading would throw away the connection that is nearly there and open another,
    /// and a line that allows one connection makes the provider sit on the second until
    /// the first times out: every extra press made the start slower, from ~3 s to 45 s
    /// on the real line. Space and Enter on a still-focused Resume button sent exactly
    /// this. A different title or start point is a new request and still wins (H4), and
    /// once it plays, a deliberate replay reloads as before.
    fn still_opening(&self, what: What, start_secs: Option<f64>) -> Option<PlayerState> {
        let same = {
            let i = self.inner();
            i.failed.is_none()
                && i.current
                    .as_ref()
                    .is_some_and(|c| c.plan.what == what && c.asked == start_secs)
        };
        // `state()` takes `inner` again, and the lock is not reentrant: released above.
        if !same {
            return None;
        }
        let state = self.state();
        // Reconnecting after the stream died is not opening: a press then is the viewer
        // asking for it now rather than after the backoff.
        let opening = matches!(state.report.status, Status::Loading | Status::Buffering)
            && !state.reconnecting;
        opening.then_some(state)
    }

    /// Stop, and forget what was playing (H4: this also cancels a tune in flight).
    pub fn stop(&self) -> Result<PlayerState> {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let _tuning = self.tuning.lock();
        {
            let mut i = self.inner();
            i.current = None;
            i.failed = None;
            i.last_position = None;
        }
        let report = self.engine.stop()?;
        let state = self.compose(report);
        self.publish(&state);
        Ok(state)
    }

    /// Apply a change to the engine and return what the page should now show.
    pub fn apply(
        &self,
        change: impl FnOnce(&Engine) -> std::result::Result<Report, VideoError>,
    ) -> Result<PlayerState> {
        let report = change(&self.engine)?;
        Ok(self.compose(report))
    }

    /// Rejoin the live edge, resuming if paused (README §7.6).
    pub fn back_to_live(&self) -> Result<PlayerState> {
        if self.engine.report().status == Status::Paused {
            self.engine.set_paused(false)?;
        }
        self.apply(|e| e.seek(Seek::Live))
    }

    /// Load a candidate, carrying on through any the engine refuses outright. The
    /// caller holds `tuning`.
    fn load(&self, now: f64, mut candidate: usize, mut start: Option<f64>) {
        loop {
            let request = {
                let mut i = self.inner();
                let Some(cur) = i.current.as_ref() else {
                    return;
                };
                let r = cur.request(candidate, start);
                i.loads += 1;
                r
            };
            let refused = match self.engine.load(request) {
                Ok(_) => return,
                Err(VideoError::Refused(why)) => NetFailure::classify(&why),
                Err(VideoError::EmptyUrl) => {
                    self.give_up(NetFailure::classify("the address is empty"));
                    return;
                }
                Err(VideoError::Gone) => {
                    let mut f = NetFailure::classify("the video engine has stopped");
                    f.message = "Aurora's video engine has stopped. Restart Aurora to play \
                                 again."
                        .into();
                    f.actions = Vec::new();
                    f.retryable = false;
                    self.give_up(f);
                    return;
                }
            };
            let loads = self.inner().loads;
            self.inner().handled = loads;
            match self.failed(now, &refused, None) {
                Some((c, s)) => {
                    candidate = c;
                    start = s;
                }
                None => return,
            }
        }
    }

    /// The candidate in use failed: record it, and ask recovery what next. `Some` is a
    /// load to make now.
    fn failed(
        &self,
        now: f64,
        failure: &NetFailure,
        died_at: Option<f64>,
    ) -> Option<(usize, Option<f64>)> {
        let (next, source) = {
            let mut i = self.inner();
            let cur = i.current.as_mut()?;
            let source = cur.source_id();
            (cur.recovery.failed(now, failure, died_at), source)
        };
        if let Some(id) = source {
            let db = self.db.lock();
            if let Err(e) = sources::record_failure(&db, id, crate::now_unix()) {
                tracing::warn!("could not record a source failure: {e}");
            }
        }
        tracing::info!(code = ?failure.code, ?next, "playback failed");
        match next {
            Next::Load {
                candidate,
                start_secs,
            } => Some((candidate, start_secs)),
            Next::Wait { .. } => None,
            Next::GiveUp => {
                self.give_up(failure.clone());
                None
            }
        }
    }

    /// Nothing more will be tried: report it, with only the actions that work (H11).
    fn give_up(&self, failure: NetFailure) {
        let state = {
            let mut i = self.inner();
            let (live, several) = i.current.as_ref().map_or((false, false), |c| {
                (c.plan.live, c.plan.candidates.len() > 1)
            });
            i.failed = Some(actionable(failure, live, several));
            drop(i);
            self.state()
        };
        self.publish(&state);
    }

    /// Look at the engine and recover if it needs it. `now` is [`Session::now`]'s
    /// clock; tests pass their own.
    pub fn tick(&self, now: f64) {
        let Some(_tuning) = self.tuning.try_lock() else {
            return; // H10: a tune is in flight.
        };
        let report = self.engine.report();
        enum Act {
            Fail(NetFailure, Option<f64>),
            Load(usize, Option<f64>),
        }
        let mut act = None;
        let mut proved = None;
        {
            let mut i = self.inner();
            if i.failed.is_some() || i.current.is_none() {
                return;
            }
            let loads = i.loads;
            match report.status {
                Status::Error if i.handled != loads => {
                    i.handled = loads;
                    let failure = report
                        .error
                        .clone()
                        .unwrap_or_else(|| NetFailure::classify("playback stopped"));
                    act = Some(Act::Fail(failure, i.last_position));
                }
                Status::Playing => {
                    if report.position_secs > 0.0 {
                        i.last_position = Some(report.position_secs);
                    }
                    let first = i.proved != loads;
                    i.proved = loads;
                    let cur = i.current.as_mut().expect("checked above");
                    cur.recovery.playing(now);
                    if first {
                        proved = cur.source_id();
                    }
                }
                Status::Error => {}
                _ => {
                    if let Some(cur) = i.current.as_mut() {
                        cur.recovery.interrupted();
                    }
                }
            }
            if act.is_none() {
                if let Some(Next::Load {
                    candidate,
                    start_secs,
                }) = i.current.as_mut().and_then(|c| c.recovery.due(now))
                {
                    act = Some(Act::Load(candidate, start_secs));
                }
            }
        }
        if let Some(id) = proved {
            let db = self.db.lock();
            if let Err(e) = sources::record_ok(&db, id, crate::now_unix()) {
                tracing::warn!("could not record a working source: {e}");
            }
        }
        match act {
            Some(Act::Fail(failure, died_at)) => {
                if let Some((c, s)) = self.failed(now, &failure, died_at) {
                    self.load(now, c, s);
                }
                self.publish(&self.state());
            }
            Some(Act::Load(c, s)) => {
                self.load(now, c, s);
                self.publish(&self.state());
            }
            None => {}
        }
    }
}

/// Keep only the actions the page can carry out for this failure (H11).
fn actionable(mut f: NetFailure, live: bool, several_sources: bool) -> NetFailure {
    let bad_provider = matches!(f.code, ErrorCode::Unauthorized | ErrorCode::Forbidden);
    f.actions.retain(|a| match a {
        ErrorAction::Retry => true,
        ErrorAction::TryAnotherSource => live && several_sources,
        ErrorAction::OpenSettings => bad_provider,
        // No command stands behind it.
        ErrorAction::ReportBroken => false,
    });
    if bad_provider && !f.actions.contains(&ErrorAction::OpenSettings) {
        f.actions.push(ErrorAction::OpenSettings);
    }
    if f.retryable && !f.actions.contains(&ErrorAction::Retry) {
        f.actions.insert(0, ErrorAction::Retry);
    }
    f
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
