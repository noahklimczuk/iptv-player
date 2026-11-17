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

/// `pub const NAME: u32 = N;` in `aurora-app/src/play/resolve.rs`, as N.
fn resolve_const(name: &str) -> u32 {
    let resolve = app_src("play/resolve.rs");
    between(&resolve, &format!("pub const {name}: u32 ="), ";")
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("{name} in play/resolve.rs is not a plain number"))
}

#[test]
fn the_live_buffer_is_the_one_windows_uses() {
    let windows = resolve_const("LIVE_BUFFER_SECS");
    assert_eq!(
        crate::playback::LIVE_CACHE_SECS,
        windows,
        "aurora-app buffers live TV for {windows}s; playback.rs LIVE_CACHE_SECS says otherwise"
    );
}

#[test]
fn the_vod_buffer_is_the_one_windows_uses() {
    let windows = resolve_const("FILM_BUFFER_SECS");
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
    // Windows' `What::Catchup` arm chooses the source a replay is built from. If it
    // changes which one (or which columns it reads), catchup.rs must follow.
    fn sql(block: &str) -> String {
        let from = block.find("SELECT").expect("a SELECT");
        block[from..]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
    // Anchored on the query itself: `What::Catchup` appears more than once there.
    const QUERY: &str = "SELECT c.name, c.catchup_mode";
    let resolve = app_src("play/resolve.rs");
    let windows = sql(&format!("{QUERY}{}", between(&resolve, QUERY, "LIMIT 1")));
    let ours = sql(between(
        include_str!("catchup.rs"),
        "// The channel's best source",
        "LIMIT 1",
    ));
    assert_eq!(
        ours, windows,
        "aurora-app play/resolve.rs picks a catch-up source differently now; update catchup.rs"
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

/// `pub const NAME: TYPE = EXPR;` in `aurora-app/src/series.rs`, as the text of EXPR.
fn series_const(name: &str) -> String {
    let series = app_src("series.rs");
    let decl = between(&series, &format!("pub const {name}:"), ";");
    decl.split_once('=').map(|(_, v)| v.trim().to_string()).unwrap_or_default()
}

#[test]
fn episode_listing_waits_and_batches_as_windows_does() {
    assert_eq!(series_const("SWEEP_BATCH"), crate::series::SWEEP_BATCH.to_string());
    assert_eq!(series_const("SWEEP_CONCURRENCY"), crate::series::SWEEP_CONCURRENCY.to_string());
    assert_eq!(series_const("LISTING_RETRY_AFTER_SECS"), "6 * 3600");
    assert_eq!(crate::series::LISTING_RETRY_AFTER_SECS, 6 * 3600);
    assert_eq!(
        series_const("LISTING_FRESH_FOR"),
        "std::time::Duration::from_secs(6 * 3600)"
    );
    assert_eq!(crate::series::LISTING_FRESH_FOR.as_secs(), 6 * 3600);
}
