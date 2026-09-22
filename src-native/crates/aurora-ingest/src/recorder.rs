//! Writing a live stream to disk (README §7.7).
//!
//! A recording is a long-lived HTTP read, which is a different animal from the fetches
//! in [`crate::http`]: the size cap and the 120-second request budget that protect a
//! playlist fetch would kill a two-hour recording at minute two. So this builds its own
//! client, with the recording's own window as the only ceiling.

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use aurora_core::neterr::NetFailure;

/// How much longer than its scheduled window a recording may run before the HTTP layer
/// gives up on it. Generous: the padding is meant to absorb overruns, this only stops a
/// wedged socket from holding a thread for ever.
const WINDOW_SLACK_SECS: u64 = 600;

/// Copy buffer. Large enough that a 20 Mb/s stream isn't doing thousands of tiny
/// writes, small enough that the stop flag is checked several times a second.
const CHUNK: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub struct RecordRequest {
    pub url: String,
    pub dest: PathBuf,
    /// Seconds the recording is scheduled to run, padding included.
    pub window_secs: u64,
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
}

/// How a recording ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub path: PathBuf,
    pub bytes: u64,
    /// `None` if the stream ended or was stopped cleanly.
    pub error: Option<String>,
}

impl Outcome {
    /// A recording that produced nothing is a failure even if the transport was happy:
    /// an empty file in the library is worse than an error the user can see.
    pub fn is_usable(&self) -> bool {
        self.bytes > 0
    }
}

/// Live counters for a recording in flight, readable while it runs.
#[derive(Debug, Default)]
pub struct Progress {
    bytes: AtomicU64,
    /// Unix seconds of the last successful write, for stall detection.
    last_write: AtomicI64,
}

impl Progress {
    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    pub fn last_write(&self) -> i64 {
        self.last_write.load(Ordering::Relaxed)
    }

    /// Whether nothing has arrived for `limit_secs`. The supervisor surfaces this; it
    /// cannot itself interrupt a blocked read.
    pub fn is_stalled(&self, now: i64, limit_secs: i64) -> bool {
        let last = self.last_write();
        last > 0 && now - last > limit_secs
    }
}

/// A recording in flight.
#[derive(Debug)]
pub struct Handle {
    stop: Arc<AtomicBool>,
    progress: Arc<Progress>,
    join: Option<JoinHandle<Outcome>>,
    path: PathBuf,
}

impl Handle {
    pub fn progress(&self) -> &Arc<Progress> {
        &self.progress
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Ask the copy loop to finish at the next chunk boundary.
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn is_finished(&self) -> bool {
        self.join.as_ref().is_some_and(|j| j.is_finished())
    }

    /// Cut the recording short and wait for the file to be flushed and closed. This is
    /// what the scheduled stop time calls.
    pub fn stop(self) -> Outcome {
        self.request_stop();
        self.wait()
    }

    /// Wait for the stream to end on its own, without asking it to stop. A provider
    /// that closes the connection when the programme ends finishes this way.
    pub fn wait(mut self) -> Outcome {
        match self.join.take() {
            Some(join) => join.join().unwrap_or_else(|_| Outcome {
                path: self.path.clone(),
                bytes: 0,
                // A panicked writer thread has already lost the recording; say so
                // rather than reporting a clean stop.
                error: Some("the recorder stopped unexpectedly".into()),
            }),
            None => Outcome {
                path: self.path.clone(),
                bytes: 0,
                error: Some("recording was already finished".into()),
            },
        }
    }
}

/// The seam the DVR schedules against. `StreamRecorder` is what ships; tests supply
/// their own so they never have to move real bytes.
pub trait Recorder: Send + Sync {
    fn start(&self, request: RecordRequest) -> Result<Handle, NetFailure>;
}

/// Records by streaming the provider's URL straight to a file.
///
/// No remux: providers serve MPEG-TS, which is designed to be cut anywhere, so a
/// recording interrupted by a crash or a dropped connection is still playable up to the
/// point it stopped. Remuxing to MP4 would leave an unplayable file instead.
#[derive(Debug, Default)]
pub struct StreamRecorder;

impl Recorder for StreamRecorder {
    fn start(&self, request: RecordRequest) -> Result<Handle, NetFailure> {
        if let Some(parent) = request.dest.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                NetFailure::classify(&format!("cannot create recording folder: {e}"))
            })?;
        }

        let client = build_client(&request)?;
        let response = client
            .get(&request.url)
            .send()
            .map_err(|e| NetFailure::classify(&e.to_string()))?;
        if !response.status().is_success() {
            return Err(NetFailure::classify(&format!(
                "HTTP {}",
                response.status().as_u16()
            )));
        }

        record_from_reader(response, &request.dest)
    }
}

