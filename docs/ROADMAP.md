# Roadmap

Phases mirror `README.md` §21. Status is honest about what is *verified* versus merely
*written* — see the verification table below.

| Phase | Scope | Status |
|---|---|---|
| 0 — Spike | libmpv behind transparent WebView2 | **Run on Windows 11, and it works.** WebView2 153 and libmpv v0.41: 1920x1080 H.264 at 60fps on `d3d11va-copy`, composited behind the UI, input still reaching the UI over it, and the surface following a resize and a move to a display of another scale factor. `AUDIT/test-report.md` §11 has the numbers and `tests-host/scenarios/video_surface.py` keeps them honest. One miss: a channel change took 1.91s against the 1.5s budget. |
| 1 — Foundation | Workspace, typed IPC, SQLite + migrations, tokens, app shell, CI | **Done** |
| 2 — Ingestion | M3U + Xtream + XMLTV fetching and parsing, classification, series grouping, rules | **Done.** aurora-ingest fetches, parses, reconciles and indexes; credentials go to the OS store. Stalker portals (§4.3, Optional) are not built. Episode listings are: a request each, 28,693 of them on the panel this was measured against, so they are fetched when a show is opened and swept in bounded batches in the background (`aurora-app/src/series.rs`). |
| 3 — Player | Playback service, OSD, shortcuts | **Trait, NullBackend, mpv backend, OSD and hotkeys done.** Tuning now tries a channel's sources in order and rolls over when one dies, and a host-side heartbeat emits the `player.state` event the UI has always listened for and nothing ever sent. **Real decoding is verified** (Phase 0, above). Running it also closed the two failures no suite could reach: a dead stream left the player `Loading` for ever because the event pump stopped at the first `Err` (F-30), and a missing libmpv killed the process in the loader with no window and no log, which the `NullBackend` fallback could not possibly report (F-34). |
| 4 — Live TV | Channel list, zap, banner, number entry, favorites | **Done.** The playlist editor (§7.3) adds rename, renumber, regroup, hide and bulk edit over channels, movies and series; drag-to-reorder, custom logos, named favourite lists and the user-defined rules engine are not built. |
| 5 — EPG | XMLTV ingest, matching, guide grid, info pane | **Done** |
| 6 — Movies | Metadata enrichment, rails, hero, hover preview, detail modal, browse | **Done, and run against the real API.** TMDB was called with a real key for the first time, matching 39 of 80 titles from a panel's catalogue. Artwork is served from the cache: `assetProtocol` is granted at startup rather than in the manifest, because a portable copy keeps its artwork beside the exe and an installed one under `%LOCALAPPDATA%`, and `useAssetSrc` starts from the remote URL so an ungranted scope degrades to what shipped before. The cache warms **on view**, not by prefetching — an unordered `LIMIT` over 117,587 rows cached 39 posters against 117 on screen with no overlap at all (F-35). `AUDIT/test-report.md` §14. |
| 7 — Series | Seasons, episodes, detail tabs, skip markers, Up Next | **Done.** Skip Intro/Recap/Credits, the Next Episode button, Up Next autoplay, and per-show auto-skip/autoplay preferences. |
| 8 — DVR | Recording, timeshift, catch-up | **Done.** Scheduling with padding, conflict detection against the connection limit, series rules, reminders, a quota, and a recordings library. The recorder has now met a real provider: two channels reached `completed` and a third came back `failed — Your provider didn't respond`, which is a stream that accepts the request and sends nothing, reported in words a viewer could act on. What is still unconfirmed is the *file* — the assertion that a recording is larger than 64 kB has not had a green run, because the harness's per-scenario wipe keeps failing on a checkout under OneDrive. Catch-up builds the four common URL conventions and plays from the guide; the conventions come from documentation, not from observed traffic. Timeshift pauses, rewinds and returns to live on mpv's own on-disk cache (D21); the arithmetic behind its scrub bar is tested, and running it found that mpv has no `cache-dir` option at all — `cache-on-disk=yes` was accepted and the directory beside it refused, so the buffer had been landing in mpv's own folder and `bytes_on_disk` was measuring an empty directory. Fixed to `demuxer-cache-dir`, asked of libmpv v0.41 directly. No stream has yet been buffered and seeked into. |
| 9 — Personalization | Profiles, parental controls, search, palette | **Done.** Profiles with PINs and a picker, certification ceilings, kids profiles, adult categories hidden by default, attempt throttling. Per-channel/category locks are stored but have no UI yet. |
| 13 — First run | Wizard: add provider, validate, import | **Done** (README §13). |
| 10 — QoL | §13 list, TV mode, multi-view, PiP, remote | **Partial:** themes, TV density, reduce-motion, keyboard map, command palette, and **multi-view** — 2×2, 3×3, 1+3 and 1+5, one mpv instance per tile, audio focus on `1`–`9`, saved layouts, and a connection-limit check that counts recordings in flight and refuses before opening rather than after. PiP, sleep timer, tray and backup/restore not built. |
| 11 — Hardening | Perf budgets, soak, diagnostics | **Partial.** A 1.0 audit pass fixed 23 findings, two of them Critical (every command on the main thread; a playlist panicking the process) — see `AUDIT/findings.md`. Soak numbers are measured: 2,000 zaps always end on the right channel, 2,400 concurrent tunes across 8 threads never wedge, 20,000 tunes grow RSS by 0 kB. Diagnostics, log rotation and log export exist, and Settings → Diagnostics now names the video engine, because a black rectangle has two completely different causes. Of the §16 budgets that need a window, **zap time has a number and it is over**: 1.91s, 1.96s and 2.13s against 1.5s, on one machine against a public CDN rather than a provider. Startup is still unmeasured. |
| 12 — Release | Installers, signing, auto-update | **Partial.** A merge builds and publishes an NSIS installer *and* the portable zip, both carrying the GPL/LGPL texts and `THIRD-PARTY-NOTICES.md`, which Settings → About reads from beside the executable. **Both kinds of copy swap their own files** — the installed one used to run the installer, which for a standard user meant UAC asking for administrator credentials they do not have, so an installed copy simply could not update itself (D25, checklist 1c). Both paths have now run on Windows: a portable 0.11.0 renamed its own running exe and came back as 0.11.1, and an MSI per-user upgrade kept a real library of 22,121 channels, 117,510 films and 453,072 programmes intact. The per-machine NSIS install, its Start-menu entry and the uninstall sweep need an administrator account, not another machine. Nothing is code-signed — accepted rather than fixed, since the publisher and the user are the same person (checklist 2). |

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
| An update is only kept if its length and SHA-256 match what GitHub published, and only fetched from this project's own releases | 10 `aurora-ingest` + 8 `aurora-app` tests: a wrong digest, a wrong length, a cut connection and four lookalike hosts, each leaving nothing executable behind |
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
| Every screen renders and the journeys work | 154 Playwright runs against the production bundle |
| Video actually decodes and composites | **Verified on Windows 11** — 1920x1080 H.264 60fps, `d3d11va-copy`, 95.4% of a sampled grid over the middle of the window changing between two desktop captures (a still window measures 0.0%). `AUDIT/test-report.md` §11 |
| A recording survives a real provider's stream | **Partly** — two channels reached `completed` and one failed with a reason a viewer could act on, so the recorder works against a provider; the assertion that the file on disk exceeds 64 kB has not had a green run |
| TMDB's real responses match what the client expects | **Verified** — called with a real key, matching 39 of 80 titles from a panel's catalogue and returning posters that downloaded and displayed. `AUDIT/test-report.md` §14 |
| The WebView can load a cached image | **Verified** — `http://asset.localhost/C:/…/artwork/2bf6d0` decoded at 600x900; 117 of 117 images on a screen came from the cache, 0 from the network, 0 broken. The form this project builds leaves `/` and `:` unencoded where Tauri's own `convertFileSrc` percent-encodes them, and it is the one that was run |
| mpv keeps a live stream on disk and can be seeked into it | **Not verified** — but the options are no longer taken from documentation alone: asked directly, libmpv v0.41 has no `cache-dir`, so the buffer had been going to mpv's own folder (fixed to `demuxer-cache-dir`). No stream has yet been buffered and seeked |

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

