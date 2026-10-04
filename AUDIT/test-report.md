# Test report

What was actually run on this machine, and what came back. Every number below is
pasted from a real run, not estimated. The machine is the Linux container this audit
was carried out in; anything Windows-only is called out as unverified and listed in
`AUDIT/release-checklist.md`.

Toolchain: `rustc 1.94.1`, `cargo 1.94.1`, `node v22.22.2`, `pnpm 10.33.0`,
Chromium via Playwright 1.49.

---

## 1. Suites, before and after

| Suite | Before | After | Δ |
|---|---|---|---|
| `aurora-core` unit | 248 | 255 | +7 |
| `aurora-core` integration (`hostile_input.rs`) | — | 9 | **+9 (new)** |
| `aurora-db` unit | 196 | 206 | +10 |
| `aurora-ingest` unit | 173 | 186 | +13 |
| `aurora-player` unit | 21 | 21 | — |
| `aurora-app` unit | 69 | 88 | +19 |
| `aurora-app` `contract.rs` | 4 | 7 | +3 |
| `aurora-app` `command_args.rs` | 1 | 2 | +1 |
| `aurora-app` `stress.rs` (soak, `--ignored`) | — | 3 | **+3 (new)** |
| Rust doc-tests | 1 | 1 | — |
| **Rust total** | **713** | **778** | **+65** |

`cargo test --workspace --all-targets` reports **774**: it excludes the doc-test and
does not run the three `#[ignore]` soak tests, which are run separately in §6.
| Playwright journeys | 84 | 97 | +13 |
| Vitest (TypeScript units) | **0 — suite exited 1** | 12 | **+12 (new)** |
| `scripts/version.test.mjs` | 14 | 14 | — |
| **Grand total** | **811** | **901** | **+90** |

`pnpm test` answered `No test files found, exiting with code 1` before this pass.
There was not one unit test for any TypeScript in the repository.

---

## 2. Rust — `cargo test --workspace --all-targets`

```
     Running unittests src/lib.rs (target/debug/deps/aurora_app-c7213eff18c94f3a)
test result: ok. 88 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/command_args.rs
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/contract.rs
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/stress.rs
test result: ok. 0 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out
     Running unittests src/lib.rs (target/debug/deps/aurora_core-7b7bc4e7e9c669e8)
test result: ok. 255 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/hostile_input.rs
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running unittests src/lib.rs (target/debug/deps/aurora_db-bd0e95b5181c7846)
test result: ok. 206 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running unittests src/lib.rs (target/debug/deps/aurora_ingest-124e72a51c90ddaf)
test result: ok. 186 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running unittests src/lib.rs (target/debug/deps/aurora_player-b7221529f6786562)
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
   Doc-tests aurora_core
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Summed: `88 + 2 + 7 + 255 + 9 + 206 + 186 + 21 = 774`, plus 1 doc-test and the 3
soak tests below = **778**.

**Warnings: zero.** `cargo clippy --workspace --all-targets -- -D warnings` exits 0
with no output, and `cargo fmt --all --check` is clean. No lint is allowed or
suppressed anywhere in the diff.

---

## 3. Release build

```
$ rm -rf target/release && cargo build --release --workspace
    Finished `release` profile [optimized] target(s) in 5m 03s

