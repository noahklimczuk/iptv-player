//! Nothing may hold the database lock and then ask for something that takes it again.
//!
//! `services.db` is a `parking_lot::Mutex`. It is not reentrant, and locking it twice
//! from one thread is not an error that surfaces — it blocks forever, with the first
//! guard still held, so the database stays locked for every command issued afterwards.
//!
//! `assistant_status` did exactly that: it took the lock and then read `has_key` from
//! `gemini::key_available`, which goes `resolve_key` -> `enabled` -> `db.lock()`. It is
//! the first command the assistant screen issues, so opening the assistant hung on that
//! line and took the database with it. `gemini_status`, six hundred lines away in the
//! file that defines the gate, scopes its own lock in a block to avoid the same thing —
//! so the hazard was known and the discipline simply was not checked anywhere.
//!
//! Checked as a property of the source, like `aurora-contract`'s `command_args.rs`, for
//! the same reason those are: a deadlock cannot be asserted by calling the function,
//! because the test would hang rather than fail, and `Services::new` wants a real data
//! directory and a player backend. What can be checked cheaply is the shape that causes
//! it.

use std::path::{Path, PathBuf};

/// Functions that take `services.db.lock()` themselves, named the way another module
/// has to name them. Calling one of these while a guard is alive is the deadlock.
///
/// Qualified, because the name alone does not identify the function: `metadata.rs` has
/// its own `key_available`, and that one reads the credential store and never touches
/// the database. Matching bare names reported it as a deadlock it cannot have.
const RELOCKS_QUALIFIED: &[&str] = &[
    "gemini::key_available(",
    "gemini::resolve_key(",
    "gemini::enabled(",
];

/// The same three as `gemini.rs` itself writes them, where they need no path.
const RELOCKS_BARE: &[&str] = &["key_available(", "resolve_key(", "enabled("];

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir)
        .expect("the crate's src directory")
        .flatten()
    {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Comments blanked, line structure kept, so the prose explaining the hazard — including
/// this file and the paragraph above `resolve_key` — is not read as the hazard.
fn without_comments(source: &str) -> String {
    // Over `char`s rather than bytes: these files are full of box-drawing section
    // rules, and slicing two bytes at a time lands inside one.
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    let mut in_block = false;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if in_block {
            if c == '*' && next == Some('/') {
                in_block = false;
                out.push_str("  ");
                i += 2;
                continue;
            }
        } else if c == '/' && next == Some('*') {
            in_block = true;
            out.push_str("  ");
            i += 2;
            continue;
        } else if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                out.push(' ');
                i += 1;
            }
            continue;
        }
        out.push(if in_block && c != '\n' { ' ' } else { c });
        i += 1;
    }
    out
}

/// Every place a lock guard is still alive when one of `RELOCKS` is called.
///
/// Brace depth is enough to see it: a guard bound by `let x = ...db.lock()` is dropped
/// when the block that binds it closes, so a call is dangerous exactly while the depth
/// has not fallen back below the binding's.
fn offenders(source: &str, is_gemini: bool) -> Vec<String> {
    let needles: Vec<&str> = if is_gemini {
        RELOCKS_BARE.to_vec()
    } else {
        RELOCKS_QUALIFIED.to_vec()
    };
    let mut found = Vec::new();
    let mut depth: i32 = 0;
    // Depths at which a live `db.lock()` guard is currently bound.
    let mut guards: Vec<i32> = Vec::new();

    for (n, line) in without_comments(source).lines().enumerate() {
        // Leaving a block drops any guard bound at or below the depth we leave.
        let opens = line.matches('{').count() as i32;
        let closes = line.matches('}').count() as i32;

        if !guards.is_empty() {
            for needle in &needles {
                if line.contains(needle) {
                    found.push(format!("{}: {}", n + 1, line.trim()));
                }
            }
        }

        depth += opens;
        depth -= closes;
        guards.retain(|d| *d <= depth);

        // A binding, not a temporary: `let _ = db.lock()` or a bare `db.lock()` in an
        // expression statement does not keep a guard past the statement. `let db = ...`
        // does, and so does `let mut db = ...`.
        let binds = line.contains("db.lock()")
            && line.contains("let ")
            && !line.contains("let _ =")
            && !line.contains("let _db");
        if binds {
            guards.push(depth);
        }
    }
    found
}

