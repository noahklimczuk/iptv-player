//! Every command the UI believes in must exist on the host.
//!
//! `shared/ipc.ts` is the contract both sides code against, and nothing checked that
//! the host actually implements it. Seven commands were declared and never registered
//! — `providers.list`, which runs on launch; `library.rails` and `library.stats`, which
//! are the home screen and the Settings counts; `mylist.toggle`, `favorites.toggle`,
//! `progress.get` and `player.setSpeed`. Each one answered "command not found" on
//! Windows while the mock transport answered all of them, so the browser preview
//! looked complete and the shipped app had no home screen.
//!
//! The test reads both sides as text rather than linking them, because they are in
//! different languages and the only thing worth pinning is that the names line up.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // crates/aurora-app -> crates -> src-native -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root")
        .to_path_buf()
}

/// `'library.setFilters'` in the contract is `library_set_filters` on the host.
fn to_command_name(declared: &str) -> String {
    let mut out = String::with_capacity(declared.len() + 4);
    for c in declared.chars() {
        match c {
            '.' => out.push('_'),
            c if c.is_ascii_uppercase() => {
                out.push('_');
                out.push(c.to_ascii_lowercase());
            }
            c => out.push(c),
        }
    }
    out
}

/// The keys of the `Commands` interface in the shared contract.
fn declared_commands(contract: &str) -> Vec<String> {
    let start = contract
        .find("export interface Commands")
        .expect("the contract has a Commands interface");
    let rest = &contract[start..];
    let end = rest
        .find("\nexport type CommandName")
        .expect("Commands is followed by CommandName");

    rest[..end]
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let quoted = line.strip_prefix('\'')?;
            let (name, after) = quoted.split_once('\'')?;
            // A key, not a string in a comment or a union.
            after.starts_with(':').then(|| name.to_string())
        })
        .filter(|name| name.contains('.'))
        .collect()
}