/// Record from an arbitrary byte source rather than an HTTP response.
///
/// The HTTP path is just this with a socket on the front. Exposed so a caller can
/// record from something else — and so the scheduler can be tested end to end without
/// moving bytes over a network.
pub fn record_from_reader(
    source: impl Read + Send + 'static,
    dest: &Path,
) -> Result<Handle, NetFailure> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| NetFailure::classify(&format!("cannot create recording folder: {e}")))?;
    }
    let file = File::create(dest)
        .map_err(|e| NetFailure::classify(&format!("cannot open recording file: {e}")))?;

    let stop = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(Progress::default());
    let path = dest.to_path_buf();

    let join = std::thread::Builder::new()
        .name("aurora-recorder".into())
        .spawn({
            let stop = Arc::clone(&stop);
            let progress = Arc::clone(&progress);
            let path = path.clone();
            move || copy_to_disk(source, file, path, stop, progress)
        })
        .map_err(|e| NetFailure::classify(&format!("cannot start recorder: {e}")))?;

    Ok(Handle {
        stop,
        progress,
        join: Some(join),
        path,
    })
}

fn build_client(request: &RecordRequest) -> Result<reqwest::blocking::Client, NetFailure> {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(referrer) = &request.referrer {
        if let Ok(v) = reqwest::header::HeaderValue::from_str(referrer) {
            headers.insert(reqwest::header::REFERER, v);
        }
    }
    let mut builder = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        // The recording's own window, not a fetch budget. Without a ceiling at all, a
        // wedged socket would hold the thread until the process exits.
        .timeout(Duration::from_secs(request.window_secs + WINDOW_SLACK_SECS))
        .redirect(reqwest::redirect::Policy::limited(5))
        // Never gzip: this is a transport stream, and asking would only invite a
        // provider to hand back something the file can't be cut from.
        .no_gzip();
    if let Some(ua) = &request.user_agent {
        builder = builder.user_agent(ua.clone());
    }
    builder
        .build()
        .map_err(|e| NetFailure::classify(&e.to_string()))
}

