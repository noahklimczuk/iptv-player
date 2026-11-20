# Code and optimisation audit — November 2026

A pass over the whole Windows app and the shared crates (`src-native/crates/*`,
`shared/`, `src-ui/`), for defects and for work the app does that it does not need to.
`apple/` is out of scope; another agent owns it.

## How it was done

Reading 111,000 lines line by line finds style, not slowness. So this audit started from
evidence, then read the code the evidence pointed at:

- **What the installed app logged.** `dblock.rs` logs every hold of, or wait for, the
  library lock over 100 ms, with the source line. Two weeks of the real app's log name the
  places that actually make it slow.
- **Timing on a copy of a real library.** 25,257 films, 8,504 series, 213,304 episodes,
  254,310 guide programmes, 1.75 million credits, 735 MB. Every number below that says
  "measured" is from that copy, never the live profile.
- **Tools.** `cargo clippy` with the correctness, panic and performance lints CI does not
  turn on (2,994 warnings, filtered to the 300 in non-test code and read); `cargo audit`;
  `pnpm audit`; the built bundle's contents.
- **Targeted reading.** Every place the library lock is held across something slow;
  search; the import; the DVR scheduler; the assistant; UI listeners, timers and
  subscriptions.

## Findings

Severity is what a viewer loses, on the scale in `AUDIT/findings.md`.

| ID | Sev | Area | Finding | Status |
|---|---|---|---|---|
| A-01 | High | assistant | `assistant_send` holds the library's **write** lock for the whole conversation turn, Gemini's network round trips included (read timeout 90 s, several rounds). Every write in the app waits behind it: the DVR starting a recording, saving a watch position, a refresh. The real log shows a 4.1 s hold. | Fixed (batch 2): a connection of its own |
| A-02 | High | import / DVR | A refresh holds the write lock for the whole of `sync::apply` — 42 s and 52 s in the real log. The DVR's ten-second tick waits behind it (38 s and 48 s logged), so a recording due during a refresh starts up to a minute late and loses its opening. | Fixed (batch 2): the lock is taken step by step |
| A-03 | Medium | search | The palette took its 40 results **before** removing what the library filters hide. With "English only" or duplicate hiding on, hidden titles used up the limit and a search could return a handful of films, or none, while visible ones matched further down. Same for guide listings on filtered channels. | Fixed (batch 1) |
| A-04 | Medium | search / perf | The guide half of search took **3.5–7 s per keystroke**: SQLite followed `idx_prog_time` through every programme in start order, past ones included, fetching each row at random to test its title. Typing four letters occupied all three read connections, so browsing stalled too. | Fixed (batch 1): 0.45 s, same rows |
| A-05 | Low | search / perf | The people half read all 583,341 people and sorted them on every keystroke (250–490 ms). | Fixed (batch 1): index, < 10 ms |
| A-06 | Low | search / perf | Every keystroke read every visible id of all three kinds into hash sets (40,000 ids, ~160 ms) to test 40 hits against them. | Fixed (batch 1): one key lookup per hit |
| A-07 | Low | import / perf | A full `ANALYZE` at the end of every refresh took 1.8 s inside the write lock, 1.1 s of it on `credits`, which a refresh never writes. | Fixed (batch 1): 0.2 s |
| A-08 | Low | logging | The "could not be compacted" error carried 18 spaces mid-sentence: a string continuation missing its backslash. | Fixed (batch 1) |
| A-09 | Low | UI / perf | The development mock and its generated library ship in the release bundle and are built on every launch — 160 films and their SVG artwork, channels, guide — though the real app never calls them. | Fixed (batch 3): loaded only without a host; startup bundle 554.6 kB → 484.6 kB |
| A-10 | Low | home / perf | Rebuilding Home after watching something holds a read connection 0.3–1.3 s, mostly one point query per card (`hydrate`). | Fixed (batch 3): one query per kind per page, 20–40% faster; the rest is each card's cast list |
| A-11 | Info | deps | `pnpm audit`: one moderate advisory, `minimatch` under `eslint` (dev only, not shipped). `cargo audit`: no vulnerabilities; seven "unmaintained" notices, all transitive through Tauri. | No action |
| A-12 | Low | AI / perf | The Gemini rail and status read the library through the writer, so a refresh or a recording write held them up for no reason. | Fixed (batch 3): reader pool |
| A-13 | High | privacy | mpv's own log lines name the stream address being opened, and an Xtream address carries the account's username and password in its path. The player rebuild (#173) dropped the redaction the old player applied, so `aurora.log` held them (four times in one evening of the real log) and Settings → Export logs copied the file as it was. | Fixed (batch 4): lines redacted when written, and again on export |
| A-14 | Medium | series | Opening a show asked the provider for its episode listing three times at once (three UI callers, no single-flight on the host) and wrote its episodes three times: `get_series_info` for one show three times within 3 ms in the real log. | Fixed (batch 4): one request per show at a time |
| A-15 | Low | text | Eleven more strings had the A-08 bug, a line continuation without its backslash, including the parental-lock error a viewer reads ("…without one nothing              locked could be watched"). | Fixed (batch 4), with a test that scans every crate for it |
| A-16 | Low | home / perf | Home's cards carried each title's cast list, about 90% of a card's query (35–44 ms vs 3–4 ms for 200 films), and no card shows it. | Fixed (#225) |

## What was checked and found sound

- **Panics.** `panic = "abort"` makes any panic a crash, so every `unwrap`, `expect` and
  unchecked index in non-test code was read. They are compiled-in regexes, slices guarded
  by a length check on the line before, or invariants (`last.expect(...)` after a loop
  that always sets it). None reachable from input.
- **Sync's vault** (`aurora-sync/src/vault.rs`): every slice of remote bytes is length
  checked before it is cut.
- **Other lock holders in the log** — Home (`home.rs`), metadata enrichment, series
  episodes, playlists — run on read connections or hold the writer only for local,
  bounded writes. Gemini recommendations and the update check do their network calls with
  no lock held.
- **UI.** Every `addEventListener` has its removal, every interval its clear, every
  `listen()` goes through `subscribe()`, which handles unmounting before it resolves.
  No zustand selector builds a new object per render. Posters load lazily. The palette
  debounces and discards stale answers.

## Batches

Fixed five at a time, one PR each, merged on green CI.

1. Search and import statistics: A-03, A-04, A-05, A-06, A-07, A-08.
2. Write-lock holds: A-01, A-02, and channel-to-guide matching saved in one transaction instead of one per channel.
3. Startup and Home: A-09, A-10, A-12.
4. From the real app and its log: A-13, A-14, A-15; A-16 on its own (#225).
