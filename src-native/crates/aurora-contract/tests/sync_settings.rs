//! Every setting key the code uses is classified for sync, and nothing else is.
//!
//! Sync carries settings from an allowlist (`aurora_core::sync_settings`), so a key left
//! out of it never leaves the device. That is the safe failure, but it is still a
//! failure: a new filter that should follow the viewer would quietly not. This makes the
//! omission loud instead, at the point where someone adds the key and knows the answer.
//!
//! It reads the sources as text. Setting keys are named as `const …KEY…: &str = "a.b"`
//! in every module that touches the `settings` repo; a key with no dot is a credential
//! name or a cache key of another table, not a setting.

use std::fs;
use std::path::{Path, PathBuf};

use aurora_core::sync_settings::{classify, SETTINGS};

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// `const NAME: &str = "value";` where NAME mentions KEY, from one file's text.
fn key_consts(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let line = line.strip_prefix("pub ").unwrap_or(line);
        let Some(rest) = line.strip_prefix("const ") else {
            continue;
        };
        let Some((name, rest)) = rest.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if !name.contains("KEY") {
            continue;
        }
        let Some(rest) = rest.trim().strip_prefix("&str") else {
            continue;
        };
        let Some(value) = rest
            .trim()
            .strip_prefix('=')
            .map(str::trim)
            .and_then(|v| v.strip_prefix('"'))
            .and_then(|v| v.split_once('"'))
            .map(|(v, _)| v)
        else {
            continue;
        };
        found.push((name.to_string(), value.to_string()));
    }
    found
}

/// Every setting key in the sources, with the file it came from.
fn setting_keys() -> Vec<(String, PathBuf)> {
    let mut files = Vec::new();
    rust_files(&crates_dir(), &mut files);
    let mut keys = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        // Only modules that use the settings repo name setting keys.
        if !text.contains("settings::") {
            continue;
        }
        for (_, value) in key_consts(&text) {
            if value.contains('.') {
                keys.push((value, file.clone()));
            }
        }
    }
    keys
}

#[test]
fn the_scan_finds_the_keys_it_should() {
    // Guards the guard: a parser that found nothing would pass the test below forever.
    let keys: Vec<String> = setting_keys().into_iter().map(|(k, _)| k).collect();
    for expected in [
        "timeshift.folder",
        "filter.english_only",
        "credentials.namespace",
    ] {
        assert!(
            keys.iter().any(|k| k == expected),
            "the scan did not find {expected}; found {keys:?}"
        );
    }
}

#[test]
fn every_setting_key_is_classified_for_sync() {
    let missing: Vec<String> = setting_keys()
        .into_iter()
        .filter(|(k, _)| classify(k).is_none())
        .map(|(k, f)| format!("{k} ({})", f.display()))
        .collect();
    assert!(
        missing.is_empty(),
        "these settings are not in aurora_core::sync_settings::SETTINGS. Add each one as \
         Personal (follows the viewer to every device) or Device (stays on this one):\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn every_classified_key_is_still_used() {
    let used: Vec<String> = setting_keys().into_iter().map(|(k, _)| k).collect();
    let stale: Vec<&str> = SETTINGS
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !used.iter().any(|u| u == k))
        .collect();
    assert!(
        stale.is_empty(),
        "these keys are classified but nothing uses them any more: {stale:?}"
    );
}
