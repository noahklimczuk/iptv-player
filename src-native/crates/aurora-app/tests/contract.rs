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
