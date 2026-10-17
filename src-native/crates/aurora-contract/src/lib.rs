//! Where the mechanical guards on the IPC contract live, and why they live here.
//!
//! `tests/contract.rs` and `tests/command_args.rs` check that the two ends of the app
//! agree: that every command `shared/ipc.ts` declares is registered on the host and the
//! other way round, that `PlayerState`'s fields match, that every `#[tauri::command]`
//! names its parameter `args` and none of them runs on the main thread, that the CSP
//! allows the image schemes the UI uses. That defect class is what they exist for —
//! seven declared commands were never registered, and the mock transport answered all
//! of them, so the browser preview looked complete while the shipped app had no home
//! screen.
//!
//! They used to sit in `aurora-app/tests/`, which put them behind Tauri. `aurora-app` is
//! out of `default-members` for a good reason — Tauri's Linux backend needs GTK dev
//! packages and a bare `cargo test` should work on any host — but the consequence was
//! that a plain `cargo test` ran 915 tests across four crates and *none of these*. They
//! ran under `cargo test --workspace`, which is what CI does, so the gate was never
//! missing; it was missing from the run a developer makes before pushing, which is the
//! one that catches the mistake while it is cheap (R-162).
//!
//! Neither needs the host to build. They read `shared/ipc.ts`, `main.rs`,
//! `tauri.conf.json` and the sources of `aurora-app` as text, and serialise a handful of
//! real structs -- `PlayerState`, `ChannelRow`, `ProgrammeRow` -- to compare field names
//! against what the contract declares. Those come from `aurora-db`, `aurora-player` and
//! `aurora-core`, none of which pulls in Tauri, which is this crate's only constraint.
//! `aurora-app` itself is reached by path and never linked, which is why there is no code
//! in this file.
