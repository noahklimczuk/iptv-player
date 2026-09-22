# Roadmap

Phases mirror `README.md` §21. Status is honest about what is *verified* versus merely
*written* — see the verification table below.

| Phase | Scope | Status |
|---|---|---|
| 0 — Spike | libmpv behind transparent WebView2 | **Written, compiles for Windows, NOT run.** See the caveat below. |
| 1 — Foundation | Workspace, typed IPC, SQLite + migrations, tokens, app shell, CI | **Done** |
| 2 — Ingestion | M3U + Xtream + XMLTV fetching and parsing, classification, series grouping, rules | **Done.** aurora-ingest fetches, parses, reconciles and indexes; credentials go to the OS store. Stalker portals (§4.3, Optional) and per-series episode listings are not built. |
| 3 — Player | Playback service, OSD, shortcuts | **Trait, NullBackend, mpv backend, OSD and hotkeys done.** Real decoding unverified. |
| 4 — Live TV | Channel list, zap, banner, number entry, favorites | **Done.** The playlist editor (§7.3) adds rename, renumber, regroup, hide and bulk edit over channels, movies and series; drag-to-reorder, custom logos, named favourite lists and the user-defined rules engine are not built. |
| 5 — EPG | XMLTV ingest, matching, guide grid, info pane | **Done** |
| 6 — Movies | Metadata enrichment, rails, hero, hover preview, detail modal, browse | **Done, but unverified against the real API.** TMDB matching, the client, credits storage, the artwork disk cache and the settings panel are built and tested; nothing has run with a real key. The cache downloads, evicts and reports — but the UI still renders from remote URLs, because *serving* from it needs Tauri's asset protocol confirmed on hardware (same gate as Phase 0). |
| 7 — Series | Seasons, episodes, detail tabs, skip markers, Up Next | **Done.** Skip Intro/Recap/Credits, the Next Episode button, Up Next autoplay, and per-show auto-skip/autoplay preferences. |
| 8 — DVR | Recording, timeshift, catch-up | **Recording and catch-up done; timeshift not started.** Scheduling with padding, conflict detection against the connection limit, series rules, reminders, a quota, and a recordings library. The recorder writes MPEG-TS to disk and has never been pointed at a real provider. Catch-up builds the four common URL conventions and plays from the guide; the conventions come from documentation, not from observed traffic. Timeshift (pause live TV) is not built. |
| 9 — Personalization | Profiles, parental controls, search, palette | **Done.** Profiles with PINs and a picker, certification ceilings, kids profiles, adult categories hidden by default, attempt throttling. Per-channel/category locks are stored but have no UI yet. |
| 13 — First run | Wizard: add provider, validate, import | **Done** (README §13). |
| 10 — QoL | §13 list, TV mode, multi-view, PiP, remote | **Partial:** themes, TV density, reduce-motion, keyboard map, command palette. Multi-view, PiP, sleep timer, tray, backup/restore not built. |
| 11 — Hardening | Perf budgets, soak, diagnostics | **Not started.** No §16 budget is measured yet. One known correctness issue is listed below. |
| 12 — Release | Installers, signing, auto-update | **CI type-checks Windows; no installer is produced.** |

## What is actually verified

| Claim | Evidence |
|---|---|
| Parsers handle hostile real-world input | 173 `aurora-core` tests, run on every commit |
| Schema, migrations, reconciliation, search | 135 `aurora-db` tests |
| Playback state machine and error taxonomy | 13 `aurora-player` tests |
| Fetching, retry, gzip, credential redaction | 109 `aurora-ingest` tests, against a server that simulates 401/403/404/429/timeout/mid-stream disconnect |
| A refresh never destroys user data | `aurora-ingest::sync` tests assert renames, numbers and hidden flags survive |
| Skip markers: chapter parsing, learning, merge | 25 `aurora-core` + 13 `aurora-db` tests |
| The Tauri host compiles and its URL resolution works | 21 `aurora-app` tests (Linux, with GTK dev packages) |
| Recording scheduling: padding, conflicts, rule matching | 19 `aurora-core` + 32 `aurora-db` tests |
| A stream is written to disk, and a cut stream keeps what it got | 7 `aurora-ingest` tests against the failure-simulating server |
| The scheduler starts, stops and finalises recordings | 11 `aurora-app` tests driving `Dvr::tick` on a test clock |
| A recording title that Windows would reject becomes a legal filename | 10 `aurora-core` tests (`CON`, `Ratched: Season 1`, trailing dots, MAX_PATH) |
| The mpv/Win32 backend compiles for Windows | `cargo check --target x86_64-pc-windows-msvc` |
| Metadata matching declines rather than guessing | 15 `aurora-core` tests: sequels, remakes, ambiguous titles, foreign originals |
| TMDB responses are parsed, including the ones missing half their fields | 16 `aurora-ingest` tests |
| Enrichment records every outcome and never asks twice | 15 `aurora-db` + 11 `aurora-ingest` tests |
| Artwork is cached, evicted and survives a crashed download | 21 `aurora-ingest` tests |
| Every screen renders and the journeys work | 45 Playwright runs against the production bundle |
| Video actually decodes and composites | **Not verified anywhere yet** — Phase 0 |
| A recording survives a real provider's stream | **Not verified** — the recorder has only met the test server |
| TMDB's real responses match what the client expects | **Not verified** — parsed from the documented shape, never called with a key |
| The WebView can load a cached image | **Not verified** — `artwork::asset_url` builds the URL from Tauri's documented format, and nothing has run the app to confirm the asset protocol serves it |

