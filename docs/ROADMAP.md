# Roadmap

Phases mirror `README.md` §21. Status is honest: this is what is actually built.

| Phase | Scope | Status |
|---|---|---|
| 0 — Spike | libmpv behind transparent WebView2 | **Code written, unverified.** Requires a Windows host; see the note below. |
| 1 — Foundation | Workspace, typed IPC, SQLite + migrations, tokens, app shell, CI | **Done** |
| 2 — Ingestion | M3U + Xtream parsing, classification, series grouping, rules | **Done (parsers + models); provider HTTP client pending** |
| 3 — Player | Playback service, OSD, shortcuts | **UI + state machine done; mpv backend Windows-gated** |
| 4 — Live TV | Channel list, zap, banner, number entry, favorites | **Done** |
| 5 — EPG | XMLTV ingest, matching, guide grid, info pane | **Done** |
| 6 — Movies | Rails, hero, hover preview, detail modal, browse | **Done** |
| 7 — Series | Seasons, episodes, Up Next, next-episode autoplay | **Done** |
| 8 — DVR | Recording, timeshift, catch-up | **Not started** |
| 9 — Personalization | Profiles, parental controls, search, command palette | **Search + palette done; profiles pending** |
| 10 — QoL | §13 list, TV mode, multi-view, PiP, remote | **Partial** |
| 11 — Hardening | Perf budgets, soak, diagnostics | **Not started** |
| 12 — Release | Installers, signing, auto-update | **CI build only** |

## The Phase 0 caveat

README §21 requires the compositing spike to be proven before anything else is built. That spike
needs a Windows machine with WebView2 and `mpv-2.dll`; this build environment is Linux. The spike
code exists (`aurora-player/src/mpv.rs`, `aurora-app/src/window.rs`) and CI compiles it for
`x86_64-pc-windows-msvc`, but **compiling is not proving**. Before Phase 8 work begins, someone
must run `cargo tauri dev` on Windows and confirm: video renders behind the UI, controls receive
input, resize does not tear, and zap time is under 1.5 s.

Everything else was built in the order above because it is verifiable without Windows, and because
all of it is backend-agnostic — if the compositing approach has to change, the UI and core do not.
