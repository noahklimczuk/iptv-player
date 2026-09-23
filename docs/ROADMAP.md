# Roadmap

Phases mirror `README.md` §21. Status is honest about what is *verified* versus merely
*written* — see the verification table below.

| Phase | Scope | Status |
|---|---|---|
| 0 — Spike | libmpv behind transparent WebView2 | **Written, compiles for Windows, NOT run.** See the caveat below. |
| 1 — Foundation | Workspace, typed IPC, SQLite + migrations, tokens, app shell, CI | **Done** |
| 2 — Ingestion | M3U + Xtream + XMLTV fetching and parsing, classification, series grouping, rules | **Done.** aurora-ingest fetches, parses, reconciles and indexes; credentials go to the OS store. Stalker portals (§4.3, Optional) and per-series episode listings are not built. |
| 3 — Player | Playback service, OSD, shortcuts | **Trait, NullBackend, mpv backend, OSD and hotkeys done.** Tuning now tries a channel's sources in order and rolls over when one dies, and a host-side heartbeat emits the `player.state` event the UI has always listened for and nothing ever sent. Real decoding is still unverified. |
| 4 — Live TV | Channel list, zap, banner, number entry, favorites | **Done.** The playlist editor (§7.3) adds rename, renumber, regroup, hide and bulk edit over channels, movies and series; drag-to-reorder, custom logos, named favourite lists and the user-defined rules engine are not built. |
| 5 — EPG | XMLTV ingest, matching, guide grid, info pane | **Done** |
| 6 — Movies | Metadata enrichment, rails, hero, hover preview, detail modal, browse | **Done, but unverified against the real API.** TMDB matching, the client, credits storage, the artwork disk cache and the settings panel are built and tested; nothing has run with a real key. The cache downloads, evicts and reports — but the UI still renders from remote URLs, because *serving* from it needs Tauri's asset protocol confirmed on hardware (same gate as Phase 0). |
| 7 — Series | Seasons, episodes, detail tabs, skip markers, Up Next | **Done.** Skip Intro/Recap/Credits, the Next Episode button, Up Next autoplay, and per-show auto-skip/autoplay preferences. |
| 8 — DVR | Recording, timeshift, catch-up | **Done.** Scheduling with padding, conflict detection against the connection limit, series rules, reminders, a quota, and a recordings library. The recorder writes MPEG-TS to disk and has never been pointed at a real provider. Catch-up builds the four common URL conventions and plays from the guide; the conventions come from documentation, not from observed traffic. Timeshift pauses, rewinds and returns to live on mpv's own on-disk cache (D21); the arithmetic behind its scrub bar is tested, the cache itself has never been filled by a real stream. |
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
| The host answers every command the UI declares, and sends every `PlayerState` field it declares | `aurora-app/tests/contract.rs`, which reads `shared/ipc.ts` and diffs it against the handler list and against the serialized struct |
| Recording scheduling: padding, conflicts, rule matching | 19 `aurora-core` + 32 `aurora-db` tests |
| A stream is written to disk, and a cut stream keeps what it got | 7 `aurora-ingest` tests against the failure-simulating server |
| The scheduler starts, stops and finalises recordings | 11 `aurora-app` tests driving `Dvr::tick` on a test clock |
| A recording title that Windows would reject becomes a legal filename | 10 `aurora-core` tests (`CON`, `Ratched: Season 1`, trailing dots, MAX_PATH) |
| The rewind window is bounded by bytes, minutes *and* how long the channel has been on | 16 `aurora-core` tests: a 4K feed in a gigabyte, a radio stream in the same gigabyte, ten seconds after a zap |
| Pausing live TV falls behind it, and rewinding stops at the oldest moment held | 8 `aurora-player` + 5 `aurora-app` tests on a modelled stream whose live edge the test advances |
| Turning the buffer off actually stops it | An `aurora-player` test that the off state is emitted rather than left unsaid — mpv's properties outlive a load |
| The mpv/Win32 backend compiles for Windows | `cargo check --target x86_64-pc-windows-msvc` |
| Metadata matching declines rather than guessing | 15 `aurora-core` tests: sequels, remakes, ambiguous titles, foreign originals |
| TMDB responses are parsed, including the ones missing half their fields | 16 `aurora-ingest` tests |
| Enrichment records every outcome and never asks twice | 15 `aurora-db` + 11 `aurora-ingest` tests |
| Artwork is cached, evicted and survives a crashed download | 21 `aurora-ingest` tests |
| Every screen renders and the journeys work | 82 Playwright runs against the production bundle |
| Video actually decodes and composites | **Not verified anywhere yet** — Phase 0 |
| A recording survives a real provider's stream | **Not verified** — the recorder has only met the test server |
| TMDB's real responses match what the client expects | **Not verified** — parsed from the documented shape, never called with a key |
| The WebView can load a cached image | **Not verified** — `artwork::asset_url` builds the URL from Tauri's documented format, and nothing has run the app to confirm the asset protocol serves it |
| mpv keeps a live stream on disk and can be seeked into it | **Not verified** — `cache-on-disk`, `demuxer-max-back-bytes` and `force-seekable` are set from mpv's documentation, and `demuxer-cache-state` is read from its documented shape; no stream has been buffered |

