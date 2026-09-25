# Architecture — as built, as measured

Phase 1 recon for the 1.0 hardening pass. This is a *map of what the code actually
does*, written from reading it and building it, not from the design docs. Where it
disagrees with `docs/ARCHITECTURE.md` the disagreement is called out, because those
gaps are where the bugs are.

Companion documents: `AUDIT/findings.md` (every defect), `AUDIT/test-report.md`
(what was run and what came back), `AUDIT/release-checklist.md` (what a human still
has to do).

---

## 1. Project context

| | |
|---|---|
| **Name** | Aurora TV (`aurora-tv`, crate `aurora-app`) |
| **Language / UI** | Rust 2021 (workspace, 5 crates) + React 18 + TypeScript 5.6 + Vite 6 + Tailwind v4, hosted by **Tauri 2.11** |
| **Playback engine** | **libmpv** via `libmpv2-sys`, `#[cfg(windows)]`, rendering into a child HWND composited *under* a transparent WebView2 |
| **Persistence** | SQLite via `rusqlite` 0.32 (`bundled`), forward-only migrations, FTS5 search index |
| **Supported sources** | M3U / M3U8 (URL or file), Xtream Codes `player_api.php`, XMLTV EPG (plain or gzip). **No Stalker portal** (deferred, `docs/DECISIONS.md` D13) |
| **Features** | Live TV, VOD, series, catch-up, EPG grid, DVR (scheduled recording + series rules + reminders + quota), timeshift/pause-live, favourites, My List, watch progress, multi-profile with Argon2 PINs, parental certification ceilings, TMDB metadata enrichment, artwork disk cache, playlist editor, library filters, themes, command palette, GitHub-release auto-update |
| **Target OS** | Windows 10 22H2+ / Windows 11, **x64 only** (no ARM64 target configured) |
| **Distribution** | NSIS installer + MSI via `tauri build`, plus a portable zip. Published per-merge to GitHub Releases. **Unsigned.** |

### What is verified vs. what is written

The player is the part nobody has run. `aurora-player/src/mpv.rs` compiles for
`x86_64-pc-windows-msvc` and has never decoded a frame; `MpvBackend::attach` and
`MpvBackend::pump` are written and **called from nowhere**, so the video surface is
never created and mpv's event queue is never drained. Everything upstream of the
player — parsing, storage, matching, scheduling, HTTP — is exercised by 712 Rust
tests and 84 Playwright journeys on this machine.

---

## 2. Crate map and dependency direction

```
                     ┌───────────────────────────────┐
                     │  src-ui (React 18 + TS)       │
                     │  features/, components/, ipc/ │
                     └──────────────┬────────────────┘
                                    │ window.__TAURI_INTERNALS__.invoke
                                    │ (mock transport when absent — D4)
                     ┌──────────────▼────────────────┐
                     │  shared/ipc.ts                │  hand-kept mirror,
                     │  Commands / Events / models   │  pinned by contract.rs
                     └──────────────┬────────────────┘
                                    │
   ┌────────────────────────────────▼──────────────────────────────────┐
   │ aurora-app  (Tauri 2 host)                                        │
   │   commands.rs  library.rs  playlist.rs  profiles.rs  providers.rs │
   │   dvr.rs (scheduler + 15 commands)   metadata.rs   updates.rs     │
   │   timeshift.rs   playback.rs (failover + heartbeat)   window.rs   │
   │   services.rs — one Arc<Mutex<Connection>>, one player, one http  │
   └───┬──────────────┬─────────────────┬────────────────┬─────────────┘
       │              │                 │                │
 ┌─────▼──────┐ ┌─────▼───────┐  ┌──────▼──────┐  ┌──────▼────────┐
 │aurora-core │ │ aurora-db   │  │aurora-ingest│  │ aurora-player │
 │ pure, I/O- │ │ rusqlite    │  │ reqwest     │  │ trait +       │
 │ free       │◄─┤ schema,    │  │ blocking,   │  │ NullBackend   │
 │ parsers,   │  │ repo/*     │◄─┤ sync, epg,  │  │ + MpvBackend  │
 │ matching,  │  │ migrations │  │ tmdb,       │  │ (#[cfg(win)]) │
 │ rules      │  │ FTS5       │  │ artwork,    │  └───────────────┘
 └────────────┘  └────────────┘  │ recorder,   │
                                 │ credentials │
                                 └─────────────┘
```

`aurora-core` depends on nothing platform-specific — that is what keeps the majority
of a Windows-only app testable on a Linux CI runner (`docs/DECISIONS.md` D2/D3).
`aurora-app` is excluded from `default-members` because Tauri needs GTK on Linux and
MSVC on Windows.