## What a real subscription showed

Pointed at an actual Xtream panel for the first time: 22,305 live channels, 122,274
films, 28,670 series, one connection, and a server clock in Europe/Paris. Read-only —
`examples/probe`, plus a few seconds of two streams. What it found, because none of it
was guessable from the spec:

- **The country prefix is separated by a star.** 18,460 channel names are `US ★ QVC HD`,
  and U+2605 was not a separator, so the prefix was recognised on 2.1% of the playlist
  and the country went into the match key. Fixed; unclassified went from 98% to 15%.
- **VOD and series carry the same prefix**, and it survives cleaning: `AR ★ …` becomes
  `AR …`, which is the title enrichment sends to TMDB. Not fixed — stripping it changes
  what is displayed, not only what is matched, so it wants a decision rather than a
  guess.
- **`get.php` returns 404.** This panel serves `player_api.php` only, so there is no M3U
  and no `catchup=` attribute anywhere; the Xtream path's `mode: "xc"` is the only mode a
  channel here can get.
- **Catch-up does not work on this panel, by any convention Aurora can build.** 20 of 188
  sampled channels advertise `tv_archive=1` with a two-day window.
  `/streaming/timeshift.php?…` (what `mode: "xc"` builds) returns 404, and so does the
  `/timeshift/{user}/{pass}/{dur}/{start}/{id}.ts` path form. The `?utc=&lutc=` form
  returns 200 — and serves **live**: it answers the same way for a timestamp ten days old
  (outside the advertised window) and for one an hour in the future, redirecting to the
  live path each time. So the refusal Aurora currently gives is the honest outcome, and
  the convention that would have looked like it worked is the one that lies.