## The refresh lock — fixed

`providers_refresh` used to hold the single writer connection across the whole of
`sync::run`, which downloads a playlist and a potentially very large guide. Every other
command blocked for the duration, and because the DVR scheduler takes the same lock
every ten seconds to ask whether a recording is due, a recording falling inside a long
refresh did not start until the refresh ended. A responsiveness problem that had become
a correctness one.

`sync::run` is now `sync::fetch` followed by `sync::apply`. The split is in the types
rather than in a convention: `fetch` is handed no `Connection` and `apply` is handed no
`HttpClient`, so putting a download back under the lock means changing a signature and
reading why. The host calls the halves separately and takes the lock only for `apply`.

Two tests hold it: one applies an import with the test server already shut down, and one
runs a contender thread standing in for the DVR and fails if it is locked out. Reverting
to the old shape takes that thread from dozens of acquisitions to zero.

What it did not change: `apply` still runs under the lock, which is bounded local work
rather than a network wait. And a failed import leaves *less* behind than before — a
download that fails now writes nothing at all, where previously it could abort partway
through writing.

## The contract and the host — fixed

`shared/ipc.ts` declared commands the host never registered, and nothing noticed. Eight
of them: `library.rails` (the whole home page), `library.stats` (the Settings counts),
`library.series`, `library.genres`, `mylist.toggle`, `favorites.toggle`, `progress.get`
and `player.setSpeed`. Each answered "command not found" on Windows while the mock
transport answered all of them, so the browser preview looked complete and the shipped
app had no home screen.

They are implemented, and `crates/aurora-app/tests/contract.rs` is what keeps it that
way: it reads the `Commands` interface out of `shared/ipc.ts`, reads the
`generate_handler!` list out of `main.rs`, and fails naming any command the UI can call
and the host cannot answer. The two sides are in different languages and the only thing
worth pinning is that the names line up — so that is all it pins, and a command added
without a handler now fails on Linux in a second instead of on Windows in a month.

The same drift had happened one layer down, in the payload rather than the name.
`PlayerState` declared `itemKind`, the mock filled it in, and the host's struct had no
such field — nor did it ever set `channelId` or `itemId`, which were declared, defaulted
to `None` and never written. The UI gates Skip Intro, Skip Recap, Skip Credits, the Next
Episode button and Up Next autoplay on `player.itemKind === 'episode'`
(`useEpisodeAids`), so every one of them worked in every browser journey and not one of
them could ever have appeared in the shipped app. What is playing now travels with the
load — one `Playing { kind, id, channel_id }` on `LoadOptions`, so the three cannot
disagree — and the contract test serializes the real `PlayerState` and diffs its keys
against the interface in both directions, which fails on a field the UI believes in and
on a field the host sends that nothing declares.

