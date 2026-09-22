# Roadmap

Phases mirror `README.md` §21. Status is honest about what is *verified* versus merely
*written* — see the verification table below.

| Phase | Scope | Status |
|---|---|---|
| 0 — Spike | libmpv behind transparent WebView2 | **Written, compiles for Windows, NOT run.** See the caveat below. |
| 1 — Foundation | Workspace, typed IPC, SQLite + migrations, tokens, app shell, CI | **Done** |
| 2 — Ingestion | M3U + Xtream + XMLTV fetching and parsing, classification, series grouping, rules | **Done.** aurora-ingest fetches, parses, reconciles and indexes; credentials go to the OS store. Stalker portals (§4.3, Optional) and per-series episode listings are not built. |
| 3 — Player | Playback service, OSD, shortcuts | **Trait, NullBackend, mpv backend, OSD and hotkeys done.** Real decoding unverified. |
| 4 — Live TV | Channel list, zap, banner, number entry, favorites | **Done** |
| 5 — EPG | XMLTV ingest, matching, guide grid, info pane | **Done** |
| 6 — Movies | Rails, hero, hover preview, detail modal, browse | **Done** |
| 7 — Series | Seasons, episodes, detail tabs, skip markers, Up Next | **Done.** Skip Intro/Recap/Credits, the Next Episode button, Up Next autoplay, and per-show auto-skip/autoplay preferences. |
| 8 — DVR | Recording, timeshift, catch-up | **Recording done; timeshift not started.** Scheduling with padding, conflict detection against the connection limit, series rules, reminders, a quota, and a recordings library. The recorder writes MPEG-TS to disk and has never been pointed at a real provider. Timeshift (pause live TV) and catch-up playback are not built. |
| 9 — Personalization | Profiles, parental controls, search, palette | **Done.** Profiles with PINs and a picker, certification ceilings, kids profiles, adult categories hidden by default, attempt throttling. Per-channel/category locks are stored but have no UI yet. |
| 13 — First run | Wizard: add provider, validate, import | **Done** (README §13). |
| 10 — QoL | §13 list, TV mode, multi-view, PiP, remote | **Partial:** themes, TV density, reduce-motion, keyboard map, command palette. Multi-view, PiP, sleep timer, tray, backup/restore not built. |
| 11 — Hardening | Perf budgets, soak, diagnostics | **Not started.** No §16 budget is measured yet. |
| 12 — Release | Installers, signing, auto-update | **CI type-checks Windows; no installer is produced.** |

## What is actually verified

| Claim | Evidence |
|---|---|
| Parsers handle hostile real-world input | 157 `aurora-core` tests, run on every commit |
| Schema, migrations, reconciliation, search | 120 `aurora-db` tests |
| Playback state machine and error taxonomy | 13 `aurora-player` tests |
| Fetching, retry, gzip, credential redaction | 74 `aurora-ingest` tests, against a server that simulates 401/403/404/429/timeout/mid-stream disconnect |
| A refresh never destroys user data | `aurora-ingest::sync` tests assert renames, numbers and hidden flags survive |
| Skip markers: chapter parsing, learning, merge | 25 `aurora-core` + 13 `aurora-db` tests |
| The Tauri host compiles and its URL resolution works | 18 `aurora-app` tests (Linux, with GTK dev packages) |
| Recording scheduling: padding, conflicts, rule matching | 19 `aurora-core` + 32 `aurora-db` tests |
| A stream is written to disk, and a cut stream keeps what it got | 7 `aurora-ingest` tests against the failure-simulating server |
| The scheduler starts, stops and finalises recordings | 11 `aurora-app` tests driving `Dvr::tick` on a test clock |
| A recording title that Windows would reject becomes a legal filename | 10 `aurora-core` tests (`CON`, `Ratched: Season 1`, trailing dots, MAX_PATH) |
| The mpv/Win32 backend compiles for Windows | `cargo check --target x86_64-pc-windows-msvc` |
| Every screen renders and the journeys work | 39 Playwright runs against the production bundle |
| Video actually decodes and composites | **Not verified anywhere yet** — Phase 0 |
| A recording survives a real provider's stream | **Not verified** — the recorder has only met the test server |

## The Phase 0 caveat

README §21 requires the compositing spike to be proven before anything else is built.
That needs a Windows machine with WebView2 and `mpv-2.dll`; this was built on Linux.
The code exists (`aurora-player/src/mpv.rs`, `aurora-app/src/window.rs`) and type-checks
for `x86_64-pc-windows-msvc`, but **compiling is not proving.**

Before Phase 8 work begins, someone must run `cargo tauri dev` on Windows and confirm:

1. Video renders behind the UI, not in a separate window.
2. The UI receives input while video plays underneath.
3. Resizing, snapping and alt-tab do not tear the two surfaces apart.
4. Zap time is under the 1.5 s budget in README §16.

If (1) or (3) fails, the fallback is a native XAML/WinUI shell for the player surface
with the web layer confined to non-playback screens. `aurora-core`, `aurora-db` and
`aurora-player` are all backend-agnostic and would port unchanged — which is why they
were built first.

## Nearest useful next steps

1. **Run the Phase 0 spike on Windows.** Everything else is downstream of that answer.
2. **Point it at a real subscription.** Ingestion is built and tested against a local
   server, but has never met an actual provider — the fork-tolerance in
   `aurora_core::xtream` is written from the spec, not from observed traffic.
3. **TMDB enrichment**, so the catalog shows real artwork instead of generated
   gradients.
4. **Timeshift** — pause and rewind live TV, the half of Phase 8 that recording does
   not cover. It needs a ring buffer on disk and a player that can seek inside a
   still-growing file, which is a different problem from scheduling.
5. **Catch-up playback.** The schema and the UI already carry a provider's catch-up
   window; nothing builds the URL to play from it yet.
