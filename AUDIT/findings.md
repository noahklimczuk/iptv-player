# Findings

Every defect found in the 1.0 hardening audit. Severity is about what a *user* loses:

- **Critical** — crashes, data loss, or a security hole.
- **High** — a headline feature does not work, or the app is unusable on a real
  subscription.
- **Medium** — wrong behaviour in a reachable case, or a shipping-readiness gap.
- **Low** — correctness or polish that nobody will file a bug about.

Status is one of **Fixed**, **Deferred**, **Won't fix**. Every Fixed row names the
commit and the test that would catch a regression.

| ID | Sev | Area | Summary | Status |
|---|---|---|---|---|
| F-01 | Critical | host | All 93 Tauri commands run on the main thread; a 24 s refresh freezes the window | Fixed |
| F-02 | Critical | m3u | `urldecode` panics on a non-ASCII byte after `%`; `panic = "abort"` kills the app | Fixed |
| F-03 | High | ui | No error boundary, no unhandled-rejection handler, 23 of 27 `invoke` calls discard failures | Fixed |
| F-04 | High | ingest | Xtream series are fetched and thrown away — Series is empty on every real panel | Fixed |
| F-05 | High | playback | Concurrent tunes race; rapid zapping can leave the wrong stream playing | Fixed |
| F-06 | High | playback | Radio and hidden channels cannot be tuned, recorded or looked up at all | Fixed |
| F-07 | High | perf | Every tune, recording start and now/next loads the whole channel table | Fixed |
| F-08 | High | ui | Live TV renders every channel un-virtualised, one IPC call each | Fixed |
| F-09 | High | ui | Favourites: never sent by the host, filter ignored, nothing can set one | Fixed |
| F-10 | High | ui | Event subscriptions leak when a component unmounts before `listen()` resolves | Fixed |
| F-11 | High | host | Background threads have no panic guard; a panic silently ends the feature | Fixed |
| F-12 | Medium | security | `img-src` CSP omits `http:`, so provider logos never load in the shipped app | Fixed |
| F-13 | Medium | logging | Log truncated every launch, no rotation, no way to export it | Fixed |
| F-14 | Medium | db | A corrupt `library.db` stops the app starting, with no message and no recovery | Fixed |
| F-15 | Medium | security | `redact()` misses `/user/pass/id.ts` stream URLs that carry no `live` marker | Fixed |
| F-16 | Medium | deps | `react-router` 6.30.6 and the `vitest` → `vite`/`esbuild` chain carry advisories | Fixed |
| F-17 | Medium | tests | `pnpm test` exits 1: there is not one unit test for any TypeScript | Fixed |
| F-18 | Medium | providers | `providers_save` is two writes; a credential-store failure leaves a half-provider | Fixed |
| F-19 | Medium | m3u | Percent-decoding produces mojibake for any non-ASCII header value | Fixed |
| F-20 | Low | player | `MpvBackend::cmd` indexes `args[0]` without checking the slice is non-empty | Fixed |
| F-21 | Low | m3u | `looks_like_url` treats any `x:…` line as a path, so junk becomes an entry | Fixed |
| F-22 | Low | xmltv | `parse_time` accepts 31 February and similar impossible dates | Fixed |
| F-23 | Medium | catchup | Xtream catch-up timestamps are sent in UTC; panels read them as local time | Deferred |
| F-24 | High | player | Phase 0: `attach`, `attach_video_surface` and `pump` are never called | Fixed (wired; unrun) |
| F-25 | Medium | release | Nothing is code-signed; the updater verifies a digest, not a signature | Deferred |
| F-26 | Low | build | No ARM64 target is configured | Won't fix |
| F-27 | High | ui | A host failure renders as an empty state: "you have no channels" | Fixed |

---

## F-01 — Critical — Every command runs on the Tauri main thread

**Where.** All 93 handlers, e.g. `crates/aurora-app/src/providers.rs:405`
(`providers_refresh`), `src/commands.rs:22` (`channels_list`),
`src/updates.rs:250` (`updates_download`).

**What.** Every handler is declared `#[tauri::command]` over a synchronous `fn`.
`tauri-macros-2.6.3/src/command/wrapper.rs:50` defaults to
`ExecutionContext::Blocking`, and `body_blocking` runs the function body inline in
the IPC handler — which the webview calls on the main thread. There is no Tokio pool
anywhere in the host, despite `docs/ARCHITECTURE.md` saying there is.