## The Phase 0 caveat

README §21 requires the compositing spike to be proven before anything else is built.
That needs a Windows machine with WebView2 and `mpv-2.dll`; this was built on Linux. The
backend exists (`aurora-player/src/mpv.rs`) and type-checks for `x86_64-pc-windows-msvc`,
but **compiling is not proving** — and less is wired than a file list suggests.

Three things the spike needs are written and never called, so a run today would show no
video for reasons that have nothing to do with compositing:

- `MpvBackend::attach` creates the child HWND video renders into and hands mpv its `wid`.
  Nothing calls it, so there is no video surface and mpv has nowhere to draw.
- `window::attach_video_surface` — the host half — is not called either, and does not
  call `attach`; it makes the WebView2 background transparent and resizes a surface that
  does not exist yet. (Despite the name, the rest of `window.rs` is URL resolution.)
- `MpvBackend::pump` drains mpv's event queue and is the only thing that updates
  position, tracks, buffering, errors and the timeshift window. Nothing calls it, so the
  state the OSD mirrors would never change after a load. The player heartbeat in
  `main.rs` reads that state every 250 ms; it does not produce it.

None of it is hard, and none of it is worth guessing at from Linux: it is Win32 message
plumbing whose correctness is only observable on the machine it runs on. Budget an hour
of wiring before the questions below can even be asked.

Then, running `cargo tauri dev` on Windows, confirm:

1. Video renders behind the UI, not in a separate window.
2. The UI receives input while video plays underneath.
3. Resizing, snapping and alt-tab do not tear the two surfaces apart.
4. Zap time is under the 1.5 s budget in README §16.

If (1) or (3) fails, the fallback is a native XAML/WinUI shell for the player surface
with the web layer confined to non-playback screens. `aurora-core`, `aurora-db` and
`aurora-player` are all backend-agnostic and would port unchanged — which is why they
were built first.

## Nearest useful next steps

1. **Wire and run the Phase 0 spike on Windows.** Everything else is downstream of that
   answer, and the caveat above lists the three calls that are missing before it can be
   asked.
2. **Point it at a real subscription.** `cargo run -p aurora-ingest --example probe`
   does this without writing anything — see docs/BUILDING.md. The first attempt already
   found two bugs: a bare panel host could not be entered in the wizard at all, and
   every refused connection was reported as a DNS failure because reqwest's error text
   embeds the URL and every Xtream URL contains `username=`.

   Previously: Ingestion is built and tested against a local
   server, but has never met an actual provider — the fork-tolerance in
   `aurora_core::xtream` is written from the spec, not from observed traffic.
3. **Catch-up against a real provider.** `aurora_core::catchup` builds the four
   conventions panels use (Xtream `timeshift.php`, append, shift, flussonic) and
   "Watch from start" plays them, but which convention a given panel actually honours
   is only observable with a subscription. The refusals are deliberate: a channel that
   advertises catch-up without saying how to ask for it gets a message, not a guessed
   URL that fails silently at the player.
4. **The filters against a real playlist.** `aurora_core::lang` is written from the
   conventions playlists use and tested against names shaped like them, but the only way
   to know how much of a real 10,000-entry subscription it can classify is to point
   `examples/probe.rs` at one and read the language histogram. The failure mode to watch
   for is the opposite of the obvious one: not content wrongly hidden, but a provider
   whose tagging is so sparse that "English only" hides almost nothing.

5. **Serve artwork from the cache.** The cache downloads and evicts; the UI still
   points at remote URLs. Closing that needs `assetProtocol` enabled in
   `tauri.conf.json`, scoped to the cache folder, and the swap done with a fallback to
   the remote URL so a misconfigured protocol degrades to today's behaviour rather than
   breaking every image. It is one config flag and one component change, but neither is
   verifiable without running the app.
