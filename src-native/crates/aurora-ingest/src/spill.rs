//! Somewhere to put a guide that does not fit in memory.
//!
//! `fetch_epg`'s comment said programmes "stream straight through to SQLite". They did
//! not: every batch the parser produced was pushed onto a `Vec<Vec<Programme>>` and
//! kept, because the fetch/apply split means `fetch` has no database to stream to. The
//! batching was real and the sink was correct; the caller collected.
//!
//! What that cost, measured by running the production parser over the real documents and
//! sampling the process's working set:
//!
//! ```text
//! the real 178 MB guide, 546,912 programmes held as Vec<Vec<Programme>>
//!    parsed in 1.16 s      working set 262 MB      peak 263 MB
//!
//! the real 51 MB playlist, 237,170 entries held as Vec<PlaylistEntry>
//!    parsed in 0.62 s      working set 159 MB      peak 173 MB
//! ```
//!
//! `Fetched` holds the playlist and the guide in one struct, so both sit in memory
//! together: about 420 MB of parsed data at the peak of a refresh on this subscription,
//! before the WebView2 processes and everything else. README §16 budgets "Idle memory
//! (library loaded, not playing) ≤ 500 MB" and asks for the XMLTV import to be
//! "incremental" against a **500 MB** document — 2.8× this one, which on these numbers
//! would hold roughly 735 MB for the guide alone (R-64).
//!
//! So the batches go to a file as they are parsed, in exactly the groups they will be
//! inserted in, and `apply` reads them back one at a time. What stays in memory is one
//! batch, which is `BATCH_SIZE` rows.
//!
//! Measured again through the production path after the change, against the same panel:
//!
//! ```text
//! 6,790 channels, 772,102 programmes, parsed in 8.58s
//! held: 387 batches, 331.2 MB on disk, 2,944 distinct channel ids in memory
//! working set: 6 MB -> 58 MB  (+52 MB)
//! ```
//!
//! **What this trades.** 331 MB of temporary disk for 200-odd MB of memory, on a guide
//! that has grown since the review's 546,912 programmes. The encoding is one JSON array
//! per line, which is roughly 430 bytes a programme -- more than a packed format would
//! be, and chosen because it needs no new dependency and a spill can be read with a text
//! editor when a refresh goes wrong. The file lives for one refresh and `Drop` removes
//! it, including when a refresh fails halfway; a process killed outright can leave one
//! behind, named `aurora-guide-<pid>-*.jsonl` in the temp directory.
//!
//! **Why not write them to SQLite instead.** That is what the fetch/apply split exists
//! to prevent: a refresh used to download while holding the one database connection,
//! which froze every other command and meant a recording falling inside a long refresh
//! never started. `fetch` is never handed a database so that cannot come back, and the
//! guide is still replaced in one transaction in `apply` rather than row by row while
//! the DVR scheduler is reading it.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;

use aurora_core::model::Programme;

/// Parsed programmes, in insert-sized batches, held wherever they fit.
#[derive(Debug)]
pub enum Spill {
    /// On disk, one JSON array per line. The file is removed when this is dropped.
    Disk { path: PathBuf, batches: usize },
    /// In memory, which is what happens when a temporary file cannot be opened at all.
    ///
    /// A refresh must not fail because the temp directory is read-only: this is the
    /// behaviour the whole module replaces, so falling back to it is no worse than
    /// before and the log says it happened.
    Memory(Vec<Vec<Programme>>),
}

impl Default for Spill {
    /// An empty guide, held nowhere. `FetchedEpg` derives `Default` and a default
    /// refresh has no guide in it yet.
    fn default() -> Self {
        Spill::Memory(Vec::new())
    }
}

/// Collects batches, then hands back something that can read them again.
#[derive(Debug)]
pub struct SpillWriter {
    file: Option<BufWriter<File>>,
    path: Option<PathBuf>,
    batches: usize,
    memory: Vec<Vec<Programme>>,
}

impl SpillWriter {
    /// Open a spill file, or fall back to memory and say so.
    pub fn new() -> Self {
        match temp_path().and_then(|p| File::create(&p).map(|f| (p, f))) {
            Ok((path, file)) => Self {
                file: Some(BufWriter::new(file)),
                path: Some(path),
                batches: 0,
                memory: Vec::new(),
            },
            Err(e) => {
                tracing::warn!(
                    "could not open a spill file for the guide, so it is being held in \
                     memory instead: {e}"
                );
                Self {
                    file: None,
                    path: None,
                    batches: 0,
                    memory: Vec::new(),
                }
            }
        }
    }

