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

/// `pub const NAME: Duration = Duration::from_xxx(N);` or `const NAME: i64 = N;` in
/// `aurora-app/src/sync.rs`, as N in the unit given.
fn sync_const(decl: &str) -> u64 {
    let sync = app_src("sync.rs");
    let text = between(&sync, decl, ";");
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    digits
        .parse()
        .unwrap_or_else(|_| panic!("`{decl}` in sync.rs has no plain number: {text:?}"))
}

#[test]
fn sync_paces_and_waits_for_a_position_as_windows_does() {
    assert_eq!(
        crate::sync::TICK_SECS,
        sync_const("pub const TICK: Duration = Duration::from_secs("),
        "aurora-app's sync TICK changed; sync.rs TICK_SECS says otherwise"
    );
    assert_eq!(
        crate::sync::RESUME_BUDGET_MS,
        sync_const("pub const RESUME_BUDGET: Duration = Duration::from_millis("),
        "aurora-app's RESUME_BUDGET changed; sync.rs RESUME_BUDGET_MS says otherwise"
    );
    assert_eq!(
        crate::sync::RESUME_FRESH_SECS as u64,
        sync_const("const RESUME_FRESH_SECS: i64 ="),
        "aurora-app's RESUME_FRESH_SECS changed; sync.rs says otherwise"
    );
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
        "require_settings(",
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
    decl.split_once('=')
        .map(|(_, v)| v.trim().to_string())
        .unwrap_or_default()
}

#[test]
fn episode_listing_waits_and_batches_as_windows_does() {
    assert_eq!(
        series_const("SWEEP_BATCH"),
        crate::series::SWEEP_BATCH.to_string()
    );
    assert_eq!(
        series_const("SWEEP_CONCURRENCY"),
        crate::series::SWEEP_CONCURRENCY.to_string()
    );
    assert_eq!(series_const("LISTING_RETRY_AFTER_SECS"), "6 * 3600");
    assert_eq!(crate::series::LISTING_RETRY_AFTER_SECS, 6 * 3600);
    assert_eq!(
        series_const("LISTING_FRESH_FOR"),
        "std::time::Duration::from_secs(6 * 3600)"
    );
    assert_eq!(crate::series::LISTING_FRESH_FOR.as_secs(), 6 * 3600);
}

/// `const NAME: TYPE = EXPR;` (public or not) in an `aurora-app` source, as EXPR's text.
fn app_const(file: &str, name: &str) -> String {
    let src = app_src(file);
    let decl = between(&src, &format!("const {name}:"), ";");
    decl.split_once('=')
        .map(|(_, v)| v.trim().to_string())
        .unwrap_or_default()
}

#[test]
fn the_home_feed_pages_and_refreshes_as_windows_does() {
    use crate::home::*;
    assert_eq!(app_const("home.rs", "FIRST_PAGE"), FIRST_PAGE.to_string());
    assert_eq!(app_const("home.rs", "PAGE"), PAGE.to_string());
    assert_eq!(app_const("home.rs", "MAX_PAGE"), MAX_PAGE.to_string());
    assert_eq!(
        app_const("home.rs", "CONTINUE_LIMIT"),
        CONTINUE_LIMIT.to_string()
    );
    assert_eq!(
        app_const("home.rs", "FETCHES_PER_REFRESH"),
        FETCHES_PER_REFRESH.to_string()
    );
    assert_eq!(app_const("home.rs", "TRENDING_SHOWN_FOR"), "4 * 86_400");
    assert_eq!(TRENDING_SHOWN_FOR, 4 * 86_400);
    assert_eq!(app_const("home.rs", "TRENDING_REFRESH"), "86_400");
    assert_eq!(TRENDING_REFRESH, 86_400);
    assert_eq!(app_const("home.rs", "ALSO_LIKED_REFRESH"), "14 * 86_400");
    assert_eq!(ALSO_LIKED_REFRESH, 14 * 86_400);
}

/// The JavaScript list in `export const NAME = [a, b, c] as const;`, as numbers.
fn ui_numbers(file: &str, name: &str) -> Vec<f64> {
    let src = ui_src(file);
    between(&src, &format!("export const {name} = ["), "]")
        .split(',')
        .map(|n| {
            n.trim()
                .parse()
                .unwrap_or_else(|_| panic!("{name} in {file} is not a list of numbers"))
        })
        .collect()
}

/// `export const NAME = N;` in a Windows UI source, as N.
fn ui_number(file: &str, name: &str) -> String {
    between(&ui_src(file), &format!("export const {name} ="), ";")
        .trim()
        .to_string()
}