#[test]
fn no_command_holds_the_database_lock_while_asking_for_the_key() {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);
    assert!(files.len() > 5, "found only {} sources", files.len());

    let mut all = Vec::new();
    let mut saw_gemini = false;
    for path in &files {
        let source = std::fs::read_to_string(path).expect("a readable source file");
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let is_gemini = name == "gemini.rs";
        saw_gemini |= is_gemini;
        for hit in offenders(&source, is_gemini) {
            all.push(format!("{name}:{hit}"));
        }
    }
    // The file that defines the gates has to be among the ones scanned, or the stricter
    // bare-name rule was never applied to anything.
    assert!(saw_gemini, "gemini.rs was not scanned");
    assert!(
        all.is_empty(),
        "these take the database lock and then call something that takes it again, \
         which blocks forever on a parking_lot mutex. Read the value before locking, \
         or scope the lock in a block as `gemini_status` does:\n  {}",
        all.join("\n  ")
    );
}

/// The scan finds the shape it is looking for, so the test above is not passing because
/// the pattern is unrecognisable (R-159).
#[test]
fn the_scan_catches_the_bug_it_was_written_for() {
    // `assistant_status` as it actually was.
    let guilty = r#"
pub fn assistant_status(services: State<'_, Services>) -> Result<AssistantStatus> {
    let db = services.db.lock();
    Ok(AssistantStatus {
        has_key: crate::gemini::key_available(&services),
        messages: history(&db, 1).len(),
    })
}
"#;
    assert_eq!(offenders(guilty, false).len(), 1, "missed the original bug");

    // And the fix: the same call, before the lock.
    let fixed = r#"
pub fn assistant_status(services: State<'_, Services>) -> Result<AssistantStatus> {
    let has_key = crate::gemini::key_available(&services);
    let db = services.db.lock();
    Ok(AssistantStatus { has_key, messages: history(&db, 1).len() })
}
"#;
    assert!(
        offenders(fixed, false).is_empty(),
        "the fix still reads as a hit"
    );

    // And `gemini_status`'s shape: the guard is scoped, so the later call is fine.
    let scoped = r#"
pub fn gemini_status(services: State<'_, Services>) -> Result<GeminiStatus> {
    let has_history = {
        let db = services.db.lock();
        !store::history(&db, 1).unwrap_or_default().is_empty()
    };
    Ok(GeminiStatus { has_key: key_available(&services), has_history })
}
"#;
    assert!(
        offenders(scoped, true).is_empty(),
        "a scoped lock must not be a hit"
    );

    // Prose about the hazard is not the hazard.
    let commented = r#"
pub fn fine(services: &Services) -> bool {
    let db = services.db.lock();
    // Do not call key_available(&services) here: it locks the database again.
    store::history(&db, 1).is_ok()
}
"#;
    assert!(
        offenders(commented, true).is_empty(),
        "a comment was read as a call"
    );

    // Inside `gemini.rs` the gates need no path, so the bare rule has to see them —
    // otherwise that file is the one place the check does not apply.
    let bare = r#"
pub fn something(services: &Services) -> bool {
    let db = services.db.lock();
    let _ = store::history(&db, 1);
    key_available(services)
}
"#;
    assert_eq!(
        offenders(bare, true).len(),
        1,
        "the bare rule caught nothing"
    );
    assert!(
        offenders(bare, false).is_empty(),
        "an unqualified name outside gemini.rs is somebody else's function"
    );
}
