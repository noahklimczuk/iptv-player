//! Recordings: the scheduler that turns rows in `recordings` into files, and the
//! commands the Recordings screen, the guide and the player call.
//!
//! Mirrors `aurora-app/src/dvr.rs`. The plan (docs/APPLE_PORT_AND_SYNC.md §2.3) left the
//! DVR on Windows; the port records on the Apple device instead, so it has the same
//! screen with the same behaviour (apple/README.md says where it differs: it records only
//! while Aurora is running, which on iPhone and Apple TV means while it is open).
//!
//! The schedule, the rules, conflicts, quotas and the recorder itself are the shared
//! crates' (`aurora_db::repo::dvr`, `aurora_core::dvr`, `aurora_ingest::recorder`); what
//! is here is the host's part, line for line where it can be: `Dvr::tick` and its four
//! steps, and the `dvr_*` commands. `mirrors.rs` checks the constants and the words.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use aurora_core::dvr::RecordingState as State;
use aurora_core::fsname::recording_filename;
use aurora_db::repo::dvr::{self as repo};
use aurora_db::repo::settings;
use aurora_db::rusqlite::{Connection, OptionalExtension};
use aurora_ingest::credentials::CredentialStore;
use aurora_ingest::recorder::{Handle, RecordRequest, Recorder};
use aurora_video::Request;
use parking_lot::Mutex;

use crate::error::{AuroraError, Result};
use crate::playback::mpv_options;
use crate::vod::{VodKind, VodTune};
use crate::{now_unix, AuroraCore};

/// Default ceiling on simultaneous recordings, used until a provider declares its own.
pub(crate) const DEFAULT_MAX_CONCURRENT: usize = 2;

/// How long a recording may produce nothing before it is called stalled.
pub(crate) const STALL_SECS: i64 = 120;

/// How often the scheduler wakes. Ten seconds is well inside the default one-minute
/// pre-padding, so a recording still starts before its programme does.
pub(crate) const TICK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

/// Settings key for the recordings folder. Absent means "next to the library".
pub(crate) const FOLDER_KEY: &str = "dvr.folder";
/// Settings key for the disk budget, in bytes. Zero means unlimited.
pub(crate) const QUOTA_KEY: &str = "dvr.quotaBytes";

/// How far ahead rule expansion looks. A fortnight covers every provider's guide, and
/// scheduling further out than the guide is guesswork.
pub(crate) const EXPANSION_HORIZON_SECS: i64 = 14 * 86_400;

/// A manual recording outranks anything a rule scheduled: the user asked for this one
/// by name.
pub(crate) const MANUAL_PRIORITY: i32 = 10;

/// How long before an airing a guide reminder is due, unless told otherwise.
pub(crate) const REMINDER_LEAD_SECS: i64 = 120;

/// The reasons a recording did not happen, as the Recordings screen shows them.
pub(crate) const MISSED: &str = "Aurora was not running when this was due";
pub(crate) const TOO_MANY: &str = "too many recordings at once for this subscription";
pub(crate) const NOTHING_SENT: &str = "the provider sent nothing";
/// Why a recording ended early when the system closed Aurora in the middle of it.
/// Apple-only: Windows always finishes its recordings on the way out (`shutdown`),
/// while iOS and tvOS stop an app without asking.
pub(crate) const INTERRUPTED: &str = "Aurora was closed while this was recording";

/// The tightest connection limit across enabled providers, or `None` if no provider has
/// declared one. Mirrors `aurora-app/src/dvr.rs` `line_limit`, which multi-view asks too.
pub(crate) fn line_limit(db: &Connection) -> Option<usize> {
    db.query_row(
        "SELECT MIN(max_connections) FROM providers WHERE enabled = 1 AND max_connections > 0",
        [],
        |r| r.get::<_, Option<i64>>(0),
    )
    .ok()
    .flatten()
    .map(|n| n.max(1) as usize)
}

// ---------------------------------------------------------------------------
// What Swift sees
// ---------------------------------------------------------------------------

/// Where a recording stands. Mirrors `aurora_core::dvr::RecordingState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum RecordingState {
    Scheduled,
    Recording,
    Completed,
    /// Did not record, or recorded incompletely. `reason` says why.
    Failed,
    /// Dropped because a higher-priority recording needed the connection.
    Skipped,
}

impl From<State> for RecordingState {
    fn from(s: State) -> Self {
        match s {
            State::Scheduled => Self::Scheduled,
            State::Recording => Self::Recording,
            State::Completed => Self::Completed,
            State::Failed => Self::Failed,
            State::Skipped => Self::Skipped,
        }
    }
}

impl From<RecordingState> for State {
    fn from(s: RecordingState) -> Self {
        match s {
            RecordingState::Scheduled => Self::Scheduled,
            RecordingState::Recording => Self::Recording,
            RecordingState::Completed => Self::Completed,
            RecordingState::Failed => Self::Failed,
            RecordingState::Skipped => Self::Skipped,
        }
    }
}

/// A scheduled or finished recording, with what only the running DVR knows. Windows'
/// `RecordingView`, flattened.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Recording {
    pub id: i64,
    pub channel_id: i64,
    pub rule_id: Option<i64>,
    pub title: String,
    pub sub_title: Option<String>,
    pub season: Option<u16>,
    pub episode: Option<u16>,
    /// The programme's own times.
    pub air_start: i64,
    pub air_stop: i64,
    /// With padding: when the recorder starts and stops.
    pub start: i64,
    pub stop: i64,
    pub state: RecordingState,
    pub reason: Option<String>,
    pub priority: i32,
    pub file_path: Option<String>,
    pub bytes: i64,
    pub duration_secs: i64,
    pub keep: bool,
    pub watched: bool,
    /// Bytes on disk right now, for a recording still in flight.
    pub live_bytes: Option<u64>,
    pub channel_name: Option<String>,
    pub channel_logo: Option<String>,
}

/// One airing to record. Windows' `ScheduleArgs`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ScheduleRecording {
    pub channel_id: i64,
    pub title: String,
    pub sub_title: Option<String>,
    pub season: Option<u16>,
    pub episode: Option<u16>,
    pub air_start: i64,
    pub air_stop: i64,
    /// `None` is the default padding (`aurora_core::dvr`).
    pub pre_padding_secs: Option<i64>,
    pub post_padding_secs: Option<i64>,
}

