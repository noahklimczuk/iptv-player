//! What the real guide costs to import, measured through the production path.
//!
//! `fetch_epg` used to push every batch the parser produced onto a `Vec<Vec<Programme>>`
//! and keep it, under a comment saying programmes "stream straight through to SQLite".
//! They cannot: `fetch` is handed no database, deliberately, so the fetch/apply split
//! can guarantee the download never holds the one connection. The review measured the
//! result by running the parser over this subscription's own documents and sampling the
//! process:
//!
//! ```text
//! the real 178 MB guide, 546,912 programmes held as Vec<Vec<Programme>>
//!    working set 262 MB      peak 263 MB
//! the real 51 MB playlist, 237,170 entries held as Vec<PlaylistEntry>
//!    working set 159 MB      peak 173 MB
//! ```
//!
//! `Fetched` holds both at once — about 420 MB of parsed data at the peak of a refresh —
//! against README §16's "Idle memory (library loaded, not playing) ≤ 500 MB", and §16
//! asks for the XMLTV import to be incremental against a document 2.8× this size
//! (R-64).
//!
//! This runs the same parse against the same panel and reports what it holds now. Both
//! numbers are printed either way, because the point is the measurement and not only
//! the threshold.
//!
//! ```text
//! set -a; . /c/aurora-dev/panel.env; set +a
//! cargo test -p aurora-ingest --release --test real_guide_memory -- --ignored --nocapture
//! ```
//!
//! `--release` matters: a debug build's allocator and `serde` paths are not what ships.

use aurora_core::model::{EpgChannel, Programme};
use aurora_ingest::epg::{self, BatchingSink, BATCH_SIZE};
use aurora_ingest::http::{HttpClient, HttpConfig};
use aurora_ingest::spill::{Spill, SpillWriter};
use aurora_ingest::xtream::XtreamClient;

/// What the old code held for this subscription, from the review. The assertion is
/// deliberately loose — this is a memory measurement on a machine doing other things —
/// but it has to be well under what collecting the guide cost.
const OLD_WORKING_SET_MB: u64 = 262;

fn panel() -> (String, String, String) {
    let var = |k: &str| {
        std::env::var(k).unwrap_or_else(|_| {
            panic!(
                "{k} is not set. Source the panel credentials first:\n  \
                 set -a; . /c/aurora-dev/panel.env; set +a"
            )
        })
    };
    (
        var("AURORA_PANEL_URL"),
        var("AURORA_PANEL_USER"),
        var("AURORA_PANEL_PASS"),
    )
}

/// This process's working set, in MB, asked of the OS rather than guessed at.
///
/// Through PowerShell because the alternative is a `windows` dependency in a test, and
/// what is wanted is one number twice. Returns `None` where that is not available, and
/// the test then reports the structure instead of the size.
#[cfg(windows)]
fn working_set_mb() -> Option<u64> {
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "[math]::Round((Get-Process -Id {}).WorkingSet64 / 1MB)",
                std::process::id()
            ),
        ])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

#[cfg(not(windows))]
fn working_set_mb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

#[test]
#[ignore = "real panel: downloads the whole guide. Run with --ignored --nocapture"]
fn the_real_guide_is_not_held_in_memory_while_it_is_parsed() {
    let (base, user, pass) = panel();
    let http = HttpClient::new(HttpConfig::default()).expect("an HTTP client");
    let url = XtreamClient::new(&http, &base, &user, &pass).xmltv_url();

    let before = working_set_mb();

    let mut channels: Vec<EpgChannel> = Vec::new();
    let mut spill = SpillWriter::new();
    let mut touched: Vec<String> = Vec::new();
    // The production sink, assembled exactly as `sync::fetch_epg` assembles it.
    let mut sink = BatchingSink::new(
        |chans: &[EpgChannel]| {
            channels.extend_from_slice(chans);
            Ok(())
        },
        |progs: &[Programme]| {
            touched.extend(progs.iter().map(|p| p.channel_id.clone()));
            spill.push(progs)
        },
        BATCH_SIZE,
    );

    let at = std::time::Instant::now();
    let stats = epg::fetch_into(&http, &url, &mut sink).expect("the guide downloads");
    sink.finish();
    assert!(sink.error.is_none(), "{:?}", sink.error);
    drop(sink);
    let parsed = at.elapsed();

    let spilled = spill.finish().expect("the spill finishes");
    let after = working_set_mb();

    touched.sort_unstable();
    touched.dedup();

    let bytes = match &spilled {
        Spill::Disk { path, .. } => std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        Spill::Memory(_) => 0,
    };
    println!(
        "guide: {} channels, {} programmes, parsed in {:.2}s",
        stats.channels,
        stats.programmes,
        parsed.as_secs_f64()
    );
    println!(
        "held: {} batches, {:.1} MB on disk, {} distinct channel ids in memory",
        spilled.batches(),
        bytes as f64 / (1024.0 * 1024.0),
        touched.len()
    );

    assert!(stats.programmes > 1000, "this guide is too small to measure");
    assert!(
        matches!(spilled, Spill::Disk { .. }),
        "the guide was held in memory, which is the thing this is about"
    );
    assert!(bytes > 0, "the spill file is empty");

    // The structural claim, which holds whatever the machine is doing: what stays in
    // memory is the channel list and one id per channel, not the programmes.
    assert!(
        touched.len() <= stats.programmes / 10,
        "{} distinct ids for {} programmes, so the ids are not being deduplicated",
        touched.len(),
        stats.programmes
    );

    if let (Some(before), Some(after)) = (before, after) {
        let grew = after.saturating_sub(before);
        println!("working set: {before} MB -> {after} MB (+{grew} MB)");
        assert!(
            grew < OLD_WORKING_SET_MB / 2,
            "parsing the guide grew the working set by {grew} MB; collecting it cost \
             {OLD_WORKING_SET_MB} MB, so this is not an improvement"
        );
    } else {
        println!("working set: not available on this host");
    }
}