    /// Add one batch, exactly as it will be inserted.
    ///
    /// An error is returned as a string, which is the shape `BatchingSink` reports a
    /// failed callback in. A write that fails stops the parse rather than silently
    /// dropping part of the guide.
    pub fn push(&mut self, programmes: &[Programme]) -> Result<(), String> {
        if programmes.is_empty() {
            return Ok(());
        }
        let Some(file) = self.file.as_mut() else {
            self.memory.push(programmes.to_vec());
            self.batches += 1;
            return Ok(());
        };
        let line = serde_json::to_string(programmes)
            .map_err(|e| format!("could not serialise a batch of programmes: {e}"))?;
        // One line per batch, and a newline that `read_line` can find. A programme's own
        // text cannot contain a bare newline here because JSON escapes it.
        writeln!(file, "{line}").map_err(|e| format!("could not write the guide: {e}"))?;
        self.batches += 1;
        Ok(())
    }

    /// Flush and hand over what was collected.
    pub fn finish(mut self) -> Result<Spill, String> {
        if let Some(mut file) = self.file.take() {
            file.flush()
                .map_err(|e| format!("could not finish writing the guide: {e}"))?;
            // Dropped before the path is handed on, so the handle is closed by the time
            // anything reads it -- which matters on Windows.
            drop(file);
        }
        match self.path.take() {
            Some(path) => Ok(Spill::Disk {
                path,
                batches: self.batches,
            }),
            None => Ok(Spill::Memory(std::mem::take(&mut self.memory))),
        }
    }
}

