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

## D7 — Recordings are the provider's stream copied verbatim, not remuxed

**Context.** A DVR has to turn a live HTTP stream into a file. The obvious instinct is to
remux into MP4 so the result is a "normal" video file.

**Decision.** Copy the bytes straight to `.ts` with no container work at all.

**Consequence.** MPEG-TS is designed to be cut anywhere: a recording interrupted by a crash,
a dropped connection or a closed window is playable up to the point it stopped. An MP4
truncated the same way is a broken file, because its index is written at the end. It also
means no ffmpeg dependency and no CPU cost during recording. The price is larger files and a
container some external players handle less gracefully — acceptable, since the player that
matters here is mpv, which handles it natively.

## D8 — The DVR keeps its own clock, on its own thread

**Context.** Recordings are due whether or not the UI is open, the window is focused, or
anyone is looking.

**Decision.** `aurora_app::dvr::Dvr::tick` does the whole job — reap finished, stop expired,
start due — and a dedicated thread calls it every ten seconds. The UI learns what changed
from a `dvr.tick` event; it never drives the schedule.

**Consequence.** A minimised window still records. Because `tick` takes `now` as an argument,
the entire scheduler is testable on a fake clock without waiting for real time to pass, which
is what the eleven `aurora-app` DVR tests do. The trade is a fixed poll interval rather than a
timer per recording; ten seconds is well inside the default one-minute pre-padding, so a
recording still starts before its programme does.

## D9 — Conflicts are resolved when the schedule changes, not at record time

**Context.** A subscription allows N simultaneous streams. More than N recordings can be
scheduled for the same window.

**Decision.** `find_conflicts` runs over the upcoming schedule and the UI shows the clash as
soon as it exists, naming the recording that will lose.

**Consequence.** A viewer who sees "will not record" a day ahead can do something about it; one
who finds out afterwards cannot. The scheduler still enforces the limit at record time as a
backstop, but by then it is only reporting.

## D11 — The metadata matcher declines rather than guessing

**Context.** A search for a title returns several plausible results. Something has to pick.

**Decision.** `aurora_core::tmdb::pick_best` returns `None` below a confidence floor, and
also when the top two candidates are too close to separate. Popularity breaks ties and
does nothing else. The outcome — including "nothing matched" — is stored, so the same
question is never asked twice.

**Consequence.** A library with missing posters is obviously incomplete and prompts
someone to look. A library where *The Matrix* wears *The Matrix Reloaded*'s poster looks
finished and is wrong. Without a year to separate them, "The Office" is two shows and
"Alone" is six films, and choosing the popular one is a guess wearing a confident face —
so it declines instead. `metadata.rematch` is the escape hatch for the cases where it is
confidently wrong anyway.

## D12 — Enrichment and playlist import each own their own columns

**Context.** Two writers touch `movies` and `series`: the provider refresh and the
metadata pass. Either could clobber the other.

**Decision.** The refresh's UPDATE list omits `overview`, `backdrop`, `logo_art`,
`tmdb_id` and the rest; enrichment writes those through `COALESCE(?, existing)` and never
touches `title`, `url` or `provider_key`.

**Consequence.** The two can run in any order, as often as they like, without a refresh
wiping fetched artwork or a TMDB result with a missing overview blanking the one the
playlist supplied. It is a convention rather than something the schema enforces, so both
sides say so in a comment where the columns are listed.

## D14 — An unknown language is kept, not hidden

`aurora_core::lang` answers "what language is this in?" with a code or with nothing, and
"English only" hides only what came back as a language other than English. Untagged
content stays.

The alternative — treat unknown as foreign — is more thorough and much worse. Providers
tag inconsistently: a playlist will label its Arabic and French sections carefully and
leave the English-language blocks bare, because to that provider English is the default.
Hiding the unknown would empty most libraries of exactly the content the viewer wanted
to keep, and the failure would look like a broken import rather than a filter doing its
job.

So the detector is allowed to say nothing, and it says nothing often: a bare `CNN`, a
`VIP|` prefix, `The German Doctor`. The settings screen shows the untagged count beside
the non-English one, because that number is what explains why the filter is not more
aggressive.

## D15 — Filters run at query time; nothing is deleted

English-only and duplicate collapsing are `AND` clauses on the list queries, driven by
two columns (`lang_code`, `quality_rank`) computed once at import. Turning a filter off
restores the library exactly, because nothing left.

