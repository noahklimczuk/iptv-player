//! A refresh must record the instant it stamped its rows, not the instant it finished.
//!
//! `channels::list` shows the **Dropped** badge — "your provider no longer lists this
//! channel" — when `last_seen_at < providers.last_refresh_at`. Every row a refresh
//! touches is stamped with `options.now_unix`, taken before the download begins. So if
//! `last_refresh_at` is set from a second call to the clock, the comparison becomes
//! "stamped at the start < finished at the end", which is true of *every row in the
//! library*, and the whole channel list wears a badge saying the provider has dropped it.
//!
//! It did. On a panel with 22,121 channels and a guide, a refresh takes minutes, so the
//! gap was never subtle — it was every channel, every time.
//!
//! Checked as a property of the source rather than by running a refresh, like
//! `lock_discipline.rs` and `aurora-contract`'s `command_args.rs`, and for the same
//! reason: reproducing it needs a provider, a network and a clock that advances during
//! the import. What can be checked cheaply is the shape that causes it.

use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn source(name: &str) -> String {
    let path = crate_root().join("src").join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

/// Strip `//` comments, so the explanation of the bug is not read as the bug.
///
/// Over `Vec<char>` rather than byte indices: these files contain box-drawing characters
/// in their section rules, and slicing a `str` in the middle of one panics.
fn without_comments(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// The `UPDATE providers … SET last_refresh_at` statement, and what it binds.
#[test]
fn the_refresh_timestamp_is_the_one_the_rows_were_stamped_with() {
    let src = without_comments(&source("providers.rs"));
    let at = src
        .find("SET last_refresh_at")
        .expect("providers.rs no longer sets last_refresh_at; this test needs rewriting");

    // The parameters follow the SQL, within the same `db.execute` call. A generous window
    // rather than a parse: what matters is which clock appears, and both candidates are
    // short, distinctive and cannot appear for another reason inside it.
    let tail = &src[at..(at + 400).min(src.len())];
    let params = tail
        .find("params![")
        .map(|p| &tail[p..])
        .expect("the update's parameters");

    assert!(
        params.contains("options.now_unix"),
        "last_refresh_at must be bound to `options.now_unix` -- the instant every row \
         this refresh wrote was stamped with. Binding anything else makes \
         `last_seen_at < last_refresh_at` true for the whole library, and every channel \
         wears the Dropped badge. Found: {params}"
    );
    assert!(
        !params.contains("now_unix()"),
        "last_refresh_at is bound to a fresh call to the clock. That is the end of the \
         refresh, and the rows carry its beginning -- minutes earlier on a real panel. \
         Found: {params}"
    );
}

/// The stamp the rows get and the stamp the provider records have to be one value.
///
/// `sync::apply` hands `options.now_unix` to every upsert, and `channels::stale` -- the
/// count in the refresh report -- is passed the same thing. `channels::list` compares
/// against the stored column instead, and the comment on `stale` claims the two "compute
/// the same way". They can only do that while the column holds that same instant.
#[test]
fn the_report_and_the_badge_ask_the_same_question() {
    let sync = without_comments(&source("../../aurora-ingest/src/sync.rs"));
    assert!(
        sync.contains("channels::stale(db, options.provider_id, options.now_unix)"),
        "the refresh report's own staleness count no longer uses `options.now_unix`, so \
         it and the Dropped badge can now disagree"
    );
}