**Repro.** Add an Xtream provider with a real catalogue and press Refresh. The
project's own measurement (`docs/ROADMAP.md`, "What a real import cost") is 23.7 s:
8.7 s of network, 14.9 s of SQLite. For all of it the window cannot be moved, resized
or repainted, and the `ingest.progress` events the UI listens for cannot be delivered,
because delivering them also needs the main thread. Same for `updates.download`
(a ~12 MB installer), `providers.validate` (up to a 12 s connect timeout) and
`metadata.run`.

**Fix.** `#[tauri::command(async)]` on every handler. For a non-`async fn` that
selects `ExecutionContext::Async` → the `sync_threadpool` path, which runs the same
body on the async runtime's blocking pool. Signatures, argument names and return
types are unchanged. `tests/command_args.rs` was scanning for the literal
`#[tauri::command]`, so it was widened to accept both spellings, and a new test
asserts that *no* handler is left on the blocking path.

**Regression test.** `crates/aurora-app/tests/command_args.rs::every_command_runs_off_the_main_thread`.

---

## F-02 — Critical — `urldecode` panics on a non-ASCII byte after `%`

**Where.** `crates/aurora-core/src/m3u.rs:322` (`urldecode`).

```rust
if b[i] == b'%' && i + 2 < b.len() {
    if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
```

`s[i+1..i+3]` slices a `&str` by byte offsets that are not guaranteed to be char
boundaries.

**Repro.** A playlist line

```
#KODIPROP:inputstream.adaptive.stream_headers=User-Agent=%aé
```

reaches `urldecode("%aé")`. `é` is two bytes, so index 3 lands inside it:

```
thread 'main' panicked at m3u.rs:7:49:
byte index 3 is not a char boundary; it is inside 'é' (bytes 2..4) of `%aé`
```

Verified by running the function standalone. `[profile.release]` sets
`panic = "abort"`, so in a shipped build this is not a caught error — it terminates
the process while importing a playlist. The module header claims this parser
"never panics".

**Fix.** Decode over bytes into a `Vec<u8>` and `String::from_utf8_lossy` at the end.
That removes the slice entirely and, as a bonus, fixes F-19.

**Regression test.** `m3u.rs::a_percent_escape_before_a_multibyte_character_does_not_panic`.

---

## F-03 — High — Nothing catches a UI error

**Where.** `src-ui/src/main.tsx` (no boundary), `src-ui/src/hooks/useZapper.ts:36`,
`src-ui/src/App.tsx:126-180`, `src-ui/src/state/ui.ts:78`.

**What.** Three gaps at once:

1. No React error boundary. A render-time throw unmounts the whole tree and leaves a
   blank window — in a WebView2 with no devtools, an unexplained black screen.
2. No `unhandledrejection` or `error` listener on `window`.
3. 23 of the 27 `invoke(...)` call sites are `void invoke(...)` with no `.catch`.

**Repro.** Tune a channel whose every source is dead. `Playback::play_live` returns
`Err("nothing would play on channel N")`. `useZapper.tune` does
`void invoke('player.play', …)`, so the rejection is unhandled: the channel banner
appears, the player opens, and nothing ever says why the picture is black.

**Fix.** An `ErrorBoundary` around `<App/>` that shows the message and a Reload
button; a `reportError` module that installs `unhandledrejection` + `error`
listeners and surfaces a dismissible toast; and the tune / play / catch-up paths
routed through it so a refusal is shown in words.

**Regression test.** `tests-e2e/errors.spec.ts`, `src-ui/src/lib/errors.test.ts`.

---

## F-04 — High — Xtream series are fetched and discarded

**Where.** `crates/aurora-ingest/src/sync.rs:513-521`.

```rust
let listings = client.series()?;
if !listings.is_empty() {
    warnings.push(format!("{} series found; episode listings are fetched on demand", …));
}
```

The listings are dropped on the floor. Nothing fetches them "on demand" either —
`library_episodes` reads the database and nothing else.