#[test]
fn the_player_menus_offer_what_windows_offers() {
    use crate::player::*;
    const MODEL: &str = "features/player/model.ts";
    assert_eq!(
        ui_numbers(MODEL, "SPEEDS"),
        SPEEDS.to_vec(),
        "model.ts SPEEDS"
    );
    let sleep: Vec<f64> = SLEEP_MINUTES.iter().map(|&m| f64::from(m)).collect();
    assert_eq!(
        ui_numbers(MODEL, "SLEEP_MINUTES"),
        sleep,
        "model.ts SLEEP_MINUTES"
    );
    assert_eq!(
        ui_number(MODEL, "SLEEP_EXTEND_MINUTES"),
        SLEEP_EXTEND_MINUTES.to_string()
    );
    assert_eq!(ui_number(MODEL, "HIDE_AFTER_MS"), HIDE_AFTER_MS.to_string());
    assert_eq!(
        ui_number("features/player/UpNextCard.tsx", "COUNTDOWN_SECS"),
        UP_NEXT_COUNTDOWN_SECS.to_string()
    );
    assert_eq!(
        app_const("commands.rs", "UP_NEXT_TAIL_SECS"),
        format!("{UP_NEXT_TAIL_SECS:.1}")
    );
    // The Aspect menu: Windows' words, in Windows' order.
    let model = ui_src(MODEL);
    let windows = between(&model, "export const ASPECTS", "\n];");
    let mut at = 0;
    for (_, label, detail) in ASPECTS {
        let line = format!("label: '{label}', detail: '{detail}'");
        let found = windows[at..].find(&line).unwrap_or_else(|| {
            panic!("model.ts ASPECTS has no `{line}` here; update player.rs ASPECTS")
        });
        at += found + line.len();
    }
}

#[test]
fn the_sleep_timer_keeps_windows_limits() {
    assert_eq!(
        app_const("play/sleep.rs", "WARNING_SECS"),
        crate::sleep::WARNING_SECS.to_string()
    );
    assert_eq!(app_const("play/sleep.rs", "MAX_MINUTES"), "12 * 60");
    assert_eq!(crate::sleep::MAX_MINUTES, 12 * 60);
}

#[test]
fn a_typed_channel_number_is_read_as_windows_reads_it() {
    // `channel_number_from_typed` (playlist.rs) repeats `numberPatch`: empty clears,
    // ASCII digits only, anything else sends nothing.
    let windows = ui_src("features/playlist/number.ts");
    for needle in [
        "if (text === '') return { number: 0 };",
        "/^[0-9]+$/",
        "return undefined;",
    ] {
        assert!(
            windows.contains(needle),
            "number.ts no longer has `{needle}`; update channel_number_from_typed"
        );
    }
}

#[test]
fn zapping_follows_windows_zapper() {
    // `zap.rs` repeats `useZapper`: the digit timeout, the four-digit buffer, the step
    // rule, and the list that up and down walk.
    let zapper = ui_src("hooks/useZapper.ts");
    let timeout = format!("const DIGIT_TIMEOUT_MS = {};", crate::zap::DIGIT_TIMEOUT_MS);
    assert!(
        zapper.contains(&timeout),
        "useZapper.ts no longer has `{timeout}`; update zap.rs"
    );
    let ui = ui_src("state/ui.ts");
    let buffer = format!("(s.digits + d).slice(0, {})", crate::zap::MAX_DIGITS);
    assert!(
        ui.contains(&buffer),
        "ui.ts no longer has `{buffer}`; update zap.rs"
    );
    let step = ui_src("hooks/zap.ts");
    for needle in [
        "if (current < 0) return delta === 1 ? 0 : count - 1;",
        "return (current + delta + count) % count;",
    ] {
        assert!(
            step.contains(needle),
            "zap.ts no longer has `{needle}`; update step_index"
        );
    }
    let app = ui_src("App.tsx");
    for needle in [
        "useCommand('channels.list', {}, [])",
        "useZapper(channels ?? []",
    ] {
        assert!(
            app.contains(needle),
            "App.tsx no longer has `{needle}`; update channel_step"
        );
    }
}

#[test]
fn a_provider_is_checked_and_edited_the_way_windows_does_it() {
    let app = app_src("providers.rs");
    let ours = include_str!("provider_edit.rs");
    // Which failures earn a second look, and which mean the panel is refusing us: the
    // same lists, or a check here asks a throttling panel again where Windows stops.
    for (name, list) in [
        (
            "fn worth_probing(",
            "ErrorCode::Timeout | ErrorCode::Refused | ErrorCode::Dns | ErrorCode::Tls",
        ),
        (
            "fn is_refusing_us(",
            "ErrorCode::Dropped | ErrorCode::RateLimited | ErrorCode::ConnectionLimit",
        ),
    ] {
        let squash = |t: &str| t.split_whitespace().collect::<String>();
        assert!(
            squash(between(&app, name, "}")).contains(&squash(list)),
            "aurora-app `{name}` no longer matches {list}; update provider_edit.rs"
        );
        assert!(squash(between(ours, name, "}")).contains(&squash(list)));
    }
    // The playlist address offered when the API refuses.
    let format = "{}/get.php?username={}&password={}&type=m3u_plus&output=ts";
    assert!(
        app.contains(format),
        "aurora-app builds the playlist address differently now"
    );
    assert!(ours.contains(format));
    // The password rule: None keeps, "" clears, anything else replaces.
    assert!(
        app.contains("Some(p) if p.is_empty() =>"),
        "providers_update's password rule changed"
    );
    assert!(
        app.contains("\"UPDATE providers SET name = ?2, kind = ?3, base_url = ?4, username = ?5")
    );
}

