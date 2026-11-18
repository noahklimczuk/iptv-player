//! The core's log, handed to the app to write into the system's.
//!
//! On Windows `aurora-app` installs a `tracing` subscriber that writes `aurora.log`
//! at `info` (`main.rs`), and Settings → Diagnostics exports that file. On Apple the
//! log is the unified system log, which is where mpv's lines already go. Until this,
//! nothing installed a subscriber, so every warning the shared crates raise (an import
//! that skipped rows, a guide that would not parse, a library started again) was
//! dropped. Now each line is formatted as Windows formats it and given to Swift, which
//! writes it with `Logger`; the Diagnostics export then finds it beside mpv's.
//!
//! The shared crates already redact every URL they log; `redact_log` runs again over
//! the whole export regardless.

use std::io::Write;
use std::sync::{Arc, OnceLock};

/// Where log lines go: the app's `Logger`.
#[uniffi::export(with_foreign)]
pub trait LogSink: Send + Sync {
    /// One formatted line. `level` is 0 error, 1 warn, 2 info.
    fn line(&self, level: u8, text: String);
}

static SINK: OnceLock<Arc<dyn LogSink>> = OnceLock::new();

/// Start sending the core's log to `sink`, at Windows' level (`info`). Only the first
/// call takes effect: a process has one subscriber.
#[uniffi::export]
pub fn install_log(sink: Arc<dyn LogSink>) {
    if SINK.set(sink).is_err() {
        return;
    }
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .without_time()
        .with_writer(|| LineWriter::default())
        .try_init();
}

/// One event's text, handed over when the formatter is done with it.
#[derive(Default)]
struct LineWriter {
    buf: Vec<u8>,
}

impl Write for LineWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for LineWriter {
    fn drop(&mut self) {
        let text = String::from_utf8_lossy(&self.buf);
        let text = text.trim_end();
        if text.is_empty() {
            return;
        }
        if let Some(sink) = SINK.get() {
            sink.line(level_of(text), text.to_string());
        }
    }
}

/// The level, from the formatted line (`" WARN aurora_ingest: ..."`).
fn level_of(text: &str) -> u8 {
    match text.split_whitespace().next() {
        Some("ERROR") => 0,
        Some("WARN") => 1,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Lines(Mutex<Vec<(u8, String)>>);

    impl LogSink for Lines {
        fn line(&self, level: u8, text: String) {
            self.0.lock().unwrap().push((level, text));
        }
    }

    #[test]
    fn the_cores_warnings_reach_the_app_and_its_chatter_does_not() {
        let lines = Arc::new(Lines::default());
        install_log(lines.clone());
        tracing::warn!("a guide that would not parse");
        tracing::debug!("not wanted");
        let got = lines.0.lock().unwrap().clone();
        let warn = got
            .iter()
            .find(|(_, t)| t.contains("a guide that would not parse"))
            .expect("the warning arrived");
        assert_eq!(warn.0, 1);
        assert!(!got.iter().any(|(_, t)| t.contains("not wanted")));
    }
}