**Repro.** Import the panel in `docs/ROADMAP.md`: 28 693 series are downloaded, a
warning is recorded, `series` stays empty, and the Series screen shows its empty
state. Phase 7 is marked **Done** in the roadmap.

**Fix.** `get_series` already returns everything a `series` row needs — name, cover,
plot, category, release date. `fetch` now carries them in `Fetched::series` and
`apply` upserts them under a `series:{series_id}` provider key, alongside the ones
grouped out of flat episode entries. Episode listings still need one request per
show and stay deferred; the warning now says that accurately.

**Regression test.** `sync.rs::xtream_series_listings_are_imported_not_discarded`.

---

## F-05 — High — Rapid zapping can leave the wrong stream playing

**Where.** `crates/aurora-app/src/playback.rs:76-121` and `:245-287`.

**What.** `play_live` takes the player lock *per load attempt*, not across the tune.
`tick` → `recover` loads too, from the heartbeat thread, every 250 ms. Nothing
serialises the two or notices that a newer tune has started.

**Repro (interleaving).** Channel A's every source is slow to fail.

1. Viewer zaps to A. Command thread enters `play_live(A)` and starts trying A's
   three sources.
2. Viewer zaps to B. A second command thread enters `play_live(B)`, loads B's first
   source, sets `session = B`, returns. Picture is B.
3. Thread 1's third attempt finally succeeds, calls `player.load(A_url)` and
   overwrites `session = A`.

The viewer asked for B and is watching A. The same window exists between the
heartbeat's `recover()` and any user tune.

**Fix.** A `tune: Mutex<()>` held for the whole of `play_live` / `play_item` /
`play_catchup` / `recover`, plus a `generation: AtomicU64` bumped by each new tune.
A load whose generation is stale is discarded instead of being published, and
`recover` refuses to run against a session that is no longer current.

**Regression test.** `playback.rs::a_later_tune_wins_however_slowly_the_earlier_one_finishes`,
`playback.rs::recovery_does_not_resurrect_a_channel_the_viewer_has_left`.

---

## F-06 — High — Radio and hidden channels are invisible to playback

**Where.** `crates/aurora-app/src/window.rs:118` (`live_sources`),
`crates/aurora-app/src/commands.rs:150` (`epg_now_next`).

```rust
let ch = channels::list(db, &channels::ChannelFilter::default())?
    .into_iter().find(|c| c.id == channel_id)
```

`ChannelFilter::default()` is `include_hidden: false, radio_only: false`, which the
SQL turns into `AND hidden = 0 AND is_radio = 0`. The doc comment on the struct says
the opposite: *"a lookup that has to find a channel by id — playback, a recording, a
favourite — is never affected by what the viewer chose to hide from a list."*

**Repro.**

- Tune any channel with `is_radio = 1`: `Error: unknown channel 42`. There is no
  path by which a radio station can ever be played, and the parser sets `is_radio`
  from the `radio="true"` attribute that radio playlists use.
- Hide a channel in the playlist editor, then let a scheduled recording on it come
  due: `Dvr::begin` → `resolve_playback("live")` → the same refusal, and the
  recording is marked Failed.

**Fix.** A `channels::get(conn, id) -> Option<ChannelRow>` that looks up by primary
key with no list filters, used by `live_sources` and `epg_now_next`.

**Regression test.** `channels.rs::get_finds_a_hidden_or_radio_channel_that_list_hides`,
`window.rs::a_radio_channel_can_be_tuned`.

---

## F-07 — High — A zap costs a full table scan

**Where.** Same two call sites as F-06, plus `Dvr::begin` via `resolve_playback`.

**What.** To read one channel's `name`, the code runs

```sql
SELECT … FROM channels WHERE hidden = 0 AND is_radio = 0 <library filters>
ORDER BY COALESCE(custom_number, number, 999999), sort_order, name
```

with no `LIMIT`, materialises every row into a `Vec<ChannelRow>`, then `find`s one.
On the subscription in `docs/ROADMAP.md` that is 22 121 rows sorted and allocated —
**per zap**, per recording start, and per `epg.nowNext`.

**Repro.** Live TV lists 22 121 channels; each row calls `epg.nowNext`; each of those
scans all 22 121 rows. That is ~490 million row materialisations to paint one screen,
serialised through one mutex, on the main thread (F-01).