/// A period where more recordings overlap than the subscription can carry.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RecordingConflict {
    pub start: i64,
    pub stop: i64,
    /// Everything scheduled across that window, most important first.
    pub slot_ids: Vec<i64>,
    /// How many would have to be dropped.
    pub over_by: u32,
}

/// A series rule as the rules tab shows it. `aurora_db::repo::dvr::RuleView`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SeriesRule {
    pub id: i64,
    pub title: String,
    pub channel_id: Option<i64>,
    pub new_only: bool,
    /// 0 is Monday.
    pub weekdays: Option<Vec<u8>>,
    pub around_local_minute: Option<u16>,
    pub pre_padding_secs: i64,
    pub post_padding_secs: i64,
    pub keep_episodes: Option<u16>,
    pub priority: i32,
    pub enabled: bool,
    /// How many recordings this rule has scheduled or made.
    pub scheduled: i64,
    pub channel_name: Option<String>,
}

/// A new series rule. Windows' `CreateRuleArgs`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct NewSeriesRule {
    pub title: String,
    pub channel_id: Option<i64>,
    pub new_only: bool,
    pub weekdays: Option<Vec<u8>>,
    pub around_local_minute: Option<u16>,
    pub pre_padding_secs: Option<i64>,
    pub post_padding_secs: Option<i64>,
    pub keep_episodes: Option<u16>,
}

/// A "remind me" entry from the guide.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Reminder {
    pub id: i64,
    pub channel_id: i64,
    pub title: String,
    pub start: i64,
    pub lead_secs: i64,
}

/// What the storage meter shows. Windows' `Storage`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DvrStorage {
    pub folder: String,
    pub used_bytes: i64,
    /// Zero means no limit.
    pub quota_bytes: i64,
    /// Recordings that would be deleted to get back under the quota.
    pub prunable: Vec<i64>,
    pub max_concurrent: u32,
}

/// A recording that did not start or did not finish, and why.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FailedRecording {
    pub id: i64,
    pub reason: String,
}

/// What one tick changed. Windows' `TickReport`, which it sends as `dvr.tick`.
#[derive(Debug, Default, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TickReport {
    pub started: Vec<i64>,
    pub completed: Vec<i64>,
    pub failed: Vec<FailedRecording>,
    /// Running, but nothing has arrived for a while.
    pub stalled: Vec<i64>,
}

impl TickReport {
    pub fn is_empty(&self) -> bool {
        self.started.is_empty()
            && self.completed.is_empty()
            && self.failed.is_empty()
            && self.stalled.is_empty()
    }
}

/// Told after every tick that changed something: Windows' `dvr.tick` event, which the
/// Recordings screen, the guide's marks and the stream warning all refresh on.
#[uniffi::export(with_foreign)]
pub trait DvrListener: Send + Sync {
    fn ticked(&self, report: TickReport);
}

// ---------------------------------------------------------------------------
// The scheduler
// ---------------------------------------------------------------------------

struct Active {
    handle: Handle,
    /// When the recorder actually opened the stream, which is not the scheduled start
    /// if Aurora was opened late.
    started_at: i64,
    stop_at: i64,
}

/// Mirrors `aurora-app/src/dvr.rs` `Dvr`.
pub(crate) struct Dvr {
    db: Arc<Mutex<Connection>>,
    recorder: Arc<dyn Recorder>,
    /// Where Swift said recordings go; the library's folder until it says.
    folder: Mutex<PathBuf>,
    active: Arc<Mutex<HashMap<i64, Active>>>,
    /// Recordings whose stream is being opened right now, on a thread of their own:
    /// counted against the limit, and not picked up twice by the next tick.
    starting: Arc<Mutex<HashSet<i64>>>,
    forced_max_concurrent: Option<usize>,
    credentials: Arc<dyn CredentialStore>,
    /// Whether the ten-second clock is running (`dvr_start`).
    ticking: AtomicBool,
}

impl Dvr {
    /// Recordings holding a connection now, starting ones included: what multi-view
    /// counts against the line (Windows' `active_ids().len()`).
    pub(crate) fn active_count(&self) -> usize {
        self.active.lock().len() + self.starting.lock().len()
    }

    pub(crate) fn new(
        db: Arc<Mutex<Connection>>,
        recorder: Arc<dyn Recorder>,
        folder: PathBuf,
        credentials: Arc<dyn CredentialStore>,
    ) -> Self {
        Self {
            db,
            recorder,
            folder: Mutex::new(folder),
            active: Arc::new(Mutex::new(HashMap::new())),
            starting: Arc::new(Mutex::new(HashSet::new())),
            forced_max_concurrent: None,
            credentials,
            ticking: AtomicBool::new(false),
        }
    }