- **A latent timezone bug in `catchup::xtream_url`.** It formats `start` in UTC. This
  panel reports `time_now` in Europe/Paris and `server_info.timezone` says so, and Xtream
  panels read that parameter in their own local time — two hours out here. Nothing reads
  `server_info.timezone` at all. Unverifiable against this provider, since the endpoint
  404s, so it is named rather than changed.
- **Real bitrates, for the timeshift budget.** A shopping channel runs at 5.9 Mb/s and an
  HD network at 11.5. One gigabyte therefore holds about 24 minutes of the first and 12
  of the second, so the default 1 GB / 30 min budget binds on bytes here, not on minutes
  — which is exactly what `aurora_core::timeshift` is for, and the scrub bar will offer
  the twelve minutes that exist rather than the thirty the setting names.
- **Streams are plain MPEG-TS over HTTP**, 188-byte aligned, reached by a 302 from the
  Cloudflare front to a bare-IP origin carrying a time-bound token. No range support
  implied, which is the case `force-seekable=yes` exists for. One first request answered
  HTTP 555 from Cloudflare before subsequent ones succeeded.
- **One connection.** `max_connections: 1`. A timeshift buffer of our own would not have
  been merely wasteful on this account; it would have made pausing live TV impossible
  while watching it (docs/DECISIONS.md D21).

## What a real import cost

`examples/import.rs` runs the app's own `sync::fetch` and `sync::apply` against a real
panel into a throwaway database. First time, on the subscription above — 22,305
channels, 122,283 films, a 482,190-programme guide:

| | |
|---|---|
| fetch | 8.7 s (authenticate 0.6, playlist 4.2, guide 3.9) |
| apply | 14.9 s (channels 0.3, movies 1.0, series 3.8, EPG matching 8.6, indexing 1.2) |
| total | **23.7 s** |
| peak memory | 325 MB after fetch, **470 MB** at the end |
| database | **204 MB** |
| search, after | 0–2 ms per query |

Twenty-four seconds and a fifth of a gigabyte is a faster and fatter import than anyone
had guessed at. Three things it settled:

- **The memory D20 warned about is real and is fine.** Holding the whole download
  between the halves costs 325 MB on a library this size. That is the number to watch if
  a provider ever ships a guide several times this one's size, and it is nowhere near a
  problem today.
- **Classification works on real names**: 20,347 of 22,121 channels get a language
  (en 7,417, ar 2,425, fr 1,466, es 1,388, de 821…), and quality ranks are populated.
  This is `lang_code`, not `language` — the latter is what the provider claimed, and
  this panel claims nothing.
- **De-duplication is doing something.** 22,305 channels arrive and 22,121 are stored;
  122,283 films arrive and 117,383 are stored. The provider lists the same stream id
  more than once, roughly 4% of the time for films.

Two things it found that are not fine:

- **No series were imported at all — fixed.** `xtream_entries` fetched all 28,693
  series listings and then discarded them with a warning that episode listings are
  deferred; the *series* were deferred too, so nothing was written. The wizard offered
  "Series" as something to import, Phase 7 was marked Done, and on a real panel the
  Series screen had nothing in it. The rows come from the one call already made, so
  they are written now, under a `series:{id}` key, with their artwork, category and
  year. The episode listings genuinely do cost one request per show and are still
  deferred; the warning says that rather than implying the shows were too.