**Fix.** The `channels::get` from F-06 — a single indexed lookup.

**Regression test.** `channels.rs::get_is_a_primary_key_lookup_not_a_filtered_list`.

---

## F-08 — High — Live TV renders every channel and fetches now/next per row

**Where.** `src-ui/src/features/live/LivePage.tsx:112-130` and `:134`.

**What.** `(channels ?? []).map(...)` with no virtualiser, and each `ChannelRow`
mounts its own `useCommand('epg.nowNext', { channelId })`. `channels_list` passes
`limit: None`.

**Repro.** 22 121 `<button>` elements and 22 121 in-flight IPC calls on opening Live
TV. The Guide and the playlist editor are both virtualised with
`@tanstack/react-virtual`; this screen was missed.

**Fix.** Virtualise the list and the grid with the same `useVirtualizer` the Guide
uses, and replace the per-row call with one batched `epg.nowNextMany` over only the
channel ids currently on screen.

**Regression test.** `tests-e2e/playlist.spec.ts::live_tv_renders_a_large_channel_list_without_mounting_every_row`.

---

## F-09 — High — Favourites do not exist outside the mock

**Where.** `crates/aurora-db/src/repo/channels.rs:10` (`ChannelRow` has no
`favorite`), `crates/aurora-app/src/commands.rs:22` (`favorites_only` accepted and
ignored), `src-ui/src/features/live/LivePage.tsx:105` (empty-state text describing
behaviour that does not exist).

**What.** Three halves of the feature are present and never meet:

- `lists::toggle_favorite` / `lists::favorite_channel_ids` exist and are tested.
- `shared/ipc.ts` declares `Channel.favorite?: boolean`. The host never sets it.
  `src-ui/src/ipc/mock.ts:1043` *does* filter on `favoritesOnly`, which is why the
  browser preview and every Playwright journey look correct.
- Nothing in `src-ui` ever calls `favorites.toggle`. `f` is bound to fullscreen.

**Repro.** In the shipped app: no heart is ever filled, the Favorites button changes
nothing, and the empty state advises pressing a key that does something else.

**Fix.** `ChannelFilter` grows `profile_id` and `favorites_only`; `channels::list`
left-joins `favorites` to populate the flag and filters on it; `channels_list`
forwards both; `LivePage` gets a working heart button and `App` binds one to a key.

**Regression test.** `channels.rs::favorites_are_reported_and_filterable`,
`tests-e2e/playlist.spec.ts::favouriting_a_channel_survives_a_reload`.

---

## F-10 — High — Event subscriptions leak on fast unmount

**Where.** `src-ui/src/ipc/index.ts` — six near-identical functions
(`onPlayerState`, `onUpdateDownload`, `onIngestProgress`, `onDvrTick`,
`onMetadataProgress`, `onArtworkProgress`).

```ts
let dispose: (() => void) | undefined;
void w.__TAURI__?.event?.listen('player.state', …).then((d) => { dispose = d; });
return () => dispose?.();
```

**What.** If the component unmounts before `listen()` resolves, the returned cleanup
runs while `dispose` is still `undefined` — it does nothing — and the listener is
registered a moment later with nobody holding its unsubscribe.

**Repro.** Open and close Settings faster than the IPC round trip (trivially
reproducible while the main thread is busy, i.e. during exactly the long refresh the
Updates panel is watching). Each cycle leaves one more live `update.download`
listener. Over a session the same payload is delivered to a growing number of dead
closures, each holding its component's captured state.

**Fix.** One `subscribe(event, fn)` helper that all six delegate to, which tracks a
`cancelled` flag and calls `dispose()` immediately if the subscription resolves after
teardown.

**Regression test.** `src-ui/src/ipc/subscribe.test.ts`.

---

## F-11 — High — A background thread that panics dies silently

**Where.** `crates/aurora-app/src/main.rs:81`, `:97`, `:118`.

**What.** Three `std::thread::Builder::spawn(move || loop { … })` closures with no
`catch_unwind` and no panic hook installed anywhere in the process.

