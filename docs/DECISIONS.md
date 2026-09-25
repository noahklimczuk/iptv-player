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

## D17 — The updater installs, and checks the file against a published digest first

Aurora installs from a GitHub release rather than a store, so nothing would otherwise
tell a viewer that the bug they hit was fixed a week ago. The app asks GitHub for the
newest published release, compares it against `env!("CARGO_PKG_VERSION")`, and says so
in Settings.

It used to stop there, and the reason it stopped was specific: an updater that downloads
an executable and runs it "on the strength of an HTTP response" is a materially
different offer from a link to a page someone can read first. That argument was right
about the risk and wrong about the options, because it assumed the only way to verify a
download is a code-signing key this project does not have.

GitHub publishes a SHA-256 for every release asset, in the same authenticated API
response that names the version and the download URL. So the installer is fetched to
`<data>/updates/`, hashed as it is written, and only renamed into place if the length
and the digest both match what that response said. A mismatch deletes the file rather
than keeping it, because the thing being described is about to be executed. A release
that publishes no digest is not downloaded at all — "no checksum" is a refusal, not a
step to skip.

Three more rules, each of which is a thing that could otherwise go wrong:

- **The URL is never handed in.** `updates.download` takes no arguments; the host uses
  the URL from the release it just checked, and that URL has to start with
  `https://github.com/noahklimczuk/iptv-player/releases/download/`. The trailing slash
  is load-bearing — without it, `github.com.example.invalid` and
  `github.com@example.invalid` both pass. Same reasoning as `updates.openReleases`
  taking no URL, one step further along.
- **The filename is built from the version**, which is parsed as `major.minor.patch`,
  so nothing the network said reaches the filesystem as a name.
- **Nothing is silent.** The viewer presses Download, watches a bar, and presses
  Install and restart. The installer runs visibly rather than with `/S`, because these
  builds are unsigned and Windows is going to say something about that — which the
  person doing the installing should see.

Two refusals worth naming. A **portable** copy is not offered the button at all: the
NSIS installer would install into Program Files and leave the folder actually running
untouched, which is how someone ends up with two copies and updates neither. And an
install **while a recording is in progress** is postponed with a message saying so: a
recording cannot be taken again later, and an update can.

What this still is not: **signature verification**, which README §18 asks for. The
digest and the file both come from GitHub, so this defends against a corrupted or
substituted download, not against whoever can publish a release. Closing that properly
means a minisign keypair — the private half a repository secret the release workflow
signs with, the public half compiled into the app — which is a key somebody has to
create and keep, and so is a decision rather than a commit. Until then the app says
plainly, on the button, that what it checked was a checksum.

Two consequences shape the rest of the code. The version comparison uses
`aurora_core::version` rather than string ordering, because `"0.9.0" > "0.10.0"` is true
of strings and would stop the updater offering anything ever again past `.9`. And the
check is cached for six hours and stored, so opening Settings costs no network and a
machine that is offline does not retry in a loop; a failed check leaves the last good
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

## D19 — The metadata key is built in, not asked for

Artwork, overviews and cast come from TMDB, which needs a key. Asking each person to
go and get one is a wall in front of the feature that makes the library look like a
library, and most people will simply not have posters.

So the release build bakes one in: CI passes a repository secret through
`AURORA_TMDB_KEY`, and `option_env!` puts it in the binary. Nothing is committed — the
key is not in a source file, not in git history, and not in the repository, which
matters here because the repository is public and a key in a public file is scraped
within minutes and revoked.

Three things follow from it.

A key a person supplies themselves wins over the built-in one. Someone who went and
got a key wants it used, and it carries their own rate limit rather than sharing the
built-in one with every copy of this build.

An unset secret has to behave as no key at all. CI sets the variable unconditionally,
so an absent secret arrives as an empty string; compiling that to `Some("")` would
send keyless requests that fail with a 401 nobody could explain. Blank is treated as
absent, and the app asks for a key exactly as it did before.

And the limit is worth stating plainly rather than implying: the key is inside the
shipped executable, and `strings` will find it. That is true of every application that
ships with a key, and no amount of obfuscation changes it. What the arrangement buys
is that the key is absent from the source, rotatable by changing one secret, and
never typed by a viewer. What it does not buy is secrecy from anyone holding the
installer.