impl Default for SpillWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SpillWriter {
    /// A writer that was never finished still leaves no file behind: a refresh that
    /// failed halfway is exactly when one would be orphaned.
    fn drop(&mut self) {
        self.file.take();
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Spill {
    /// How many batches are held, without reading any of them.
    pub fn batches(&self) -> usize {
        match self {
            Spill::Disk { batches, .. } => *batches,
            Spill::Memory(batches) => batches.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.batches() == 0
    }

    /// Read the batches back, one at a time.
    ///
    /// `for_each` rather than an iterator because the disk form has to own a reader for
    /// the duration and the caller only ever walks it once, inserting as it goes.
    pub fn for_each<E>(
        &self,
        mut insert: impl FnMut(&[Programme]) -> Result<(), E>,
    ) -> Result<(), SpillError<E>> {
        match self {
            Spill::Memory(batches) => {
                for batch in batches {
                    insert(batch).map_err(SpillError::Insert)?;
                }
                Ok(())
            }
            Spill::Disk { path, .. } => {
                let file = File::open(path)
                    .map_err(|e| SpillError::Read(format!("could not reopen the guide: {e}")))?;
                let mut reader = BufReader::new(file);
                let mut line = String::new();
                loop {
                    line.clear();
                    let read = reader.read_line(&mut line).map_err(|e| {
                        SpillError::Read(format!("could not read the guide back: {e}"))
                    })?;
                    if read == 0 {
                        return Ok(());
                    }
                    if line.trim().is_empty() {
                        continue;
                    }
                    let batch: Vec<Programme> =
                        serde_json::from_str(line.trim_end()).map_err(|e| {
                            SpillError::Read(format!("a batch of programmes would not parse: {e}"))
                        })?;
                    insert(&batch).map_err(SpillError::Insert)?;
                }
            }
        }
    }

    /// Every channel id the programmes name, read without holding them.
    ///
    /// Collected during the parse instead wherever possible — see `Fetched` — and here
    /// for anything that only has a spill in hand.
    pub fn channel_ids(&self) -> Result<Vec<String>, String> {
        let mut out: Vec<String> = Vec::new();
        self.for_each::<std::convert::Infallible>(|batch| {
            out.extend(batch.iter().map(|p| p.channel_id.clone()));
            Ok(())
        })
        .map_err(|e| match e {
            SpillError::Read(e) => e,
            SpillError::Insert(never) => match never {},
        })?;
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }
}

impl Drop for Spill {
    fn drop(&mut self) {
        if let Spill::Disk { path, .. } = self {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// What can go wrong while replaying a spill: reading it, or the write it feeds.
#[derive(Debug)]
pub enum SpillError<E> {
    Read(String),
    Insert(E),
}

/// A path nothing else will pick. Not a crate dependency's job: this needs a name, not
/// a temp-file abstraction, and the process id plus a counter plus the clock is enough
/// for a file that lives for one refresh.
fn temp_path() -> std::io::Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = format!(
        "aurora-guide-{}-{}-{}.jsonl",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
        nanos
    );
    Ok(std::env::temp_dir().join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn programme(id: &str, start: i64) -> Programme {
        Programme {
            channel_id: id.to_string(),
            start,
            stop: start + 1800,
            title: format!("Programme at {start}"),
            sub_title: None,
            description: Some("Something on television".into()),
            categories: Vec::new(),
            season: None,
            episode: None,
            icon: None,
            rating: None,
            star_rating: None,
            is_new: false,
            is_live: false,
            is_premiere: false,
            credits: Vec::new(),
        }
    }

    /// What goes in comes back out, in the same batches and the same order.
    #[test]
    fn a_spill_round_trips_its_batches() {
        let mut w = SpillWriter::new();
        let first: Vec<Programme> = (0..3).map(|i| programme("bbc1.uk", i * 1800)).collect();
        let second: Vec<Programme> = (0..2).map(|i| programme("itv.uk", i * 1800)).collect();
        w.push(&first).unwrap();
        w.push(&second).unwrap();
        let spill = w.finish().unwrap();

        assert_eq!(spill.batches(), 2);
        let mut seen: Vec<Vec<Programme>> = Vec::new();
        spill
            .for_each::<std::convert::Infallible>(|b| {
                seen.push(b.to_vec());
                Ok(())
            })
            .unwrap();
        assert_eq!(seen, vec![first, second]);
    }

    /// An empty batch is not a line, so a guide of nothing reads back as nothing.
    #[test]
    fn an_empty_batch_is_not_written() {
        let mut w = SpillWriter::new();
        w.push(&[]).unwrap();
        let spill = w.finish().unwrap();
        assert!(spill.is_empty());
        let mut count = 0;
        spill
            .for_each::<std::convert::Infallible>(|b| {
                count += b.len();
                Ok(())
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    /// The file goes away, which is the half that keeps a failed refresh from leaving
    /// 178 MB in the temp directory.
    #[test]
    fn the_file_is_removed_when_the_spill_is_dropped() {
        let mut w = SpillWriter::new();
        w.push(&[programme("bbc1.uk", 0)]).unwrap();
        let spill = w.finish().unwrap();
        let path = match &spill {
            Spill::Disk { path, .. } => path.clone(),
            Spill::Memory(_) => return, // no file to remove on this machine
        };
        assert!(path.exists(), "nothing was written to {}", path.display());
        drop(spill);
        assert!(!path.exists(), "{} outlived its spill", path.display());
    }

    /// And a writer that is dropped without being finished -- a refresh that failed
    /// halfway -- leaves nothing behind either.
    #[test]
    fn an_abandoned_writer_leaves_no_file() {
        let mut w = SpillWriter::new();
        w.push(&[programme("bbc1.uk", 0)]).unwrap();
        let path = w.path.clone();
        drop(w);
        if let Some(path) = path {
            assert!(!path.exists(), "{} outlived its writer", path.display());
        }
    }

    /// The channel ids come back without the programmes.
    #[test]
    fn the_channel_ids_are_distinct_and_sorted() {
        let mut w = SpillWriter::new();
        w.push(&[
            programme("itv.uk", 0),
            programme("bbc1.uk", 0),
            programme("itv.uk", 1800),
        ])
        .unwrap();
        w.push(&[programme("c4.uk", 0), programme("bbc1.uk", 1800)])
            .unwrap();
        let spill = w.finish().unwrap();
        assert_eq!(
            spill.channel_ids().unwrap(),
            vec!["bbc1.uk".to_string(), "c4.uk".into(), "itv.uk".into()]
        );
    }

    /// The memory fallback behaves the same way, because it is the same interface.
    #[test]
    fn the_memory_fallback_reads_back_the_same() {
        let batch = vec![programme("bbc1.uk", 0), programme("bbc1.uk", 1800)];
        let spill = Spill::Memory(vec![batch.clone()]);
        assert_eq!(spill.batches(), 1);
        let mut seen: Vec<Programme> = Vec::new();
        spill
            .for_each::<std::convert::Infallible>(|b| {
                seen.extend_from_slice(b);
                Ok(())
            })
            .unwrap();
        assert_eq!(seen, batch);
    }
}