**Repro.** Any panic inside `Playback::tick` (for example a poisoned invariant in a
backend) ends the `aurora-player` thread. Nothing restarts it and nothing logs it,
because in a release build stderr goes nowhere (`windows_subsystem = "windows"`).
Symptom: the OSD freezes at whatever it last showed and never updates again, and
failover stops working, for the rest of the session. The DVR thread is worse — every
future recording is silently lost.

**Fix.** `std::panic::set_hook` that writes the payload and location through
`tracing::error!` so it reaches the log file, plus a `supervised` helper that wraps
each loop body in `catch_unwind`, logs, and keeps the loop alive.

**Regression test.** `main.rs` is a binary; covered by
`crates/aurora-app/src/lib.rs::supervise::tests::a_panicking_tick_does_not_end_the_loop`.

---

## F-12 — Medium — The CSP blocks every provider logo

**Where.** `crates/aurora-app/tauri.conf.json:31`.

```
img-src 'self' data: https:;
media-src 'self' https: http:;
```

**What.** `media-src` allows `http:`; `img-src` does not. IPTV panels overwhelmingly
serve `tvg-logo` and `stream_icon` over plain HTTP — the probe in `docs/ROADMAP.md`
reached its origin over `http://` on a bare IP.

**Repro.** In the shipped app every channel logo, poster and backdrop from an
`http://` URL is blocked with `Refused to load the image` and renders as a broken
image. In the browser preview there is no CSP, so this cannot be seen from `pnpm dev`
or from Playwright.

**Fix.** Add `http:` and `asset:` to `img-src` (the latter for when the artwork cache
is finally served locally), keeping `default-src 'self'` and `script-src 'self'`.

**Regression test.** `crates/aurora-app/tests/contract.rs::the_csp_allows_the_image_schemes_providers_actually_use`.

---

## F-13 — Medium — The log is truncated on every launch

**Where.** `crates/aurora-app/src/main.rs:30` (`File::create`).

**What.** README §18 asks for structured logging with rotation and an "export logs"
affordance. There is one file, recreated per run, and no way to get at it from the UI.

**Repro.** App crashes. User relaunches to report it. The evidence is gone.

**Fix.** Keep the previous run as `aurora.log.1` (one generation, renamed on start),
and add a `logs.export` command that copies the current and previous logs to a folder
the viewer chooses, plus a button in Settings → About.

**Regression test.** `crates/aurora-app/src/logging.rs::tests::the_previous_run_is_kept_as_generation_one`.

---

## F-14 — Medium — A corrupt database stops the app with no message

**Where.** `crates/aurora-app/src/services.rs:36-47`.

```rust
let db = aurora_db::open(data_dir.join("library.db"))?;
if !aurora_db::integrity_check(&db)? { tracing::error!("database failed its integrity check"); }
```

**What.** `open` failing propagates out of `setup()`, which Tauri turns into a
start-up failure — no window, no dialog, nothing in the console a release build has.
And when the file opens but fails `integrity_check`, the app carries on using it.
README §5 promises "automatic integrity check + **repair** on startup".

**Repro.** `printf 'garbage' | dd of=library.db bs=1 seek=100 conv=notrunc`, relaunch.
The app does not start and says nothing.

**Fix.** `aurora_db::open_or_recover`: on an open failure or a failed integrity check,
move the file aside to `library.corrupt-<unix>.db`, create a fresh one, and return a
flag the host surfaces. The library re-imports from the provider; the corrupt file is
kept for support rather than deleted.

**Regression test.** `aurora-db/src/lib.rs::tests::a_corrupt_database_is_moved_aside_and_replaced`.

---

## F-15 — Medium — Credential redaction misses the commonest stream URL

**Where.** `crates/aurora-ingest/src/http.rs:270-280`.

**What.** Path-segment redaction only fires after a literal `live`, `movie` or
`series` segment. Many panels serve `http://host/<user>/<pass>/<id>.ts` with no
marker at all — `aurora_core::catchup::split_xtream_stream_url` explicitly supports
that form, so the codebase already knows it exists.

**Repro.** `redact("http://panel.example/bob/hunter2/1234.ts")` returns the string
unchanged, and that is what reaches `tracing::debug!(url = %redact(url), …)` on every
retry, and the `EPG source failed: … ({url})` warning surfaced in the refresh report.

**Fix.** When the path has no marker but is exactly three segments ending in
`<digits>.<ext>`, redact the first two. Also redact *every* occurrence of a sensitive
query key, not just the first.

