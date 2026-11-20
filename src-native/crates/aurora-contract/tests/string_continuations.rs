//! No string literal may carry a line's indentation in the middle of its text.
//!
//! A long message is split with a `\` at the end of the line, which swallows the newline
//! and the next line's leading whitespace. Lose the backslash and the two halves are
//! joined by a run of spaces instead: "a lock is opened with it, so without one nothing
//! `              `locked could be watched." That shipped in the parental-lock error a
//! viewer reads, and in eleven log lines and test messages beside it (A-08 in
//! `AUDIT/code-audit-2026-11.md` found the first one; a second pass found the rest).
//!
//! Checked as a property of the source, like `command_args.rs` beside it: the strings
//! are spread across every crate and most are only ever seen when something goes wrong.

use std::path::{Path, PathBuf};

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates")
        .to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("a readable directory") {
        let path = entry.expect("an entry").path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if path.is_dir() {
            // `examples` line their output up in columns on purpose.
            if name != "target" && name != "examples" {
                rust_files(&path, out);
            }
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
}

/// The ordinary string literals on one line, as char slices without their quotes.
/// Raw strings and char literals are skipped well enough for this purpose: a `'"'` would
/// open a false literal, so a quote right after an apostrophe is ignored.
fn literals(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' && chars.get(i + 1) == Some(&'/') {
            break;
        }
        if chars[i] == '"' && (i == 0 || chars[i - 1] != '\'') {
            let mut text = String::new();
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1;
                }
                if let Some(&c) = chars.get(i) {
                    text.push(c);
                }
                i += 1;
            }
            found.push(text);
        }
        i += 1;
    }
    found
}

/// Text, then eight or more spaces, then text: the shape a lost backslash leaves.
fn has_gap(text: &str) -> bool {
    let mut run = 0;
    let mut seen_text = false;
    for c in text.chars() {
        if c == ' ' {
            run += 1;
        } else {
            if seen_text && run >= 8 {
                return true;
            }
            seen_text = true;
            run = 0;
        }
    }
    false
}

#[test]
fn no_string_literal_has_indentation_in_the_middle() {
    let mut files = Vec::new();
    rust_files(&crates_dir(), &mut files);
    assert!(files.len() > 20, "found only {} sources", files.len());

    let mut hits = Vec::new();
    for path in &files {
        let source = std::fs::read_to_string(path).expect("a readable source");
        for (n, line) in source.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if literals(line).iter().any(|l| has_gap(l)) {
                hits.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "these strings join two lines with the second one's indentation; end the first \
         line with `\\` instead:\n  {}",
        hits.join("\n  ")
    );
}

/// The scan finds the shape it is for, so the test above is not passing on a pattern it
/// cannot recognise.
#[test]
fn the_scan_catches_the_bug_it_was_written_for() {
    let broken = r#"            "Set a parental PIN first: a lock is opened with it, so without one nothing              locked could be watched.""#;
    assert!(literals(broken).iter().any(|l| has_gap(l)));

    let fixed = r#"            "Set a parental PIN first: a lock is opened with it, so without one nothing \"#;
    assert!(!literals(fixed).iter().any(|l| has_gap(l)));
    // Leading indentation inside a literal is a choice, not this bug.
    assert!(!has_gap("        indented"));
}