## D20 — A refresh downloads before it takes the database

`sync::run` is two halves that cannot be confused: `fetch` takes an `HttpClient` and no
`Connection`, `apply` takes a `Connection` and no `HttpClient`.

The problem it solves was a correctness one. A refresh downloads a playlist and a guide
that can run to tens of megabytes, and it used to do that holding the single writer
connection. The DVR scheduler takes the same lock every ten seconds to decide whether a
recording is due, so a recording that fell inside a long refresh did not start until the
refresh ended — and on a slow provider that is minutes.

The fix could have been a comment and a habit. Making it a type means a future edit that
reintroduces it has to change a signature first, which is the only kind of rule that
survives.

Two consequences worth stating. The whole guide is held in memory between the halves,
which is what `import_epg` already did for one source at a time and is now true of all
of them at once — acceptable for the one or two guide URLs a provider publishes, and the
thing to revisit if that ever stops being true. And a failed download now writes nothing
at all, where before it could abort partway through the writes; that is strictly better,
but it is a change in what a failure leaves behind rather than a neutral refactor.

`apply` still holds the lock for its duration. That is deliberate: it is local, bounded
work, and an import visible halfway through would show a library whose search index had
been cleared and not yet rebuilt.

## D21 — Timeshift is mpv's own cache, not a buffer of our own

**Pause live TV keeps the stream in mpv's on-disk demuxer cache. Aurora writes no ring
buffer and opens no second connection.**

The obvious build is the one README §7.6 describes literally: a ring buffer on disk that
something fills from the provider while the player reads from behind it. That something
would need its own HTTP connection to the stream — and a connection is exactly what an
IPTV subscription rations. The DVR already refuses to start a recording that would exceed
`max_connections` (§7.7), because exceeding it does not queue, it gets the line cut. A
buffer writer of our own would spend a second connection on the channel already playing,
so watching one channel with pause available could cost what a two-tuner subscription
sells as its whole capacity.

That turned out to understate it. The first real subscription this was run against
reports `max_connections: 1` (docs/ROADMAP.md, "What a real subscription showed"), so a
second connection would not have been expensive — it would have made pausing live TV
mutually exclusive with watching it.

mpv has the same buffer already, on the connection it is playing: `--cache-on-disk` with
`--demuxer-max-back-bytes` keeps the past on disk, and `--force-seekable` lets it be
seeked into. One connection, no second copy of the bytes, and nothing to keep in step.

What Aurora owns is the arithmetic. mpv enforces a cap in *bytes*, and a person thinks in
*minutes* — a gigabyte is hours of a radio stream, eighteen minutes of an 8 Mb/s feed and
seven of a 20 Mb/s one. So the budget carries both, `aurora_core::timeshift` decides which
one binds at the observed bitrate, and the window it reports is bounded by a third thing
that neither cap describes: how long the channel has actually been on. Ten seconds after a
zap there is ten seconds of rewind, whatever the settings say. That arithmetic is pure and
has tests; the OSD's scrub bar is drawn from it, and a bar that offered half an hour of
rewind into a seven-minute buffer would be worse than no bar at all.

Consequences worth stating plainly. The buffer is per playing stream, so a zap starts it
over — nothing of the previous channel is reachable, which is what a single-connection
buffer means. The options are properties of one long-lived mpv handle rather than
arguments to one file, so turning the buffer off has to be *said* on the next load
(`demuxer-max-back-bytes=0`) and not merely left unsaid, or a channel tuned afterwards
would inherit it. Changing the size applies at the next tune, because re-loading to resize
a cache would black out whatever is on. And none of it has been run: like everything
downstream of the Phase 0 spike, the mpv side compiles for Windows and has never met a
real stream, so `aurora_core::timeshift`'s bound on the window is the honest one and
mpv's own `demuxer-cache-state` is read only as a refinement where it answers.

## D13 — Deferred from this pass