- **The guide is emptier than its coverage number suggests.** 9,476 channels carry an
  EPG id, which is what "coverage" counts, but only **2,949** have any programmes: the
  XMLTV holds 4,851 channels, and many ids on the panel's channels appear nowhere in it.
  13% of the library has a guide, not 43%.

## The Phase 0 caveat — lifted

README §21 requires the compositing spike to be proven before anything else is built. It
has now been run on Windows 11 with WebView2 153 and libmpv v0.41, and it works: video
decodes with `d3d11va-copy` and composites behind the UI, the UI still takes input over
it, and the surface follows a resize. The numbers are in `AUDIT/test-report.md` §11 and
`tests-host/scenarios/video_surface.py` keeps them honest.

One measurement missed its budget: a channel change took 1.91s against the 1.5s in
README §16. And the run found that mpv has no `cache-dir` option, so the timeshift buffer
had been landing in mpv's own directory rather than the viewer's — fixed, and §12 of the
report explains it.

What follows is the caveat as it stood, which is worth keeping for the reasoning about
what "wired" meant:

> That needed a Windows machine with WebView2 and `libmpv-2.dll`; the audit was done on
> Linux. The backend existed (`aurora-player/src/mpv.rs`) and type-checked for
> `x86_64-pc-windows-msvc`, but **compiling is not proving**.

Three things the spike needed were written and never called, so a run would have shown
no video for reasons that have nothing to do with compositing. **They are wired now**
(`AUDIT/findings.md` F-24):

- `MpvBackend::attach` creates the child HWND video renders into and hands mpv its
  `wid`. It is called from `main.rs` at setup, through the `PlayerBackend` trait —
  which is where it had to go: the app layer holds a `Box<dyn PlayerBackend>` and could
  not otherwise reach it, which is the real reason it was never called.
- `window::attach_video_surface` — the host half — now takes the window's `HWND` and
  passes it on, rather than resizing a surface that did not exist yet.
- `MpvBackend::pump` drains mpv's event queue and is the only thing that updates
  position, tracks, buffering, errors and the timeshift window. `Playback::tick` calls
  it before reading, so the heartbeat produces the state it mirrors rather than only
  reading it.

`tauri.conf.json` also had `"transparent": false`, which alone was enough to show
nothing, and the OSD painted an opaque gradient over the whole window — the difference
between floating over live video and being a gradient with buttons on it.

The part that was always going to need the machine has had it. `aurora-player`
type-checks for `x86_64-pc-windows-msvc`, but `aurora-app` cannot be cross-compiled off
Windows — rustls's `ring` wants an MSVC C compiler — so CI's Windows job was the first
thing ever to compile `window.rs` and `main.rs`.

The four questions this section asked, answered on the machine (`AUDIT/test-report.md`
§11):

1. **Video renders behind the UI, not in a separate window.** One top-level window for
   the process; mpv's `STATIC` surface is a `WS_CHILD` of it, last of two children in
   z-order.
2. **The UI receives input while video plays underneath.** A click on the OSD changes
   what is on screen.
3. **Resizing does not tear the two apart.** After a resize, client `1220x740` and
   surface `1220x740`; moved to a second display of a different scale factor the surface
   follows exactly. Snapping and alt-tab were not separately exercised.
4. **Zap time is 1.91s, against the 1.5s budget** — the one answer that missed.

So the fallback is not needed: a native XAML/WinUI shell for the player surface, with
the web layer confined to non-playback screens, was what (1) or (3) failing would have
cost. `aurora-core`, `aurora-db` and `aurora-player` are all backend-agnostic and would
have ported unchanged — which is why they were built first, and it remains the reason
the option is still open if compositing ever stops working.

## Nearest useful next steps

1. **Run the Phase 0 spike on Windows. Done — there is a picture.** Everything else was
   downstream of it. The diagnostic it left behind is still the right one if a build ever
   comes up black: Settings → Diagnostics names the video engine, and the log says which
   step failed — `video surface ready`, `no video surface: …`, or `no main window at
   setup`.