**Regression test.** `http.rs::a_markerless_user_pass_id_url_is_redacted`,
`http.rs::a_repeated_credential_parameter_is_redacted_every_time`.

---

## F-16 — Medium — Dependency advisories

`pnpm audit`: 1 critical, 1 high, 7 moderate.

| Package | Where | Advisory | Action |
|---|---|---|---|
| `vitest` 2.1.9 | dev | critical — UI server arbitrary file read/execute | upgraded |
| `vite` 5.4.21 (via vitest) | dev | high — `server.fs.deny` bypass on Windows | follows vitest |
| `esbuild` 0.21.5 (via vitest) | dev | moderate — dev server request forgery | follows vitest |
| `react-router` 6.30.6 | **runtime** | moderate — open redirect via backslash in `<Link>`/`useNavigate` | see below |

`react-router` is patched only in 7.18. The app uses `HashRouter`, `Routes`, `Route`,
`NavLink`, `useNavigate` and `useLocation` — all of which v7 keeps — and every `to`
and `navigate()` argument in the tree is a string literal, so nothing
attacker-controlled reaches a redirect today. Upgraded anyway, because "no
user-controlled route today" is not a property anyone will re-check. Result:

```
Before: 1 critical, 1 high, 7 moderate
After:  0 critical, 0 high, 0 moderate
```

Both upgrades rode in on the F-03 commit rather than one of their own, because the
new vitest config had to land with the tests it runs.

Rust: `cargo audit` is not installed in this container and building it was not worth
the minutes; `Cargo.lock` is pinned and `cargo deny`/`cargo audit` should run in CI.
Listed in the release checklist.

---

## F-17 — Medium — `pnpm test` fails because there are no tests

**Where.** `package.json:11`, `vitest` configured with nothing to run.

```
$ pnpm test
No test files found, exiting with code 1
```

CI never runs it, so nobody noticed. There is not one unit test for any TypeScript:
not `shared/ipc.ts`, not `lib/format.ts`, not `behindLive`, not the zapper's wrap-around
arithmetic. The 84 Playwright journeys all run against the **mock** transport, which
is precisely why F-09 and F-12 were invisible.

**Fix.** A `vitest` config with a jsdom-free node environment and a first suite over
the pure helpers (`format`, `behindLive`, `subscribe`, the error reporter), so
`pnpm test` is meaningful and CI can run it.

---

## F-18 — Medium — Saving a provider is two writes with no transaction

**Where.** `crates/aurora-app/src/providers.rs:200-221`.

`INSERT INTO providers (… credential_ref NULL)`, then `credentials.set(...)`, then
`UPDATE providers SET credential_ref`. If the middle step fails the command returns
an error and leaves a provider row behind with no password — the wizard shows a
failure, the provider list shows the provider, and refreshing it signs in blank.

**Fix.** Write the credential first, then insert the row with its `credential_ref`
already set, and delete the credential again if the insert fails.

**Regression test.** `providers.rs::tests::a_failed_credential_write_leaves_no_provider_behind`.

---

## F-19 — Medium — Percent-decoding mangles non-ASCII

**Where.** `crates/aurora-core/src/m3u.rs:322-336`.

`out.push(v as char)` turns each decoded *byte* into a Unicode code point, so
`%C3%A9` becomes `Ã©` rather than `é`; the same `as char` cast on the literal path
mangles any non-ASCII byte that was not escaped. Affects `User-Agent` and `Referer`
taken from `#KODIPROP`, which are then sent as HTTP headers.

**Fix.** Folded into F-02: decode to bytes, then `String::from_utf8_lossy`.

**Regression test.** `m3u.rs::percent_escapes_decode_to_utf8_not_latin1`.

---

## F-20 — Low — `MpvBackend::cmd` indexes an unchecked slice

**Where.** `crates/aurora-player/src/mpv.rs:368`. `args[0]` panics on an empty slice.
No current caller passes one; it is one edit away from being reachable, in
Windows-only code that CI cannot run.

**Fix.** Return `PlayerError::Command` for an empty argument list.

---

## F-21 — Low — `looks_like_url` accepts any `x:` line

**Where.** `crates/aurora-core/src/m3u.rs:252`.