Not yet built, and not silently dropped (README working-agreement rule 4). Tracked in
`docs/ROADMAP.md` against their phases: Stalker portals (§4.3, marked Optional), downloads (§8.6,
Optional), casting, voice search, gamepad (§14.3), and a local artwork cache (§12 —
enrichment stores absolute TMDB image URLs, so every poster is currently fetched from the
network on each paint).

## D22 — Every command is `(async)`, and the attribute is tested

**Context.** `tauri-macros` defaults a `#[tauri::command]` over a synchronous `fn` to
`ExecutionContext::Blocking`, and `body_blocking` runs the body inline in the IPC
handler — which the webview calls on the main thread. All 94 handlers here are
synchronous, so all 94 were running there, despite `docs/ARCHITECTURE.md` describing a
Tokio pool that does not exist.

**Decision.** `#[tauri::command(async)]` on every one, which for a synchronous `fn`
selects the `sync_threadpool` path: same body, same signature, run on the async
runtime's blocking pool.

**Consequence.** A refresh measured at 23.7 seconds against a real panel no longer
freezes the window for its duration — and, more to the point, the `ingest.progress`
events the progress bar is drawn from can actually be delivered while it runs, since
delivering one needs the main thread the refresh was holding. The same applies to
`updates.download` (a twelve-megabyte file), `providers.validate` (a twelve-second
connect timeout) and `channels.list` (twenty-two thousand rows sorted and serialised).

The reason this is a decision rather than a commit is the failure mode. The attribute
is one token, a command written without it looks completely correct, and the symptom —
a window that stops repainting — appears only on Windows, under a real provider, in a
build nobody runs in CI. So `tests/command_args.rs` fails naming any handler left on
the blocking path. Anything that can be lost by forgetting one token needs a test, not
a convention.

## D23 — A tune is held together; a rollover is not allowed to fight it

**Context.** `load_current` takes the player lock for one `load` and gives it back, and
a tune is a *sequence* of loads — a channel has several URLs and tries them in order.
Every Tauri command runs on its own thread, so pressing Ch+ twice is two `play_live`
calls, and the second fits straight through the gap between the first one's attempts.
Whichever finished last won, whichever the viewer asked for last. The 250 ms heartbeat
did the same thing from the other side: its rollover waited for the player lock and
then used it, so a stream that died during a zap rolled the *old* channel forward on
top of the new one.

**Decision.** A `tune` mutex held for the whole of `play_live`, `play_item`,
`play_catchup` and `stop`, plus a generation counter bumped before the lock is taken.
A tune that gets in first and finishes late can tell its result is no longer wanted and
returns `AppError::Superseded` rather than publishing a picture nobody asked for. The
heartbeat uses `try_lock` and gives up rather than waiting — waiting would be worse
than useless, since by the time the lock came free the error it is reacting to would
already have been overtaken.

**Consequence.** `Superseded` is a distinct error, not a failure: the UI recognises it
and stays silent, because a toast for every double press of Ch+ would be worse than
the silence it replaced. And rapid zapping is now something with a measurement rather
than a hope — 2,000 zaps at ~49,000/s, always ending on the channel last asked for,
with the heartbeat running and the stream killed every fiftieth zap
(`aurora-app/tests/stress.rs`).

## D24 — An unreadable library is replaced, and the viewer is told

**Context.** `aurora_db::open` propagated a corrupt file straight out of
`Services::new`, which Tauri turns into a start-up failure: no window, no dialog, and
no console in a release build to print to. The app did not launch and gave no reason.
A *failed* integrity check was worse — logged at error level and then used anyway,
against README §5's promise of "check + repair on startup".

**Decision.** `open_or_recover` moves the unusable file aside, WAL sidecars and all,
and starts a fresh one. Renamed rather than deleted, and the host keeps what it had to
do so Settings can say so.

**Consequence.** Two things follow that are worth stating. The old file is kept because
it holds favourites, watch progress and recordings metadata somebody may want
recovered, and destroying the only copy of that to clear an error message is not a
trade this should make unasked — it is on disk with a timestamp, and a person can
decide. And recovering *silently* would mean opening Aurora to find the library gone
with no explanation, which is a worse experience than the crash it replaced; so the
Diagnostics panel says it happened, where the old file went, and that a refresh will
rebuild the library.