#[test]
fn a_typed_address_is_asked_about_when_windows_asks() {
    let theirs = between(
        &ui_src("features/setup/SetupWizard.tsx"),
        "const DETECT_AFTER_TYPING_MS =",
        ";",
    )
    .trim()
    .to_string();
    assert_eq!(
        theirs,
        crate::provider_edit::DETECT_AFTER_TYPING_MS.to_string()
    );
}

#[test]
fn pause_live_tv_is_stored_and_defaulted_as_windows_does_it() {
    use crate::timeshift::*;
    let theirs = app_src("play/timeshift.rs");
    for (name, ours) in [
        ("ENABLED_KEY", ENABLED_KEY),
        ("BYTES_KEY", BYTES_KEY),
        ("SECS_KEY", SECS_KEY),
    ] {
        let value = between(&theirs, &format!("pub const {name}: &str ="), ";")
            .trim()
            .to_string();
        assert_eq!(
            value,
            format!("{ours:?}"),
            "play/timeshift.rs {name} is {value}; timeshift.rs says {ours:?}"
        );
    }
    let on = between(&theirs, "pub const ENABLED_BY_DEFAULT: bool =", ";")
        .trim()
        .to_string();
    assert_eq!(on, ENABLED_BY_DEFAULT.to_string());
    // The leftover rule: files in the folder, and mpv's own never among them.
    assert!(theirs.contains("m.is_file().then(|| (e.path(), m.len()))"));
}

#[test]
fn back_to_live_and_the_settings_screen_offer_what_windows_offers() {
    // `rewind_live_target` is `Seek::Live` with the same margin.
    let engine = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap()
            .join("src-native/crates/aurora-video/src/engine.rs"),
    )
    .unwrap();
    assert!(engine.contains("Seek::Live => (w.live_secs - LIVE_MARGIN_SECS).max(w.start_secs)"));
    let panel = ui_src("features/settings/TimeshiftPanel.tsx");
    let sizes: Vec<u64> = between(&panel, "const SIZES = [", "]")
        .split(',')
        .map(|n| n.trim().parse().unwrap())
        .collect();
    let minutes: Vec<u32> = between(&panel, "const MINUTES = [", "]")
        .split(',')
        .map(|n| n.trim().parse().unwrap())
        .collect();
    assert_eq!(sizes, crate::timeshift::BUFFER_SIZES_GB);
    assert_eq!(minutes, crate::timeshift::REWIND_LIMITS_MIN);
    // The live edge `behind_secs` uses.
    assert_eq!(ui_number("features/player/model.ts", "LIVE_EDGE_SECS"), "5");
    assert_eq!(aurora_core::timeshift::LIVE_EDGE_SECS, 5.0);
}

#[test]
fn about_states_the_licence_and_releases_page_windows_states() {
    let logging = app_src("logging.rs");
    assert!(
        logging.contains(&format!(
            "license: \"{}\".to_string()",
            crate::diagnostics::LICENSE
        )),
        "aurora-app app_about's licence changed; update diagnostics::LICENSE"
    );
    let updates = app_src("updates.rs");
    assert!(
        updates.contains(&format!(
            "pub const RELEASES_URL: &str = \"{}\";",
            crate::diagnostics::RELEASES_URL
        )),
        "aurora-app RELEASES_URL changed; update diagnostics::RELEASES_URL"
    );
}

#[test]
fn gemini_keeps_its_key_cache_and_switch_where_windows_does() {
    use crate::gemini::*;
    for (name, ours) in [
        ("CREDENTIAL_KEY", CREDENTIAL_KEY),
        ("CACHE_KEY", CACHE_KEY),
        ("ENABLED_KEY", ENABLED_KEY),
    ] {
        assert_eq!(app_const("gemini.rs", name), format!("{ours:?}"), "{name}");
    }
    assert_eq!(app_const("gemini.rs", "FRESH_FOR_SECS"), "6 * 60 * 60");
    assert_eq!(FRESH_FOR_SECS, 6 * 60 * 60);
    assert_eq!(
        app_const("gemini.rs", "HISTORY_FOR_PROMPT"),
        HISTORY_FOR_PROMPT.to_string()
    );
    assert_eq!(
        app_const("gemini.rs", "DEFAULT_ENABLED"),
        DEFAULT_ENABLED.to_string()
    );
    assert_eq!(app_const("library.rs", "RAIL_SIZE"), RAIL_SIZE.to_string());
    // The fingerprint is Windows' own, so a cache written by either reads the same.
    let windows = app_src("gemini.rs");
    for line in [
        "let mut hash: u64 = 0xcbf2_9ce4_8422_2325;",
        "hash = hash.wrapping_mul(0x100_0000_01b3);",
        "eat(&((w.watched * 100.0).round() as i64).to_le_bytes());",
    ] {
        assert!(
            windows.contains(line),
            "aurora-app gemini.rs no longer has `{line}`"
        );
        assert!(
            include_str!("gemini.rs").contains(line),
            "gemini.rs lost `{line}`"
        );
    }
}

