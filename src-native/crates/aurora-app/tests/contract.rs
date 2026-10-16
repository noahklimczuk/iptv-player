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

/// Both directions, for one interface and the Rust type the host serialises into it.
///
/// `missing` is a field the UI reads and the host never sends; `extra` is a field the
/// host sends that the contract does not declare. Returned rather than asserted so the
/// caller can report every interface at once, rather than hiding the second mismatch
/// behind the first and finding them one `cargo test` at a time.
fn field_mismatch(contract: &str, interface: &str, sent: serde_json::Value) -> Vec<String> {
    let declared = declared_fields(contract, interface);
    assert!(
        !declared.is_empty(),
        "parsed no fields at all out of {interface} — the scan is broken"
    );
    let sent = sent
        .as_object()
        .unwrap_or_else(|| panic!("{interface}'s Rust type does not serialise to an object"));

    let mut out = Vec::new();
    for f in declared.iter().filter(|f| !sent.contains_key(*f)) {
        out.push(format!(
            "{interface}.{f}: the UI reads it, the host never sends it"
        ));
    }
    for k in sent.keys().filter(|k| !declared.contains(k)) {
        out.push(format!(
            "{interface}.{k}: the host sends it, the contract does not declare it"
        ));
    }
    out
}

/// What the host sends must be what the contract declares — for every interface here.
///
/// This test existed and ran against `PlayerState` alone, which is one of the 64
/// interfaces `shared/ipc.ts` declares. It was written because of `itemKind`: declared
/// in the contract, answered by the mock, never sent by the host. The UI gates Skip
/// Intro, Skip Credits and Up Next on `player.itemKind === 'episode'`, so all three
/// worked in every browser journey and none of them could ever appear on Windows.
///
/// The idea was right and nothing generalised it. Across the table below the same two
/// assertions would have caught three more of this review's findings, each of which
/// took a hand sweep of all 64 interfaces to find instead:
///
/// | Finding | Which half |
/// |---|---|
/// | `Movie.match` / `Series.match` — a documented feature that exists only in the mock | `missing` |
/// | `EpisodeRow.url` — the provider's password crossing into the WebView | **`extra`** |
/// | `Episode.airDate`, `Episode.seriesId` — declared, never sent | `missing` |
///
/// The payoff is specific: a `#[serde(skip)]` forgotten on a credential-bearing field
/// becomes a red test rather than a leak, and a field the mock answers and the host
/// does not becomes a red test rather than a feature that works in all 154 Playwright
/// journeys and in none of the shipped builds (R-154).
///
/// ## What is not here, and why
///
/// Pairs are listed by hand because the pairing is a judgement: the two sides are
/// written in different languages and some interfaces have no single struct behind them
/// at all. Left out deliberately:
///
/// * **Argument structs** (`MoviesArgs`, `ScheduleArgs`, …). They are the other
///   direction — the UI builds them and the host deserialises — so a missing field is a
///   deserialisation error the `commands` test above already reaches, not a silent null.
/// * **Interfaces assembled in a command body** rather than serialised from one struct,
///   such as `Progress` and `SearchResults`, where there is no type to pair against.
/// * **`serde(flatten)` and view wrappers**, where the JSON keys do not come from one
///   struct's field list and this scan would report confident nonsense.
///
/// Those three are the known blind spots. Everything with a struct behind it belongs in
/// the table, and adding a row to it is the cheapest test in the repository.
///
/// It earned that on the first run, with three mismatches nobody had reported:
/// `Channel.tvgId` sent and never declared, and `Episode.seriesId` and
/// `Episode.airDate` declared and never sent.
#[test]
fn the_fields_the_host_sends_are_the_ones_the_contract_declares() {
    use serde_json::to_value as v;

    let contract = std::fs::read_to_string(repo_root().join("shared/ipc.ts")).expect("ipc.ts");

    // `Default` on every row struct here, which is all this test needed to exist.
    let pairs: Vec<(&str, serde_json::Value)> = vec![
        (
            "PlayerState",
            v(aurora_player::PlayerState::default()).unwrap(),
        ),
        (
            "Track",
            v(aurora_player::Track {
                id: 1,
                kind: aurora_player::TrackKind::Audio,
                title: None,
                language: None,
                codec: None,
                channels: None,
                default: true,
            })
            .unwrap(),
        ),
        (
            "Channel",
            v(aurora_db::repo::channels::ChannelRow::default()).unwrap(),
        ),
        (
            "Programme",
            v(aurora_db::repo::epg::ProgrammeRow::default()).unwrap(),
        ),
        (
            "Movie",
            v(aurora_db::repo::library::MovieRow::default()).unwrap(),
        ),
        (
            "Series",
            v(aurora_db::repo::library::SeriesRow::default()).unwrap(),
        ),
        (
            "Episode",
            v(aurora_db::repo::library::EpisodeRow::default()).unwrap(),
        ),
        (
            "Category",
            v(aurora_db::repo::library::Category::default()).unwrap(),
        ),
        (
            "LibraryStats",
            v(aurora_db::repo::library::LibraryStats::default()).unwrap(),
        ),
        (
            "EpgCoverage",
            v(aurora_db::repo::library::EpgCoverage::default()).unwrap(),
        ),
        (
            "PlaylistEntry",
            v(aurora_db::repo::playlist::PlaylistRow::default()).unwrap(),
        ),
        (
            "Profile",
            v(aurora_db::repo::profiles::Profile::default()).unwrap(),
        ),
        (
            "CreditEntry",
            v(aurora_db::repo::enrichment::CreditRow::default()).unwrap(),
        ),
        // Built rather than defaulted: its two enums have no sensible default and
        // giving a domain type one to satisfy a test is the wrong way round.
        (
            "SkipMarker",
            v(aurora_core::markers::SkipMarker::new(
                aurora_core::markers::MarkerKind::Intro,
                0.0,
                30.0,
                aurora_core::markers::MarkerSource::Chapters,
            ))
            .unwrap(),
        ),
    ];

    let mut problems: Vec<String> = Vec::new();
    for (interface, sent) in pairs {
        problems.extend(field_mismatch(&contract, interface, sent));
    }

    assert!(
        problems.is_empty(),
        "the contract and the host disagree about {} field(s):\n  {}",
        problems.len(),
        problems.join("\n  ")
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

/// The playlist sweep asks more often than the staleness it is looking for.
///
/// Two numbers in two files that only make sense together: `REFRESH_AFTER_SECS` in
/// `providers.rs` is how old a library may get, and `REFRESH_SWEEP_INTERVAL` in `main.rs`
/// is how often anything checks. If the interval is the longer of the two, a provider
/// that comes due just after a check waits almost twice as long as it should — a daily
/// refresh that silently becomes every other day, which is the kind of thing nobody
/// notices because the guide is only *sometimes* out of date.
///
/// Read out of the sources rather than compared as constants, because the interval lives
/// in a binary this test cannot link against.
#[test]
fn the_refresh_sweep_runs_more_often_than_the_staleness_it_looks_for() {
    let root = repo_root();
    let main = std::fs::read_to_string(root.join("src-native/crates/aurora-app/src/main.rs"))
        .expect("main.rs");
    let providers =
        std::fs::read_to_string(root.join("src-native/crates/aurora-app/src/providers.rs"))
            .expect("providers.rs");

    /// `60 * 60` and friends, evaluated.
    fn product(expr: &str) -> i64 {
        expr.split('*')
            .map(|p| p.trim().replace('_', "").parse::<i64>().expect("a number"))
            .product()
    }

    let find = |text: &str, name: &str| -> i64 {
        let at = text
            .find(&format!("{name}:"))
            .unwrap_or_else(|| panic!("{name} is gone — this test is about it"));
        let rest = &text[at..];
        let open = rest.find('(').expect("a duration or a literal");
        let close = rest.find(')').expect("a closing paren");
        product(&rest[open + 1..close])
    };

    // `REFRESH_AFTER_SECS: i64 = 24 * 60 * 60;` has no parens, so it is read differently.
    let after = {
        let at = providers
            .find("REFRESH_AFTER_SECS")
            .expect("REFRESH_AFTER_SECS is gone — this test is about it");
        let rest = &providers[at..];
        let eq = rest.find('=').expect("an assignment");
        let end = rest.find(';').expect("a statement");
        product(&rest[eq + 1..end])
    };
    let sweep = find(&main, "REFRESH_SWEEP_INTERVAL");

    assert!(
        sweep < after,
        "the sweep runs every {sweep}s but looks for libraries older than {after}s — a          provider coming due just after a check would wait up to {}s",
        after + sweep
    );
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