2. **Import the series. Done.** A full import found 28,693 series fetched from the
   panel and thrown away, so the Series screen was empty on a real subscription while
   Phase 7 was marked Done. The rows are written now, with everything `get_series`
   already sent and nothing ever kept — genre, plot, rating, added date
   (`docs/DECISIONS.md` D26). Their *episode* listings are a separate request each,
   28,693 of them, so they are fetched when a show is opened and swept in bounded
   batches in the background (`aurora-app/src/series.rs`); a card no longer sits at
   "0 seasons" for ever.

   Earlier attempts found: a bare panel host could not be entered in the wizard at all,
   and every refused connection was reported as a DNS failure because reqwest's error
   text embeds the URL and every Xtream URL contains `username=`.
3. **Catch-up against a real provider.** `aurora_core::catchup` builds the four
   conventions panels use (Xtream `timeshift.php`, append, shift, flussonic) and
   "Watch from start" plays them, but which convention a given panel actually honours
   is only observable with a subscription. The refusals are deliberate: a channel that
   advertises catch-up without saying how to ask for it gets a message, not a guessed
   URL that fails silently at the player.
4. **The filters against a real playlist. Done — and the feared failure is not there.**
   `probe.rs` now prints the histogram, run over 6,698 real channels:

   ```
   languages         en=6514  es=174  it=2  bn=1  de=1  hi=1  no=1  pl=1
   classified        6698 of 6698 (100%), 0 unknown
   via group only    1098 (16% of classified)
   "English only"    keeps 6514, removes 184, cannot judge 0
   ```

   The worry was a provider tagged so sparsely that "English only" hides almost nothing
   and nobody can tell whether that is because the list really is English or because the
   classifier has no opinion. It removes 2.7% here — but **nothing went unclassified**,
   which is what settles it: the list really is almost all English. The probe now says
   which of the two it is rather than leaving the number to be read either way.

   Two things worth knowing from the same run. 1,098 channels (16%) are classified only
   by their category name — `24/7 ★ Bewitched` says nothing itself — so a provider that
   renames categories reclassifies them silently. And `split_country_prefix` reads `NCIS`
   as a country code on 3 channels, which is the documented cost of the `★` separator and
   0.04% of the list; tightening it to known codes would break this provider's own
   non-standard `QFR`, `LAT` and `CAF` prefixes, so it stays.

5. **Serve artwork from the cache. Done, and the plan was wrong about one thing.** The
   scope is granted at startup rather than in `tauri.conf.json`, because no single path
   in the manifest covers both a portable copy's folder and an installed one's
   `%LOCALAPPDATA%`; `useAssetSrc` starts from the remote URL, so an ungranted scope
   degrades to the old behaviour rather than breaking every image. What only running it
   could show: **prefetching cannot work at this size** — an unordered `LIMIT` over
   117,587 rows cached 39 posters while 117 were on screen, with no overlap at all
   (F-35) — so the cache warms on view instead.

### What is actually left

Everything above is answered. What remains needs something this repository cannot
supply, and `AUDIT/release-checklist.md` is the authority on it:

- **Catch-up on a panel that answers** (item 3, F-23). Two subscriptions now advertise
  `tv_archive` and neither serves one: eighteen requests across three channels at 20, 60
  and 180 minutes back, with `start` spelled both ways, returned 404 or an empty 200. The
  UTC/local timezone fix cannot be made against evidence until a panel answers.
- **That a recording is a file, not just a state** (item 3). Needs a run of
  `python tests-host/run.py real_panel` with panel credentials, on a checkout that is not
  under OneDrive — its cloud placeholders deny the per-scenario wipe.
- **The zap budget.** 1.91s against 1.5s, measured against a public CDN rather than a
  provider. Worth re-measuring against a real panel before optimising anything, since the
  number may be the CDN.
- **Install and uninstall** (item 5). Needs an administrator account: Tauri's NSIS build
  requests elevation before it parses `/CURRENTUSER`. The upgrade half is done.
- **Code signing** (item 2) — accepted, not fixed, while the publisher and the user are
  the same person. Revisit the moment anyone else is asked to install this.
- **Not built, by choice rather than oversight:** Stalker portals; drag-to-reorder,
  custom logos, named favourite lists and the user-defined rules engine (Phase 4);
  multi-view, PiP, sleep timer, tray and backup/restore (Phase 10); a UI for the
  per-channel and per-category locks already stored (Phase 9); media keys and global
  shortcuts.