/// The command functions inside `generate_handler![…]`.
fn registered_commands(main: &str) -> Vec<String> {
    let start = main.find("generate_handler![").expect("an invoke handler");
    let rest = &main[start..];
    let end = rest.find("])").expect("the handler list is closed");

    rest[..end]
        .lines()
        .filter_map(|line| {
            let line = line.trim().trim_end_matches(',');
            let name = line.rsplit("::").next()?;
            (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
                .then(|| name.to_string())
        })
        .collect()
}

#[test]
fn the_host_registers_every_command_the_contract_declares() {
    let root = repo_root();
    let contract = std::fs::read_to_string(root.join("shared/ipc.ts")).expect("shared/ipc.ts");
    let main = std::fs::read_to_string(root.join("src-native/crates/aurora-app/src/main.rs"))
        .expect("main.rs");

    let declared = declared_commands(&contract);
    let registered = registered_commands(&main);

    assert!(
        declared.len() > 70,
        "only parsed {} commands from the contract — the scan is broken",
        declared.len()
    );
    assert!(
        registered.len() > 70,
        "only parsed {} commands from the handler — the scan is broken",
        registered.len()
    );

    let missing: Vec<_> = declared
        .iter()
        .filter(|d| !registered.contains(&to_command_name(d)))
        .collect();

    assert!(
        missing.is_empty(),
        "the UI can call these and the host does not answer:\n  {}",
        missing
            .iter()
            .map(|m| format!("{m} (expected fn {})", to_command_name(m)))
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn the_contract_declares_every_command_the_host_registers() {
    let root = repo_root();
    let contract = std::fs::read_to_string(root.join("shared/ipc.ts")).expect("shared/ipc.ts");
    let main = std::fs::read_to_string(root.join("src-native/crates/aurora-app/src/main.rs"))
        .expect("main.rs");

    // The other direction, which is the one that went wrong. A command the UI can call
    // and the host cannot answer is a visible error; a command the host answers and the
    // contract does not mention is *silence*, and silence is what `library.syncChapters`
    // got. It was written, registered, given a table and a derivation — and never
    // declared, so no caller could be written against it and none was. `skip_markers`
    // was empty on every real library and Skip Intro never appeared, with nothing
    // failing anywhere to say why.
    let declared: Vec<String> = declared_commands(&contract)
        .iter()
        .map(|d| to_command_name(d))
        .collect();
    let registered = registered_commands(&main);
    assert!(
        registered.len() > 70,
        "only parsed {} commands from the handler — the scan is broken",
        registered.len()
    );

    let undeclared: Vec<_> = registered
        .iter()
        .filter(|r| !declared.contains(r))
        .collect();

    assert!(
        undeclared.is_empty(),
        "the host answers these and the contract does not declare them, so nothing in \
         the UI can call them:\n  {}",
        undeclared
            .iter()
            .map(|r| r.to_string())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// The field names of one `export interface` in the contract.
///
/// Field lines end in a semicolon; the doc comments around them do not, which is enough
/// to tell them apart without parsing TypeScript.
fn declared_fields(contract: &str, interface: &str) -> Vec<String> {
    let start = contract
        .find(&format!("export interface {interface} {{"))
        .unwrap_or_else(|| panic!("the contract declares {interface}"));
    let rest = &contract[start..];
    let end = rest.find("\n}").expect("the interface is closed");

    rest[..end]
        .lines()
        .skip(1)
        .filter_map(|line| {
            let line = line.trim();
            if !line.ends_with(';') || line.starts_with('*') || line.starts_with('/') {
                return None;
            }
            let (name, _) = line.split_once(':')?;
            Some(name.trim_end_matches('?').to_string())
        })
        .filter(|name| name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .collect()
}

/// The UI reads `PlayerState` on every frame, and the host has to send all of it.
///
/// `itemKind` was declared here, answered by the mock, and never sent by the host. The
/// UI gates Skip Intro, Skip Credits and Up Next on `player.itemKind === 'episode'`
/// (`useEpisodeAids`), so all three worked in every browser journey and none of them
/// could ever appear on Windows. Exactly the failure the command test above exists for,
/// one layer down: the mock is generous and the host is the one that ships.
#[test]
fn the_player_state_the_host_sends_is_the_one_the_contract_declares() {
    let contract = std::fs::read_to_string(repo_root().join("shared/ipc.ts")).expect("ipc.ts");
    let declared = declared_fields(&contract, "PlayerState");
    assert!(
        declared.len() > 15,
        "only parsed {} fields — the scan is broken",
        declared.len()
    );

    let sent = serde_json::to_value(aurora_player::PlayerState::default()).expect("serializes");
    let sent = sent.as_object().expect("a JSON object");

    let missing: Vec<&String> = declared.iter().filter(|f| !sent.contains_key(*f)).collect();
    assert!(
        missing.is_empty(),
        "the UI reads these and the host never sends them: {missing:?}"
    );

    let extra: Vec<&String> = sent.keys().filter(|k| !declared.contains(k)).collect();
    assert!(
        extra.is_empty(),
        "the host sends these and the contract does not declare them: {extra:?}"
    );
}

#[test]
fn the_name_mapping_matches_the_transport() {
    // The same transformation `src-ui/src/ipc/index.ts` applies before calling invoke.
    assert_eq!(to_command_name("library.setFilters"), "library_set_filters");
    assert_eq!(to_command_name("providers.list"), "providers_list");
    assert_eq!(to_command_name("player.playCatchup"), "player_play_catchup");
    assert_eq!(to_command_name("dvr.list"), "dvr_list");
}

#[test]
fn the_field_scan_reads_names_and_not_the_prose_around_them() {
    let sample = "export interface PlayerState {\n\
                  \x20 /** What is loaded, for the OSD title. */\n\
                  \x20 title: string | null;\n\
                  \x20 /**\n\
                  \x20  * Several lines: status: not a field.\n\
                  \x20  */\n\
                  \x20 match?: number;\n\
                  \x20 stats: PlaybackStats | null;\n\
                  }\n";
    assert_eq!(
        declared_fields(sample, "PlayerState"),
        ["title", "match", "stats"]
    );
}

/// The Content-Security-Policy has to allow the schemes real providers actually use.
///
/// `img-src` was `'self' data: https:`. IPTV panels overwhelmingly serve `tvg-logo`
/// and `stream_icon` over plain HTTP — the probe in docs/ROADMAP.md reached its origin
/// over `http://` on a bare IP — so in the shipped app every one of those was blocked
/// with "Refused to load the image" and rendered as a broken icon. `media-src` already
/// allowed `http:`, so somebody had thought about this for streams and not for
/// pictures.
///
/// Invisible from a browser: `vite preview` serves no CSP at all, which is what every
/// Playwright journey runs against.
#[test]
/// Every command the UI sends is a `fetch` to Tauri's own IPC origin, and the CSP has to
/// allow it.
///
/// It did not. `default-src 'self'` with no `connect-src` meant the WebView refused
/// *every* call before it reached the host — "Connecting to 'http://ipc.localhost/
/// providers_list' violates the following Content Security Policy directive" — so the
/// app launched, painted its boot screen, and sat on "Opening your library…" for ever
/// with the host completely idle.
///
/// Nothing caught it. The suite runs against a browser and a mock transport, where there
/// is no custom protocol and no CSP; the only other test of this string checks `img-src`,
/// which is the directive that was last got wrong (F-12). A policy is a list of
/// directives and testing one of them is testing one of them.
#[test]
fn the_csp_allows_the_ipc_the_ui_actually_uses() {
    let config =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json"))
            .expect("tauri.conf.json");
    let parsed: serde_json::Value = serde_json::from_str(&config).expect("valid JSON");
    let csp = parsed["app"]["security"]["csp"]
        .as_str()
        .expect("a csp is set");

    let connect = csp
        .split(';')
        .map(str::trim)
        .find(|d| d.starts_with("connect-src"))
        .expect(
            "connect-src is declared: without it `default-src` applies and every command              the UI sends is refused by the WebView",
        );

    // Windows serves the IPC over this origin; the scheme covers the other platforms.
    for source in ["ipc:", "http://ipc.localhost"] {
        assert!(
            connect.contains(source),
            "connect-src must allow {source}, or the UI cannot talk to the host at all:              {connect}"
        );
    }
}

fn the_csp_allows_the_image_schemes_providers_actually_use() {
    let config =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json"))
            .expect("tauri.conf.json");
    let parsed: serde_json::Value = serde_json::from_str(&config).expect("valid JSON");
    let csp = parsed["app"]["security"]["csp"]
        .as_str()
        .expect("a csp is set");

    let img = csp
        .split(';')
        .map(str::trim)
        .find(|d| d.starts_with("img-src"))
        .expect("img-src is declared");

    for scheme in ["http:", "https:", "data:"] {
        assert!(
            img.split_whitespace().any(|s| s == scheme),
            "img-src must allow {scheme}, or provider artwork served over it is blocked: {img}"
        );
    }

    // The things that must stay shut: a page that can be told what to execute is a
    // different application from this one.
    assert!(
        csp.contains("script-src 'self'"),
        "scripts must stay same-origin: {csp}"
    );
    assert!(
        !csp.contains("script-src 'self' 'unsafe-inline'"),
        "inline scripts must not be allowed: {csp}"
    );
    assert!(
        csp.starts_with("default-src 'self'"),
        "the default must stay same-origin: {csp}"
    );
}

/// The Phase 0 spike must stay wired.
///
/// `attach`, `attach_video_surface` and `pump` were each written, each correct as far
/// as anyone could tell, and each called from nowhere — so a Windows run showed no
/// video for reasons that had nothing to do with compositing, and the OSD would have
/// frozen after the first frame because nothing drained mpv's event queue. They were
/// unreachable structurally: the app layer holds a `Box<dyn PlayerBackend>`, and
/// neither method was on the trait.
///
/// This reads the sources rather than running anything, because the thing that went
/// wrong is not behaviour — it is a call that does not exist. Nothing on this machine
/// can run the Windows path, so what is worth pinning is that the calls are there.
#[test]
fn the_video_surface_and_the_event_pump_are_actually_called() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let read = |name: &str| std::fs::read_to_string(src.join(name)).expect(name);

    let main = read("main.rs");
    assert!(
        main.contains("window::attach_video_surface("),
        "nothing creates the video surface, so mpv has nowhere to draw"
    );
    assert!(
        main.contains("tauri::WindowEvent::Resized"),
        "nothing repositions the video surface, so it tears away from the WebView"
    );

    let window = read("window.rs");
    assert!(
        window.contains("player.attach("),
        "attach_video_surface no longer attaches anything"
    );
    assert!(
        window.contains(".hwnd()"),
        "the surface is not given the host window's handle"
    );

    let playback = read("playback.rs");
    assert!(
        playback.contains("player.pump("),
        "the heartbeat reads a state that nothing produces: position, tracks, \
         buffering, errors and the timeshift window all arrive as backend events"
    );
}

/// The compositing model needs the window itself to be transparent, not only the
/// WebView2's background (README §2.1). This was `false`, which alone would have been
/// enough to show no video.
/// The background passes are started at launch, not only by a refresh.
///
/// This is the same shape of bug as `library.syncChapters` and `progress.save` before it:
/// the work was written, registered and correct, and nothing called it. Both passes were
/// reachable only from a provider refresh, so a library imported once and then simply used
/// never finished its metadata and never learned how many seasons anything had — with
/// nothing failing anywhere to say so.
///
/// A source grep rather than a behavioural test because what is being guarded is the call
/// itself. There is no way to observe "it would have run" that does not amount to reading
/// the same line.
#[test]
fn the_background_passes_are_started_at_launch() {
    let main =
        std::fs::read_to_string(repo_root().join("src-native/crates/aurora-app/src/main.rs"))
            .expect("main.rs");
    assert!(
        main.contains("catch_up_in_background("),
        "nothing starts the metadata and episode-listing passes at launch, so they run          only when somebody presses Refresh"
    );

    let lib = std::fs::read_to_string(repo_root().join("src-native/crates/aurora-app/src/lib.rs"))
        .expect("lib.rs");
    for called in ["enrich_in_background(", "sweep_in_background("] {
        assert!(
            lib.contains(called),
            "catch_up_in_background does not call {called} — one of the two passes is              still refresh-only"
        );
    }
}

#[test]
fn the_window_is_transparent_so_video_can_show_through() {
    let config =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json"))
            .expect("tauri.conf.json");
    let parsed: serde_json::Value = serde_json::from_str(&config).expect("valid JSON");
    let window = &parsed["app"]["windows"][0];

    assert_eq!(
        window["transparent"].as_bool(),
        Some(true),
        "an opaque window hides the mpv surface behind it"
    );
    // The label the host looks the window up by. Absent means Tauri's default, "main",
    // which is what `get_webview_window("main")` asks for.
    if let Some(label) = window["label"].as_str() {
        assert_eq!(
            label, "main",
            "main.rs looks up the window labelled \"main\""
        );
    }
}

/// The updater has to know where the data lives before Tauri exists to tell it, so it
/// carries its own copy of the bundle identifier. If the two drift, a copy with no
/// `data-dir.txt` would look for its staged update in a folder nothing writes to, and
/// the update would silently never apply.
#[test]
fn the_updaters_bundle_identifier_matches_the_manifest() {
    let conf: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
    assert_eq!(
        conf["identifier"].as_str(),
        Some(aurora_app::updates::BUNDLE_IDENTIFIER),
        "tauri.conf.json's identifier and updates::BUNDLE_IDENTIFIER must agree"
    );
}

/// The installer must put Aurora somewhere it can write to.
///
/// This is the whole reason in-app updates work. `updates::asset_kind` asks one
/// question — can this process write to the folder it is running from — and answers
/// "replace my own files" or "run the installer" on that basis alone. A per-user
/// install can write to its folder. A per-machine install in Program Files cannot, so
/// it falls back to downloading a 39 MB installer and asking for elevation, which for
/// a standard account is not an update path at all.
///
/// `installMode: "both"` let the installer decide, and it chose Program Files: a copy
/// that had been updating itself in place went back to running the installer every
/// time, with nothing in the app to say why. That is what this pins.
///
/// MSI is excluded for the same reason rather than as tidying — an MSI installs
/// per-machine by construction, so shipping one re-creates the folder the app cannot
/// update itself in.
#[test]
fn the_bundle_installs_per_user_so_a_copy_can_update_itself() {
    let config =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json"))
            .expect("tauri.conf.json");
    let parsed: serde_json::Value = serde_json::from_str(&config).expect("valid JSON");

    let mode = parsed["bundle"]["windows"]["nsis"]["installMode"]
        .as_str()
        .expect("an NSIS install mode is set");
    assert_eq!(
        mode, "currentUser",
        "the installer must install into the viewer's own profile; {mode:?} can land in \
         Program Files, where the app cannot replace its own files and has to run the \
         installer instead"
    );

    let targets: Vec<&str> = parsed["bundle"]["targets"]
        .as_array()
        .expect("bundle targets are a list")
        .iter()
        .filter_map(|t| t.as_str())
        .collect();
    assert!(
        !targets.contains(&"msi"),
        "an MSI installs per-machine, which puts Aurora back somewhere it cannot update \
         itself: {targets:?}"
    );
}