    /// Wait until nothing is still opening a stream, or give up. False if it gave up.
    pub(crate) fn settle(&self, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if self.starting.lock().is_empty() {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[cfg(test)]
    fn with_max_concurrent(mut self, max: usize) -> Self {
        self.forced_max_concurrent = Some(max.max(1));
        self
    }

    pub(crate) fn folder(&self) -> PathBuf {
        self.folder.lock().clone()
    }

    /// How many recordings this subscription allows at once, asked per call because a
    /// refresh can change it at any moment (R-108).
    pub(crate) fn max_concurrent(&self, db: &Connection) -> usize {
        self.forced_max_concurrent
            .unwrap_or_else(|| line_limit(db).unwrap_or(DEFAULT_MAX_CONCURRENT))
    }

    /// Live byte count for a recording in flight, for the progress bar.
    pub(crate) fn bytes_so_far(&self, id: i64) -> Option<u64> {
        self.active
            .lock()
            .get(&id)
            .map(|a| a.handle.progress().bytes())
    }

    /// Recordings a previous run left `recording`: the system stopped Aurora before
    /// `shutdown` could finish them. Each keeps what reached the disk and completes,
    /// cut short, as Windows' `reap` completes a stream that ended early; with nothing
    /// on disk it fails. Run once, before the first tick; a row this run is recording
    /// is left alone.
    pub(crate) fn recover_interrupted(&self, report: &mut TickReport) -> Result<()> {
        let folder = self.folder();
        let db = self.db.lock();
        for rec in repo::list(&db, Some(State::Recording))? {
            if self.active.lock().contains_key(&rec.id) || self.starting.lock().contains(&rec.id) {
                continue;
            }
            let path = folder.join(recording_filename(
                &rec.title,
                rec.season,
                rec.episode,
                rec.air_start,
            ));
            let meta = std::fs::metadata(&path).ok().filter(|m| m.len() > 0);
            if let Some(meta) = meta {
                // From the file's first byte to its last: the row's `start` includes
                // padding a recording begun mid-programme never had.
                let secs = |t: std::io::Result<std::time::SystemTime>| {
                    t.ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64)
                };
                let began = secs(meta.created()).unwrap_or(rec.start).max(rec.start);
                let ended = secs(meta.modified()).unwrap_or(began).min(rec.stop);
                repo::set_file(
                    &db,
                    rec.id,
                    &path.to_string_lossy(),
                    meta.len() as i64,
                    (ended - began).max(0),
                )?;
                repo::set_state(&db, rec.id, State::Completed, Some(INTERRUPTED))?;
                report.completed.push(rec.id);
            } else {
                let _ = std::fs::remove_file(&path);
                repo::set_state(&db, rec.id, State::Failed, Some(INTERRUPTED))?;
                report.failed.push(FailedRecording {
                    id: rec.id,
                    reason: INTERRUPTED.into(),
                });
            }
        }
        Ok(())
    }

    /// Advance the schedule by one step. Safe to call as often as you like.
    pub(crate) fn tick(&self, now: i64) -> Result<TickReport> {
        let mut report = TickReport::default();
        self.reap(now, &mut report)?;
        self.abandon_missed(now, &mut report)?;
        self.start_due(now, &mut report)?;
        self.note_stalls(now, &mut report);
        Ok(report)
    }

    /// Collect recordings that ended: the stream stopped, or the window closed.
    fn reap(&self, now: i64, report: &mut TickReport) -> Result<()> {
        let ending: Vec<i64> = {
            let active = self.active.lock();
            active
                .iter()
                .filter(|(_, a)| a.stop_at <= now || a.handle.is_finished())
                .map(|(id, _)| *id)
                .collect()
        };

        for id in ending {
            let Some(entry) = self.active.lock().remove(&id) else {
                continue;
            };
            let started_at = entry.started_at;
            let outcome = entry.handle.stop();
            let duration = (now - started_at).max(0);

            let db = self.db.lock();
            if outcome.is_usable() {
                repo::set_file(
                    &db,
                    id,
                    &outcome.path.to_string_lossy(),
                    outcome.bytes as i64,
                    duration,
                )?;
                // A cut-short recording still plays up to the cut, so it completes with
                // the reason attached rather than failing.
                repo::set_state(&db, id, State::Completed, outcome.error.as_deref())?;
                report.completed.push(id);
            } else {
                let reason = outcome.error.unwrap_or_else(|| NOTHING_SENT.into());
                // Nothing was written: no zero-byte file left in the folder.
                let _ = std::fs::remove_file(&outcome.path);
                repo::set_state(&db, id, State::Failed, Some(&reason))?;
                report.failed.push(FailedRecording { id, reason });
            }
        }
        Ok(())
    }

    /// Recordings whose window closed while Aurora was not running.
    fn abandon_missed(&self, now: i64, report: &mut TickReport) -> Result<()> {
        let db = self.db.lock();
        let missed = repo::list(&db, Some(State::Scheduled))?;
        for rec in missed.into_iter().filter(|r| r.stop <= now) {
            repo::set_state(&db, rec.id, State::Failed, Some(MISSED))?;
            report.failed.push(FailedRecording {
                id: rec.id,
                reason: MISSED.into(),
            });
        }
        Ok(())
    }

    fn start_due(&self, now: i64, report: &mut TickReport) -> Result<()> {
        let due = {
            let db = self.db.lock();
            repo::due(&db, now)?
        };
        let limit = {
            let db = self.db.lock();
            self.max_concurrent(&db)
        };

        for rec in due {
            if self.active.lock().contains_key(&rec.id) || self.starting.lock().contains(&rec.id) {
                continue;
            }
            let busy = self.active.lock().len() + self.starting.lock().len();
            if busy >= limit {
                // `due` is in priority order, so what matters started first; the rest
                // are skipped rather than queued, because their airtime is now.
                let db = self.db.lock();
                repo::set_state(&db, rec.id, State::Skipped, Some(TOO_MANY))?;
                report.failed.push(FailedRecording {
                    id: rec.id,
                    reason: TOO_MANY.into(),
                });
                continue;
            }

            // Claimed now, opened on a thread: a provider that accepts the connection
            // and sends nothing would otherwise hold up every tick (see the Windows
            // comment at the same place).
            {
                let db = self.db.lock();
                repo::set_state(&db, rec.id, State::Recording, None)?;
            }
            self.starting.lock().insert(rec.id);
            report.started.push(rec.id);

            let id = rec.id;
            let db = Arc::clone(&self.db);
            let recorder = Arc::clone(&self.recorder);
            let folder = self.folder();
            let active = Arc::clone(&self.active);
            let starting = Arc::clone(&self.starting);
            let credentials = Arc::clone(&self.credentials);
            let spawned = std::thread::Builder::new()
                .name(format!("aurora-rec-{id}"))
                .spawn(move || {
                    let outcome = Self::open_stream(
                        &db,
                        recorder.as_ref(),
                        &folder,
                        credentials.as_ref(),
                        &rec,
                        now,
                    );
                    starting.lock().remove(&id);
                    match outcome {
                        Ok(entry) => {
                            active.lock().insert(id, entry);
                        }
                        Err(e) => {
                            let reason = e.to_string();
                            tracing::warn!("recording {id} could not start: {reason}");
                            let db = db.lock();
                            let _ = repo::set_state(&db, id, State::Failed, Some(&reason));
                        }
                    }
                });
            if let Err(e) = spawned {
                self.starting.lock().remove(&id);
                let reason = format!("could not start a thread for this recording: {e}");
                let db = self.db.lock();
                repo::set_state(&db, id, State::Failed, Some(&reason))?;
                report.failed.push(FailedRecording { id, reason });
            }
        }
        Ok(())
    }

    /// Open the provider's stream and start writing it to disk.
    fn open_stream(
        db: &Arc<Mutex<Connection>>,
        recorder: &dyn Recorder,
        folder: &Path,
        credentials: &dyn CredentialStore,
        rec: &repo::Recording,
        now: i64,
    ) -> Result<Active> {
        let url = {
            let db = db.lock();
            // The channel's best source, the one a viewer tuning in now would get.
            crate::playback::live_sources(&db, credentials, rec.channel_id)?
                .into_iter()
                .next()
                .map(|s| s.url)
                .ok_or_else(|| AuroraError::other("This channel has no source to record."))?
        };

        let dest = folder.join(recording_filename(
            &rec.title,
            rec.season,
            rec.episode,
            rec.air_start,
        ));

        let handle = recorder
            .start(RecordRequest {
                url,
                dest,
                window_secs: (rec.stop - now).max(1) as u64,
                user_agent: None,
                referrer: None,
            })
            .map_err(|e| AuroraError::other(e.message))?;

        Ok(Active {
            handle,
            started_at: now,
            stop_at: rec.stop,
        })
    }

    fn note_stalls(&self, now: i64, report: &mut TickReport) {
        let active = self.active.lock();
        for (id, entry) in active.iter() {
            if entry
                .handle
                .progress()
                .is_stalled(now, STALL_SECS, entry.started_at)
            {
                report.stalled.push(*id);
            }
        }
        report.stalled.sort_unstable();
    }

    /// Stop everything. Recordings in flight complete with what they have.
    pub(crate) fn shutdown(&self, now: i64) {
        self.settle(std::time::Duration::from_secs(5));
        if self.active.lock().is_empty() {
            return;
        }
        let mut report = TickReport::default();
        for entry in self.active.lock().values_mut() {
            entry.stop_at = now;
        }
        if let Err(e) = self.reap(now, &mut report) {
            tracing::error!("could not finalise recordings on shutdown: {e}");
        }
    }
}

/// Run every enabled rule over the guide as it stands. Idempotent, so a refresh and a
/// new rule can both call it. Mirrors `aurora-app/src/dvr.rs` `expand_rules_now`.
pub(crate) fn expand_rules_now(db: &Connection, now: i64) -> Result<Vec<i64>> {
    let programmes = repo::upcoming_programmes(db, now, now + EXPANSION_HORIZON_SECS)?;
    Ok(repo::expand_rules(
        db,
        &programmes,
        local_utc_offset_secs(db),
        now,
    )?)
}

/// The viewer's UTC offset, which rules with a weekday or a time of day are matched in.
/// Windows asks the `time` crate; SQLite asks the same C library, without a dependency
/// for one number. Falls back to UTC, as Windows does.
fn local_utc_offset_secs(db: &Connection) -> i32 {
    db.query_row(
        "SELECT CAST(strftime('%s','now','localtime') AS INTEGER)
              - CAST(strftime('%s','now') AS INTEGER)",
        [],
        |r| r.get::<_, i64>(0),
    )
    .map(|s| s as i32)
    .unwrap_or(0)
}

/// Where a stored file is now. An iPhone app's container moves when the app is updated,
/// which leaves the absolute path a recording was written to pointing nowhere; the file
/// itself moved with the container, under the same name in the recordings folder.
fn on_disk(stored: &str, folder: &Path) -> Option<PathBuf> {
    let path = PathBuf::from(stored);
    if path.exists() {
        return Some(path);
    }
    let moved = folder.join(path.file_name()?);
    moved.exists().then_some(moved)
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[uniffi::export]
impl AuroraCore {
    /// Where recordings are written. Swift points it at a folder the system keeps;
    /// a `dvr.folder` setting, as Windows reads one, wins over it.
    pub fn use_recordings_folder(&self, path: String) {
        let stored = settings::get::<String>(&self.db.lock(), FOLDER_KEY)
            .ok()
            .flatten();
        *self.dvr.folder.lock() = PathBuf::from(stored.unwrap_or(path));
    }

    /// Start the scheduler's ten-second clock, once; `listener` hears every tick that
    /// changed something. Windows starts the same loop when its window opens (`main.rs`).
    pub fn dvr_start(self: Arc<Self>, listener: Arc<dyn DvrListener>) {
        if self.dvr.ticking.swap(true, Ordering::SeqCst) {
            return;
        }
        let dvr = Arc::clone(&self.dvr);
        let spawned = std::thread::Builder::new()
            .name("aurora-dvr".into())
            .spawn(move || {
                let mut recovered = TickReport::default();
                match dvr.recover_interrupted(&mut recovered) {
                    Ok(()) if !recovered.is_empty() => listener.ticked(recovered),
                    Ok(()) => {}
                    Err(e) => tracing::error!("could not finish interrupted recordings: {e}"),
                }
                loop {
                    // A panic would end the scheduler outright, and a DVR that has silently
                    // stopped has no symptom until the programme is gone.
                    let ticked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        dvr.tick(now_unix())
                    }));
                    match ticked {
                        Ok(Ok(report)) if !report.is_empty() => listener.ticked(report),
                        Ok(Ok(_)) => {}
                        Ok(Err(e)) => tracing::error!("DVR tick failed: {e}"),
                        Err(_) => tracing::error!("DVR tick panicked"),
                    }
                    std::thread::sleep(TICK_INTERVAL);
                }
            });
        if let Err(e) = spawned {
            self.dvr.ticking.store(false, Ordering::SeqCst);
            tracing::error!("could not start the recordings scheduler: {e}");
        }
    }

    /// One step of the schedule now, rather than at the next tick.
    pub fn dvr_tick(&self) -> Result<TickReport> {
        self.dvr.tick(now_unix())
    }

    /// Finish everything in flight with what it has: Aurora is closing.
    pub fn dvr_shutdown(&self) {
        self.dvr.shutdown(now_unix());
    }

    /// Recordings in `state`, or all of them.
    pub fn dvr_list(&self, state: Option<RecordingState>) -> Result<Vec<Recording>> {
        let (rows, names) = {
            let db = self.db.lock();
            let rows = repo::list(&db, state.map(State::from))?;
            // Only the channels these rows name (R-117).
            let ids: BTreeSet<i64> = rows.iter().map(|r| r.channel_id).collect();
            let mut names = HashMap::new();
            for id in ids {
                if let Some(channel) = aurora_db::repo::channels::get(&db, id)? {
                    names.insert(id, (channel.name, channel.logo));
                }
            }
            (rows, names)
        };
        Ok(rows
            .into_iter()
            .map(|r| {
                let (channel_name, channel_logo) = names
                    .get(&r.channel_id)
                    .map(|(n, l)| (Some(n.clone()), l.clone()))
                    .unwrap_or((None, None));
                Recording {
                    live_bytes: self.dvr.bytes_so_far(r.id),
                    id: r.id,
                    channel_id: r.channel_id,
                    rule_id: r.rule_id,
                    title: r.title,
                    sub_title: r.sub_title,
                    season: r.season,
                    episode: r.episode,
                    air_start: r.air_start,
                    air_stop: r.air_stop,
                    start: r.start,
                    stop: r.stop,
                    state: r.state.into(),
                    reason: r.reason,
                    priority: r.priority,
                    file_path: r.file_path,
                    bytes: r.bytes,
                    duration_secs: r.duration_secs,
                    keep: r.keep,
                    watched: r.watched,
                    channel_name,
                    channel_logo,
                }
            })
            .collect())
    }

    /// Schedule one airing. `None` means it was already on the schedule.
    pub fn dvr_schedule(&self, recording: ScheduleRecording) -> Result<Option<i64>> {
        let db = self.db.lock();
        Ok(repo::schedule(
            &db,
            &repo::NewRecording {
                channel_id: recording.channel_id,
                title: &recording.title,
                sub_title: recording.sub_title.as_deref(),
                season: recording.season,
                episode: recording.episode,
                air_start: recording.air_start,
                air_stop: recording.air_stop,
                pre_padding_secs: recording
                    .pre_padding_secs
                    .unwrap_or(aurora_core::dvr::DEFAULT_PRE_PADDING_SECS),
                post_padding_secs: recording
                    .post_padding_secs
                    .unwrap_or(aurora_core::dvr::DEFAULT_POST_PADDING_SECS),
                priority: MANUAL_PRIORITY,
                ..Default::default()
            },
            now_unix(),
        )?)
    }

    /// Cancel a recording not yet made, or stop one in flight ("Stop" on the row).
    ///
    /// Windows deletes the row and leaves the stream writing until its window closes,
    /// holding a connection of the subscription's and filling a file no row names. Here
    /// the stream stops with it and the partial file goes, which is what "Stop" and the
    /// row disappearing both say happened.
    pub fn dvr_cancel(&self, id: i64) -> Result<bool> {
        let cancelled = {
            let db = self.db.lock();
            repo::cancel(&db, id)?
        };
        if let Some(entry) = self.dvr.active.lock().remove(&id) {
            let outcome = entry.handle.stop();
            let _ = std::fs::remove_file(&outcome.path);
        }
        Ok(cancelled)
    }

    /// Delete a recording and its file.
    pub fn dvr_delete(&self, id: i64) -> Result<bool> {
        let path = {
            let db = self.db.lock();
            repo::delete(&db, id)?
        };
        if let Some(path) = path {
            // The row is gone; a file that will not delete is logged, not an error.
            if let Some(file) = on_disk(&path, &self.dvr.folder()) {
                if let Err(e) = std::fs::remove_file(&file) {
                    tracing::warn!("could not delete recording file {}: {e}", file.display());
                }
            }
            return Ok(true);
        }
        Ok(false)
    }

    pub fn dvr_set_keep(&self, id: i64, keep: bool) -> Result<()> {
        let db = self.db.lock();
        repo::set_keep(&db, id, keep)?;
        Ok(())
    }

    pub fn dvr_set_watched(&self, id: i64, watched: bool) -> Result<()> {
        let db = self.db.lock();
        repo::set_watched(&db, id, watched)?;
        Ok(())
    }

    pub fn dvr_conflicts(&self) -> Result<Vec<RecordingConflict>> {
        let db = self.db.lock();
        let limit = self.dvr.max_concurrent(&db);
        Ok(repo::conflicts(&db, now_unix(), limit)?
            .into_iter()
            .map(|c| RecordingConflict {
                start: c.start,
                stop: c.stop,
                slot_ids: c.slot_ids,
                over_by: c.over_by as u32,
            })
            .collect())
    }

    pub fn dvr_rules(&self) -> Result<Vec<SeriesRule>> {
        let db = self.db.lock();
        Ok(repo::list_rules(&db)?
            .into_iter()
            .map(|r| SeriesRule {
                id: r.id,
                title: r.title,
                channel_id: r.channel_id,
                new_only: r.new_only,
                weekdays: r.weekdays,
                around_local_minute: r.around_local_minute,
                pre_padding_secs: r.pre_padding_secs,
                post_padding_secs: r.post_padding_secs,
                keep_episodes: r.keep_episodes,
                priority: r.priority,
                enabled: r.enabled,
                scheduled: r.scheduled,
                channel_name: r.channel_name,
            })
            .collect())
    }

    /// Create a series rule and schedule from the guide already here: "Record series"
    /// that leaves the schedule empty looks broken.
    pub fn dvr_create_rule(&self, rule: NewSeriesRule) -> Result<i64> {
        let db = self.db.lock();
        let now = now_unix();
        let id = repo::create_rule(
            &db,
            &repo::NewRule {
                title: &rule.title,
                channel_id: rule.channel_id,
                new_only: rule.new_only,
                weekdays: rule.weekdays,
                around_local_minute: rule.around_local_minute,
                pre_padding_secs: rule.pre_padding_secs,
                post_padding_secs: rule.post_padding_secs,
                keep_episodes: rule.keep_episodes,
                ..Default::default()
            },
            now,
        )?;
        if let Err(e) = expand_rules_now(&db, now) {
            tracing::warn!("new rule created but could not be expanded yet: {e}");
        }
        Ok(id)
    }

    pub fn dvr_delete_rule(&self, id: i64) -> Result<bool> {
        let db = self.db.lock();
        Ok(repo::delete_rule(&db, id)?)
    }

    pub fn dvr_set_rule_enabled(&self, id: i64, enabled: bool) -> Result<()> {
        let db = self.db.lock();
        repo::set_rule_enabled(&db, id, enabled)?;
        Ok(())
    }

    pub fn dvr_reminders(&self) -> Result<Vec<Reminder>> {
        let db = self.db.lock();
        Ok(repo::list_reminders(&db)?
            .into_iter()
            .map(|r| Reminder {
                id: r.id,
                channel_id: r.channel_id,
                title: r.title,
                start: r.start,
                lead_secs: r.lead_secs,
            })
            .collect())
    }

    /// `None` means this airing already has a reminder.
    pub fn dvr_add_reminder(
        &self,
        channel_id: i64,
        title: String,
        start: i64,
        lead_secs: Option<i64>,
    ) -> Result<Option<i64>> {
        let db = self.db.lock();
        Ok(repo::add_reminder(
            &db,
            channel_id,
            &title,
            start,
            lead_secs.unwrap_or(REMINDER_LEAD_SECS),
            now_unix(),
        )?)
    }

    pub fn dvr_remove_reminder(&self, id: i64) -> Result<bool> {
        let db = self.db.lock();
        Ok(repo::remove_reminder(&db, id)?)
    }

    pub fn dvr_storage(&self) -> Result<DvrStorage> {
        let db = self.db.lock();
        let quota: i64 = settings::get_or(&db, QUOTA_KEY, 0)?;
        Ok(DvrStorage {
            folder: self.dvr.folder().to_string_lossy().into_owned(),
            used_bytes: repo::total_bytes(&db)?,
            quota_bytes: quota,
            prunable: if quota > 0 {
                repo::over_quota(&db, quota)?
            } else {
                Vec::new()
            },
            max_concurrent: self.dvr.max_concurrent(&db) as u32,
        })
    }

    /// Everything needed to play a recording, as a film with `kind` Recording. Mirrors the `What::Recording` arm of
    /// `aurora-app/src/play/resolve.rs`, refusals word for word.
    pub fn tune_recording(&self, id: i64, start_at_secs: Option<f64>) -> Result<VodTune> {
        let db = self.db.lock();
        crate::parental::allowed(&db, aurora_db::repo::parental::Target::Recording(id))?;
        let row: Option<(Option<String>, String, String)> = db
            .query_row(
                "SELECT file_path, title, state FROM recordings WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (path, title, state) = row.ok_or_else(|| {
            AuroraError::not_found(
                "This recording is no longer in your library. It may have been deleted to \
                 stay under the storage quota.",
            )
        })?;
        let path = match (path.as_deref().map(str::trim), state.as_str()) {
            (Some(p), _) if !p.is_empty() => p.to_string(),
            (_, "scheduled") => {
                return Err(AuroraError::other(
                    "This recording has not started yet, so there is nothing to play.",
                ))
            }
            (_, "recording") => {
                return Err(AuroraError::other(
                    "This recording is still being written. It can be played once it \
                     finishes.",
                ))
            }
            _ => {
                return Err(AuroraError::other(
                    "This recording has no file on disk. The Recordings screen says why.",
                ))
            }
        };
        let Some(file) = on_disk(&path, &self.dvr.folder()) else {
            return Err(AuroraError::other(format!(
                "The file for this recording is gone: {path}"
            )));
        };
        let options = Request {
            live: false,
            cache_secs: crate::vod::VOD_CACHE_SECS,
            start_secs: start_at_secs,
            ..Default::default()
        };
        // Played as a film is, so its position is kept as Windows keeps it.
        Ok(VodTune {
            kind: VodKind::Recording,
            id,
            title,
            url: file.to_string_lossy().into_owned(),
            mpv_options: mpv_options(&options),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::neterr::NetFailure;
    use aurora_db::repo::dvr::NewRecording;
    use std::io::{Cursor, Read};
    use std::sync::atomic::AtomicUsize;

    fn test_store() -> Arc<dyn CredentialStore> {
        Arc::new(aurora_ingest::credentials::MemoryStore::default())
    }

    /// Records canned bytes, so a tick can be driven without a network.
    #[derive(Default)]
    struct FakeRecorder {
        body: Vec<u8>,
        fail_with: Option<String>,
        starts: AtomicUsize,
    }

    impl Recorder for FakeRecorder {
        fn start(&self, request: RecordRequest) -> std::result::Result<Handle, NetFailure> {
            self.starts.fetch_add(1, Ordering::Relaxed);
            if let Some(message) = &self.fail_with {
                return Err(NetFailure::classify(message));
            }
            aurora_ingest::recorder::record_from_reader(
                Cursor::new(self.body.clone()),
                &request.dest,
            )
        }
    }

    /// A source that never ends, for a recording that is still running.
    struct Endless;
    impl Read for Endless {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(std::time::Duration::from_millis(5));
            buf[0] = 1;
            Ok(1)
        }
    }

    struct EndlessRecorder;
    impl Recorder for EndlessRecorder {
        fn start(&self, request: RecordRequest) -> std::result::Result<Handle, NetFailure> {
            aurora_ingest::recorder::record_from_reader(Endless, &request.dest)
        }
    }

    fn seeded_db() -> Arc<Mutex<Connection>> {
        let conn = aurora_db::open_memory().unwrap();
        conn.execute(
            "INSERT INTO providers (id,name,kind,base_url,created_at)
             VALUES (1,'P','m3u','https://example.com',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO channels (id,provider_id,provider_key,name,match_key,last_seen_at)
             VALUES (1,1,'k','BBC One','bbc one',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO channel_sources (channel_id,url,priority)
             VALUES (1,'http://example.com/live.ts',0)",
            [],
        )
        .unwrap();
        Arc::new(Mutex::new(conn))
    }

    fn schedule_at(db: &Arc<Mutex<Connection>>, title: &str, air_start: i64) -> i64 {
        schedule_with(db, title, air_start, 0)
    }

    fn schedule_with(
        db: &Arc<Mutex<Connection>>,
        title: &str,
        air_start: i64,
        priority: i32,
    ) -> i64 {
        repo::schedule(
            &db.lock(),
            &NewRecording {
                channel_id: 1,
                title,
                air_start,
                air_stop: air_start + 1800,
                pre_padding_secs: 0,
                post_padding_secs: 0,
                priority,
                ..Default::default()
            },
            0,
        )
        .unwrap()
        .unwrap()
    }

    fn state_of(db: &Arc<Mutex<Connection>>, id: i64) -> State {
        repo::get(&db.lock(), id).unwrap().unwrap().state
    }

    fn wait_until(mut cond: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !cond() {
            assert!(std::time::Instant::now() < deadline, "timed out");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn dvr(db: &Arc<Mutex<Connection>>, recorder: Arc<dyn Recorder>, dir: &Path) -> Dvr {
        Dvr::new(Arc::clone(db), recorder, dir.to_path_buf(), test_store())
    }

    #[test]
    fn a_recording_the_system_cut_off_keeps_what_reached_the_disk() {
        let dir = tempfile::tempdir().unwrap();
        let db = seeded_db();
        let kept = schedule_at(&db, "News", 1_000);
        let empty = schedule_at(&db, "Film", 2_000);
        for id in [kept, empty] {
            repo::set_state(&db.lock(), id, State::Recording, None).unwrap();
        }
        std::fs::write(
            dir.path()
                .join(recording_filename("News", None, None, 1_000)),
            [7; 512],
        )
        .unwrap();
        let dvr = dvr(&db, Arc::new(FakeRecorder::default()), dir.path());

        let mut report = TickReport::default();
        dvr.recover_interrupted(&mut report).unwrap();

        assert_eq!(report.completed, vec![kept]);
        let rec = repo::get(&db.lock(), kept).unwrap().unwrap();
        assert_eq!(rec.state, State::Completed);
        assert_eq!(rec.reason.as_deref(), Some(INTERRUPTED));
        assert_eq!(rec.bytes, 512);
        assert!(rec.file_path.is_some());
        assert_eq!(state_of(&db, empty), State::Failed);
        assert_eq!(report.failed[0].reason, INTERRUPTED);
    }

    #[test]
    fn recovery_leaves_a_recording_this_run_is_making_alone() {
        let dir = tempfile::tempdir().unwrap();
        let db = seeded_db();
        let id = schedule_at(&db, "News", 1_000);
        let dvr = dvr(&db, Arc::new(EndlessRecorder), dir.path());
        dvr.tick(1_000).unwrap();
        assert!(dvr.settle(std::time::Duration::from_secs(5)));

        let mut report = TickReport::default();
        dvr.recover_interrupted(&mut report).unwrap();

        assert!(report.is_empty());
        assert_eq!(state_of(&db, id), State::Recording);
        dvr.shutdown(1_010);
    }

    #[test]
    fn a_due_recording_starts_and_completes_when_the_stream_ends() {
        let dir = tempfile::tempdir().unwrap();
        let db = seeded_db();
        let id = schedule_at(&db, "News", 1_000);
        let recorder = Arc::new(FakeRecorder {
            body: vec![7; 4096],
            ..Default::default()
        });
        let dvr = dvr(&db, recorder, dir.path());

        let report = dvr.tick(1_000).unwrap();
        assert_eq!(report.started, vec![id]);
        assert!(dvr.settle(std::time::Duration::from_secs(5)));
        wait_until(|| {
            dvr.active
                .lock()
                .get(&id)
                .is_some_and(|a| a.handle.is_finished())
        });

        let report = dvr.tick(1_010).unwrap();
        assert_eq!(report.completed, vec![id]);
        let rec = repo::get(&db.lock(), id).unwrap().unwrap();
        assert_eq!(rec.state, State::Completed);
        assert_eq!(rec.bytes, 4096);
        assert!(Path::new(rec.file_path.as_deref().unwrap()).exists());
    }

    #[test]
    fn a_recording_that_produces_nothing_fails_and_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let db = seeded_db();
        let id = schedule_at(&db, "Empty", 1_000);
        let dvr = dvr(&db, Arc::new(FakeRecorder::default()), dir.path());
        dvr.tick(1_000).unwrap();
        assert!(dvr.settle(std::time::Duration::from_secs(5)));
        wait_until(|| {
            dvr.active
                .lock()
                .get(&id)
                .is_some_and(|a| a.handle.is_finished())
        });
        let report = dvr.tick(1_010).unwrap();
        assert_eq!(report.failed[0].id, id);
        assert_eq!(state_of(&db, id), State::Failed);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_recorder_that_cannot_start_fails_the_recording_with_a_reason() {
        let dir = tempfile::tempdir().unwrap();
        let db = seeded_db();
        let id = schedule_at(&db, "Refused", 1_000);
        let recorder = Arc::new(FakeRecorder {
            fail_with: Some("HTTP 555".into()),
            ..Default::default()
        });
        let dvr = dvr(&db, recorder, dir.path());
        dvr.tick(1_000).unwrap();
        assert!(dvr.settle(std::time::Duration::from_secs(5)));
        let rec = repo::get(&db.lock(), id).unwrap().unwrap();
        assert_eq!(rec.state, State::Failed);
        assert!(rec.reason.is_some());
    }

    #[test]
    fn a_recording_is_cut_off_when_its_window_closes() {
        let dir = tempfile::tempdir().unwrap();
        let db = seeded_db();
        let id = schedule_at(&db, "Long", 1_000);
        let dvr = dvr(&db, Arc::new(EndlessRecorder), dir.path());
        dvr.tick(1_000).unwrap();
        assert!(dvr.settle(std::time::Duration::from_secs(5)));
        wait_until(|| dvr.bytes_so_far(id).unwrap_or(0) > 0);
        let report = dvr.tick(2_800).unwrap();
        assert_eq!(report.completed, vec![id]);
        assert_eq!(state_of(&db, id), State::Completed);
    }

    #[test]
    fn ticking_twice_does_not_start_the_same_recording_again() {
        let dir = tempfile::tempdir().unwrap();
        let db = seeded_db();
        schedule_at(&db, "Once", 1_000);
        let recorder = Arc::new(FakeRecorder {
            body: vec![1; 10],
            ..Default::default()
        });
        let dvr = dvr(&db, Arc::clone(&recorder) as Arc<dyn Recorder>, dir.path());
        dvr.tick(1_000).unwrap();
        dvr.tick(1_001).unwrap();
        assert!(dvr.settle(std::time::Duration::from_secs(5)));
        assert_eq!(recorder.starts.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn the_concurrency_limit_skips_the_lowest_priority_recording() {
        let dir = tempfile::tempdir().unwrap();
        let db = seeded_db();
        let high = schedule_with(&db, "High", 1_000, 10);
        let low = schedule_with(&db, "Low", 1_000, 0);
        let dvr = dvr(&db, Arc::new(EndlessRecorder), dir.path()).with_max_concurrent(1);
        let report = dvr.tick(1_000).unwrap();
        assert_eq!(report.started, vec![high]);
        assert_eq!(
            report.failed,
            vec![FailedRecording {
                id: low,
                reason: TOO_MANY.into()
            }]
        );
        assert_eq!(state_of(&db, low), State::Skipped);
        assert!(dvr.settle(std::time::Duration::from_secs(5)));
        dvr.shutdown(1_100);
    }

    #[test]
    fn a_one_connection_line_caps_the_dvr_without_being_told() {
        let dir = tempfile::tempdir().unwrap();
        let db = seeded_db();
        db.lock()
            .execute("UPDATE providers SET max_connections = 1", [])
            .unwrap();
        let dvr = dvr(&db, Arc::new(EndlessRecorder), dir.path());
        assert_eq!(dvr.max_concurrent(&db.lock()), 1);
        db.lock()
            .execute("UPDATE providers SET max_connections = NULL", [])
            .unwrap();
        assert_eq!(dvr.max_concurrent(&db.lock()), DEFAULT_MAX_CONCURRENT);
    }

    #[test]
    fn a_recording_missed_while_aurora_was_closed_is_marked_failed() {
        let dir = tempfile::tempdir().unwrap();
        let db = seeded_db();
        let id = schedule_at(&db, "Missed", 1_000);
        let dvr = dvr(&db, Arc::new(FakeRecorder::default()), dir.path());
        let report = dvr.tick(5_000).unwrap();
        assert_eq!(
            report.failed,
            vec![FailedRecording {
                id,
                reason: MISSED.into()
            }]
        );
        assert_eq!(state_of(&db, id), State::Failed);
    }

    #[test]
    fn a_tick_with_nothing_to_do_reports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let db = seeded_db();
        let dvr = dvr(&db, Arc::new(FakeRecorder::default()), dir.path());
        assert!(dvr.tick(1_000).unwrap().is_empty());
    }

    #[test]
    fn shutdown_finalises_a_recording_in_flight() {
        let dir = tempfile::tempdir().unwrap();
        let db = seeded_db();
        let id = schedule_at(&db, "Running", 1_000);
        let dvr = dvr(&db, Arc::new(EndlessRecorder), dir.path());
        dvr.tick(1_000).unwrap();
        assert!(dvr.settle(std::time::Duration::from_secs(5)));
        wait_until(|| dvr.bytes_so_far(id).unwrap_or(0) > 0);
        dvr.shutdown(1_100);
        assert_eq!(state_of(&db, id), State::Completed);
        assert!(dvr.active.lock().is_empty());
    }

    #[test]
    fn a_moved_container_still_finds_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("News.ts"), b"x").unwrap();
        let found = on_disk("/old/container/Recordings/News.ts", dir.path()).unwrap();
        assert_eq!(found, dir.path().join("News.ts"));
        assert!(on_disk("/old/container/Recordings/Gone.ts", dir.path()).is_none());
    }

    #[test]
    fn the_commands_schedule_list_and_refuse_to_play_what_is_not_recorded() {
        let (core, _dir) = crate::testing::core();
        {
            let db = core.db.lock();
            db.execute(
                "INSERT INTO providers (id,name,kind,base_url,created_at)
                 VALUES (1,'P','m3u','https://example.com',0)",
                [],
            )
            .unwrap();
            db.execute(
                "INSERT INTO channels (id,provider_id,provider_key,name,match_key,last_seen_at)
                 VALUES (1,1,'k','BBC One','bbc one',0)",
                [],
            )
            .unwrap();
        }
        let start = now_unix() + 3_600;
        let id = core
            .dvr_schedule(ScheduleRecording {
                channel_id: 1,
                title: "News".into(),
                sub_title: None,
                season: None,
                episode: None,
                air_start: start,
                air_stop: start + 1_800,
                pre_padding_secs: None,
                post_padding_secs: None,
            })
            .unwrap()
            .unwrap();
        let listed = core.dvr_list(Some(RecordingState::Scheduled)).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].channel_name.as_deref(), Some("BBC One"));
        assert_eq!(listed[0].priority, MANUAL_PRIORITY);
        assert_eq!(
            listed[0].start,
            start - aurora_core::dvr::DEFAULT_PRE_PADDING_SECS
        );
        let refused = core.tune_recording(id, None).unwrap_err().to_string();
        assert!(refused.contains("has not started yet"), "{refused}");
        assert!(core.dvr_cancel(id).unwrap());
        assert!(core
            .dvr_list(Some(RecordingState::Scheduled))
            .unwrap()
            .is_empty());
        assert_eq!(core.dvr_storage().unwrap().max_concurrent, 2);
    }

    #[test]
    fn the_local_offset_is_a_whole_number_of_quarter_hours() {
        let db = seeded_db();
        let offset = local_utc_offset_secs(&db.lock());
        assert_eq!(offset % 900, 0);
        assert!(offset.abs() <= 14 * 3_600);
    }
}