#[test]
fn the_assistant_is_windows_assistant() {
    // Everything above the Tauri commands is copied; compare it whole, less the lines
    // the copy had to change (the module note, the imports and the error type).
    let windows = app_src("assistant.rs");
    let theirs = &windows[..windows
        .find("/* ── Commands")
        .expect("aurora-app assistant.rs has no Commands section")];
    let ours = include_str!("assistant.rs");
    let ours = &ours[..ours
        .find("/* ── The Apple core's commands")
        .expect("assistant.rs lost its exports")];
    let normal = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|l| !l.starts_with("//!"))
            .filter(|l| !l.starts_with("use crate::"))
            .map(|l| {
                l.replace(", uniffi::Record)]", ")]")
                    .replace(
                        "return Err(AuroraError::other(\"Ask me something first.\"));",
                        "return Err(AppError::Other(\"Ask me something first.\".into()));",
                    )
                    .replace(
                        "return Err(AuroraError::Network { message: why });",
                        "return Err(AppError::Other(why));",
                    )
            })
            .collect()
    };
    let (theirs, ours) = (normal(theirs), normal(ours));
    for (n, (a, b)) in theirs.iter().zip(&ours).enumerate() {
        assert_eq!(
            a,
            b,
            "assistant.rs differs from aurora-app's at line {}",
            n + 1
        );
    }
    assert_eq!(
        theirs.len(),
        ours.len(),
        "assistant.rs and aurora-app's differ in length"
    );
}

#[test]
fn the_seek_preview_is_windows_seek_preview() {
    // From the constants to `clock_at`, and the tests, copied whole; the PNG step and
    // its test are Windows' alone (the pictures stay in Swift here).
    let windows = app_src("play/preview.rs");
    let ours = include_str!("preview.rs");
    let theirs_body = between(
        &windows,
        "use super::resolve::What;",
        "/// A frame as a PNG",
    );
    let ours_body = between(
        ours,
        "Catchup { channel: i64, start: i64, stop: i64 },\n}",
        "/* ── The Apple core's commands",
    );
    let theirs_tests = between(
        &windows,
        "#[cfg(test)]",
        "    #[test]\n    fn a_frame_becomes_a_png",
    );
    let ours_tests = &ours[ours
        .find("#[cfg(test)]")
        .expect("preview.rs lost its tests")
        + "#[cfg(test)]".len()..];
    let lines = |text: &str| -> Vec<String> {
        text.lines()
            .map(|l| l.replace(", uniffi::Record)]", ")]"))
            .filter(|l| !l.trim().is_empty())
            .collect()
    };
    let theirs: Vec<String> = lines(theirs_body)
        .into_iter()
        .chain(lines(theirs_tests))
        .collect();
    let mut ours: Vec<String> = lines(ours_body)
        .into_iter()
        .chain(lines(ours_tests))
        .collect();
    // Windows' tests end where its PNG test begins; ours close the module there.
    assert_eq!(ours.pop().as_deref(), Some("}"));
    for (n, (a, b)) in theirs.iter().zip(&ours).enumerate() {
        assert_eq!(
            a,
            b,
            "preview.rs differs from aurora-app's at line {}",
            n + 1
        );
    }
    assert_eq!(
        theirs.len(),
        ours.len(),
        "preview.rs and aurora-app's differ in length"
    );
}

/* ── Parental controls (F-36 to F-38) ─────────────────────────────────────── */

#[test]
fn a_parental_refusal_says_what_windows_says() {
    use crate::error::Refusal;
    let errors = app_src("error.rs");
    for (refusal, variant) in [
        (Refusal::NotAllowed, "NotAllowed,"),
        (Refusal::Locked, "Locked,"),
        (Refusal::TimeUp, "TimeUp,"),
        (Refusal::PinRequired, "ParentalPinRequired,"),
    ] {
        // `#[error("…")]` sits just above the variant it words.
        let at = errors
            .find(&format!("    {variant}"))
            .unwrap_or_else(|| panic!("AppError::{variant} is gone from aurora-app"));
        let attr = errors[..at]
            .rfind("#[error(")
            .expect("an #[error] above it");
        let words = quoted(&errors[attr..at]).concat();
        assert_eq!(
            refusal.message(),
            words,
            "AppError::{variant} changed its words"
        );
    }
}

