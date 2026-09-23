//! Every command must take its arguments as one parameter named `args`.
//!
//! Tauri keys the invoke payload by the *parameter's name*: `fn f(args: A)` reads
//! `{"args": …}` and `fn f(text: String)` reads `{"text": …}`, and it errors outright
//! when the key is missing rather than falling back to the payload as a whole
//! (`tauri::ipc::command`). The UI's transport sends one shape for all of them, so a
//! command that names its parameter anything else is unreachable from the app.
//!
//! This is not hypothetical. The transport spread arguments across the payload instead
//! of nesting them under `args`, and every one of the sixty-one commands that takes
//! arguments failed to deserialize on Windows — including the provider detection the
//! first-run wizard needs before its buttons will light up. The mock transport reads
//! the flat object, so nothing in the test suite could see it.
//!
//! Reading the source rather than reflecting on the macro is deliberate: the wrapper
//! the macro generates does not expose the key it chose, and the thing worth pinning is
//! the convention a person breaks when they write the next command.

use std::path::Path;

/// Parameters supplied by the framework rather than sent by the caller.
fn is_injected(param: &str) -> bool {
    let t = param
        .split_once(':')
        .map(|(_, t)| t)
        .unwrap_or(param)
        .trim();
    t.starts_with("State<")
        || t.starts_with("tauri::")
        || t.starts_with("AppHandle")
        || t.starts_with("Window")
        || t.starts_with("WebviewWindow")
}

/// Split a parameter list on top-level commas, so `State<'_, Services>` stays whole.
fn split_params(params: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for c in params.chars() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    out.push(current);
    out.into_iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

#[test]
fn every_command_names_its_argument_parameter_args() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut checked = 0usize;
    let mut offenders = Vec::new();

    for entry in std::fs::read_dir(&dir).expect("src directory") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("read source");

        for (offset, _) in source.match_indices("#[tauri::command]") {
            let rest = &source[offset..];
            let Some(fn_at) = rest.find("fn ") else {
                continue;
            };
            let after = &rest[fn_at + 3..];
            let Some(open) = after.find('(') else {
                continue;
            };
            let name = after[..open].trim().to_string();
            let Some(close) = after[open..].find(')') else {
                continue;
            };

            checked += 1;
            for param in split_params(&after[open + 1..open + close]) {
                if is_injected(&param) {
                    continue;
                }
                let ident = param.split(':').next().unwrap_or("").trim();
                if ident != "args" {
                    offenders.push(format!(
                        "{}::{name} takes `{ident}`; the UI transport only ever sends `args`",
                        path.file_name().unwrap().to_string_lossy(),
                    ));
                }
            }
        }
    }

    assert!(
        checked > 50,
        "only found {checked} commands — the scan is broken"
    );
    assert!(
        offenders.is_empty(),
        "commands the UI cannot reach:\n  {}",
        offenders.join("\n  ")
    );
}