$ ls -l target/release/aurora-app
-rwxr-xr-x 2 root root 13477592 aurora-app
```

Clean, zero warnings, with the shipping profile as committed (`lto = true`,
`codegen-units = 1`, `panic = "abort"`, `strip = true`). `aurora-app` links on Linux
against `NullBackend`; the Windows link against libmpv is the one thing this container
cannot do and is item 1 of the release checklist.

No debug code, no `dbg!`, no hardcoded test URLs: every fixture host in the tree is
`example.com` or `127.0.0.1`, and `grep -rn "todo!\|unimplemented!\|dbg!"` over
`src-native/crates/*/src` returns nothing.

---

## 4. TypeScript

```
$ pnpm exec tsc -p tsconfig.json --noEmit
(clean)

$ pnpm exec vitest run
 RUN  v5.0.1 /home/user/iptv-player
 Test Files  2 passed (2)
      Tests  12 passed (12)
   Duration  225ms

$ node --test scripts/version.test.mjs
# tests 14
# pass 14
# fail 0

$ pnpm exec vite build
✓ 431 modules transformed.
../dist/assets/index-*.css   11.80 kB │ gzip:   3.61 kB
../dist/assets/index-*.js   494.40 kB │ gzip: 153.42 kB
✓ built in 2.71s
```

---

## 5. End-to-end journeys

```
$ pnpm exec playwright test
  96 passed (1.4m)
```

Run twice back to back, and `playlist.spec.ts` a further three times with
`--repeat-each=3` (42 passed), after the virtualised list exposed two races in the
test helpers — a filter switch read before its panel had loaded, and a scroll that
stopped while the virtualiser was still measuring. Both are fixed at the source
rather than retried; see the `test(e2e)` commit.

Twelve new ones: three on error surfacing (`errors.spec.ts`), one on favourites and
one on virtualisation (`providers.spec.ts`, `playlist.spec.ts`), and seven sweeping
every screen (`walkthrough.spec.ts`).

**The standing limitation, stated plainly:** all 96 run against the *mock* transport,
in Chromium, served by `vite preview`. That is exactly why F-09 (the host never sent
`Channel.favorite`) and F-12 (the CSP blocked every `http:` logo) survived so long —
the mock filled the field in, and `vite preview` serves no CSP at all. Both now have
a test that reads the *host* rather than the mock: `contract.rs` diffs the command
list and the `PlayerState` keys against `shared/ipc.ts`, and now reads the CSP too.

---

## 6. Soak — `cargo test -p aurora-app --test stress --release -- --ignored`

```
running 3 tests
test a_long_session_does_not_grow_without_bound ...
    20000 tunes + ticks in 434.924927ms: RSS 5824 kB -> 5824 kB (0 kB, 0.000 kB per tune)
ok
test concurrent_zapping_never_deadlocks_or_wedges ...
    2400 concurrent tunes across 8 threads in 127.779946ms, 2075 won, 2076 loads
ok
test two_thousand_zaps_always_end_on_the_channel_last_asked_for ...
    2000 zaps in 40.169629ms (49789/s), 2000 loads, 36 heartbeats, 0 refused
ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

A second run after the clean release rebuild, to show the numbers are stable rather
than a lucky pass:

```
    20000 tunes + ticks in 403.447369ms: RSS 5724 kB -> 5724 kB (0 kB, 0.000 kB per tune)
    2400 concurrent tunes across 8 threads in 124.45758ms, 2114 won, 2115 loads
    2000 zaps in 40.432743ms (49465/s), 2000 loads, 36 heartbeats, 0 refused
```

Read carefully, those three lines say:

- **Rapid zapping.** 2,000 tunes across 400 channels with three sources each, with the
  250 ms heartbeat running throughout and the stream killed every fiftieth zap. The
  player ends on the channel last asked for, every time. Before F-05 the same shape of
  interleaving left the *previous* channel playing — reproduced deterministically in
  `playback.rs::a_later_tune_wins_however_slowly_the_earlier_one_finishes`, which
  reports `loaded in order: ["ch2-0.ts", "ch1-2.ts"]` when the fix is removed.
- **Concurrency.** 2,400 tunes from eight threads, which is what the IPC surface
  actually is now that every command runs on the thread pool. 2,075 won and the rest
  came back `Superseded`; nothing deadlocked, and a deliberate tune afterwards still
  lands.
- **Memory.** 20,000 tunes and 20,000 heartbeats, with a seek and a stop every 500:
  RSS did not move. Not "grew slowly" — `5824 kB -> 5824 kB`.

---

## 7. Fixtures added

`aurora-core/tests/hostile_input.rs` — generated rather than committed, because a 6 MB
playlist in git is a 6 MB playlist in every clone for ever:

| Fixture | What it pins |
|---|---|
| 50,000-entry M3U (6.4 MB) | parses whole, in order, inside the budget; catches an accidental O(n²) |
| Hostile M3U | BOM + CRLF, a percent escape before a multibyte char (F-02), unterminated quote, missing comma, orphan `#EXTINF`, a non-URL line, a bare `1:30` (F-21), a duplicate, unknown directives — the good entries all survive |
| Non-text bytes | Latin-1 in a name, a NUL, a lone CR |
| One 200,000-char line | no entry invented, a warning raised |
| Real panel naming | `US ★ QVC HD`, `AR ★ Inception (2010)`, `EN ★ Ratched S01E03` classify Live / Movie / Episode |
| XMLTV DST | Europe/London spring-forward and the repeated hour of fall-back land an hour apart in UTC and sort correctly |
| Odd offsets | `+0530`, `+1045`, `-0530`, `+01:00` |
| Hostile XMLTV | no channel, no title, unparseable start, 31 February (F-22), stop before start, entities, CDATA, missing stop |
| 48,000-programme guide | streams without buffering the document |

`aurora-ingest/src/sync.rs` — the provider side:

| Fixture | What it pins |
|---|---|
| HTML error page served as HTTP 200 | reported as the provider misbehaving, not as a serde message |
| 401 / 403 / 404 / 429 / 500 / 502 / 503 | refused cleanly, nothing written, no password in the message |
| Expired account (200 + valid JSON saying no) | refused before anything is written |
| Broken catalogue mid-refresh | the existing library is left intact |
| Loosely typed JSON | `"101"`, `"202"`, `"1"`, nulls, a missing id — the valid rows still import |
| Gzip by extension and by content type | both inflate |
| Concatenated gzip members | import whole |
| A guide that 404s | costs a warning, not the import |

---

## 8. Screens walked

`tests-e2e/walkthrough.spec.ts` visits all eight and asserts each draws, with console
errors treated as failures. Screenshots are written to `screenshots/`.

| Screen | Happy path | Empty state | Error state |
|---|---|---|---|
| Home | ✅ rails + hero | ✅ (no provider → wizard) | ✅ via the hook's notice |
| Live TV | ✅ | ✅ Favorites with none set | ✅ `channels.list` refused |
| Guide | ✅ grid, now-line, info pane | ✅ empty category | ✅ catch-up refusal, inline |
| Movies / Series | ✅ | ✅ "Nothing matches those filters" | ✅ via the hook's notice |
| Recordings | ✅ | ✅ | ✅ `dvr.list` refused |
| Playlist | ✅ | ✅ | ✅ via the hook's notice |
| Settings | ✅ incl. new Diagnostics | n/a | ✅ export failure reported |
| Player OSD | ✅ transport, tracks, stats | ✅ stopped | ✅ tune failure named |

Themes: all four (`dark`, `oled`, `light`, `contrast`) render with text and background
distinct. Keyboard: every sidebar destination reachable, `Alt+1…5` verified.

---

## 9. Dependency audit

```
$ pnpm audit
Before: 1 critical, 1 high, 7 moderate
After:  No known vulnerabilities found
```

`vitest` 2.1.9 → 5.0.1 (clears the critical UI-server file-read, the high
`server.fs.deny` bypass via its nested vite, the esbuild dev-server issue and the
mocker traversal). `react-router-dom` 6.30.6 → 7.18.4 — the only *runtime* advisory of
the nine; the app uses `HashRouter`, `Routes`, `Route`, `NavLink`, `useNavigate` and
`useLocation`, all of which v7 keeps, and the typecheck and all 96 journeys pass on it
unchanged.

**Rust: run, after a correction.** The first pass of this audit said "not run —
`cargo-audit` is not installed in this container and building it was not worth the
minutes", and handed the job to CI. CI then ran it against `./Cargo.lock`, which does
not exist here (the workspace lives under `src-native`), so the step errored instead of
auditing — and took the four steps after it down with it, including all 97 journeys.

Built and run properly, it is a three-minute install and this:

```
$ cargo audit          # in src-native/
    Scanning Cargo.lock for vulnerabilities (512 crate dependencies)

Crate:    quick-xml   Version: 0.36.2
Title:    Quadratic run time when checking a start tag for duplicate attribute names
ID:       RUSTSEC-2026-0194   Severity: 7.5 (high)   Solution: Upgrade to >=0.41.0

Crate:    quick-xml   Version: 0.36.2
Title:    Unbounded namespace-declaration allocation in `NsReader` enables
          memory-exhaustion denial of service
ID:       RUSTSEC-2026-0195   Severity: 7.5 (high)   Solution: Upgrade to >=0.41.0

error: 2 vulnerabilities found!
warning: 7 allowed warnings found
```

Both are in the XMLTV parser's dependency, and XMLTV is a file fetched from whatever
address the viewer's provider gave — untrusted input by definition, and large by
design. After upgrading to 0.42 (which the tree already carried a copy of, so this
unified two copies into one):

```
$ cargo audit
error: 0 vulnerabilities                       exit 0
warning: 7 allowed warnings found
```

The seven warnings are informational and `cargo audit` does not fail on them: five
unmaintained build-time crates (`proc-macro-error`, four `unic-*`), and `glib` 0.18.5's
`VariantStrIter` unsoundness, which arrives through Tauri's GTK dependencies and is not
in the shipped Windows binary.

---

## 10. What could not be verified here, and why

Honest list. Everything below needs a Windows machine with libmpv, or a real
subscription, and no amount of work in this container changes that.

**Most of it has since been run on one — see §11.** The rows are left as they were
written, because what a container could not answer is worth keeping beside the answers;
the right-hand column says where each one went.

| Claim | Why not | Since |
|---|---|---|
| Video decodes and composites behind the WebView | Phase 0. The wiring is **done** (F-24): `attach`, `pump` and `attach_video_surface` are called, the window is transparent, and the OSD no longer paints over the video. `aurora-player` type-checks for `x86_64-pc-windows-msvc`; `aurora-app` could not be cross-checked here because rustls's `ring` needs an MSVC C compiler, so CI's Windows job compiles `window.rs` and `main.rs` first. Nothing has met a display. Item 1 of the checklist. | **Answered (§11)** — 1920x1080 H.264 at 60fps, `hwdec=d3d11va-copy`, composited behind the UI. |
| The commands really do run off the main thread | The macro path is proven (`ExecutionContext::Blocking` vs `sync_threadpool` in `tauri-macros-2.6.3`) and pinned by a test, but "the window stays responsive during a 24-second refresh" is an observation somebody has to make on Windows. | Partly: the window drew and took clicks throughout a 22,121-channel import (§11). |
| A recording survives a real provider's stream | The recorder has only met the test server. | Still open. |
| Catch-up works on a real panel | The probe in `docs/ROADMAP.md` found `timeshift.php` 404s on the one panel available, so F-23 (the UTC/local timezone bug) cannot be fixed against evidence. | Still open. |
| TMDB's real responses | Parsed from the documented shape, never called with a key. | Still open. |
| Artwork served from the cache | `assetProtocol` is still not enabled; the UI renders remote URLs. The CSP now allows `asset:` so the swap is one config flag away. | Still open (checklist item 7). |
| The licence notices reach a viewer | The files are bundled and Settings → About reads them from beside the executable, which is verified against the mock; whether the packaged `.exe` really has them beside it needs the installer to have run. | Still open (checklist item 5). |
| The installer, upgrade and uninstall | No Windows runner here. | Still open (checklist item 5). |
| High-DPI, multi-monitor, per-monitor DPI | Needs the real shell. | **Answered for two monitors at different scale factors** (§11); a single window moved between them keeps its surface and its picture. Fullscreen and a live DPI change while playing are still unrun. |
| Media keys | Needs the real shell. | **Not a verification question — they are not implemented.** `useHotkeys.ts` binds `k`, `j`, `l`, `f`, `g`, `i` and the arrows; nothing anywhere binds `MediaPlayPause` or registers a global shortcut. No machine would have shown this working. |
| Screen-reader behaviour | Roles, names and tab order are in place and asserted where they can be; an actual NVDA/Narrator pass is a human job. | Still a human job. |

---

## 11. The Windows pass

Everything in §1–§9 was run in a Linux container. This section is a different machine:
the one this ships for, with a display, a GPU and libmpv. It is what closes item 1 of
`AUDIT/release-checklist.md`, which every other open question was downstream of.

```
Windows 11 Home 26200           rustc 1.98.1 (MSVC)       pnpm 10.34.5
VS Build Tools 17.14            MSVC 14.44.35207          Windows SDK 10.0.26100
WebView2 runtime 153.0.4234.48  msedgedriver 153.0.4234.48
libmpv v0.41.0-1078-g35af06172  (mpv-dev-x86_64-20260926-git-35af06172b.7z)
```

That archive name is the thing item 8 asks to record per release. Its DLL is called
`libmpv-2.dll` — not `mpv-2.dll`, which is what `docs/BUILDING.md` and the checklist
both called it. CI globs for `*mpv*.dll` and reads the name back off the file, so CI was
right and the prose was wrong.

One trap worth writing down, because CI cannot hit it: `vswhere -latest -products *`
resolved to **SQL Server Management Studio**, which is installed through the Visual
Studio installer and has no C compiler in it. Selecting the toolchain by
`-requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64` is what finds the Build
Tools on a developer machine.

### The link that had never happened

```
$ cargo build --release -p aurora-app        # the committed profile, LTO and all
    Finished `release` profile [optimized] target(s) in 7m 02s
-rwxr-xr-x  12811264  aurora-app.exe
```

Zero warnings. The checklist said to expect "a compile error or two" the first time
`window.rs` and `main.rs` met a Windows compiler. There were none.

### The suites, on Windows

| Suite | Result |
|---|---|
| `cargo test --workspace --all-targets` | **871 passed**, 0 failed, 3 ignored (the soak tests) |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean **after** the fix in §12 — it had never run against this code |
| `cargo audit` | **0 vulnerabilities**, 7 informational warnings |
| Playwright journeys | **114 passed** (1.3m), Windows Chromium |
| Vitest | 14 passed |
| `scripts/version.test.mjs` | 14 passed |

### The host harness, against the real binary

`tests-host/run.py` runs on Windows now (`docs/TESTING_THE_HOST.md`): no Xvfb,
msedgedriver behind `tauri-driver`, and a per-scenario wipe that fails loudly rather
than silently handing the next scenario the previous library. Against a real
subscription:

| Scenario | What it found |
|---|---|
| `starts_up`, `first_run_import`, `update_download` | pass |
| `real_panel` | **22,121 channels, 117,587 films, 28,529 series** imported; Live TV, Movies and Series all drew. "Live TV says No channels" does not reproduce, and F-04's series listings are written rather than counted. |
| `browse` | 203 shelves over 117,587 films; the filters narrow correctly |
| `recommendations` | 24 cards carrying a reason, ranked in 0.09s |
| `series_episodes` | 28,529 shows, 6 episodes after opening one — they are fetched on open, which is why `episodes` is 0 immediately after an import |
| `tour` | all eight screens drew |

Credentials are redacted against a real panel too —
`player_api.php?username=***&password=***` — which is F-15 holding outside a test.

### Item 1, in numbers

`tests-host/scenarios/video_surface.py` asks the desktop rather than the WebView, since
a WebDriver capture photographs a transparent WebView over nothing at all:

| Question | Answer |
|---|---|
| Is anything being decoded? | `1920x1080`, `H.264 / AVC`, `60fps`, `hwDecoder=d3d11va-copy`, `bufferSecs=8.0` — mpv's own numbers, read back through `player_state` |
| Is video *behind* the UI? | One top-level window for the process; mpv's `STATIC` surface is a `WS_CHILD` of it and **last of 2 children** in z-order |
| Does it reach the screen? | **95.4%** of a sampled grid over the middle of the window changed between two captures 1.2s apart. Two captures of a still window measure 0.0%. |
| Does the UI take input over it? | A click on the OSD changes what is on screen |
| Do the surfaces stay together? | After a resize: client `1220x740`, surface `1220x740` |
| Zap time | **1.91s**, and 1.96s and 2.13s on re-runs, against the 1.5s budget in README §16 |
| Two monitors, and a scale change | Moved to a second display of a different scale factor, the surface follows exactly — client `1442x902`, surface `1442x902` — and video keeps reaching the screen (16.2% of the middle moving) |

The captures are in `screenshots/host/`, and `video_surface-playing-1.png` is a frame of
the stream with its timecode running — Phase 0, in one file.

**Zap is over budget.** 1.91s is one measurement, on one machine, against a public CDN
rather than a provider — so the scenario reports it rather than failing on it. It is
still the first real number this project has had, and it is outside the budget the
project set itself.

---

## 12. What running it on Windows found

Six things, none of which any suite in §1–§9 could have caught.

**0. Without libmpv, the app dies in the loader and says nothing** (F-34, now fixed).
Found by
asking what a *failed* update leaves behind. `create_backend` reads as though a missing
libmpv degrades to `NullBackend` with a line in the log; it cannot, because `mpv.lib`
makes the DLL a load-time import and the loader runs before `main`. Measured with the
DLL moved aside:

```
alive: False   exit: 3221225781 (0xC0000135, STATUS_DLL_NOT_FOUND)
aurora.log grew: 0 bytes
stdout/stderr: ''
```

Holding it open with an exclusive handle — what a virus scanner does — ends the process
the same way with `0xC0000043`. No window, no log, no message, and every instruction
this project gives for diagnosing a bad start begins "the log says which step failed".
The fix is to delay-load it so the failure becomes a value the existing fallback can
report.

**1. A stream that will not open leaves the player loading for ever** (F-30). The worst
of them, and the most ordinary thing that happens to an IPTV player. The event pump
drained with `while let Some(Ok(event))`, which stops at the first `Err` and discards
everything queued behind it — and an `Err` is exactly how a dead stream arrives, because
libmpv2 reads the error code off the end-file event before it builds an `Event`. So the
status stayed where the load left it, no error was ever published, and the failover in
README §7.14 never fired. Reproduced against a URL that refuses every connection:

```
statuses seen: ['loading'], error=None
```

Underneath it, the error branch described failures by reading an `error-string`
property, which mpv does not have — so every playback error in this application's
history was classified from the literal word "unknown". Both fixed, with
`tests-host/scenarios/stream_failure.py` as the regression test.

**2. `cache-dir` is not an mpv option, so the timeshift buffer was never where it was
meant to be.** Every tune of a buffered live channel logged `mpv rejected cache-dir:
Raw(-8)` — `MPV_ERROR_PROPERTY_NOT_FOUND`. Asked directly, libmpv v0.41 has no
`cache-dir` at all:

```
           cache-dir: set_property=property not found  set_option=option not found  option-info=None
   demuxer-cache-dir: set_property=success             set_option=success           option-info='demuxer-cache-dir'
```

`cache-on-disk=yes` was accepted and the directory beside it refused, so mpv buffered
into its own default folder rather than the one the viewer chose: a portable copy kept
state outside itself, and `timeshift::bytes_on_disk` measured an empty directory, so the
budget was never enforced against anything. Fixed in `aurora-player/src/backend.rs`,
with a test that also asserts the name mpv does not have is no longer emitted.

**3. Clippy fails on the Windows-only code, because it had never seen it.** `mpv.rs`
carried four `const _: Option<…> = None;` lines whose stated purpose was to silence
unused-import warnings "for the resize/message plumbing the host wires up". Nothing
wires it up — resizing goes through `SetWindowPos` — so `WPARAM`, `LPARAM`, `LRESULT`,
`RECT` and `HashMap` were simply unused, and the workaround was hiding that rather than
serving it. Imports and workaround both removed.

**4. `option_env!("AURORA_TMDB_KEY")` is not tracked by cargo.** With a warm target
directory, the key compiled in is whatever the last build happened to see — so rotating
the secret can leave the old one in the binary with nothing to say so. `build.rs` now
emits `cargo:rerun-if-env-changed=AURORA_TMDB_KEY`.

**5. `player_controls` could only ever have passed on a platform with no video.** It
read the player's status out of the `player control` log line, which records the status
**at the moment the command was issued**. `NullBackend` has nothing to load, so that is
already `Playing`; mpv's `play` returns while still `Loading`, because opening a stream
is asynchronous. Against a real panel it failed with `('play', 'loading')` and went on
failing when the timeout was raised to 60 seconds — it was not slow, it was asking the
wrong question. It now polls `player_state` for what the host is doing *now*.

And one thing about the machine rather than the code: this checkout lives under OneDrive,
whose cloud placeholders deny `rmdir` on a directory written moments ago. That breaks the
harness's per-scenario wipe, and it means every build artefact under `target/` is being
synced. `CARGO_TARGET_DIR` belongs somewhere outside it.

**`run.py` honours it now**, so that is one environment variable rather than two — the
data directory follows the executable, and pointing the build elsewhere used to mean also
pointing `AURORA_TEST_EXE` at the result. Measured both ways on the same directory:

```
OneDrive tree : FAILED after 30.5s   (wipe exhausts its 120 retries)
external tree : wiped ok in 0.00s
```

With `CARGO_TARGET_DIR=C:/aurora-target`, the three scenarios that need no subscription
— `starts_up`, `first_run_import`, `update_download` — pass on Windows 11, and the wipe
between all eleven invoked scenarios succeeds. The other eight skip for want of panel
credentials, which is what they should do.

---

## 13. The portable self-update (checklist item 1b)

The claim the whole in-app update path rests on — that Windows lets a program rename its
own running `.exe` — had never been tested, because none of it is `#[cfg(windows)]` and
Linux proves nothing about it. A portable copy of 0.11.0, given a staged 0.11.1:

```
   old exe 13,589,504   new exe 12,845,056
   [first run]       it says it is version 0.11.0
   staged:           ['.ready', 'aurora-app.exe']
   [after staging]   it now says it is version 0.11.1
   log:              a staged update was applied before launch: applied 1 files
   previous/         ['aurora-app.exe']
   staged/ gone      True
   [third run]       previous/ still there: False
```

The swap works, the rollback copy is kept, and the launch after that forgets it — which
is exactly the lifecycle the checklist describes.

Two things worth recording beyond the pass. Applying an update **restarts the app**, so
anything driving it has to kill by path rather than by PID — the process that comes back
is not the one that was started, and the first attempt at this test was blocked by its
own orphan. And a refused apply does not roll forward into a broken install: with the
staged update unappliable, the old copy kept running, and the next launch after the
obstruction cleared applied it successfully.

Step 4 of that item — break it on purpose — is what found F-34, above.

---

## 14. The artwork cache, on a real library (checklist item 7)

`aurora-ingest::artwork` had downloaded, evicted and reported since it was written, and
nothing read from it: every poster in the library was fetched from the network on each
paint. Turning it on needed three things, and only two of them were in the plan.

**What the plan had.** `assetProtocol` enabled — with the folder granted at startup
rather than in the manifest, since a portable copy keeps its artwork beside the exe and
an installed one under `%LOCALAPPDATA%` — and image components that prefer a local copy
while always starting from the remote URL, so nothing can be worse than it was.

**What only running it could show.** Prefetching cannot work at this size (F-35). The
first attempt cached 39 posters and painted 117 images with **no overlap at all**,
because `artwork_prefetch` takes an unordered `LIMIT` from a 117,587-row table. So the
cache warms on view instead: a screen asks for what it needs, gets what is on disk, and
the host fetches the rest on its own thread.

Against a live panel:

```
   cache before: 45 files
   first look : 117 images, 1 from the cache (a cold cache should be near zero)
   cache after: 45 -> 181 files, 26,751,383 bytes
   images     : 117 total, 117 from the cache (41 of them fetched so far - the rest are
                lazy and below the fold), 0 from the network, 0 broken
   example    : http://asset.localhost/C:/…/artwork/2bf6d0 (600x900)
```

Three things worth keeping from that run beyond the pass:

- **The asset protocol serves the URL form this project builds.** `artwork::asset_url`
  leaves `/` and `:` unencoded where Tauri's own `convertFileSrc` percent-encodes the
  whole path; its comment had said "nothing has run the app to confirm the WebView
  serves it". It does — that example decoded at 600x900.
- **TMDB was called with a real key for the first time**, matching 39 of 80 titles from
  the panel's catalogue and returning posters that were then downloaded and displayed.
  The row in §10 that read "parsed from the documented shape, never called with a key"
  is closed.
- **Asking the mock a question it cannot answer cost two journeys.** The first version
  of the hook ran its effect and its IPC call for every card in every grid, including in
  a browser — where there is no host, no cache and nothing the answer could ever be but
  "no". That was enough extra settling time to make the end-to-end suite flaky: 114/114
  before the change, 113/114 after it, with a *different* test failing each run
  (`browse-paging`, then `continue-watching`). Both were verified as pre-existing-clean
  by reverting the change and re-running — 114/114 — rather than by assuming. The hook
  now returns the remote URL immediately when `isNativeHost()` is false, which is both
  the fix and the honest behaviour, and the suite is back to 114/114.
- **`loading="lazy"` makes most of a grid unmeasurable.** Of 117 cached images only 41
  had been fetched; the rest sat below the fold with `naturalWidth === 0` because the
  browser had never asked for them. An earlier version of the scenario counted those as
  broken and accused the asset protocol of failing on 76 images it had never requested —
  which is a good reminder that `naturalWidth` alone does not distinguish "broken" from
  "not asked for yet". The test now judges only images whose `complete` is true.