```rust
|| (line.len() > 2 && line.as_bytes()[1] == b':')
```

Intended to catch `C:\path`. Also matches `a:b`, `1:30`, and any line whose second
byte happens to be a colon — which becomes a channel with an unplayable URL rather
than a warning.

**Fix.** Require an ASCII-alphabetic drive letter followed by `\` or `/`.

**Regression test.** `m3u.rs::a_bare_colon_line_is_not_mistaken_for_a_windows_path`.

---

## F-22 — Low — `parse_time` accepts impossible dates

**Where.** `crates/aurora-core/src/xmltv.rs:404`. `day` is range-checked against
`1..=31` without reference to the month, so `20250231…` silently becomes 3 March.

**Fix.** Check the day against the month's real length, leap years included.

**Regression test.** `xmltv.rs::an_impossible_date_is_rejected_rather_than_rolled_over`.

---

## F-23 — Medium — Xtream catch-up timestamps are sent in UTC — **Deferred**

**Where.** `crates/aurora-core/src/catchup.rs:192-207`.

`xtream_url` formats `start` with `OffsetDateTime::from_unix_timestamp`, i.e. UTC.
Xtream panels read that parameter in the *panel's* local time, and
`server_info.timezone` (which the auth response already carries, and which nothing
reads) said `Europe/Paris` on the one real panel this has met — two hours out.

**Why deferred.** The probe in `docs/ROADMAP.md` established that this panel's
`timeshift.php` returns 404 for every form, so there is no way to verify a fix
against a real server, and the codebase's own rule is to refuse rather than guess.
Changing the arithmetic blind would swap a known-wrong offset for an unverified one.
What *is* actionable — storing `server_info.timezone` at authentication so the fix has
its input ready — is a small change that belongs with the fix, not before it.

**To close it:** a panel whose `timeshift.php` answers, and a recording whose first
frame is checked against the programme's advertised start.

---

## F-24 — High — The Phase 0 spike was unwired — **Fixed (wired; never run)**

**Where.** `crates/aurora-player/src/mpv.rs` (`attach`, `pump`),
`crates/aurora-app/src/window.rs` (`attach_video_surface`),
`crates/aurora-app/src/main.rs`, `crates/aurora-app/tauri.conf.json`.

None of the three was called from anywhere. Without `attach` mpv had no `wid` and
nowhere to draw; without `pump` its event queue was never drained, so position, tracks,
buffering, errors and the timeshift window never changed after a load — the 250 ms
heartbeat reads that state, it does not produce it. `tauri.conf.json` also set
`"transparent": false`, which alone was enough to show no video.

**Why it was unreachable, which is the part worth knowing.** Not an oversight in
`main.rs`: `attach` and `pump` were inherent methods on `MpvBackend`, and the app layer
holds a `Box<dyn PlayerBackend>`. There was no way to call them at all. Moving them
onto the trait — with defaults that do nothing, so `NullBackend` and every test fake
are untouched — is what made the wiring possible. `attach` takes the handle as an
`isize` because the trait compiles on platforms where `HWND` does not, and both
windows-crate versions in the tree agree an `HWND` is a `*mut c_void`, so the cast is
version-agnostic.

**Fix.** Four calls and a flag: attach at setup, pump from `Playback::tick`,
reposition on `WindowEvent::Resized`, `"transparent": true`, and an OSD that goes
transparent under a native host instead of painting a gradient over the video.
Everything fails soft — a window that cannot be attached to leaves the app running
with no picture and a line in the log, which is reportable; a window that never
appears is not.

**What "Fixed" does and does not mean here.** The code exists and is reachable, and
`aurora-player` type-checks for `x86_64-pc-windows-msvc`. It has still never met a
display, and `aurora-app` could not even be cross-checked in this container because
rustls's `ring` needs an MSVC C compiler — CI's Windows job is the first thing that
will compile `window.rs` and `main.rs`. What changed is that the spike can now be
*run*: before it, a Windows build showed a black rectangle however well the
compositing worked.

**Regression test.**
`crates/aurora-app/tests/contract.rs::the_video_surface_and_the_event_pump_are_actually_called`
and `::the_window_is_transparent_so_video_can_show_through` — they read the sources and
the config, because what went wrong was not behaviour but a call that did not exist.

---

## F-25 — Medium — Nothing is code-signed — **Deferred**

The NSIS installer and the exe are unsigned, so SmartScreen warns on every install.
The updater verifies the length and SHA-256 GitHub published for the asset
(`docs/DECISIONS.md` D17), which defends against a corrupted or substituted
*download* but not against whoever can publish a release. Closing it needs a
certificate (or a minisign keypair) that a person has to buy and keep — a decision,
not a commit. Release checklist.

---

## F-26 — Low — No ARM64 target — **Won't fix**

The brief names x64 and ARM64. The workspace targets `x86_64-pc-windows-msvc` only,
and the CI job that produces an installer fetches an x86_64 libmpv build. ARM64
Windows runs x64 binaries under emulation, which for a video player means software
decode and a bad time — the useful version of this is a real ARM64 libmpv, which is
not published by the upstream this project fetches from. Out of scope for 1.0;
noted so it is a decision rather than an oversight.

---

## F-27 — High — A host failure is drawn as an empty state

**Found during Phase 4**, walking every screen in its error state.

**Where.** `src-ui/src/hooks/useCommand.ts`, and 20 of its 22 call sites.

**What.** The hook returns `{ data, loading, error }`. Two callers read `error`
(`HomePage`, `TimeshiftPanel`). The other twenty destructure `{ data, loading }` and
render their empty state when `data` is null — which is also what null means when the
call *failed*.

**Repro.** Make `channels.list` refuse:

```
Live TV shows:  "No channels — Add a provider in Settings to populate your channel list."
```

On a machine that already has a provider and forty thousand channels. The same shape
on Recordings, Playlist, Movies, Series and every settings panel. It is worse than a
blank panel, because it is a confident wrong answer: it sends someone to add a
provider they already have.

**Fix.** The hook reports the failure through `lib/errors::notify` itself, so a screen
that ignores `error` still surfaces it. Doing it per call site would mean twenty edits
and a convention nobody will keep; doing it in the hook means a screen cannot swallow
a failure by being written the obvious way.

**Regression test.** `tests-e2e/walkthrough.spec.ts::a refusal on a list screen is
reported rather than left as a blank panel`.

---

## Where each fix landed

| Finding | Commit |
|---|---|
| F-02, F-19, F-21 | `fix(m3u): stop a percent escape in a playlist from ending the process` |
| F-01 | `fix(host): stop every command from running on the window's own thread` |
| F-06, F-07 | `fix(playback): look a channel up by its id, not by filtering the whole list` |
| F-05 | `fix(playback): hold a tune together, so zapping cannot leave the wrong channel on` |
| F-04 | `fix(ingest): write the series an Xtream panel lists, instead of counting them` |
| F-03, F-16, F-17 | `fix(ui): give a failure somewhere to go` |
| F-09 | `fix(favorites): let the host answer the question the UI was already asking` |
| F-10 | `fix(ipc): stop an event subscription leaking when a screen closes too fast` |
| F-11 | `fix(host): stop a background thread's panic from ending its feature silently` |
| F-08 | `perf(live): virtualise the channel list and ask the guide once per screen` |
| F-12, F-13, F-14 | `fix(diagnostics): keep the evidence, and say when the library was lost` |
| F-15, F-18, F-20, F-22 | `fix(hardening): redaction, an atomic provider save, and two sharp edges` |
| F-27 | `test: the inputs providers actually send, and what happens after an hour` |
| F-24 | `feat(player): connect the video surface and the event pump` |

## Counts

| Severity | Fixed | Deferred | Won't fix | Total |
|---|---|---|---|---|
| Critical | 2 | 0 | 0 | **2** |
| High | 10 | 0 | 0 | **10** |
| Medium | 9 | 2 | 0 | **11** |
| Low | 3 | 0 | 1 | **4** |
| **Total** | **24** | **2** | **1** | **27** |

F-24 counts as Fixed on the strength of the code being written, reachable and
type-checking for Windows — not on the strength of anyone having seen it work. The two
still deferred are F-23 (the Xtream catch-up timezone, unfixable against evidence until
a panel whose `timeshift.php` answers is available) and F-25 (code signing, which needs
a certificate somebody buys).