/// Every Windows command that `require_settings` guards, and what guards the same thing
/// here. A command Windows starts guarding fails this until it is placed.
#[test]
fn settings_are_locked_where_windows_locks_them() {
    // (aurora-app source, its command, our file, our function or why there is none)
    const GUARDED: &[(&str, &str, &str, Option<&str>)] = &[
        (
            "gemini.rs",
            "gemini_set_enabled",
            "gemini.rs",
            Some("set_gemini_enabled"),
        ),
        (
            "gemini.rs",
            "gemini_set_key",
            "gemini.rs",
            Some("set_gemini_key"),
        ),
        (
            "metadata.rs",
            "metadata_set_key",
            "metadata.rs",
            Some("set_metadata_key"),
        ),
        // No concurrency setting, artwork cache or self-update on Apple.
        ("metadata.rs", "metadata_set_concurrency", "", None),
        ("metadata.rs", "artwork_clear", "", None),
        ("updates.rs", "updates_set_automatic", "", None),
        (
            "sync.rs",
            "sync_create_account",
            "sync.rs",
            Some("sync_create_account"),
        ),
        ("sync.rs", "sync_sign_in", "sync.rs", Some("sync_sign_in")),
        ("sync.rs", "sync_reset", "sync.rs", Some("sync_reset")),
        ("sync.rs", "sync_sign_out", "sync.rs", Some("sync_sign_out")),
        (
            "sync.rs",
            "sync_recovery_code",
            "sync.rs",
            Some("sync_recovery_code"),
        ),
        (
            "providers.rs",
            "providers_save",
            "providers.rs",
            Some("save_provider"),
        ),
        (
            "providers.rs",
            "providers_credentials",
            "provider_edit.rs",
            Some("provider_settings"),
        ),
        (
            "providers.rs",
            "providers_update",
            "provider_edit.rs",
            Some("update_provider"),
        ),
        (
            "providers.rs",
            "providers_delete",
            "providers.rs",
            Some("remove_provider"),
        ),
        (
            "playlist.rs",
            "library_set_filters",
            "filters.rs",
            Some("set_library_filters"),
        ),
        (
            "playlist.rs",
            "playlist_update",
            "playlist.rs",
            Some("playlist_update"),
        ),
        (
            "playlist.rs",
            "playlist_set_hidden",
            "playlist.rs",
            Some("playlist_set_hidden"),
        ),
        (
            "playlist.rs",
            "playlist_hide_matching",
            "playlist.rs",
            Some("playlist_hide_matching"),
        ),
        (
            "playlist.rs",
            "playlist_reset",
            "playlist.rs",
            Some("playlist_reset"),
        ),
        (
            "play/commands.rs",
            "timeshift_set_settings",
            "timeshift.rs",
            Some("set_timeshift"),
        ),
        (
            "play/commands.rs",
            "timeshift_clear",
            "timeshift.rs",
            Some("empty_timeshift"),
        ),
    ];
    let ours = |file: &str| -> String {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(file);
        std::fs::read_to_string(root).unwrap()
    };
    /// The body of `fn name(` up to the next `fn `.
    fn body<'a>(text: &'a str, name: &str) -> Option<&'a str> {
        let from = text.find(&format!("fn {name}("))?;
        let rest = &text[from + 3..];
        Some(&rest[..rest.find("fn ").unwrap_or(rest.len())])
    }
    for &(app_file, command, our_file, our_fn) in GUARDED {
        let app = app_src(app_file);
        let theirs = body(&app, command)
            .unwrap_or_else(|| panic!("{command} is gone from aurora-app {app_file}"));
        assert!(
            theirs.contains("require_settings("),
            "aurora-app {command} no longer asks require_settings; update this list"
        );
        if let Some(our_fn) = our_fn {
            let text = ours(our_file);
            let mine = body(&text, our_fn).unwrap_or_else(|| panic!("no {our_fn} in {our_file}"));
            assert!(
                mine.contains("require_settings("),
                "{our_fn} must refuse while Settings are locked, as {command} does"
            );
        }
    }
    // And nothing Windows guards is missing from the list.
    for file in [
        "gemini.rs",
        "metadata.rs",
        "updates.rs",
        "sync.rs",
        "providers.rs",
        "playlist.rs",
        "play/commands.rs",
    ] {
        let app = app_src(file);
        for (i, _) in app.match_indices("require_settings(&services") {
            let head = &app[..i];
            let at = head.rfind("pub fn ").unwrap();
            let name: String = head[at + 7..]
                .chars()
                .take_while(|c| *c == '_' || c.is_ascii_alphanumeric())
                .collect();
            assert!(
                GUARDED.iter().any(|g| g.1 == name),
                "aurora-app {file} {name} now needs Settings unlocked; place it in GUARDED"
            );
        }
    }
}