### Dependency versions that matter

| Crate | Version | Note |
|---|---|---|
| `tauri` | 2.11.6 | 93 commands, **all declared `#[tauri::command]` without `async`** — see §7 |
| `rusqlite` | 0.32 (bundled SQLite) | one writer connection, no read pool |
| `reqwest` | blocking, rustls | gzip on, redirect limit 5 |
| `libmpv2` / `libmpv2-sys` | Windows only | links `mpv.lib`, needs `mpv-2.dll` beside the exe |
| `argon2` | 0.5 | PIN hashing |
| `keyring` | Windows only | Credential Manager |
| `react-router-dom` | 6.30.6 | **advisory GHSA open-redirect, fixed only in 7.18** |
| `vitest` | 2.1.9 | dev-only; **no test files exist**, `pnpm test` exits 1 |

---

## 3. Data flow: "user adds playlist" → picture on screen

This is the spine the audit follows. Each arrow is a real function call.

### 3.1 Add a provider (first run wizard, `SetupWizard.tsx`)

```
paste text
  → providers.detect         providers_detect()
      parse_pasted_xtream()   → splits get.php?username=…&password=… into parts
      looks_like_panel_root() → a bare host defaults to Xtream so the fields appear
  → providers.validate       providers_validate()
      xtream: missing_sign_in() guard, then XtreamClient::authenticate()
              → player_api.php, AuthResponse{user_info, server_info}
              → expiry, max_connections, is_trial surface in the wizard
      m3u:    playlist::fetch() → HttpClient::fetch_reader() → m3u::parse()
  → providers.save           providers_save()
      INSERT INTO providers (…, credential_ref NULL)
      credentials.set("aurora-provider-{id}", password)   ← Windows Credential Manager
      UPDATE providers SET credential_ref = …             ← two writes, not atomic (F-23)
```

### 3.2 Refresh / import (`providers_refresh`)

Split in two by type so a download can never hold the database (`D20`):

```
fetch(http, options, rules, on_progress) -> Fetched     [no Connection in scope]
 ├ M3u    : playlist::fetch → m3u::parse (streaming, line by line)
 │            header url-tvg/x-tvg-url → extra EPG sources
 ├ Xtream : authenticate → get_live_categories / get_live_streams
 │                       → get_vod_categories / get_vod_streams
 │                       → get_series            ← FETCHED AND DISCARDED (F-04)
 │            stream_url() builds /live|movie|series/{user}/{pass}/{id}.{ext}
 ├ rules.apply(entry) per entry — hidden entries never reach the library
 ├ classify::classify(name, url, group) → Live | Movie | Episode
 └ for each EPG url: fetch_epg → http (gzip auto) → xmltv::parse (pull parser)
        channels collected in memory, programmes batched 1000 at a time

apply(&mut Connection, fetched, options, on_progress) -> SyncReport   [no HttpClient]
 ├ channels::upsert_batch     ON CONFLICT(provider_id, provider_key) DO UPDATE
 │     custom_name / custom_number / custom_logo / custom_group / hidden / sort_order
 │     are deliberately absent from the UPDATE list (README §4.6)
 ├ channels::stale            not-seen rows kept and badged, never deleted
 ├ write_channel_sources      one row per URL → failover list
 ├ library::upsert_movies     title::clean_movie_title strips release tags, year, quality
 ├ series::group_series       flat SxxEyy entries → show/season/episode
 ├ epg_repo::upsert_channels + insert_batch
 ├ EpgIndex::build + resolve  tvg-id exact → normalised → fuzzy → manual override
 ├ reindex()                  rebuild FTS5 rows for this provider
 └ filtering::reclassify()    lang_code + quality_rank columns for query-time filters
```

`Phase` progress is emitted as a `ingest.progress` Tauri event throughout.

### 3.3 Browse → select a channel

```
App.tsx mounts
  → useCommand('providers.list')           providers_list
  → useCommand('channels.list', {})        channels_list  ← NO LIMIT (F-08)
        channels::list(ChannelFilter::default())
            SELECT … FROM channels WHERE hidden = 0 AND is_radio = 0 …
            ORDER BY COALESCE(custom_number, number, 999999), sort_order, name
  → bindPlayerState()                      player.state + 'player.state' event listener
LivePage / GuidePage / HomePage each fetch their own slices
  epg.nowNext, epg.gridSlice, library.rails, library.movies, …
```

### 3.4 Tune (the zap path)