fn copy_to_disk(
    mut source: impl Read,
    file: File,
    path: PathBuf,
    stop: Arc<AtomicBool>,
    progress: Arc<Progress>,
) -> Outcome {
    let mut writer = BufWriter::with_capacity(CHUNK, file);
    let mut buf = vec![0u8; CHUNK];
    let mut total: u64 = 0;
    let mut error = None;

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match source.read(&mut buf) {
            Ok(0) => break, // stream ended on its own
            Ok(n) => {
                if let Err(e) = writer.write_all(&buf[..n]) {
                    // A full disk mid-recording: keep what was written, report why.
                    error = Some(format!("cannot write to the recording file: {e}"));
                    break;
                }
                total += n as u64;
                progress.bytes.store(total, Ordering::Relaxed);
                progress.last_write.store(now_unix(), Ordering::Relaxed);
            }
            Err(e) => {
                error = Some(NetFailure::classify(&e.to_string()).message);
                break;
            }
        }
    }

    if let Err(e) = writer.flush() {
        error.get_or_insert(format!("cannot flush the recording file: {e}"));
    }

    Outcome {
        path,
        bytes: total,
        error,
    }
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
    use crate::testserver::{Reply, TestServer};

    fn request(url: String, dest: PathBuf) -> RecordRequest {
        RecordRequest {
            url,
            dest,
            window_secs: 30,
            user_agent: Some("AuroraTV/test".into()),
            referrer: None,
        }
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aurora-rec-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_stream_is_written_to_disk_verbatim() {
        let body = vec![7u8; 300_000]; // larger than one chunk
        let server = TestServer::always(Reply::ok(body.clone()));
        let dir = tempdir();
        let dest = dir.join("sub").join("show.ts");

        let handle = StreamRecorder
            .start(request(server.url("/live"), dest.clone()))
            .unwrap();
        let outcome = handle.wait();

        assert_eq!(outcome.error, None);
        assert_eq!(outcome.bytes, body.len() as u64);
        assert!(outcome.is_usable());
        // The folder did not exist beforehand.
        assert_eq!(fs::read(&dest).unwrap(), body);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_mid_stream_disconnect_keeps_what_was_recorded() {
        let server = TestServer::always(Reply::Truncated {
            announced: 100_000,
            send: vec![1u8; 40_000],
        });
        let dir = tempdir();
        let dest = dir.join("cut.ts");

        let outcome = StreamRecorder
            .start(request(server.url("/live"), dest.clone()))
            .unwrap()
            .wait();

        // The error is reported, and the partial file is still there: MPEG-TS cut
        // anywhere is playable up to the cut.
        assert!(
            outcome.error.is_some(),
            "a truncated stream must be reported"
        );
        assert_eq!(outcome.bytes, 40_000);
        assert!(outcome.is_usable());
        assert_eq!(fs::metadata(&dest).unwrap().len(), 40_000);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_http_error_never_creates_a_file() {
        let server = TestServer::always(Reply::status(403));
        let dir = tempdir();
        let dest = dir.join("denied.ts");

        let err = StreamRecorder
            .start(request(server.url("/live"), dest.clone()))
            .unwrap_err();

        assert!(!err.message.is_empty());
        assert!(
            !dest.exists(),
            "a refused stream must not leave an empty file"
        );
    }

    #[test]
    fn progress_is_readable_and_reports_bytes() {
        let server = TestServer::always(Reply::ok(vec![0u8; 50_000]));
        let dir = tempdir();
        let handle = StreamRecorder
            .start(request(server.url("/live"), dir.join("p.ts")))
            .unwrap();
        let progress = Arc::clone(handle.progress());
        let outcome = handle.wait();

        assert_eq!(progress.bytes(), outcome.bytes);
        assert_eq!(progress.bytes(), 50_000);
        assert!(progress.last_write() > 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stall_is_detectable_but_a_live_write_is_not_a_stall() {
        let progress = Progress::default();
        // Nothing written yet: not a stall, just not started.
        assert!(!progress.is_stalled(10_000, 30));

        progress.last_write.store(10_000, Ordering::Relaxed);
        assert!(!progress.is_stalled(10_020, 30));
        assert!(progress.is_stalled(10_100, 30));
    }

    #[test]
    fn a_recording_with_no_bytes_is_not_usable() {
        let outcome = Outcome {
            path: PathBuf::from("x.ts"),
            bytes: 0,
            error: None,
        };
        assert!(!outcome.is_usable());
    }

    #[test]
    fn the_user_agent_reaches_the_provider() {
        let server = TestServer::always(Reply::ok(b"x".to_vec()));
        let dir = tempdir();
        StreamRecorder
            .start(request(server.url("/live"), dir.join("ua.ts")))
            .unwrap()
            .wait();

        let requests = server.requests();
        assert_eq!(requests[0].header("user-agent"), Some("AuroraTV/test"));
        // A transport stream must never be asked for gzipped.
        assert_ne!(requests[0].header("accept-encoding"), Some("gzip"));
        let _ = fs::remove_dir_all(&dir);
    }
}
