//! The DVR supervisor: what turns rows in `recordings` into files on disk.
//!
//! One `tick` does the whole job — reap what finished, stop what has run its window,
//! start what is due — so there is a single place where the schedule and the recorder
//! can disagree, and it is driven by a clock the tests control.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use aurora_core::fsname::recording_filename;
use aurora_db::repo::dvr::{self as repo, Recording};
use aurora_db::repo::settings;
use aurora_db::rusqlite::Connection;
use aurora_ingest::recorder::{Handle, RecordRequest, Recorder};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::Result;
use crate::services::Services;
use crate::AppError;

/// Default ceiling on simultaneous recordings, used until a provider declares its own.
pub const DEFAULT_MAX_CONCURRENT: usize = 2;

/// How long a recording may produce nothing before it is called stalled.
pub const STALL_SECS: i64 = 120;

/// How often the scheduler wakes. Ten seconds is well inside the default one-minute
/// pre-padding, so a recording still starts before its programme does.
pub const TICK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

struct Active {
    handle: Handle,
    /// When the recorder actually opened the stream, which is not the scheduled start
    /// if Aurora was launched late.
    started_at: i64,
    stop_at: i64,
}

/// What one tick changed, for the event the UI listens to.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TickReport {
    pub started: Vec<i64>,
    pub completed: Vec<i64>,
    /// Id and the reason, as the user should read it.
    pub failed: Vec<(i64, String)>,
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

pub struct Dvr {
    db: Arc<Mutex<Connection>>,
    recorder: Arc<dyn Recorder>,
    folder: PathBuf,
    active: Mutex<HashMap<i64, Active>>,
    max_concurrent: usize,
}

impl Dvr {
    pub fn new(db: Arc<Mutex<Connection>>, recorder: Arc<dyn Recorder>, folder: PathBuf) -> Self {
        Self {
            db,
            recorder,
            folder,
            active: Mutex::new(HashMap::new()),
            max_concurrent: DEFAULT_MAX_CONCURRENT,
        }
    }

    pub fn with_max_concurrent(mut self, max: usize) -> Self {
        self.max_concurrent = max.max(1);
        self
    }