```
click / Ch+ / digit entry
  → useZapper.tune(ch)   showBanner(); void invoke('player.play', {kind:'live', id})
  → player_play          commands.rs
  → Playback::play_live(channel_id, now)
       window::live_sources(db, channel_id, now)
          channels::list(FULL LIST) .find(id)           ← O(library) per zap (F-07)
          sources::for_channel(db, id, now)             ← D16 health ordering
       timeshift::cache_for(db, data_dir)               ← per-tune, so settings apply
       for each candidate:
           player.load(url, LoadOptions{is_live, cache_secs, title, playing, timeshift})
           ok   → sources::record_ok(id, now)   and stop
           err  → sources::record_failure(id, now) and try the next
  → MpvBackend::load → mpv set_property(per-load opts) → loadfile
```

### 3.5 Playback state → OSD

```
thread "aurora-player" every 250 ms
  → Playback::tick(now)
       player.state()                       ← MpvBackend caches state; pump() would
                                              refresh it, and pump() is never called
       status == Error → recover()          ← rolls to the next source, max 6
       changed?        → emit "player.state"
  → onPlayerState() in src-ui/src/ipc/index.ts
  → useUi.setPlayer(state)  → PlayerOverlay, ChannelBanner, SkipButton, UpNextCard
```

### 3.6 EPG overlay

```
GuidePage
  → epg.gridSlice {startUnix, endUnix, channelIds}
       epg::grid_slice — programmes joined to channels by epg_channel_id
  → ChannelBanner → epg.nowNext {channelIds}
Programme selection → info pane → "Watch from start"
  → player.playCatchup {channelId, start, stop}
  → window::resolve_catchup → catchup::Mode::parse → catchup::url_for
       xc        → /streaming/timeshift.php?…&start=YYYY-MM-DD:HH-MM  (UTC — F-16)
       append    → template substitution with ${Y}${m}${d}…
       shift     → ?utc=…&lutc=…
       flussonic → …/timeshift_abs-{start}.m3u8
```

### 3.7 DVR

```
thread "aurora-dvr" every 10 s  →  Dvr::tick(now)
   reap finished → stop expired → start due
   start: window::live_sources → StreamRecorder::start(url, path)
          → its own thread copying bytes to a .ts file (D7, no remux)
   respects max_concurrent = MIN(max_connections) across enabled providers
   emits "dvr.tick" with what changed
CloseRequested → services.dvr.shutdown(now) → finalise in-flight recordings
```

---

## 4. The compositing model (Windows, unverified)

```
Top-level HWND (Tauri window)
├── mpv child HWND      z-order BELOW, libmpv draws here
└── WebView2 child HWND z-order ABOVE, transparent background, owns ALL input
```

`window::attach_video_surface` sets the WebView background transparent and calls
`player.resize(...)`. **Neither it nor `MpvBackend::attach` is called from anywhere**,
so on a real Windows run today mpv has no `wid` and there is no surface to draw into.
`MpvBackend::pump` — the only thing that would update position, tracks, buffering,
errors and the timeshift window from mpv's event queue — is likewise never called.
The 250 ms heartbeat in `main.rs` *reads* that state; it does not produce it.

---

## 5. Threading model — as designed vs. as built

`docs/ARCHITECTURE.md` says:

> The Tauri main thread owns windows and the event loop only. Ingestion, parsing, EPG
> import, and metadata enrichment run on a Tokio pool.

**That is not what the code does.** There is no Tokio pool and no `async` anywhere in
the host. All 93 commands are declared `#[tauri::command]` with a synchronous `fn`,
and `tauri-macros` compiles that to `ExecutionContext::Blocking` → the command body
runs inline in the IPC handler, i.e. on the **main thread**. Concretely:

| Command | Measured / expected cost | Runs on |
|---|---|---|
| `providers.refresh` | 23.7 s on a real subscription (`docs/ROADMAP.md`) | main thread |
| `providers.validate` | one network round trip, up to 12 s connect timeout | main thread |
| `metadata.run` | bounded batches of TMDB requests | main thread |
| `updates.download` | a ~10 MB installer over HTTP | main thread |
| `channels.list` | 22 305 rows sorted and serialised | main thread |

Threads that *are* real:

| Thread | Created in | Loop |
|---|---|---|
| `aurora-player` | `main.rs` | 250 ms → `Playback::tick` → emit `player.state` |
| `aurora-dvr` | `main.rs` | 10 s → `Dvr::tick` → emit `dvr.tick` |
| `aurora-updates` | `main.rs` | one-shot, 8 s after launch |
| recorder threads | `StreamRecorder::start` | one per in-flight recording |

