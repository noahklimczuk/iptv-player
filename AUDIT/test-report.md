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
| `aurora-app` `contract.rs` | 4 | 5 | +1 |
| `aurora-app` `command_args.rs` | 1 | 2 | +1 |
| `aurora-app` `stress.rs` (soak, `--ignored`) | — | 3 | **+3 (new)** |
| Rust doc-tests | 1 | 1 | — |
| **Rust total** | **713** | **776** | **+63** |

`cargo test --workspace --all-targets` reports **772**: it excludes the doc-test and
does not run the three `#[ignore]` soak tests, which are run separately in §6.
| Playwright journeys | 84 | 96 | +12 |
| Vitest (TypeScript units) | **0 — suite exited 1** | 12 | **+12 (new)** |
| `scripts/version.test.mjs` | 14 | 14 | — |
| **Grand total** | **811** | **898** | **+87** |

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
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
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

Summed: `88 + 2 + 5 + 255 + 9 + 206 + 186 + 21 = 772`, plus 1 doc-test and the 3
soak tests below = **776**.

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

**Rust: not run.** `cargo-audit` is not installed in this container and building it
was not worth the minutes against the rest of this work. `Cargo.lock` is committed and
pinned; running `cargo audit` (or `cargo deny`) in CI is item 6 of the release
checklist.

---

## 10. What could not be verified here, and why

Honest list. Everything below needs a Windows machine with `mpv-2.dll`, or a real
subscription, and no amount of work in this container changes that.

| Claim | Why not |
|---|---|
| Video decodes and composites behind the WebView | Phase 0. `MpvBackend::attach`, `window::attach_video_surface` and `MpvBackend::pump` are written and called from nowhere, and `tauri.conf.json` still has `"transparent": false`. F-24 — deferred, with reasons, and item 1 of the checklist. |
| The commands really do run off the main thread | The macro path is proven (`ExecutionContext::Blocking` vs `sync_threadpool` in `tauri-macros-2.6.3`) and pinned by a test, but "the window stays responsive during a 24-second refresh" is an observation somebody has to make on Windows. |
| A recording survives a real provider's stream | The recorder has only met the test server. |
| Catch-up works on a real panel | The probe in `docs/ROADMAP.md` found `timeshift.php` 404s on the one panel available, so F-23 (the UTC/local timezone bug) cannot be fixed against evidence. |
| TMDB's real responses | Parsed from the documented shape, never called with a key. |
| Artwork served from the cache | `assetProtocol` is still not enabled; the UI renders remote URLs. The CSP now allows `asset:` so the swap is one config flag away. |
| The installer, upgrade and uninstall | No Windows runner here. |
| High-DPI, multi-monitor, per-monitor DPI, media keys | Needs the real shell. |
| Screen-reader behaviour | Roles, names and tab order are in place and asserted where they can be; an actual NVDA/Narrator pass is a human job. |