Deleting would be simpler to query and impossible to undo — and pointless, since the
next provider refresh brings every deleted row back. The columns are the compromise:
classification is a Rust decision with tests, stored as data SQL can sort and compare,
so a list paint costs a predicate rather than a pass over the catalogue.

Duplicate collapsing is phrased as "no better copy of this exists" rather than as a
`GROUP BY`, which lets it compose with whatever else a query already asks — a genre, a
sort, a page — instead of forcing every query to be rewritten around a subquery. The
inner search carries the same language rule, so a 4K Spanish rip cannot suppress the
English HD copy it is not allowed to replace.

## D16 — A failing stream is demoted, never banned

A channel's sources are ordered by the provider's priority, with one adjustment: a
source that failed recently sorts last for a cooling-off period proportional to how many
times in a row it has failed, capped at six minutes. Anything that has worked since it
last failed is healthy again immediately.

The obvious alternative — sort by `fail_count` — is what the schema originally implied,
and it is wrong in both directions. It never forgets, so the stream the viewer actually
wants is permanently demoted by one bad evening; and it has no sense of *when*, so a
failure from last week counts the same as one from ten seconds ago.

Ranking also never filters. A channel whose every source is currently sick must still
offer all of them to try, because returning an empty list turns "the provider is having
a moment" into "this channel does not exist".

Rollovers within a single tune are bounded. A provider in a total outage would otherwise
spin through every URL it owns for ever, and a stopped picture with an error on it is
more honest than an endless reconnect.

## D17 — The updater checks; it does not install

Aurora installs from a GitHub release rather than a store, so nothing would otherwise
tell a viewer that the bug they hit was fixed a week ago. The app asks GitHub for the
newest published release, compares it against `env!("CARGO_PKG_VERSION")`, and says so
in Settings.

It stops there. Tauri's updater plugin would download, verify and install — and it
refuses to run without a signing keypair, whose private half has to be a repository
secret. Until that key exists, an "auto-updater" would be downloading an executable on
the strength of an HTTP response and running it, which is a materially different thing
to offer than a link to a page someone can read first.

Two consequences shape the code. The comparison uses `aurora_core::version` rather than
string ordering, because `"0.9.0" > "0.10.0"` is true of strings and would stop the
updater offering anything ever again past `.9`. And the page it opens is a compile-time
constant, not the `html_url` the API returned: a command that opens whatever URL it is
handed is a way to make the app launch something else.

The check is cached for six hours and stored, so opening Settings costs no network and a
machine that is offline does not retry in a loop. A failed check leaves the last good
answer in place rather than blanking the panel.

## D18 — The version is derived from the commits, not declared

The third number moves for a fix, the middle one for a feature (README §23). Nobody is
asked to remember that: the release build reads the conventional-commit subjects since
the last `v*` tag and works it out. One `feat` makes the release a minor; everything
else, including a batch with nothing conventional in it, is a patch — a build that ships
still needs a number of its own.

A breaking marker counts as a minor rather than a major. Below 1.0 that is what semver
prescribes, and above it the decision is a person's, not a script's, so nothing here ever
moves the first number.

The number is stamped into the manifests at build time and never committed. That keeps
CI out of the business of pushing to `main`, and it means the running binary's
`CARGO_PKG_VERSION` and the release it came from are the same by construction rather than
by anyone remembering to bump a file. The committed manifests therefore lag; `git` holds
the last released version, the tags hold the truth.

The one sharp edge is that `git log <lasttag>..HEAD` cannot be answered by a shallow
clone, and a swallowed failure there would report "no commits", which reads as a patch —
every release would be a patch and no feature would ever move the middle number, wrongly
but invisibly. So the tooling fails loudly instead, and the release job checks out full
history.

## D13 — Deferred from this pass

Not yet built, and not silently dropped (README working-agreement rule 4). Tracked in
`docs/ROADMAP.md` against their phases: Stalker portals (§4.3, marked Optional), downloads (§8.6,
Optional), timeshift (§7.6, Phase 8 — recording and catch-up are built), casting,
voice search, gamepad (§14.3), and a local artwork cache (§12 — enrichment stores absolute TMDB
image URLs, so every poster is currently fetched from the network on each paint).
