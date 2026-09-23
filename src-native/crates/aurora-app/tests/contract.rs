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
fn the_name_mapping_matches_the_transport() {
    // The same transformation `src-ui/src/ipc/index.ts` applies before calling invoke.
    assert_eq!(to_command_name("library.setFilters"), "library_set_filters");
    assert_eq!(to_command_name("providers.list"), "providers_list");
    assert_eq!(to_command_name("player.playCatchup"), "player_play_catchup");
    assert_eq!(to_command_name("dvr.list"), "dvr_list");
}