    pub fn folder(&self) -> &PathBuf {
        &self.folder
    }

    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    pub fn active_ids(&self) -> Vec<i64> {
        let mut ids: Vec<i64> = self.active.lock().keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Live byte count for a recording in flight, for the progress bar.
    pub fn bytes_so_far(&self, id: i64) -> Option<u64> {
        self.active
            .lock()
            .get(&id)
            .map(|a| a.handle.progress().bytes())
    }

    /// Advance the schedule by one step. Safe to call as often as you like.
    pub fn tick(&self, now: i64) -> Result<TickReport> {
        let mut report = TickReport::default();
        self.reap(now, &mut report)?;
        self.abandon_missed(now, &mut report)?;
        self.start_due(now, &mut report)?;
        self.note_stalls(now, &mut report);
        Ok(report)
    }

    /// Collect recordings that ended — because the stream stopped, or because their
    /// window closed.
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
                // A cut-short recording is still a recording: the file plays up to the
                // cut, so it completes with the reason attached rather than failing and
                // looking like there is nothing to watch.
                repo::set_state(
                    &db,
                    id,
                    aurora_core::dvr::RecordingState::Completed,
                    outcome.error.as_deref(),
                )?;
                report.completed.push(id);
            } else {
                let reason = outcome
                    .error
                    .unwrap_or_else(|| "the provider sent nothing".into());
                // Nothing was written, so remove the empty file rather than leaving a
                // zero-byte entry in the recordings folder.
                let _ = std::fs::remove_file(&outcome.path);
                repo::set_state(
                    &db,
                    id,
                    aurora_core::dvr::RecordingState::Failed,
                    Some(&reason),
                )?;
                report.failed.push((id, reason));
            }
        }
        Ok(())
    }

    /// Recordings whose window closed while Aurora was not running.
    ///
    /// Without this they sit as `scheduled` for ever and the schedule slowly fills with
    /// things that will never happen.
    fn abandon_missed(&self, now: i64, report: &mut TickReport) -> Result<()> {
        let db = self.db.lock();
        let missed = repo::list(&db, Some(aurora_core::dvr::RecordingState::Scheduled))?;
        for rec in missed.into_iter().filter(|r| r.stop <= now) {
            let reason = "Aurora was not running when this was due".to_string();
            repo::set_state(
                &db,
                rec.id,
                aurora_core::dvr::RecordingState::Failed,
                Some(&reason),
            )?;
            report.failed.push((rec.id, reason));
        }
        Ok(())
    }

    fn start_due(&self, now: i64, report: &mut TickReport) -> Result<()> {
        let due = {
            let db = self.db.lock();
            repo::due(&db, now)?
        };

        for rec in due {
            if self.active.lock().contains_key(&rec.id) {
                continue;
            }
            if self.active.lock().len() >= self.max_concurrent {
                // Priority ordering in `due` means the ones that matter start first; the
                // rest are skipped rather than queued, because their airtime is now.
                let reason = "too many recordings at once for this subscription".to_string();
                let db = self.db.lock();
                repo::set_state(
                    &db,
                    rec.id,
                    aurora_core::dvr::RecordingState::Skipped,
                    Some(&reason),
                )?;
                report.failed.push((rec.id, reason));
                continue;
            }

            match self.begin(&rec, now) {
                Ok(active) => {
                    let db = self.db.lock();
                    repo::set_state(
                        &db,
                        rec.id,
                        aurora_core::dvr::RecordingState::Recording,
                        None,
                    )?;
                    drop(db);
                    self.active.lock().insert(rec.id, active);
                    report.started.push(rec.id);
                }
                Err(e) => {
                    let reason = e.to_string();
                    let db = self.db.lock();
                    repo::set_state(
                        &db,
                        rec.id,
                        aurora_core::dvr::RecordingState::Failed,
                        Some(&reason),
                    )?;
                    report.failed.push((rec.id, reason));
                }
            }
        }
        Ok(())
    }

    fn begin(&self, rec: &Recording, now: i64) -> Result<Active> {
        let url = {
            let db = self.db.lock();
            let (url, _) = crate::window::resolve_playback(&db, "live", rec.channel_id, None)?;
            url
        };

        let dest = self.folder.join(recording_filename(
            &rec.title,
            rec.season,
            rec.episode,
            rec.air_start,
        ));

        let handle = self
            .recorder
            .start(RecordRequest {
                url,
                dest,
                window_secs: (rec.stop - now).max(1) as u64,
                user_agent: None,
                referrer: None,
            })
            .map_err(|e| AppError::Other(e.message))?;

        Ok(Active {
            handle,
            started_at: now,
            stop_at: rec.stop,
        })
    }

    fn note_stalls(&self, now: i64, report: &mut TickReport) {
        let active = self.active.lock();
        for (id, entry) in active.iter() {
            if entry.handle.progress().is_stalled(now, STALL_SECS) {
                report.stalled.push(*id);
            }
        }
        report.stalled.sort_unstable();
    }

    /// Stop everything, for shutdown. Recordings in flight complete with what they have.
    pub fn shutdown(&self, now: i64) {
        let ids: Vec<i64> = self.active.lock().keys().copied().collect();
        if ids.is_empty() {
            return;
        }
        let mut report = TickReport::default();
        // Pretend every window closed, so `reap` finalises them all.
        {
            let mut active = self.active.lock();
            for entry in active.values_mut() {
                entry.stop_at = now;
            }
        }
        if let Err(e) = self.reap(now, &mut report) {
            tracing::error!("could not finalise recordings on shutdown: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// IPC surface
// ---------------------------------------------------------------------------

/// Settings key for the recordings folder. Absent means "next to the library".
pub const FOLDER_KEY: &str = "dvr.folder";
/// Settings key for the disk budget, in bytes. Zero means unlimited.
pub const QUOTA_KEY: &str = "dvr.quotaBytes";

/// A recording plus what only the running DVR knows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingView {
    #[serde(flatten)]
    pub recording: Recording,
    /// Bytes on disk right now, for a recording still in flight.
    pub live_bytes: Option<u64>,
}

#[tauri::command]
pub fn dvr_list(services: State<'_, Services>, args: ListArgs) -> Result<Vec<RecordingView>> {
    let state = args.state.as_deref().and_then(parse_state);
    let rows = {
        let db = services.db.lock();
        repo::list(&db, state)?
    };
    Ok(rows
        .into_iter()
        .map(|recording| RecordingView {
            live_bytes: services.dvr.bytes_so_far(recording.id),
            recording,
        })
        .collect())
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListArgs {
    pub state: Option<String>,
}

fn parse_state(s: &str) -> Option<aurora_core::dvr::RecordingState> {
    use aurora_core::dvr::RecordingState as S;
    match s {
        "scheduled" => Some(S::Scheduled),
        "recording" => Some(S::Recording),
        "completed" => Some(S::Completed),
        "failed" => Some(S::Failed),
        "skipped" => Some(S::Skipped),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleArgs {
    pub channel_id: i64,
    pub title: String,
    pub sub_title: Option<String>,
    pub season: Option<u16>,
    pub episode: Option<u16>,
    pub air_start: i64,
    pub air_stop: i64,
    pub pre_padding_secs: Option<i64>,
    pub post_padding_secs: Option<i64>,
}

/// Schedule one airing. `None` means it was already on the schedule.
#[tauri::command]
pub fn dvr_schedule(services: State<'_, Services>, args: ScheduleArgs) -> Result<Option<i64>> {
    let db = services.db.lock();
    Ok(repo::schedule(
        &db,
        &repo::NewRecording {
            channel_id: args.channel_id,
            title: &args.title,
            sub_title: args.sub_title.as_deref(),
            season: args.season,
            episode: args.episode,
            air_start: args.air_start,
            air_stop: args.air_stop,
            pre_padding_secs: args
                .pre_padding_secs
                .unwrap_or(aurora_core::dvr::DEFAULT_PRE_PADDING_SECS),
            post_padding_secs: args
                .post_padding_secs
                .unwrap_or(aurora_core::dvr::DEFAULT_POST_PADDING_SECS),
            // A manual recording outranks anything a rule scheduled: the user asked
            // for this one by name.
            priority: 10,
            ..Default::default()
        },
        now_unix(),
    )?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdArgs {
    pub id: i64,
}

#[tauri::command]
pub fn dvr_cancel(services: State<'_, Services>, args: IdArgs) -> Result<bool> {
    let db = services.db.lock();
    Ok(repo::cancel(&db, args.id)?)
}

/// Delete a recording and its file.
#[tauri::command]
pub fn dvr_delete(services: State<'_, Services>, args: IdArgs) -> Result<bool> {
    let path = {
        let db = services.db.lock();
        repo::delete(&db, args.id)?
    };
    if let Some(path) = path {
        // The row is already gone, so a file that will not delete (still open in the
        // player, say) is logged rather than failing the command: the user asked for it
        // to leave the library, and it has.
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!("could not delete recording file {path}: {e}");
        }
        return Ok(true);
    }
    Ok(false)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlagArgs {
    pub id: i64,
    pub value: bool,
}

#[tauri::command]
pub fn dvr_set_keep(services: State<'_, Services>, args: FlagArgs) -> Result<()> {
    let db = services.db.lock();
    repo::set_keep(&db, args.id, args.value)?;
    Ok(())
}

#[tauri::command]
pub fn dvr_set_watched(services: State<'_, Services>, args: FlagArgs) -> Result<()> {
    let db = services.db.lock();
    repo::set_watched(&db, args.id, args.value)?;
    Ok(())
}

#[tauri::command]
pub fn dvr_conflicts(services: State<'_, Services>) -> Result<Vec<aurora_core::dvr::Conflict>> {
    let db = services.db.lock();
    Ok(repo::conflicts(
        &db,
        now_unix(),
        services.dvr.max_concurrent(),
    )?)
}

#[tauri::command]
pub fn dvr_rules(services: State<'_, Services>) -> Result<Vec<repo::RuleView>> {
    let db = services.db.lock();
    Ok(repo::list_rules(&db)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRuleArgs {
    pub title: String,
    pub channel_id: Option<i64>,
    pub new_only: bool,
    pub weekdays: Option<Vec<u8>>,
    pub around_local_minute: Option<u16>,
    pub pre_padding_secs: Option<i64>,
    pub post_padding_secs: Option<i64>,
    pub keep_episodes: Option<u16>,
}

#[tauri::command]
pub fn dvr_create_rule(services: State<'_, Services>, args: CreateRuleArgs) -> Result<i64> {
    let db = services.db.lock();
    let now = now_unix();
    let id = repo::create_rule(
        &db,
        &repo::NewRule {
            title: &args.title,
            channel_id: args.channel_id,
            new_only: args.new_only,
            weekdays: args.weekdays,
            around_local_minute: args.around_local_minute,
            pre_padding_secs: args.pre_padding_secs,
            post_padding_secs: args.post_padding_secs,
            keep_episodes: args.keep_episodes,
            ..Default::default()
        },
        now,
    )?;
    // Schedule from the guide we already have, rather than waiting for the next EPG
    // refresh: "Record series" that leaves the schedule empty looks broken.
    if let Err(e) = expand_rules_now(&db, now) {
        tracing::warn!("new rule created but could not be expanded yet: {e}");
    }
    Ok(id)
}

/// How far ahead rule expansion looks. A fortnight covers every provider's guide, and
/// scheduling further out than the guide is guesswork.
const EXPANSION_HORIZON_SECS: i64 = 14 * 86_400;

/// Run every enabled rule over the guide as it stands. Idempotent, so the EPG refresh
/// and the rules screen can both call it.
pub fn expand_rules_now(db: &Connection, now: i64) -> Result<Vec<i64>> {
    let programmes = repo::upcoming_programmes(db, now, now + EXPANSION_HORIZON_SECS)?;
    Ok(repo::expand_rules(
        db,
        &programmes,
        local_utc_offset_secs(),
        now,
    )?)
}

/// The viewer's UTC offset, which rules with a weekday or a time-of-day are matched in.
/// Falls back to UTC when the platform will not say — a rule matching an hour off is
/// better than one that matches nothing.
fn local_utc_offset_secs() -> i32 {
    time::UtcOffset::current_local_offset()
        .map(|o| o.whole_seconds())
        .unwrap_or(0)
}

#[tauri::command]
pub fn dvr_delete_rule(services: State<'_, Services>, args: IdArgs) -> Result<bool> {
    let db = services.db.lock();
    Ok(repo::delete_rule(&db, args.id)?)
}

#[tauri::command]
pub fn dvr_set_rule_enabled(services: State<'_, Services>, args: FlagArgs) -> Result<()> {
    let db = services.db.lock();
    repo::set_rule_enabled(&db, args.id, args.value)?;
    Ok(())
}

#[tauri::command]
pub fn dvr_reminders(services: State<'_, Services>) -> Result<Vec<repo::Reminder>> {
    let db = services.db.lock();
    Ok(repo::list_reminders(&db)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderArgs {
    pub channel_id: i64,
    pub title: String,
    pub start: i64,
    pub lead_secs: Option<i64>,
}

#[tauri::command]
pub fn dvr_add_reminder(services: State<'_, Services>, args: ReminderArgs) -> Result<Option<i64>> {
    let db = services.db.lock();
    Ok(repo::add_reminder(
        &db,
        args.channel_id,
        &args.title,
        args.start,
        args.lead_secs.unwrap_or(120),
        now_unix(),
    )?)
}

#[tauri::command]
pub fn dvr_remove_reminder(services: State<'_, Services>, args: IdArgs) -> Result<bool> {
    let db = services.db.lock();
    Ok(repo::remove_reminder(&db, args.id)?)
}

/// What the storage panel in settings shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    pub folder: String,
    pub used_bytes: i64,
    /// Zero means no limit.
    pub quota_bytes: i64,
    /// Recordings that would be deleted to get back under the quota.
    pub prunable: Vec<i64>,
    pub max_concurrent: usize,
}

#[tauri::command]
pub fn dvr_storage(services: State<'_, Services>) -> Result<Storage> {
    let db = services.db.lock();
    let quota: i64 = settings::get_or(&db, QUOTA_KEY, 0)?;
    Ok(Storage {
        folder: services.dvr.folder().to_string_lossy().into_owned(),
        used_bytes: repo::total_bytes(&db)?,
        quota_bytes: quota,
        prunable: if quota > 0 {
            repo::over_quota(&db, quota)?
        } else {
            Vec::new()
        },
        max_concurrent: services.dvr.max_concurrent(),
    })
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::dvr::RecordingState;
    use aurora_core::neterr::NetFailure;
    use aurora_db::repo::dvr::NewRecording;
    use std::io::{Cursor, Read};
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    /// A source that never ends, for testing a recording that is still running.
    struct Endless;
    impl Read for Endless {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            // Slow enough that a test can tick while it is still going.
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

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aurora-dvr-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
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
        let conn = db.lock();
        repo::schedule(
            &conn,
            &NewRecording {
                channel_id: 1,
                title,
                air_start,
                air_stop: air_start + 1800,
                // No padding: the test's clock arithmetic should be about the
                // scheduler, not about the padding.
                ..Default::default()
            },
            0,
        )
        .unwrap()
        .unwrap()
    }

    fn state_of(db: &Arc<Mutex<Connection>>, id: i64) -> RecordingState {
        repo::get(&db.lock(), id).unwrap().unwrap().state
    }

    /// Wait until a condition holds, rather than sleeping a guessed interval.
    ///
    /// The recorder runs on its own thread, so a fixed sleep is a race: on a loaded
    /// machine the next tick can cut the recording off before it has read a byte, and
    /// the test then fails for a reason that has nothing to do with what it checks.
    fn wait_until(mut cond: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if cond() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("condition did not hold within 5s");
    }

    /// Wait until a recording in flight has captured something.
    fn wait_for_bytes(dvr: &Dvr, id: i64, at_least: u64) {
        wait_until(|| dvr.bytes_so_far(id).unwrap_or(0) >= at_least);
    }

    #[test]
    fn a_due_recording_starts_and_completes_when_the_stream_ends() {
        let db = seeded_db();
        let dir = tempdir("complete");
        let id = schedule_at(&db, "The Show", 1_000);
        let dvr = Dvr::new(
            Arc::clone(&db),
            Arc::new(FakeRecorder {
                body: vec![9u8; 4_096],
                ..Default::default()
            }),
            dir.clone(),
        );

        let report = dvr.tick(1_000).unwrap();
        assert_eq!(report.started, vec![id]);
        assert_eq!(state_of(&db, id), RecordingState::Recording);

        // The canned body ends on its own, so the next tick finds it finished.
        wait_for_bytes(&dvr, id, 4_096);
        let report = dvr.tick(1_100).unwrap();
        assert_eq!(report.completed, vec![id]);
        assert!(report.failed.is_empty());

        let rec = repo::get(&db.lock(), id).unwrap().unwrap();
        assert_eq!(rec.state, RecordingState::Completed);
        assert_eq!(rec.bytes, 4_096);
        assert!(rec.file_path.is_some());
        assert_eq!(rec.duration_secs, 100);
        // The filename is derived from the title, not the id.
        assert!(rec.file_path.as_deref().unwrap().contains("The Show"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_recording_is_cut_off_when_its_window_closes() {
        let db = seeded_db();
        let dir = tempdir("window");
        let id = schedule_at(&db, "Long Show", 1_000);
        let dvr = Dvr::new(Arc::clone(&db), Arc::new(EndlessRecorder), dir.clone());

        dvr.tick(1_000).unwrap();
        assert_eq!(dvr.active_ids(), vec![id]);
        wait_for_bytes(&dvr, id, 1);

        // Past the scheduled stop: the stream is still going, the DVR is not.
        let report = dvr.tick(3_000).unwrap();
        assert_eq!(report.completed, vec![id]);
        assert!(dvr.active_ids().is_empty());
        assert_eq!(state_of(&db, id), RecordingState::Completed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_recording_that_produces_nothing_fails_and_leaves_no_file() {
        let db = seeded_db();
        let dir = tempdir("empty");
        let id = schedule_at(&db, "Dead Air", 1_000);
        let dvr = Dvr::new(
            Arc::clone(&db),
            Arc::new(FakeRecorder::default()),
            dir.clone(),
        );

        dvr.tick(1_000).unwrap();
        // An empty body ends at once, but "at once" is still another thread: tick until
        // the reap sees it, rather than sleeping a guessed interval.
        let mut failed = Vec::new();
        wait_until(|| {
            failed = dvr.tick(1_005).unwrap().failed;
            !failed.is_empty()
        });

        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].0, id);
        assert_eq!(state_of(&db, id), RecordingState::Failed);
        // Reported once: a later tick has nothing left to say about it.
        assert!(dvr.tick(1_010).unwrap().is_empty());
        // An empty file in the recordings folder is worse than no file.
        assert!(std::fs::read_dir(&dir)
            .map(|mut d| d.next().is_none())
            .unwrap_or(true));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_recorder_that_cannot_start_fails_the_recording_with_a_reason() {
        let db = seeded_db();
        let dir = tempdir("nostart");
        let id = schedule_at(&db, "Show", 1_000);
        let dvr = Dvr::new(
            Arc::clone(&db),
            Arc::new(FakeRecorder {
                fail_with: Some("HTTP 403".into()),
                ..Default::default()
            }),
            dir.clone(),
        );

        let report = dvr.tick(1_000).unwrap();
        assert!(report.started.is_empty());
        assert_eq!(report.failed.len(), 1);
        assert_eq!(state_of(&db, id), RecordingState::Failed);
        let reason = repo::get(&db.lock(), id).unwrap().unwrap().reason.unwrap();
        assert!(!reason.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_channel_with_no_source_fails_rather_than_panicking() {
        let db = seeded_db();
        db.lock()
            .execute("DELETE FROM channel_sources", [])
            .unwrap();
        let dir = tempdir("nosource");
        let id = schedule_at(&db, "Show", 1_000);
        let dvr = Dvr::new(
            Arc::clone(&db),
            Arc::new(FakeRecorder {
                body: vec![1u8; 10],
                ..Default::default()
            }),
            dir.clone(),
        );

        let report = dvr.tick(1_000).unwrap();
        assert_eq!(report.failed.len(), 1);
        assert_eq!(state_of(&db, id), RecordingState::Failed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ticking_twice_does_not_start_the_same_recording_again() {
        let db = seeded_db();
        let dir = tempdir("twice");
        schedule_at(&db, "Show", 1_000);
        let recorder = Arc::new(EndlessRecorder);
        let dvr = Dvr::new(Arc::clone(&db), recorder, dir.clone());

        assert_eq!(dvr.tick(1_000).unwrap().started.len(), 1);
        assert!(dvr.tick(1_010).unwrap().started.is_empty());
        assert_eq!(dvr.active_ids().len(), 1);

        dvr.shutdown(1_020);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_concurrency_limit_skips_the_lowest_priority_recording() {
        let db = seeded_db();
        let dir = tempdir("limit");
        let a = schedule_at(&db, "A", 1_000);
        let b = schedule_at(&db, "B", 1_000);
        // Same airtime, different titles, so both are due at once.
        {
            let conn = db.lock();
            conn.execute("UPDATE recordings SET priority = 5 WHERE id = ?1", [a])
                .unwrap();
        }

        let dvr = Dvr::new(Arc::clone(&db), Arc::new(EndlessRecorder), dir.clone())
            .with_max_concurrent(1);
        let report = dvr.tick(1_000).unwrap();

        assert_eq!(report.started, vec![a], "the higher priority one records");
        assert_eq!(report.failed.len(), 1);
        assert_eq!(state_of(&db, b), RecordingState::Skipped);

        dvr.shutdown(1_010);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_recording_missed_while_aurora_was_closed_is_marked_failed() {
        let db = seeded_db();
        let dir = tempdir("missed");
        let id = schedule_at(&db, "Last Night", 1_000);
        let dvr = Dvr::new(
            Arc::clone(&db),
            Arc::new(FakeRecorder::default()),
            dir.clone(),
        );

        // Launched the next morning: the window is long gone.
        let report = dvr.tick(100_000).unwrap();
        assert!(report.started.is_empty());
        assert_eq!(report.failed.len(), 1);
        assert_eq!(state_of(&db, id), RecordingState::Failed);
        assert!(repo::get(&db.lock(), id)
            .unwrap()
            .unwrap()
            .reason
            .unwrap()
            .contains("not running"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tick_with_nothing_to_do_reports_nothing() {
        let db = seeded_db();
        let dir = tempdir("idle");
        let dvr = Dvr::new(
            Arc::clone(&db),
            Arc::new(FakeRecorder::default()),
            dir.clone(),
        );
        assert!(dvr.tick(1_000).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shutdown_finalises_a_recording_in_flight() {
        let db = seeded_db();
        let dir = tempdir("shutdown");
        let id = schedule_at(&db, "Show", 1_000);
        let dvr = Dvr::new(Arc::clone(&db), Arc::new(EndlessRecorder), dir.clone());

        dvr.tick(1_000).unwrap();
        wait_for_bytes(&dvr, id, 1);
        dvr.shutdown(1_050);

        assert!(dvr.active_ids().is_empty());
        let rec = repo::get(&db.lock(), id).unwrap().unwrap();
        // Closing the app mid-recording keeps what was captured.
        assert_eq!(rec.state, RecordingState::Completed);
        assert!(rec.bytes > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bytes_so_far_tracks_a_live_recording() {
        let db = seeded_db();
        let dir = tempdir("bytes");
        let id = schedule_at(&db, "Show", 1_000);
        let dvr = Dvr::new(Arc::clone(&db), Arc::new(EndlessRecorder), dir.clone());

        dvr.tick(1_000).unwrap();
        wait_for_bytes(&dvr, id, 1);
        assert!(dvr.bytes_so_far(id).unwrap_or(0) > 0);
        assert_eq!(dvr.bytes_so_far(999), None);

        dvr.shutdown(1_050);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
