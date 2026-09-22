# Decision Log

Decisions taken while implementing `README.md`. Format: context → decision → consequence.

## D1 — Native stack: Rust + Tauri 2 (README §22 Q1)

**Context.** README §2.1 recommends Rust/Tauri 2, with C#/.NET 9 + WinUI 3 as the sanctioned
alternative, and §22 asks the operator to choose.

**Decision.** Rust + Tauri 2.

**Rationale.** (a) It is the README's own recommendation. (b) No .NET SDK exists in the build
environment, so the WinUI path could not be compiled or tested at all. (c) The UI layer is React
either way under Tauri, so the Netflix-grade UI work in §8/§12 is directly buildable and verifiable
here; under WinUI it would be XAML and unverifiable in this container.

**Consequence.** If the operator later prefers WinUI 3, `aurora-core` and `aurora-db` port unchanged
(they are pure Rust with no Tauri dependency) — only `aurora-app` and `src-ui` would be replaced.
This is why the core is split out as its own dependency-free crates.

## D2 — Core logic lives in Rust, not TypeScript

**Context.** README §3 mandates "the UI never touches the network or the database directly."

**Decision.** All parsing, matching, persistence, and network I/O live in `aurora-core` /
`aurora-db`. The UI is a pure view layer over typed IPC.

**Consequence.** Parsers are platform-independent Rust, so they are fully unit-testable on Linux
(`cargo test`) even though the app itself is Windows-only. This is what makes the majority of the
build verifiable in CI on any runner.

## D3 — Windows-only code is `#[cfg(windows)]`-gated behind a trait

**Context.** The mpv/WebView2/Win32 compositing layer (README §2.1) cannot compile on Linux.

**Decision.** `aurora-player` exposes a `PlayerBackend` trait. The real implementation
(`MpvBackend`, Win32 child HWND + libmpv) is `#[cfg(windows)]`. A `NullBackend` compiles everywhere
and is used by tests and by the Linux dev server.

**Consequence.** `cargo test -p aurora-core -p aurora-db` and the UI build run on any platform;
the Windows build is exercised by CI on `windows-latest`. No Windows capability is weakened to
achieve this — the gate is at the backend boundary only.

## D4 — UI runs against a mock IPC adapter when not hosted by Tauri

**Context.** The UI must be developable and reviewable without a Windows machine.

**Decision.** `src-ui/src/ipc/` picks a transport at runtime: the real Tauri `invoke` bridge when
`window.__TAURI_INTERNALS__` is present, otherwise an in-memory mock backed by synthetic fixtures.

**Consequence.** `pnpm dev` gives a fully clickable app in any browser, and Playwright can
screenshot every screen. Fixtures are synthetic (`example.com` hosts only) per README §24.

## D5 — Tailwind v4, CSS-first tokens

Design tokens (README §12) are declared as CSS custom properties in `src-ui/src/styles/tokens.css`
and exposed to Tailwind v4 via `@theme`. One source of truth, themeable at runtime by swapping a
`data-theme` attribute — which is what the OLED / Light / High-Contrast themes need.

## D6 — Deferred from this pass

Not yet built, and not silently dropped (README working-agreement rule 4). Tracked in
`docs/ROADMAP.md` against their phases: Stalker portals (§4.3, marked Optional), downloads (§8.6,
Optional), DVR recording and timeshift (§7.6–7.7, Phase 8), casting, voice search, gamepad (§14.3),
and the metadata-enrichment network client (§4.5 — the model and cache layer exist, the TMDB HTTP
calls do not).