#[test]
fn the_daily_limit_clock_is_windows_clock() {
    let app = app_src("parental.rs");
    let ours = include_str!("parental.rs");
    for decl in [
        "pub const FLUSH_SECS: f64 =",
        "pub const WARN_SECS: i64 =",
        "const MAX_GAP_SECS: f64 =",
    ] {
        assert_eq!(
            between(ours, decl, ";").trim(),
            between(&app, decl, ";").trim(),
            "aurora-app parental.rs `{decl}` changed"
        );
    }
    let strip = |s: &str| -> String { s.split_whitespace().collect::<Vec<_>>().join(" ") };
    for (start, end) in [
        ("pub fn sample(", "\n    }\n"),
        ("pub fn flush(", "\n    }\n"),
    ] {
        let theirs = strip(between(&app, start, end));
        let mine = strip(between(ours, start, end));
        assert_eq!(
            mine, theirs,
            "aurora-app ViewingClock `{start}` changed; copy it again"
        );
    }
}

#[test]
fn parental_changes_need_the_pin_where_windows_asks_for_it() {
    let profiles = app_src("profiles.rs");
    let parental = app_src("parental.rs");
    let ours_profiles = include_str!("profiles.rs");
    let ours_parental = include_str!("parental.rs");
    for (theirs, command, mine, ours) in [
        (
            &profiles,
            "pub fn profiles_create(",
            ours_profiles,
            "pub fn create_profile(",
        ),
        (
            &profiles,
            "pub fn profiles_delete(",
            ours_profiles,
            "pub fn delete_profile(",
        ),
        (
            &profiles,
            "pub fn profiles_set_limits(",
            ours_parental,
            "pub fn set_profile_limits(",
        ),
        (
            &profiles,
            "pub fn profiles_set_pin(",
            ours_profiles,
            "pub fn update_profile(",
        ),
        (
            &profiles,
            "pub fn profiles_set_parental(",
            ours_parental,
            "pub fn set_parental(",
        ),
        (
            &parental,
            "pub fn parental_set_lock(",
            ours_parental,
            "pub fn parental_set_lock(",
        ),
        (
            &parental,
            "pub fn parental_set_hide_locked(",
            ours_parental,
            "pub fn parental_set_hide_locked(",
        ),
    ] {
        assert!(
            between(theirs, command, "\n}\n").contains("require_parent("),
            "aurora-app `{command}` no longer asks require_parent"
        );
        assert!(
            between(mine, ours, "\n    }\n").contains("require_parent("),
            "`{ours}` must ask for the parental PIN, as `{command}` does"
        );
    }
}

/// Parental controls offers Windows' age and daily limits, with Windows' labels. The
/// Swift screen is not Rust, so it is read as text, as Windows' panel is.
#[test]
fn parental_controls_offer_windows_limits() {
    let panel = ui_src("features/settings/ParentalPanel.tsx");
    let swift = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../Shared/Features/ParentalControlsView.swift"),
    )
    .unwrap();
    for (ts, sw) in [
        ("const AGE_LIMITS", "private let ageLimits"),
        ("const DAILY_LIMITS", "private let dailyLimits"),
    ] {
        // The entries only, past the type: `= [` opens the list in both.
        let theirs = between(&panel, ts, "];");
        let theirs = &theirs[theirs.find("= [").unwrap() + 3..];
        let ours = between(&swift, sw, "]\n");
        let ours = &ours[ours.find("= [").unwrap() + 3..];
        // value, label pairs, in order: `{ value: 6, label: 'Up to PG' }` against
        // `(6, "Up to PG")`.
        let labels = |text: &str, q: char| -> Vec<String> {
            text.split(q)
                .skip(1)
                .step_by(2)
                .map(str::to_string)
                .collect()
        };
        assert_eq!(
            labels(ours, '"'),
            labels(theirs, '\''),
            "{ts} labels changed"
        );
        let values = |text: &str, key: &str| -> Vec<String> {
            text.split(key)
                .skip(1)
                .map(|v| {
                    v.trim_start()
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric())
                        .collect()
                })
                .collect()
        };
        let theirs_values: Vec<String> = values(theirs, "value:")
            .into_iter()
            .map(|v| if v == "null" { "nil".into() } else { v })
            .collect();
        assert_eq!(values(ours, "("), theirs_values, "{ts} values changed");
    }
}

