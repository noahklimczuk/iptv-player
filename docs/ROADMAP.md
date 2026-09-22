# Roadmap

Phases mirror `README.md` §21. Status is honest about what is *verified* versus merely
*written* — see the verification table below.

| Phase | Scope | Status |
|---|---|---|
| 0 — Spike | libmpv behind transparent WebView2 | **Written, compiles for Windows, NOT run.** See the caveat below. |
| 1 — Foundation | Workspace, typed IPC, SQLite + migrations, tokens, app shell, CI | **Done** |
| 2 — Ingestion | M3U + Xtream + XMLTV parsing, classification, series grouping, rules | **Parsers done and tested. The provider HTTP client is not written** — nothing fetches a playlist over the network yet. |
| 3 — Player | Playback service, OSD, shortcuts | **Trait, NullBackend, mpv backend, OSD and hotkeys done.** Real decoding unverified. |
| 4 — Live TV | Channel list, zap, banner, number entry, favorites | **Done** |
| 5 — EPG | XMLTV ingest, matching, guide grid, info pane | **Done** |
| 6 — Movies | Rails, hero, hover preview, detail modal, browse | **Done** |
| 7 — Series | Seasons, episodes, detail tabs, skip markers, Up Next | **Done.** Skip Intro/Recap/Credits and the Next Episode button are in; per-show auto-skip preferences are stored but have no Settings UI yet. |
| 8 — DVR | Recording, timeshift, catch-up | **Not started.** Catch-up is modelled in the schema and surfaced in the UI; nothing records. |
| 9 — Personalization | Profiles, parental controls, search, palette | **Search + command palette done.** Profiles/PIN/parental controls: schema only. |
| 10 — QoL | §13 list, TV mode, multi-view, PiP, remote | **Partial:** themes, TV density, reduce-motion, keyboard map, command palette. Multi-view, PiP, sleep timer, tray, backup/restore not built. |
| 11 — Hardening | Perf budgets, soak, diagnostics | **Not started.** No §16 budget is measured yet. |
| 12 — Release | Installers, signing, auto-update | **CI type-checks Windows; no installer is produced.** |

## What is actually verified

| Claim | Evidence |
|---|---|
| Parsers handle hostile real-world input | 82 `aurora-core` tests, run on every commit |
| Schema, migrations, reconciliation, search | 49 `aurora-db` tests |
| Playback state machine and error taxonomy | 19 `aurora-player` tests |
| Skip markers: chapter parsing, learning, merge | 25 `aurora-core` + 13 `aurora-db` tests |
| The Tauri host compiles and its URL resolution works | 4 `aurora-app` tests (Linux, with GTK dev packages) |
| The mpv/Win32 backend compiles for Windows | `cargo check --target x86_64-pc-windows-msvc` |
| Every screen renders and the journeys work | 17 Playwright runs against the production bundle |
| Video actually decodes and composites | **Not verified anywhere yet** — Phase 0 |

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
2. **Provider HTTP client** — the parsers are ready and tested; nothing calls them over
   the network yet, so the app cannot ingest a real subscription.
3. **A Settings row for auto-skip**, which is stored per profile and show but currently
   only reachable through the IPC command.
4. **Profiles**, which most of §11 depends on.
5. **DVR** (Phase 8), the largest untouched block.