## The known issue worth fixing before release

`providers_refresh` holds the single writer connection across the whole of `sync::run`,
which fetches the playlist and a potentially very large EPG over the network. Every other
command blocks for the duration.

That used to be a responsiveness problem and is now a correctness one, because the DVR
scheduler takes the same lock every ten seconds to decide whether a recording is due. A
recording that falls inside a long refresh does not start until the refresh finishes.

`metadata::enrich_batch` shows the shape of the fix and has a test that fails if the lock
is ever held across the network again: plan under the lock, fetch without it, write under
it again. Applying the same to `sync::run` means splitting it into fetch and apply halves,
which changes what partial state a failed import can leave behind — worth doing
deliberately rather than in passing. The alternative is giving the DVR its own connection,
which WAL supports but which is an architectural decision, not a patch.

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
2. **Point it at a real subscription.** `cargo run -p aurora-ingest --example probe`
   does this without writing anything — see docs/BUILDING.md. The first attempt already
   found two bugs: a bare panel host could not be entered in the wizard at all, and
   every refused connection was reported as a DNS failure because reqwest's error text
   embeds the URL and every Xtream URL contains `username=`.

   Previously: Ingestion is built and tested against a local
   server, but has never met an actual provider — the fork-tolerance in
   `aurora_core::xtream` is written from the spec, not from observed traffic.
3. **Timeshift** — pause and rewind live TV, the half of Phase 8 that recording does
   not cover. It needs a ring buffer on disk and a player that can seek inside a
   still-growing file, which is a different problem from scheduling.
4. **Catch-up against a real provider.** `aurora_core::catchup` builds the four
   conventions panels use (Xtream `timeshift.php`, append, shift, flussonic) and
   "Watch from start" plays them, but which convention a given panel actually honours
   is only observable with a subscription. The refusals are deliberate: a channel that
   advertises catch-up without saying how to ask for it gets a message, not a guessed
   URL that fails silently at the player.
5. **Six contract commands the host does not implement.** `shared/ipc.ts` declares
   them, the mock answers them, and `main.rs` registers no handler, so they work in the
   browser build and fail on Windows: `library.rails` (the whole home page),
   `library.stats` (the settings Library panel), `mylist.toggle`, `favorites.toggle`,
   `progress.get` and `player.setSpeed`. `library.series` and `library.genres` were in
   the same state until the filtering work needed them and built them. The pattern is
   worth fixing at the root: nothing makes the contract and the registration list agree,
   and a missing command is invisible until someone runs the real host. A test that
   walks `Commands` and asserts a registered handler for each would have caught all
   eight.

6. **The filters against a real playlist.** `aurora_core::lang` is written from the
   conventions playlists use and tested against names shaped like them, but the only way
   to know how much of a real 10,000-entry subscription it can classify is to point
   `examples/probe.rs` at one and read the language histogram. The failure mode to watch
   for is the opposite of the obvious one: not content wrongly hidden, but a provider
   whose tagging is so sparse that "English only" hides almost nothing.

7. **Serve artwork from the cache.** The cache downloads and evicts; the UI still
   points at remote URLs. Closing that needs `assetProtocol` enabled in
   `tauri.conf.json`, scoped to the cache folder, and the swap done with a fallback to
   the remote URL so a misconfigured protocol degrades to today's behaviour rather than
   breaking every image. It is one config flag and one component change, but neither is
   verifiable without running the app.