/// The recordings scheduler keeps Windows' numbers, keys and reasons (`dvr.rs`).
#[test]
fn the_recorder_runs_on_windows_numbers() {
    let src = app_src("dvr.rs");
    let num = |name: &str| -> String {
        app_const("dvr.rs", name)
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '*')
            .collect()
    };
    assert_eq!(
        num("DEFAULT_MAX_CONCURRENT"),
        crate::dvr::DEFAULT_MAX_CONCURRENT.to_string()
    );
    assert_eq!(num("STALL_SECS"), crate::dvr::STALL_SECS.to_string());
    assert_eq!(
        num("TICK_INTERVAL"),
        crate::dvr::TICK_INTERVAL.as_secs().to_string()
    );
    assert_eq!(num("EXPANSION_HORIZON_SECS"), "14*86400");
    assert_eq!(crate::dvr::EXPANSION_HORIZON_SECS, 14 * 86_400);
    assert_eq!(
        app_const("dvr.rs", "FOLDER_KEY"),
        format!("{:?}", crate::dvr::FOLDER_KEY)
    );
    assert_eq!(
        app_const("dvr.rs", "QUOTA_KEY"),
        format!("{:?}", crate::dvr::QUOTA_KEY)
    );
    for reason in [
        crate::dvr::MISSED,
        crate::dvr::TOO_MANY,
        crate::dvr::NOTHING_SENT,
    ] {
        assert!(
            src.contains(&format!("{reason:?}")),
            "aurora-app no longer says {reason:?}"
        );
    }
    assert!(
        src.contains(&format!("priority: {},", crate::dvr::MANUAL_PRIORITY)),
        "a manual recording's priority changed in aurora-app"
    );
    assert!(
        src.contains(&format!(
            "lead_secs.unwrap_or({})",
            crate::dvr::REMINDER_LEAD_SECS
        )),
        "a reminder's lead changed in aurora-app"
    );
    let line_limit = between(&src, "pub fn line_limit", "\n}\n");
    let ours =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/dvr.rs")).unwrap();
    let mine = between(&ours, "pub(crate) fn line_limit", "\n}\n");
    assert_eq!(
        quoted(line_limit),
        quoted(mine),
        "line_limit asks the database something else on Windows"
    );
}

/// A recording that cannot play says what Windows says (`play/resolve.rs`).
#[test]
fn a_recording_that_cannot_play_says_what_windows_says() {
    let resolve = app_src("play/resolve.rs");
    let arm = between(&resolve, "What::Recording(id) => {", "Ok(Plan {");
    let ours =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/dvr.rs")).unwrap();
    let mine = between(&ours, "pub fn tune_recording", "Ok(VodTune {");
    let words = |text: &str| -> String {
        quoted(text)
            .join(" ")
            .replace("\\\n", "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    for sentence in [
        "This recording has not started yet, so there is nothing to play.",
        "This recording is still being written. It can be played once it finishes.",
        "This recording has no file on disk. The Recordings screen says why.",
        "The file for this recording is gone: {path}",
        "It may have been deleted to stay under the storage quota.",
    ] {
        assert!(
            words(arm).contains(sentence),
            "aurora-app no longer says {sentence:?}"
        );
        assert!(
            words(mine).contains(sentence),
            "tune_recording must say {sentence:?}"
        );
    }
}

/// The Recordings screen uses Windows' words: every sentence a viewer reads on
/// `RecordingsPage.tsx` appears in `RecordingsView.swift`.
#[test]
fn the_recordings_screen_says_what_windows_says() {
    let page = ui_src("features/dvr/RecordingsPage.tsx");
    let swift = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Shared/Features/RecordingsView.swift"),
    )
    .unwrap();
    for text in [
        "Recordings",
        "Recorded",
        "Scheduled",
        "Series rules",
        "Storage",
        "A recording clashes",
        "Nothing recorded yet",
        "Press Record on anything in the guide, or set up a series rule and Aurora will keep up with it for you.",
        "Coming up",
        "Nothing scheduled",
        "Recordings you set from the guide show up here, with a countdown and anything that clashes.",
        "No series rules",
        "A series rule records every episode of a show as it airs. Set one from the Record button in the guide.",
        "Cut short",
        "New",
        "Kept",
        "Allow deleting to free space",
        "Keep — never delete for space",
        "Delete",
        "Keep",
        "Play",
        "Series rule",
        "Once",
        "REC",
        "Recording",
        "Stop",
        "Cancel",
        "Skipped",
        "Failed",
        "Any channel",
        "One channel",
        " · New episodes only",
        "Pause",
        "Resume",
        "Delete rule",
        "Could not stop that recording",
        "Could not dismiss that recording",
        "Could not delete that rule",
        "Could not pause that rule",
        "Could not resume it",
        "That did not work",
    ] {
        assert!(page.contains(text), "RecordingsPage.tsx no longer says {text:?}");
        assert!(
            swift.contains(&format!("\"{text}")) || swift.contains(&format!("{text}\"")),
            "RecordingsView.swift must say {text:?}"
        );
    }
    for (theirs, ours) in [
        ("Didn&rsquo;t record", "Didn’t record"),
        (
            "more than your subscription allows.",
            "more than your subscription allows.",
        ),
        ("will be dropped.", "will be dropped."),
        ("Delete permanently, freeing", "Delete permanently, freeing"),
        ("and forget this rule?", "and forget this rule?"),
        ("tracked", " tracked"),
    ] {
        assert!(
            page.contains(theirs),
            "RecordingsPage.tsx no longer says {theirs:?}"
        );
        assert!(
            swift.contains(ours),
            "RecordingsView.swift must say {ours:?}"
        );
    }
    let days = between(&page, "const WEEKDAYS = [", "];");
    let mine = between(&swift, "private let weekdays = [", "]");
    assert_eq!(
        quoted(mine),
        quoted(&days.replace('\'', "\"")),
        "weekday names changed"
    );
}

