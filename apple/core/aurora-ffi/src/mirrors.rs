//! The values this crate copies from `aurora-app`, checked against `aurora-app`.
//!
//! The Apple port does not link or edit the Windows host, so a few of its constants are
//! repeated here (see the crate docs). A repeated value is one that drifts: the first
//! draft of this crate gave live TV a 10-second buffer where Windows uses 8. These tests
//! read `aurora-app`'s sources as text — the same technique `aurora-contract` uses — and
//! fail when a copy no longer matches its original, naming both.

use std::path::{Path, PathBuf};

fn app_src(file: &str) -> String {
    // apple/core/aurora-ffi -> apple/core -> apple -> repo root
    let root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root")
        .to_path_buf();
    let path = root.join("src-native/crates/aurora-app/src").join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The text from `start` up to the first `end` after it.
fn between<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let from = text
        .find(start)
        .unwrap_or_else(|| panic!("`{start}` is gone from aurora-app; update this mirror"));
    let rest = &text[from + start.len()..];
    let to = rest
        .find(end)
        .unwrap_or_else(|| panic!("nothing ends `{start}` with `{end}`"));
    &rest[..to]
}

fn quoted(text: &str) -> Vec<String> {
    text.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

#[test]
fn the_live_buffer_is_the_one_windows_uses() {
    let window = app_src("window.rs");
    let body = between(
        &window,
        "pub fn cache_secs_for(is_live: bool) -> u32 {",
        "} else {",
    );
    let windows: u32 = body
        .split(|c: char| !c.is_ascii_digit())
        .find(|s| !s.is_empty())
        .and_then(|n| n.parse().ok())
        .expect("a number in the live branch of cache_secs_for");
    assert_eq!(
        crate::playback::LIVE_CACHE_SECS,
        windows,
        "aurora-app buffers live TV for {windows}s; playback.rs LIVE_CACHE_SECS says otherwise"
    );
}

#[test]
fn the_vod_buffer_is_the_one_windows_uses() {
    let window = app_src("window.rs");
    let body = between(
        &window,
        "pub fn cache_secs_for(is_live: bool) -> u32 {",
        "\n}",
    );
    let after_else = between(body, "} else {", "}");
    let windows: u32 = after_else
        .trim()
        .parse()
        .expect("a number in the VOD branch of cache_secs_for");
    assert_eq!(
        crate::vod::VOD_CACHE_SECS,
        windows,
        "aurora-app buffers films for {windows}s; vod.rs VOD_CACHE_SECS says otherwise"
    );
}

#[test]
fn the_free_guides_are_the_ones_windows_fetches() {
    let providers = app_src("providers.rs");
    let windows = quoted(between(
        &providers,
        "pub const FREE_GUIDES: &[&str] = &[",
        "];",
    ));
    let ours: Vec<String> = crate::providers::FREE_GUIDES
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        ours, windows,
        "providers.rs FREE_GUIDES differs from aurora-app's"
    );

    let key = quoted(between(
        &providers,
        "pub const FREE_GUIDES_KEY: &str =",
        ";",
    ));
    assert_eq!(key, vec![crate::providers::FREE_GUIDES_KEY.to_string()]);
}

/// The SQL that saves a provider, compared column for column, because a column added on
/// Windows and not here is a provider the two apps would store differently.
#[test]
fn a_provider_is_saved_with_the_same_columns() {
    let providers = app_src("providers.rs");
    let windows = between(&providers, "INSERT INTO providers (", ")");
    let ours = between(include_str!("providers.rs"), "INSERT INTO providers (", ")");
    let norm = |s: &str| s.split_whitespace().collect::<String>();
    assert_eq!(norm(ours), norm(windows));
}

#[test]
fn catch_up_picks_the_channel_source_windows_picks() {
    // `catchup_channel` chooses the source a replay is built from. If Windows changes
    // which one (or which columns it reads), catchup.rs must follow.
    fn sql(block: &str) -> String {
        let from = block.find("SELECT").expect("a SELECT");
        block[from..]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
    let window = app_src("window.rs");
    let windows = sql(between(&window, "pub fn catchup_channel(", "LIMIT 1"));
    let ours = sql(between(
        include_str!("catchup.rs"),
        "// The channel's best source",
        "LIMIT 1",
    ));
    assert_eq!(
        ours, windows,
        "aurora-app window.rs catchup_channel picks its source differently now; update catchup.rs"
    );
}

#[test]
fn a_provider_is_removed_the_way_windows_removes_one() {
    // Both delete the row (the schema cascades the rest) and then forget the password.
    let app = app_src("providers.rs");
    let windows = between(&app, "pub fn providers_delete(", "Ok(true)");
    let ours = between(
        include_str!("providers.rs"),
        "pub fn remove_provider(",
        "Ok(true)",
    );
    for needle in [
        "SELECT credential_ref FROM providers WHERE id = ?1",
        "DELETE FROM providers WHERE id = ?1",
        "credentials.delete(&key)",
    ] {
        assert!(
            windows.contains(needle),
            "aurora-app providers_delete no longer has `{needle}`; update remove_provider"
        );
        assert!(ours.contains(needle), "remove_provider lost `{needle}`");
    }
}

fn ui_src(file: &str) -> String {
    let root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root")
        .to_path_buf();
    let path = root.join("src-ui/src").join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

#[test]
fn a_new_profile_follows_the_windows_editors_rules() {
    // The editor's rules live in Windows' UI, not a shared crate: a kids profile
    // starts at an age limit, and a PIN is a fixed number of digits.
    let editor = ui_src("features/profiles/ProfilePicker.tsx");
    let kids = format!("maxAge: kids ? {} : null", crate::profiles::KIDS_MAX_AGE);
    assert!(
        editor.contains(&kids),
        "ProfilePicker.tsx no longer says `{kids}`; update profiles.rs KIDS_MAX_AGE"
    );
    let pin = format!("/^\\d{{{}}}$/", crate::profiles::PIN_DIGITS);
    assert!(
        editor.contains(&pin),
        "ProfilePicker.tsx no longer checks a PIN with `{pin}`; update profiles.rs PIN_DIGITS"
    );
}