None of the three named threads has a panic guard: if the closure panics the thread
dies silently and that feature stops for the rest of the session.

Locking: a single `Arc<parking_lot::Mutex<Connection>>` serialises **every** read and
write, plus `Arc<Mutex<Box<dyn PlayerBackend>>>`, plus `Playback::session` and
`Playback::last`. `Playback` takes the player lock *per operation*, not across a whole
tune, so two concurrent tunes (rapid zapping, or a zap racing the heartbeat's
`recover()`) can interleave and leave the wrong stream playing.

---

## 6. IPC contract

`shared/ipc.ts` is the single declaration; `crates/aurora-app/tests/contract.rs`
enforces it in two directions:

1. every `CommandName` in the TS interface has a handler in `generate_handler!`
2. the serialised `PlayerState` struct's keys equal the TS `PlayerState` keys

`tests/command_args.rs` additionally pins the calling convention: every command takes
its payload as one parameter literally named `args`, because Tauri keys the invoke
payload by parameter name.

Transport: `src-ui/src/ipc/index.ts` picks `window.__TAURI_INTERNALS__.invoke` when
hosted, else an in-memory mock (`src-ui/src/ipc/mock.ts`, 1 680 lines of synthetic
fixtures). Command names are converted `library.listChannels` → `library_list_channels`.

Events, all one-way host → UI: `player.state`, `ingest.progress`, `dvr.tick`,
`metadata.progress`, `artwork.progress`, `update.available`, `update.download`.
Each has a `listen()` wrapper in `ipc/index.ts` that returns an unsubscribe closure —
and each drops the unsubscribe when the component unmounts before `listen()` resolves.

---

## 7. Storage

`data_dir` is `%LOCALAPPDATA%\…` for an installed copy, or `<exe dir>\data` when a
`portable.txt` sits beside the exe.

```
<data_dir>/
  library.db        SQLite, WAL, forward-only migrations in schema.rs
  aurora.log        truncated on every launch (F-12)
  artwork/          content-addressed poster/backdrop cache, LRU eviction
  Recordings/       .ts files (configurable)
  timeshift/        mpv's on-disk demuxer cache (D21)
  updates/          downloaded installer, verified by length + SHA-256 (D17)
```

Secrets live **only** in Windows Credential Manager under service `AuroraTV`, key
`aurora-provider-{id}`; the database stores the key, never the value. On non-Windows
the store is an in-memory fallback that reports `is_persistent() == false`.

Profile PINs are Argon2id hashes in `profiles.pin_hash` with attempt throttling.

---

## 8. Build, test and release

| Step | Command | Result on this machine |
|---|---|---|
| UI bundle | `pnpm exec vite build` | 431 modules, 493 kB js / 11.8 kB css, 2.9 s |
| Rust debug + tests | `cargo test --workspace --all-targets` | **712 passed, 0 failed, 0 warnings** |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| Typecheck | `pnpm exec tsc --noEmit` | clean |
| Release tooling | `node --test scripts/version.test.mjs` | 14 passed |
| E2E | `pnpm exec playwright test` | **84 passed**, 1.7 min |
| UI unit tests | `pnpm test` (`vitest run`) | **"No test files found", exit 1** |
| Release build | `cargo build --release --workspace` | clean, see test-report |

CI (`.github/workflows/ci.yml`) runs the core job and a Windows `cargo check` on PRs,
and builds + publishes an NSIS installer and portable zip on merges to `main`. The
version number is derived from conventional-commit subjects since the last `v*` tag
and stamped into the manifests at build time (`D18`).

Nothing is code-signed. The updater verifies a GitHub-published SHA-256 and length
(`D17`) — which defends against a corrupted download, not against whoever can publish
a release.

---

## 9. Where the map and the territory disagree

| `docs/` says | The code does | Finding |
|---|---|---|
| "Ingestion … run on a Tokio pool" | every command runs on the Tauri main thread | F-01 |
| "the UI never blocks" | a 23.7 s refresh blocks the window | F-01 |
| "a lookup that has to find a channel by id … is never affected by what the viewer chose to hide" | `live_sources` filters out hidden **and all radio** channels | F-06 |
| Phase 7 Series "Done" | Xtream series are fetched and thrown away | F-04 |
| "This parser … never panics" | `urldecode` slices a `&str` at a non-char boundary | F-02 |
| §18 "structured logging with rotation" | one file, truncated per launch, no rotation, no export | F-12 |
| §17 "errors the user can act on" | 23 of 27 UI `invoke` calls discard their rejection | F-03 |