/// Adult categories show only with a parental PIN (F-39): the refusal is Windows' word for
/// word, and the panel says what Windows' panel says while the switch cannot be turned off.
#[test]
fn adult_categories_need_the_pin_as_on_windows() {
    let theirs = app_const("profiles.rs", "ADULT_NEEDS_PIN");
    assert!(
        theirs.contains(crate::parental::ADULT_NEEDS_PIN),
        "aurora-app's ADULT_NEEDS_PIN is now {theirs}"
    );
    let panel = ui_src("features/settings/ParentalPanel.tsx");
    let swift = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../Shared/Features/ParentalControlsView.swift"),
    )
    .unwrap();
    let hint = "For every profile. Set a parental PIN first to show them: only the PIN can show adult categories.";
    assert!(
        panel.contains(hint),
        "ParentalPanel no longer says {hint:?}"
    );
    assert!(
        swift.contains(hint),
        "ParentalControlsView must say {hint:?}"
    );
}

/// Settings → Privacy (F-40) offers Windows' windows and says what Windows' panel says,
/// and the core refuses the same nonsense in the same words.
#[test]
fn privacy_says_and_keeps_what_windows_does() {
    let panel = ui_src("features/settings/PrivacyPanel.tsx");
    let swift = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Shared/Features/PrivacySection.swift"),
    )
    .unwrap();
    for (days, label) in [
        ("null", "Forever"),
        ("365", "1 year"),
        ("90", "90 days"),
        ("30", "30 days"),
    ] {
        let theirs = format!("{{ days: {days}, label: '{label}' }}");
        assert!(
            panel.contains(&theirs),
            "PrivacyPanel no longer offers {theirs}"
        );
        let ours = format!(
            "({}, \"{label}\")",
            if days == "null" { "nil" } else { days }
        );
        assert!(swift.contains(&ours), "PrivacySection must offer {ours}");
    }
    for sentence in [
        "Keep watch history",
        "Anything not watched for longer than this is forgotten, on every device you sync with.",
        "Clear watch history",
        " Continue Watching, episode ticks and recommendations start again from nothing. Other profiles keep theirs.",
        "Yes, clear it",
        "Clear history",
        "Usage data",
        "None sent",
        "Aurora sends none: there is no analytics or crash reporting to switch on.",
        "Watch history is kept for good.",
        "older than that.",
        "There was nothing to forget.",
    ] {
        assert!(panel.contains(sentence), "PrivacyPanel no longer says {sentence:?}");
        assert!(swift.contains(sentence), "PrivacySection must say {sentence:?}");
    }
    let theirs = app_src("privacy.rs");
    let refusal = "History can be kept for 1 to {MAX_HISTORY_DAYS} days, or for good.";
    assert!(
        theirs.contains(refusal),
        "aurora-app no longer says {refusal:?}"
    );
    let ours =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/privacy.rs"))
            .unwrap();
    assert!(ours.contains(refusal));
}

/// Windows' Sync panel says how many changes need a newer Aurora (§3.8); so does the
/// Sync section, in the same words, from the same count.
#[test]
fn sync_says_what_needs_a_newer_aurora_as_windows_does() {
    // JSX breaks the sentence over lines; compare with the whitespace folded.
    let fold = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let panel = fold(&ui_src("features/settings/SyncPanel.tsx"));
    let swift = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Shared/Features/SyncSection.swift"),
    )
    .unwrap();
    for part in [
        "from your other devices",
        "a newer version of Aurora.",
        "kept on the server and will be read once this device updates.",
    ] {
        assert!(panel.contains(part), "SyncPanel no longer says {part:?}");
        assert!(swift.contains(part), "SyncSection must say {part:?}");
    }
    for (one, many) in [
        ("'change'", "'changes'"),
        ("'needs'", "'need'"),
        ("'It is'", "'They are'"),
    ] {
        assert!(
            panel.contains(one) && panel.contains(many),
            "SyncPanel changed {one}/{many}"
        );
        let (one, many) = (one.replace('\'', "\""), many.replace('\'', "\""));
        assert!(
            swift.contains(&one) && swift.contains(&many),
            "SyncSection must say {one}/{many}"
        );
    }
    assert!(swift.contains("status.unreadable > 0"));
}
