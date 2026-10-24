# Line-by-line review, October 2026

A read of every file, in dependency order, with the mechanical analysis run first because
it is cheap and covers everything. Findings are numbered `R-nn` to keep them apart from
`AUDIT/findings.md`, which records bugs that reached a release.

**Nothing is fixed while the review is running.** The list is settled first, then worked
through, so that a fix made early is not invalidated by something found later.

## Summary

### Fix these first

Six findings are worth acting on before the rest, because each one makes a shipped feature
either wrong or absent and none of them is hard to fix.

| # | What |
|---|---|
| **R-126** (Critical) | The provider's password is stored in plain text in `library.db` several hundred thousand times — `episodes.url`, `movies.url` and `channel_sources.url` keep the Xtream stream URL verbatim, and those URLs carry `username/password` in the path. `docs/PRIVACY.md` promises the opposite as a selling point. `library.episodes` also ships it into the WebView. |
| **R-61** | And the same credentials are not redacted from the log. Proven against the real playlist: all 6,699 live URLs are `http://host/<user>/<pass>/<id>` with no extension, and `looks_like_stream_id` requires one — so every one passes through `redact` intact. `DiagnosticsPanel` has a button whose stated purpose is to produce that file "for a bug report". |
| **R-141** | `docs/PRIVACY.md` tells a viewer to clear the Gemini key to stop their watch history going to Google. In a release build that deletes *their* key and falls back to the one compiled into the binary. There is no off switch anywhere in the host, the contract or the UI — and a green end-to-end test asserts the opt-out works. |
| **R-108** | `providers.max_connections` is never written by any production code path, so every connection budget in the app is wrong: the DVR will run two simultaneous recordings on a one-connection line, the warning strip written for that case never appears, and multi-view offers nine tiles. Three other places assert or assume the refusal works. |
| **R-118** | Nothing in the app can play a recording. `MediaKind` has no `'recording'`, no `dvr.*` command plays anything, and the Play button on every completed recording calls an optional prop nobody passes. |
| **R-115** | A viewer with OS reduced-motion enabled gets **one** button on every rail card instead of five — losing Play, My List, I-like-this, More info and Remove-from-Continue-Watching, on the home screen and both browse grids. Measured. README §12 specifies exactly three things Reduce Motion should disable, and this is not one of them. |

Close behind: **R-133** ("Record series" creates a rule that can never match anything, because
`newOnly: true` is hardcoded and the real guide has zero `<new/>` markers in 529,638
programmes), **R-139** (the provider sends every episode's runtime and plot, the parser reads
them, and the import drops them — which is why Up Next and Skip Credits never fire),
**R-130** (one guide refresh moves 14.8 MB across the IPC bridge, and every toolbar press does
it again), and **R-98** (the detail modal reads a film's resume position out of the mock,
which pre-seeds fabricated progress for four real films in every build).

### The thread running through most of this

`docs/TESTING_THE_HOST.md` opens by naming the project's largest source of shipped bugs:

> Everything in this repository that calls itself end-to-end runs the UI against the mock
> transport … it is also the single largest source of bugs this project has shipped … The
> pattern is the same every time: **the thing under test answered its own questions.**

It lists four historical instances (F-09 favourites, F-12 the CSP, `progress.save`, Continue
Watching). This review found the same pattern nine more times — R-98, R-99, R-100, R-104,
R-106, R-108, R-132, R-139, R-141 — and R-126 is the same mechanism running the other way,
where the host sends a field the contract never declared.

Three things keep it alive, and each has a small fix:

1. **The fixtures depict a fully-enriched library** (R-157). A real import writes ten fields
   per film; `makeMovie` hands every journey eighteen. Roughly half a real library stays
   unenriched permanently — `docs/ROADMAP.md` records TMDB matching 39 of 80 titles — and no
   journey has ever rendered that state.
2. **The field-level contract test covers one interface of sixty-four** (R-154).
   `contract.rs` already parses declared fields out of `shared/ipc.ts` and diffs them against
   the serialised Rust struct, in both directions, with a comment describing this exact
   defect class. Applied to `PlayerState` only. Generalised, its two assertions catch R-99 and
   R-126 directly.
3. **Five green tests certify behaviour a shipped build does not have** (R-159), because the
   fixture supplies what the host cannot — or, in `trailers.spec.ts`, because the assertion is
   an absence that cannot tell "suppressed correctly" from "nothing happened at all".

### What this review did not find

Worth stating, because it is most of the codebase. `aurora-core`, `aurora-db` and
`aurora-player` were read in full and are sound. The Tauri security surface is genuinely
minimal and correctly scoped — the asset protocol grants one cache directory, not the data
folder, which is the only reason R-126 is not remotely reachable. The updater refuses
anything without a published SHA-256 and pins the asset URL to this project's own releases.
`pip.rs` is the best-documented file in the repository and has one small finding in 832
lines. `cargo audit` reports zero vulnerabilities across 512 crates; `pnpm audit` reports
none. The decision log is current, its two numbering gaps reference nothing, and every
`Dnn` cited in the code exists.

The `tests-host` harness deserves particular mention: it drives the release binary over
WebDriver against a real SQLite file and a real panel, asserts Win32 HWND z-order, reads
`LATEST_VERSION` out of `schema.rs` rather than hardcoding it, and guards against its own
worst failure mode by asserting the video engine is real before trusting any playback
scenario. Several findings here are findings *because* that harness exists and stops just
short of them.

### Coverage

**Every file, every line: 262 files, 98,424 lines.** That is all production source in
`src-native` and `src-ui`, the whole IPC contract, all 202 Playwright journeys, the
`tests-host` harness and its eighteen scenarios, every document under `docs/`, the CI
workflow, the build scripts and every manifest. Counted with `.py` and `.ps1` included —
an earlier figure in this review omitted them and undercounted the harness by 3,500 lines.


### Findings by severity

| Severity | Count |
|---|---|
| Critical | 1 |
| High | 33 |
| Medium | 45 |
| Low | 48 |
| Info | 33 |

**163 numbered findings** across 192 sections — the rest are addenda to a numbered finding and entries recording a subsystem as verified clean.

### Index

| # | Severity | Finding |
|---|---|---|
| R-01 | High | 659 live channels are filed as films and episodes |
| R-02 | Low | `tvg-shift` is unbounded |
| R-03 | Low | `srt://` and other modern schemes are rejected as unusable |
| R-04 | Low | the skipped count conflates duplicates with unusable lines |
| R-05 | Medium | ordinary English words are stripped out of film titles |
| R-06 | Low | a duplicated episode keeps the first stream, not the best |
| R-07 | Low | `1x02` matching catches `4x4` |
| R-08 | Low | stripping a country prefix can collide two channels |
| R-09 | Info | five duplicated rows in the language table |
| R-10 | Info | Japanese written mostly in kanji is reported as Chinese |
| R-11 | Info | text before the first `|` is not read as a tag |
| R-12 | Medium | two error messages shown to the viewer contain a run of 18 spaces |
| R-13 | Low | "Try another source" is offered for failures that have no other source |
| R-14 | Info | two chapter keywords are also ordinary words |
| R-15 | Info | an "intro" chapter an hour into a film is accepted |
| R-16 | Info | `min_age` has unreachable match arms, and `+18` is not an adult marker |
| R-17 | High | a "strip from name" rule kills the app on a Turkish title |
| R-18 | Medium | the "hide adult" preset cannot match `18+`, the form playlists use |
| R-19 | Low | a non-regex rename replaces the whole name, not the matched part |
| R-20 | Low | rule ids collide, and the id is what the UI keys on |
| R-21 | Medium | 426 channels are told they have a guide that holds nothing for them |
| R-22 | High | the fuzzy pass matches channels that differ only by their number |
| R-23 | Info | the fuzzy pass is 4.5 million string comparisons |
| R-24 | Info | the guide declares 4,205 duplicate channels, and the two indexes |
| R-25 | High | TMDB title matching is an exact-equality test, and three mechanisms built on top of it are dead |
| R-26 | Low | `Window::clamp` panics on a NaN or an inverted window |
| R-27 | Info | `bytes_as_secs` can overflow a budget that did not come from `clamped` |
| R-28 | Info | `xmltv_ns` episode numbers can overflow a `u16` |
| R-29 | Info | `XmltvStats::skipped` counts two unrelated things |
| R-30 | Info | only the first `<title>` is kept, and `lang` is not read |
| R-31 | Context | what this subscription's guide actually contains |
| R-32 | High | the commonest failure on this line is diagnosed as "their server had an error" |
| R-33 | Medium | credentials go into stream URLs unescaped |
| R-34 | Low | 80 real programmes are discarded for a zero-length slot while a missing |
| R-35 | Info | the parser handles the real guide, including its BOM |
| R-36 | Medium | the recorder classifies network errors with the string the ingest |
| R-37 | High | one abandoned film erases a genre you have watched a hundred times |
| R-38 | Medium | a film on a shelf you watch is filed as a "discovery" while still |
| R-39 | Info | what the panel actually sends for films and series |
| R-40 | Low | an out-of-range `around_local_minute` makes a series rule match everything |
| R-41 | Low | `expand_rules` recomputes the programme's match key once per rule |
| R-42 | Info | `largest_fitting` reports a certainty it does not have |
| R-43 | Info | `margin * 2` in the PiP geometry can wrap before the guard runs |
| R-44 | Low | the `skip_markers` CHECK has no room for the `convention` source |
| R-45 | Medium | an unrecognised marker source becomes one the app may skip without |
| R-46 | Medium | the guard that stops Aurora moving your library aside is half dead |
| R-47 | Low | changing a recording rule's padding re-schedules everything it already |
| R-48 | Info | ruled out: the planner's statistics are not stale |
| R-49 | High | every refresh adds another complete copy of the guide, for ever |
| R-50 | Low | only the first of a channel's display names is stored |
| R-51 | Low | three columns the parser fills are never read back |
| R-52 | High | the A–Z letter bar sends 97% of the library to the letter "E" |
| R-53 | Medium | the app says dropped channels were "flagged"; nothing flags them |
| R-54 | Info | ruled out: the cast list's `ORDER BY` is a no-op but the order is right |
| R-55 | Low | `reclassify` says it is a fallback; it is the only thing that classifies |
| R-56 | Medium | search is inside its budget today and R-49 takes it out |
| R-57 | Medium | a shelf with an accent in its name contributes nothing to the rail |
| R-58 | Low | an age limit the code cannot parse becomes "no limit" |
| R-59 | Low | thirty films keep a year in their search key — **this finding was |
| R-61 | High | `redact` misses the URL shape this panel uses for every live channel |
| R-62 | High | the player logs mpv's output with no redaction at all |
| R-63 | High | pasting a panel URL with the wrong character in it kills the app |
| R-64 | Medium | the "streaming" EPG import holds the whole guide in memory |
| R-65 | Info | two sharpenings from the Xtream client |
| R-66 | High | a failed recording writes the username and password into the log |
| R-67 | Low | the stall detector cannot detect the stall the code says is characteristic |
| R-68 | Medium | the recommendation rail throws away Gemini's explanation; the |
| R-69 | High | the library database does contain credentials, and PRIVACY.md says it |
| R-70 | High | rewinding more than twelve seconds kills playback twelve seconds later |
| R-71 | High | the complete set of places a credential can reach the log |
| R-72 | High | the panic safety net does not exist in the shipped binary |
| R-73 | addendum | Open question — parallel function calls are encoded as separate model turns |
| R-74 | Medium | once the OSD hides, no key brings it back |
| R-75 | Low | the Browse grid is the one list that is not virtualized, and it grows |
| R-76 | Low | the OSD shows the provider's language tag; every list strips it |
| R-77 | Low | a doc comment has lost two clauses, and the sweep that found it was |
| R-78 | Medium | typing a radio station's number on the keypad finds nothing |
| R-79 | Info | one NULL in `categories` would empty the whole guide |
| R-80 | Info | verified fine: the guide loads every channel, and that is affordable |
| R-81 | Low | four `_ =>` fallbacks that all fail towards the permissive answer |
| R-82 | Medium | `library.refreshed` is emitted the one way Tauri rejects, and the |
| R-83 | Medium | the episode sweep re-asks shows it cannot list, up to 200 times |
| R-84 | Medium | the recommendation cache stops noticing new viewing after 40 titles |
| R-85 | Low | pressing volume-down at zero shows an error |
| R-86 | High | My List ticks never load, and switching profile shows the other |
| R-87 | Medium | every appearance setting is forgotten on restart |
| R-88 | Info | the guide's group list counts radio stations the guide then hides |
| R-89 | Low | the guide's programme lookup has a fallback key that can only ever be |
| R-90 | Low | the whole UI re-renders four times a second while anything is playing |
| R-91 | Low | Live TV's "now playing" never updates while you are looking at it |
| R-92 | Info | the artwork answer cache grows for the life of the session |
| R-93 | Info | the inlay hook forces a layout read on every scroll event in the page |
| R-94 | Medium | a scheduled recording's countdown can read "in 1h 60m" |
| R-95 | Low | "left" with nothing in front of it on a nearly-finished card |
| R-96 | Low | `bytes()` prints "1024 KB" and "1024 B" |
| R-97 | Low | a second notice restarts the first one's dismiss timer |
| R-98 | High | the detail modal reads a film's resume position out of the *mock*, and the mock has fabricated rows in it |
| R-99 | Medium | "% match" is a mock-only field, so it never appears in the real app |
| R-100 | Low | every title claims closed captions |
| R-101 | Medium | a show's resume label survives onto the next show |
| R-102 | Medium | `aria-modal="true"` on three dialogs, none of which hold focus |
| R-103 | Low | a per-show preference that fails to save stays switched on |
| R-104 | High | the "Details" tab is five invented rows, identical for all 27,494 films |
| R-105 | High | "More Like This" shows the same nine films for every title |
| R-106 | Medium | an episode's watch-progress bar is invisible unless it has artwork |
| R-107 | Info | opening any film asks the host for episode progress on series 0 |
| R-108 | High | `providers.max_connections` is never written, so every connection budget in the app is wrong |
| R-109 | Medium | "Record what's on" stores a window it did not record, and the comment explaining why is wrong |
| R-110 | High | every button on the player's error panel is inert |
| R-111 | Low | the scrub handle can stay stuck where it was dropped |
| R-112 | Medium | on Live TV a letter filter stays applied after the bar that set it is disabled |
| R-113 | Medium | the grid view's tile width is measured once and never again |
| R-114 | Low | the grid view drops the favourite and catch-up affordances the list has |
| R-115 | High | a viewer with reduced motion enabled gets no buttons on any rail card |
| R-116 | Low | the shared primitives, read line by line: four small gaps and nothing larger |
| R-117 | Medium | opening Recordings pulls the entire channel catalogue to label a handful of rows |
| R-118 | High | nothing in the app can play a recording |
| R-119 | Low | a series rule whose recordings all failed still reads "N recorded" |
| R-120 | High | the wizard asks "What should Aurora import?" and throws the answer away |
| R-121 | High | the final screen overstates guide coverage, and the host already knows the real number |
| R-122 | Medium | "You can keep using Aurora while this runs", said by a full-screen modal with no way out |
| R-123 | Medium | multi-view: the whole screen's premise is defeated by [R-108], plus two smaller gaps |
| R-124 | Medium | a typo in a channel number silently discards the renumbering |
| R-125 | Low | "Hide all 27,494 matching" is one click with no confirmation |
| R-126 | Critical | the provider's password is stored in plain text in `library.db` hundreds of thousands of times, and `docs/PRIVACY.md` says it is not |
| R-127 | Low | the assistant can show a card for a title it never looked up |
| R-128 | Low | the updater's two loopback checks can be bypassed with URL userinfo, and the defence against it is written down one branch away |
| R-129 | Low | two rapid tile changes can each pass a budget check the other invalidates |
| R-130 | High | one guide refresh moves 14.8 MB across the IPC bridge, and every toolbar press does it again |
| R-131 | Info | a hidden element rendering a timestamp to satisfy a linter |
| R-132 | Medium | five guide features are dead against this provider's guide, measured over 529,638 programmes |
| R-133 | High | "Record series" creates a rule that can never record anything |
| R-134 | Medium | the whole app re-renders four times a second while anything is playing, and the diagnosis for it is thirteen lines away |
| R-135 | Info | two contradictory doc comments on the same function, about error handling |
| R-136 | Low | a 12/24-hour clock preference that nothing reads, nothing sets, and no caller can reach |
| R-137 | Low | a window resize re-places the inlaid picture without re-clamping it |
| R-138 | Low | changing the timeshift folder orphans whatever is in the old one |
| R-139 | High | the provider sends every episode's runtime and plot, the parser reads them, and the import throws them away — which is why Up Next and Skip Credits do not fire |
| R-140 | Medium | the log export exists to be handed to somebody, and [R-61] says what is in it |
| R-141 | High | the documented way to stop sending your watch history to Google does not work in a release build |
| R-142 | addendum | [R-115] is a spec violation, not a judgement call |
| R-143 | Medium | the in-app keyboard reference omits six working bindings, and `F` does the wrong thing |
| R-144 | Info | `docs/ROADMAP.md` says picture-in-picture is not built; it is |
| R-145 | Medium | Settings → Refresh shows raw enum names for four of the nine import phases |
| R-146 | Info | `IconName` advertises a closed union and is `string` |
| R-147 | Medium | the palette's arrow-key highlight is never scrolled into view, and never announced |
| R-148 | Low | three smaller things in the palette |
| R-149 | Medium | auto-skip fires on the first episode of a session and never again |
| R-150 | Info | four stale comment pairs, each contradicting the fix beside it |
| R-151 | Medium | the hero billboard rotates under a keyboard user's hands |
| R-152 | Medium | after a PIN lockout the pad stays locked until the viewer backs out, and the countdown it shows is frozen |
| R-153 | Low | three of the four "Settings → X" pointers name a section that does not exist |
| R-154 | High | the test that would have caught five of this review's findings exists, and runs against one interface out of sixty-four |
| R-155 | Medium | there is no ESLint, and five `eslint-disable` comments suppress rules nothing runs |
| R-156 | Info | a direct push to `main` runs no tests, and this repository has had them |
| R-157 | High | every one of the 154 end-to-end journeys runs against a fully-enriched library, which no real library ever is |
| R-158 | Low | Tailwind is a build dependency that generates nothing the app uses |
| R-159 | High | five green end-to-end tests certify behaviour a shipped build does not have |
| R-160 | Info | every test count in `README.md` understates the suite, and the two docs disagree |
| R-161 | Medium | the unit-test layer covers two files, and its own config names the gap that is still open |
| R-162 | Medium | the two tests that guard the IPC contract do not run on a plain `cargo test` |
| R-163 | Info | `docs/TESTING_THE_HOST.md` describes a worker-pool hazard that the code has already closed |
| R-164 | Medium | the host harness cleans up credentials from the wrong service, so it never removes its own and is only safe by accident |

## Mechanical sweep

| What | Result |
|---|---|
| `cargo clippy -D warnings` (the existing gate) | clean |
| `cargo clippy -W pedantic -W nursery` | 1,102 warnings; almost all style. **69** are `cast_possible_truncation` / `cast_possible_wrap` — the only category that can be a real defect, and they are walked individually below. |
| `cargo test --workspace` | 15 suites, 0 failures |
| `tsc --noEmit` with `strict`, `noUnusedLocals`, `noUnusedParameters` | clean |
| ESLint | **did not exist.** No rule has ever checked React hook dependencies, exhaustive effects, or accessibility. A flat config was written during this review and kept outside the repository (see R-155); results below. |
| Playwright | 201 passed, 1 skipped |
| `tests-host` against the real panel | all green |

---

## Findings

### R-01 — High — 659 live channels are filed as films and episodes

`aurora-core/src/classify.rs`

Classification tries, in order: an Xtream path segment in the URL, an `SxxEyy` in the
name, the group title, then a file extension. On this subscription the first rule never
fires, because its live URLs are `/<user>/<pass>/<id>` — no `/live/` segment and no
extension — so the **group title decides**, and any group whose name contains "movie",
"cinema", "film" or "series" wins.

Applied to the real playlist offline:

```
live streams classified as something else: 659
   520  as movie    US ... CINEMANIA
   103  as movie    US ... MOVIES
    13  as episode  US ... PEACOCK SERIES
    12  as episode  US ... DISNEY+ SERIES
    11  as episode  US ... APPLE TV+ SERIES
```

These are 24/7 channels — live streams that happen to play films. Filed as movies they
appear in the Movies library, where the player offers a scrubber and resume for something
that has neither, and they are missing from Live TV where they belong.

The shape of the fix is the ordering: a URL that is a bare id with no extension is a live
stream, and that is stronger evidence than a word in a group name. The group hint should
only decide when the URL says nothing either way.

**Independently confirmed from the parse itself.** Running `m3u::parse` over the real
51 MB playlist and counting by kind:

```
URLs of the bare live form (host/<user>/<pass>/<id>)   6,698
entries the parser classified as MediaKind::Live        6,038
                                                 difference 660
```

Those 660 are the same channels: every live URL in the playlist has the bare form, so
anything not classified `Live` was decided by its group title. The figure matches the 659
counted by replaying the classifier rules directly, which was arrived at a different way.


### R-02 — Low — `tvg-shift` is unbounded

`aurora-core/src/m3u.rs:258`

```rust
value.and_then(|v| v.trim().parse::<f32>().ok()).map(|h| (h * 60.0).round() as i32)
```

A provider sending `tvg-shift="1e20"` yields a saturated `i32` rather than a sane offset,
and the guide for that channel is displaced by millions of minutes. Not unsafe — Rust
saturates rather than wrapping — but a nonsense value reaches the EPG. A real shift is
within about a day either way.

### R-03 — Low — `srt://` and other modern schemes are rejected as unusable

`aurora-core/src/m3u.rs:272`

`looks_like_url` allows http/https/rtsp/rtmp/rtmps/udp/rtp/file, UNC paths and drive
paths. A playlist using `srt://` — which ffmpeg and therefore mpv support, and which
providers do use — has every such entry counted as "skipped unusable URL". The warning
names the line, so it is visible rather than silent, but the entries are lost.

### R-04 — Low — the skipped count conflates duplicates with unusable lines

`aurora-core/src/m3u.rs:164`

A duplicate URL increments `skipped` without a warning, while an unusable one increments
it with a warning. The number shown after an import therefore mixes "you have 400
duplicates" with "400 lines I could not read", which are different problems for the
person reading it.

### R-05 — Medium — ordinary English words are stripped out of film titles

`aurora-core/src/title.rs:12` (`RELEASE_TAGS`), applied at `:226`

`clean_movie_title` drops any word that appears in `RELEASE_TAGS`, and that list contains
words that are also ordinary English: `cut`, `web`, `multi`, `dual`, `internal`,
`limited`, `extended`, `proper`, `cam`, `sub`, `directors`, `uncut`, `ts`.

```
'Uncut Gems'        -> 'Gems'
'Internal Affairs'  -> 'Affairs'
'Cut Throat City'   -> 'Throat City'
'Extended Family'   -> 'Family'
'The Cut'           -> 'The'
'Web of Lies'       -> 'of Lies'
```

A single-word title survives by accident — `:242` falls back to the raw string when the
result is empty, so `Cam` and `Dual` are fine — and everything else is quietly truncated.
The damage is not only cosmetic: the cleaned title is what goes to TMDB, so these films
get the wrong artwork or none, and it is what `match_key` is built from, so duplicate
collapsing groups them wrongly.

A release tag is only a release tag in the places one appears: at the end of a name, or
next to other tags. In the middle of a sentence it is a word.

### R-06 — Low — a duplicated episode keeps the first stream, not the best

`aurora-core/src/series.rs:150`

`group_series` tracks the best *quality* seen across a show's episodes, but when the same
`SxxEyy` appears twice it keeps whichever URL came first and drops the other. A provider
listing an episode in both 1080p and SD, in that order or the other, decides the stream by
file order rather than by quality — and the show's advertised quality may then be one no
episode actually plays at.

### R-07 — Low — `1x02` matching catches `4x4`

`aurora-core/src/series.rs:33`

`\b(\d{1,3})x(\d{1,4})\b` is guarded well enough that `1920x1080` does not match, but
`4x4` does, so a film with that in its name is classified as season 4 episode 4 and filed
under Series. Rare, and visible when it happens.

### R-08 — Low — stripping a country prefix can collide two channels

`aurora-core/src/title.rs:118`

The prefix rule takes any 2–4 letters followed by a separator, which the comment already
owns ("a channel actually named `NBC ★ Sports` loses its `NBC`"). The consequence not
written down is that `match_key` is what duplicate collapsing compares, so `US ★ TNT` and
`CA ★ TNT` both key to `tnt` and one disappears when "hide duplicates" is on. That may be
wanted for two feeds of one brand and is not wanted for two different regional channels,
and today there is no way to tell them apart.

### R-09 — Info — five duplicated rows in the language table

`aurora-core/src/lang.rs:22` (`TOKENS`)

`quebec`, `exyu`, `taiwan`, `indonesian` and `indonesia` each appear twice. Every pair
maps to the same code, so the `HashMap` built at `:396` is unaffected and nothing
misbehaves today. Worth removing because the next duplicate may not agree with itself,
and a table collected into a map gives no warning when it does not — checked
mechanically, there are no conflicting rows at present.

### R-10 — Info — Japanese written mostly in kanji is reported as Chinese

`aurora-core/src/lang.rs:398` (`script_language`)

The comment says "Japanese writes Han alongside kana; a stray kana character is the
giveaway", but the code has no such rule: it takes whichever script has the most
characters, and kana loses to kanji in most real titles. `トカレフ2010 運命の撃鉄` has 4
kanji to 4 kana and is decided by iteration order of the map; a title like `君の名は` with
more kanji than kana is Chinese as far as the filter is concerned.

Measured on this subscription: of 42,279 names, **5** contain Han at all and **1** also
contains kana. So this is a latent mismatch between comment and code rather than
something a viewer here will see, and it is listed to be fixed with the comment, not
ahead of anything else.

### R-11 — Info — text before the first `|` is not read as a tag

`aurora-core/src/lang.rs` (`tagged_tokens`)

The scanner only collects a segment once it is `inside` a bracket or past a pipe, so in
`ARABIC | MBC 1` the word `ARABIC` is never offered to `lookup` — only `MBC 1` is. A
two-to-four letter code in that position is still caught by `split_country_prefix`, so
the gap is limited to longer words: `ARABIC`, `SPANISH`, `PORTUGUESE`.

Measured: **27** of 42,279 names contain a pipe on this subscription, and none of their
first segments is a language word. Real but currently worthless to fix; recorded so the
next playlist that uses the shape does not cost another investigation.

### R-12 — Medium — two error messages shown to the viewer contain a run of 18 spaces

`aurora-core/src/neterr.rs:137` and `:277`

```
"The server would not accept it as sent. Nothing about retrying will                  change that; the log has what it said."
"Aurora could not tell what from the error it got back; the log has the                  details."
```

Both are `cause` strings, which the error panel renders as a paragraph — so the viewer
sees the gap. They are the two branches added most recently (`BadRequest`, and the
`Unknown` fallback), and the whitespace is left over from hand-wrapping a line that was
then joined. Nothing is broken; it looks broken, which for an error message is the
same thing.

Found by sweeping every string literal in the repository for `\S {3,}\S`. Worth
recording how: **the first version of that sweep was itself wrong** — a regex for a
Rust string literal that silently matched nothing — and it reported the file clean while
the defect was on screen. The pattern was rechecked against the line found by eye before
the result was believed. A mechanical check that has not been shown to catch a known
instance is not evidence.

The same sweep found five more, all in assertion or panic messages rather than on
screen: `aurora-app/src/pip.rs:500`, `aurora-app/tests/contract.rs:361`, `:369`, `:425`,
`aurora-db/src/repo/enrichment.rs:1018`. Same cause, no viewer impact, fix with the
others. The hits in `aurora-ingest/examples/*.rs` and `tests-host/scenarios/*.py` are
deliberate column alignment and are correct as they stand.

### R-13 — Low — "Try another source" is offered for failures that have no other source

`aurora-core/src/neterr.rs` (the `actions` block)

`ErrorAction::TryAnotherSource` is pushed unconditionally, for every code. The block
immediately below it reasons carefully that a `BadRequest` must not offer
`ReportBroken` "because the source is not what is wrong" — which is equally an argument
against offering to try a different one.

This classifier is shared, and that is the point of it: the assistant's rejected Gemini
call comes through here too, as does a provider check in the setup wizard where no
stream exists. Both can present a button whose only honest meaning is about a channel
with several URLs.

### R-14 — Info — two chapter keywords are also ordinary words

`aurora-core/src/markers.rs:182` and `:184`

`INTRO_WORDS` contains `op` and `CREDITS_WORDS` contains `ed` — both correct for anime,
where "OP" and "ED" are how the opening and ending are labelled. They are also a
preposition and a name: a chapter titled `Ed Loses the Farm` reads as the credits, and
`position_is_sane` only catches it if the chapter is in the first three quarters of the
file. The module header is explicit that whole-word matching was chosen so that
"Introduction to the Case" is not an intro; these two slip through the same door.

### R-15 — Info — an "intro" chapter an hour into a film is accepted

`aurora-core/src/markers.rs` (`position_is_sane`)

`marker.start_secs <= (duration_secs / 3.0).max(600.0)` reads as "the first ten
minutes, or the first third, whichever is more generous" — which is what the comment
says, and is right for an episode. On a three-hour film the first third is an hour, so
a mis-titled chapter that far in is accepted as an intro. The generosity was for short
files and should not keep growing.

### R-16 — Info — `min_age` has unreachable match arms, and `+18` is not an adult marker

`aurora-core/src/parental.rs:26` (`min_age`), `:89` (`looks_adult`)

Every bare number and `n+` form is answered by the digit branch above, so the arms
listing `"0"`, `"6"`, `"12"`, `"15"`, `"16+"`, `"18"` and the rest are unreachable — and
with them the `if c == "15"` inside the `"15" | "MA15" | "FSK16" | "16" | "16+"` arm,
which is the one piece of that function anybody reading it has to stop and work out. It
is dead.

Separately, `looks_adult` matches `18+` but not `+18`, which is at least as common a
way for a playlist to name that category. On this subscription it is moot — of 145
categories across live, films and series, **none** matches any adult marker and none
looks like an unmatched one — so this is a one-word list addition for the next playlist,
not a fix for this one.

### R-17 — High — a "strip from name" rule kills the app on a Turkish title

`aurora-core/src/rules.rs:201` (`replace_ci`), reached from `aurora-ingest/src/sync.rs:237`

```rust
let lower_hay = haystack.to_lowercase();
while let Some(pos) = lower_hay[i..].find(&lower_needle) {
    let abs = i + pos;
    out.push_str(&haystack[i..abs]);   // <- index from the lowercased string
    i = abs + needle.len();            //    used to slice the original
}
```

`pos` is a byte offset into `lower_hay`; it is used to slice `haystack`. Those agree
only while lowercasing preserves UTF-8 length. Three code points do not:

```
U+0130 'İ' -> 'i̇'  2->3 bytes      (Turkish dotted capital I)
U+023A 'Ⱥ' -> 'ⱥ'  2->3
U+023E 'Ⱦ' -> 'ⱦ'  2->3
```

Each one before a match shifts every later index one byte to the right. Landing
mid-character panics; landing on a boundary silently eats a byte.

**Confirmed against this subscription.** Three of its 42,424 names and group titles
carry one: `EN ★ Bu İşin Şakası Yok - 2026` (twice) and `ETR ★ İyilik`. Running the
function verbatim on them:

```
strip "2026"  -> PANICKED  (start byte index 37 is out of bounds for string of length 36)
strip " - "   -> "EN ★ Bu İşin Şakası Yok 026"      <- ate the '2'
strip "etr"   -> " ★ İyilik"                        <- correct: the match precedes the İ
```

**Why this is High and not a curiosity.** `src-native/Cargo.toml:50` sets
`panic = "abort"`, so there is no unwinding and no error path — the process dies. The
call site is `sync.rs:237`, inside the rule pass of a library refresh, and a refresh
runs by itself every 24 hours. So the failure is: the viewer adds an ordinary
house-keeping rule — "strip ` - 2026` from names" is exactly what the preset list
encourages — and some time later the application disappears while they are not looking
at it, with nothing in the log after it.

It needs no regex and no unusual rule: `Match::Contains` with `Action::StripFromName`
and `case_sensitive: false`, which is the default `Rule::new` produces.

The same arithmetic appears nowhere else — checked across `aurora-core`, `aurora-db` and
`aurora-ingest`; `rules.rs:212` is the only place a lowercased index slices an original.

### R-18 — Medium — the "hide adult" preset cannot match `18+`, the form playlists use

`aurora-core/src/rules.rs:229`

```rust
r"(?i)\b(adult|xxx|porn|18\+)\b"
```

The trailing `\b` after `18\+` can never match: `+` is not a word character, so a
boundary needs a word character next to it, and in `18+` or `18+ Channels` the next
character is a space or the end of the string. Measured:

```
"18+"           -> false
"18+ Channels"  -> false
"XXX"           -> true
"Adult Movies"  -> true
"ADULT 18+"     -> true    (matched "adult", not "18+")
"FOR ADULTS"    -> false   ("adults" is not "adult")
```

So one of the four alternatives is dead and a second misses the plural. This also puts
the preset out of step with `parental::looks_adult`, which does substring matching and
*does* catch `FOR ADULTS ONLY` and `18+ Channels` — two functions for the same question
disagreeing, which is how "adult content is hidden by default" becomes untrue for a
playlist that writes it either way. See [R-16].

### R-19 — Low — a non-regex rename replaces the whole name, not the matched part

`aurora-core/src/rules.rs:39` and `:148`

`Action::Rename` is documented as "Replace the matched portion". It does that for
`Match::Regex`; for `Contains`, `StartsWith` and `EndsWith` the arm is
`_ => replacement.clone()`, which replaces the entire name. A viewer writing "rename
`UHD` to `4K`" on a contains-match gets a channel called `4K`. Either behaviour is
defensible — the documented one is not the implemented one.

### R-20 — Low — rule ids collide, and the id is what the UI keys on

`aurora-core/src/rules.rs:62`

`id: format!("{field:?}-{pattern}").to_lowercase()` ignores the action and the match
kind, so "hide anything whose group contains SPORT" and "favourite anything whose group
contains SPORT" are both `group-sport`. `Outcome::fired` records ids, and a preview that
lists which rules fired cannot then tell them apart.

### R-21 — Medium — 426 channels are told they have a guide that holds nothing for them

`aurora-core/src/epg_match.rs:64` (`EpgIndex::build`)

**This finding was first written wrongly and is corrected here, because how it was wrong
matters.** The original version said the index was polluted by the 2,235 `<channel>`
elements this guide declares with `id=""`. It is not: `xmltv.rs:94` already drops a
channel with an empty id before the sink ever sees it. The measurement behind the first
version replayed `EpgIndex::build` over the raw document rather than over the parser's
output, so it measured a state the pipeline never reaches. The parser was read after the
matcher, which is how the claim survived long enough to be written down.

The real defect is smaller, has a different cause, and produces the same symptom. Of the
4,827 distinct channel ids that *do* reach the index, **1,882 are never referenced by
any `<programme>` element in the 178 MB document** (the production parser's own count).
They are declared and then never scheduled. Nothing filters them, so they match like any other channel:

```
<channel> elements                                        9,033
  dropped by xmltv.rs:94 for an empty id                  2,235
  reaching EpgIndex                                       6,798
  distinct ids among them                                 4,827
  of those ids, carrying no programmes at all             1,882

547,183 <programme> elements across 2,954 distinct channel ids

resolving 6,699 live channels:
  1,466  exact tvg-id  -> a channel with programmes
    359  exact tvg-id  -> a channel with NO programmes      <- counted as matched
     80  channel name  -> a channel with programmes
     67  channel name  -> a channel with NO programmes      <- counted as matched
  4,726  unmatched, on to the fuzzy pass
      1  no usable key
```

426 channels are therefore reported as having EPG data by a guide that holds not one
programme for them. `Coverage::matched` counts them, the Settings "fix these" list
leaves them out, and the guide draws an empty row instead of saying there is nothing
there. The viewer's report is "the guide is blank for this channel" and the app's own
diagnostics say it is fine.

Note this one cannot be fixed in `build` alone: whether a channel has programmes is not
known until the whole document has been read, so either the index is pruned after the
parse or coverage is computed from what the programme table actually contains.

### R-22 — High — the fuzzy pass matches channels that differ only by their number

`aurora-core/src/epg_match.rs:149`

```rust
// Fuzzy pass: only for keys of a similar length, and only for a distance of 1.
// Anything looser produces confident nonsense on channel names like "Sky 1" / "Sky 2".
if key.len() >= 5 {
```

The comment names the exact failure and the guard does not prevent it. `key.len() >= 5`
excludes only *short* names: `sky1` is four characters and is excluded, while
`spectrumxcast3` is fourteen and is not. Between two long channel names a
single-character edit is, in practice, the channel number.

Replayed over the real guide and playlist, with the parser's filter applied and the real
`title::match_key` linked in:

```
4,726 channels reach the fuzzy pass
4,597 are long enough to try
  655 get a fuzzy match
  639 of those 655 (97%) differ from their match by a single DIGIT
  624 of those 655 had SEVERAL candidates tied at the same distance
  461 land on a channel id that carries no programmes (see R-21)
```

```
'US ★ ESPN+ 156: '      key 'espn156' -> matched 'espn15'  (10 candidates tied)
'US ★ ESPN+ 157: '      key 'espn157' -> matched 'espn15'  (10 candidates tied)
'US ★ SPECTRUM XCAST 3' key 'spectrumxcast3' -> 'spectrumxcast'
'US ★ beIN SPORTS XTRA' key 'beinsportsxtra'  -> 'beinsportsxtra3' (9 tied)
```

Two independent defects, and the second is worse than the first:

1. A distance-1 match whose differing character is a digit is not a match.
2. **624 of the 655 matches were ties**, and `resolve` keeps the first candidate it
   happens to walk past — `best.as_ref().map(|(bd, _)| d < *bd)` only replaces on a
   strictly smaller distance, and every candidate here is at distance 1. So the answer
   depends on the order the provider wrote its `<channel>` elements in. `ESPN+ 156` is
   given one of ten equally-close channels with no reason to prefer it.

Every one of those channels is then shown another channel's schedule at confidence 60.
That is worse than a blank row: a blank row is understood, and a plausible wrong
programme title is believed. Ten channels showing the same thing at the same time is
also how a viewer concludes the whole guide is broken.

Together with R-21: of 6,699 live channels the resolver reports 2,627 matched, and 1,081
of those — **41%** — are either a channel with no programmes or a different channel
altogether.

### R-23 — Info — the fuzzy pass is 4.5 million string comparisons

`aurora-core/src/epg_match.rs:151`

The pass walks the whole `keys` list for every channel that reaches it: 4,597 channels
against 6,798 keys, which the length prefilter cuts to **4,475,192** actual distance
computations, inside the import. `edit_distance_at_most` bails early and the prefilter
works, so this is not a defect — but bucketing the keys by length makes the same pass a
fraction of the work, and the fix for R-22 should not be layered onto a scan of this
shape. Recorded so the two are considered together.

### R-24 — Info — the guide declares 4,205 duplicate channels, and the two indexes
disagree about which wins

`aurora-core/src/epg_match.rs:70` and `:82`

9,033 `<channel>` elements carry 4,828 distinct ids. `by_id` uses `insert`, so the last
declaration wins; `by_key` uses `entry().or_insert_with`, so the first does. For a
duplicated id the value is the same string either way, so nothing is wrong today — it is
two different tie-breaks six lines apart, and only one of them is deliberate.

Also 3 `<programme>` elements reference a channel id that no `<channel>` element
declares; those are silently dropped, which is right. Noted only because it is worth
knowing the number is 3 and not 30,000.

### R-25 — High — TMDB title matching is an exact-equality test, and three mechanisms built on top of it are dead

`aurora-core/src/tmdb.rs:83`

```rust
fn tokens(s: &str) -> Vec<String> {
    title::match_key(s)
        .split_whitespace()      // <- match_key has already removed every space
        .map(|t| t.to_string())
        .collect()
}
```

`match_key` concatenates words without a separator — `"The Matrix Reloaded"` comes back
as `"thematrixreloaded"`. So `tokens` always returns **one** element, and
`title_similarity`, which is written as a token-set F1, collapses to `ta == tb`:

```
  1.000  "The Matrix" vs "The Matrix"
  0.000  "The Matrix" vs "The Matrix Reloaded"
  0.000  "Blade Runner" vs "Blade Runner 2049"
  0.000  "The Lord of the Rings" vs "Lord of the Rings"
  1.000  "Spider-Man: No Way Home" vs "Spider-Man No Way Home"
  1.000  "Mission Impossible Fallout" vs "Mission: Impossible - Fallout"
```

Punctuation and spacing differences still match, which is why it works at all. Any
difference of a *word* — a leading article, a dropped subtitle, "Part One" — scores
zero instead of the ~0.8 the F1 was designed to give it, and the title gets no metadata.

Enumerating every score the function can return over a realistic spread of titles,
years and popularity values, the reachable set is **eight values**:

```
0.0000  0.4000  0.4072  0.4200  0.9500  0.9572  0.9700  1.0000
```

Three things built on a continuous score therefore do not work:

1. **`MIN_CONFIDENCE = 0.72`** is not a confidence threshold. Nothing can land near it;
   it sits in the gap between 0.42 and 0.95, so all it does is reject the
   year-mismatch case. The tuning that number represents is not being applied.

2. **`AMBIGUITY_MARGIN = 0.06` rejects every genuine duplicate-title case.** Two
   candidates whose normalised titles both equal the query both score exactly 1.0, a
   difference of 0, so `pick_best` returns `None`:
   ```
   two candidates both titled "Alone" (2020) -> None
   ```
   The comment says this is for when "'Alone' could be any of six films" and a year
   would make it rare. With a year or without, if TMDB returns two results with the same
   title the answer is always no match.

3. **Popularity cannot break a tie**, which is the only job the comment gives it.
   `score` is clamped to 1.0 before the tiebreak is added and clamped again after, so at
   exactly the moment two candidates need separating, both are 1.0:
   ```
   score(popularity 0)   = 1.0000
   score(popularity 950) = 1.0000
   ```

The module's stated principle — a wrong match is worse than no match — is still upheld,
because the failure is always towards declining. What is lost is recall, and the module
header's whole argument about *The Matrix* and *The Matrix Reloaded* describes a
calculation that has never run.

One line is wrong. Fixing it turns on three mechanisms that have never executed, so the
thresholds above need checking against real candidate lists afterwards rather than
trusted — they were chosen for a function that was not the one running.

**How much recall this actually costs, measured after [R-59] was corrected.** The stored
titles are cleaned at ingest, so most of them match TMDB exactly and the binary
similarity is enough:

```
"Alien Romulus"  vs "Alien: Romulus"   -> 1.000   (punctuation folds)
"Inception"      vs "Inception"        -> 1.000
"The Lord of the Rings" vs "Lord of the Rings" -> 0.000   (a whole word differs)
```

So R-25 does not break enrichment wholesale — it costs the cases where the provider's
cleaned title and TMDB's differ by a *word*: a dropped article, a subtitle the panel
omits, "Part One" against "Part 1". That is a real and unmeasured fraction of the
library rather than all of it, and the three dead mechanisms above are the stronger
reason to fix it.

### R-26 — Low — `Window::clamp` panics on a NaN or an inverted window

`aurora-core/src/timeshift.rs:166`

```rust
pub fn clamp(&self, target_secs: f64) -> f64 {
    target_secs.clamp(self.start_secs, self.live_secs)
}
```

`f64::clamp` panics if `min > max` or if either bound is NaN. Built through
`Budget::window` the invariant holds, but `Window` has public fields, derives
`Deserialize`, and crosses the IPC boundary — and its three numbers originate in mpv
properties. With `panic = "abort"` there is no recovery from either case.

**Checked against `aurora-player`, and not reachable there.** `MpvBackend::refresh_timeshift`
establishes the invariant three times over: `live` is `…unwrap_or(position).max(position)`
so it is never below the playhead, `Budget::window` ends with `.min(reading.position_secs)`,
and the `cache_start` correction is `.max(start).min(window.position_secs)`. So
`start_secs <= position_secs <= live_secs` always holds on that path. NaN is neutralised
by accident rather than by design — Rust's `f64::max`/`min` return the non-NaN operand —
but it is neutralised. No command accepts a `Window` from the UI; `Playback::seek` reads
it from `player.state()`.

So this stays a latent hazard of the public fields and the `Deserialize` derive, not a
live defect. Worth replacing `clamp` with an explicit `min`/`max` pair anyway, since the
panic would be an abort and the invariant is maintained by three separate call sites that
each have to keep it.

### R-27 — Info — `bytes_as_secs` can overflow a budget that did not come from `clamped`

`aurora-core/src/timeshift.rs:78`

`(self.bytes * 8)` is fine for anything `Budget::clamped` produces, but `bytes` is a
public `u64` on a `Deserialize` struct, so a settings row written by a different build
can exceed `u64::MAX / 8`. Release builds have overflow checks off, so it wraps quietly
and the rewind window comes out wrong rather than loudly. `saturating_mul` costs
nothing.

### R-28 — Info — `xmltv_ns` episode numbers can overflow a `u16`

`aurora-core/src/xmltv.rs:419` (`apply_episode_num`)

```rust
if let Ok(n) = s.split('/').next().unwrap_or("").trim().parse::<u16>() {
    p.season = Some(n + 1);
}
```

`xmltv_ns` is zero-based, so the `+ 1` is right, but `n` is already a `u16` — a guide
carrying `65535.0.0` wraps to season 0 in a release build (overflow checks off) and
panics in a debug one. `checked_add` and skip, or parse into `u32` and bound it.

Not reachable on this subscription: the guide contains **no `<episode-num>` element at
all**, so `apply_episode_num` never runs here and every EPG programme has
`season: None, episode: None`.

### R-29 — Info — `XmltvStats::skipped` counts two unrelated things

`aurora-core/src/xmltv.rs:98` and `:109`

A `<channel>` dropped for an empty id and a `<programme>` dropped for having no title
both increment the same counter, which is documented as "programmes discarded…". On
this guide that counter starts at **2,235** before a single programme is considered, so
the number shown after an import is not the number it claims to be. Same family as
[R-04].

### R-30 — Info — only the first `<title>` is kept, and `lang` is not read

`aurora-core/src/xmltv.rs:443`

`if p.title.is_empty() { p.title = value; }` takes whichever `<title>` the provider wrote
first. XMLTV allows one per language, so on a multilingual guide the viewer gets the
provider's document order rather than their own language. Same for `<desc>`.

Moot here — measured across all 547,166 programmes, every one has exactly **one**
`<title>` and it carries **no attributes**. Recorded because the shape is allowed by the
format and the code has no answer for it.

### R-31 — Context — what this subscription's guide actually contains

Not a defect. Recorded because several features are written against data that is not
there, and the next person to wonder why should not have to download 178 MB to find out.

```
547,166 <programme>   each with exactly one <title> and one <desc>
      0 <category>    0 <credits>  0 <rating>  0 <star-rating>  0 <episode-num>
      0 per-programme <icon>
  9,033 <channel>     8,858 <icon>, 9,033 <display-name>
    all start times carry the offset +0200, and every programme has a stop
```

So `ProgBuilder`'s `categories`, `credits`, `season`, `episode`, `icon`, `rating`,
`star_rating`, `is_new`, `is_live` and `is_premiere` are always empty or false on this
line. Anything in the UI keyed on them — a "New" badge, a star rating, a cast list on a
guide entry — shows nothing here, and that is the provider's doing rather than a bug.

Two further numbers that bear on [R-21] and [R-22]:

- **235 `<channel>` elements share the id `#.No.Channel.#`**, a sentinel the provider
  uses for channels it has no data for. It is not empty, so `xmltv.rs:94` passes it, and
  all 235 collapse onto one entry whose id no programme references. No live stream
  carries that id as its `tvg-id`, so it is reached only by name — a subset of R-21's 67.
- **4,874 of 6,699 live streams carry no `epg_channel_id` at all** (73%). That is why
  name matching and then the fuzzy pass decide most of this playlist, and it is why
  R-22's 639 bad fuzzy matches matter as much as they do.

### R-32 — High — the commonest failure on this line is diagnosed as "their server had an error"

`aurora-core/src/neterr.rs` (the 5xx branch), reached from `aurora-ingest/src/http.rs:340`,
`aurora-ingest/src/recorder.rs:161`, and the player

This panel allows **one** connection, and it reports the limit as **HTTP 555 with an
empty body**. Established directly against it:

```
holding one stream open, then requesting a second on the same line:
  second request -> HTTP 555   content-type text/html   size 0
  third request (2s after the first ended) -> HTTP 555
```

555 is not a standard status, so no classifier branch names it and there is no body to
read. It falls into `matches!(status, Some(500..=599))`, and the real classifier,
given the strings the three call sites actually produce, answers:

```
"HTTP 555"                           -> ServerError
    "Your provider's server had an error"
    "This is on their end. It usually resolves by itself."
    retryable: true   actions: [Retry, TryAnotherSource]
```

Aurora already has the right words — `ErrorCode::ConnectionLimit` says "You're already
watching on another device" and offers `OpenSettings` — but that branch is reachable only
by finding the literal text `connection limit` or `max connections` in the error, and
this panel sends no text at all.

So on a one-connection line, opening multi-view, starting a recording while watching, or
zapping faster than the panel frees the slot all produce a message that blames the
provider's server, says it will fix itself, and offers a Retry that cannot work while the
other stream is open. The app knows the line's limit — it stores `max_connections` and
`aurora-core/src/mosaic.rs:16` is explicit that this subscription's limit of 1 "makes
every layout here impossible" — so the information needed for the right answer is
already in the database when the wrong answer is produced.

Note also that the fourth form, the one `reqwest`'s own `Display` produces, is not even
classified as a server error:

```
"HTTP status server error (555 unknown status) for url"  -> Unknown
    "Something went wrong with that request"
```

which is the branch carrying the mangled whitespace from [R-12].

### R-33 — Medium — credentials go into stream URLs unescaped

`aurora-core/src/xtream.rs:217` (`stream_url`) and `:239` (`timeshift_url`)

```rust
format!("{base}/live/{username}/{password}/{stream_id}.ts")
format!("{base}/streaming/timeshift.php?username={username}&password={password}&…")
```

Neither value is percent-encoded. A password containing `/` adds a path segment; `#`
truncates everything after it into a fragment; `?` starts a query; and in the query form
`&` or `=` rewrites the parameters. Panels do issue passwords with punctuation in them.

The failure is silent and looks like bad credentials: the provider returns 401 or 404 for
a URL that does not address what was meant, and nothing in the app can tell that apart
from a wrong password — which is the one diagnosis a viewer will act on by retyping a
password that was already correct.

This line's credentials are alphanumeric, so it does not bite here. It is one call to a
percent-encoder in each place.

### R-34 — Low — 80 real programmes are discarded for a zero-length slot while a missing
stop is tolerated

`aurora-core/src/xmltv.rs:389` (`finish`)

```rust
let stop = p.stop.unwrap_or(start + 1800);   // missing stop: assume 30 minutes
if stop <= start { return None; }            // stop == start: discard
```

A provider that omits `stop` gets the benefit of the doubt; one that writes the same
value twice does not. Running the production parser over the real guide, the 254 skipped
programmes break down exactly as:

```
174  empty <title>        -- rightly dropped, there is nothing to show
 80  stop == start        -- dropped, though each has a real title
```

```
channel='Historia.es' start='20261008101000 +0200' stop='20261008101000 +0200'
  title='Augusto Pinochet: el golpe, la tortura y Occidente'
```

Those are real programmes on a real channel, lost to a provider typo that the line above
already knows how to forgive.

### R-35 — Info — the parser handles the real guide, including its BOM

Not a defect; recorded because it was in doubt and is now established. The panel's
`xmltv.php` begins with a UTF-8 BOM (`EF BB BF`), which `parse_json`'s `trim_start`
would not remove — but `player_api.php` sends no BOM, so that path is unaffected, and
`quick_xml` tolerates the one in the guide.

Running the production `xmltv::parse` over the real 178 MB document:

```
parsed in 1.04s
  channels to the sink      6,798   (0 with an empty id)
  programmes to the sink  546,912
  skipped                   2,489   = 2,235 channels + 254 programmes (see R-29, R-34)
  distinct ids declared     4,827   of which 1,882 are never scheduled
  ids scheduled but never declared      3
  titles containing '&'            11,178
```

11,178 titles contain an `&`, so the entity handling the module header describes —
`Tom &amp; Jerry` arriving as three events in quick-xml 0.42 — is carrying real weight
rather than guarding a hypothetical.

Also established: a stream URL answers **302** to a tokenised URL on a different host
(`Location: http://176.65.143.192:80/live/…?token=…`), and both HTTP clients set
`redirect::Policy::limited(5)`, so that is handled. Six different User-Agents — including
none at all, `libmpv`, and `AuroraTV/1.0.15` — all get the same 302, so this panel does
not gate streams on User-Agent. That had looked like the explanation for the first 555s
observed and it was wrong; the connection limit is the explanation.

### R-36 — Medium — the recorder classifies network errors with the string the ingest
client exists to avoid

`aurora-ingest/src/recorder.rs:159`

```rust
.map_err(|e| NetFailure::classify(&e.to_string()))
```

`aurora-ingest/src/http.rs:383` says, as a comment above `classify_reqwest`:

> `reqwest`'s Display strings do not mention the words the shared classifier looks for,
> so map its typed predicates first and fall back to the cause chain.

The recorder does exactly what that comment warns against. `e.to_string()` is the
top-level `Display` only — it does not walk the cause chain, and it does not consult
`is_timeout()`, `is_connect()` or `is_redirect()`. So a recording that fails to start
because the host did not resolve, or because the connection was refused, or because it
timed out, reports whatever generic text reqwest's outermost error happens to carry
rather than the DNS, Refused or Timeout diagnosis the same failure gets during an import.

`classify_reqwest` is private to `http.rs`; making it shared is the fix, and the recorder
is the only other caller that needs it.

### R-37 — High — one abandoned film erases a genre you have watched a hundred times

`aurora-core/src/recommend.rs:551` (`profile`), `:122` of `score_one`, and
`normalise_shares`

`profile` builds two maps and normalises each **independently** to sum to 1:
`taste.genres` (share of attention) and `taste.aversions` (share of abandonment).
`score_one` then subtracts one from the other:

```rust
let genre_affinity = (affinity(&taste.genres, genres)
    - AVERSION_WEIGHT * affinity(&taste.aversions, genres)).max(0.0);
```

Because each map is a share of its own total, the aversion side carries no information
about *how much* was abandoned. Abandon one film and that genre's aversion share is
1.0 — the same value it would have if you had abandoned two hundred.

Measured through the production code, with `DISCOVERY_CEILING` at 0.20:

```
ten action films finished, nothing abandoned      an action candidate scores 0.4500
the same ten, plus ONE abandoned a month ago                                0.1800
the same ten, plus THREE abandoned                                          0.1800
ONE HUNDRED finished, plus one abandoned                                    0.1800
```

A single film opened and left, a fortnight untouched, takes the genre from 0.45 to 0.18
— a 60% cut — and drops it **below the match threshold**, so it is demoted from "their
own taste" to discovery. Three abandonments do exactly the same as one. A hundred
finished films do not defend against one abandonment.

The module header states the opposite as a design goal:

> Enough to pull a genre down and reorder a rail, not enough for three bad nights to
> erase a year of watching.

One bad night erases a year of watching, and the second and third are free. The constant
`AVERSION_WEIGHT = 0.6` was chosen as a fraction of a comparable quantity, and after
normalisation it is not comparable to anything — the aversion term needs to carry the
weight behind it, not a share of itself.

This is the most likely cause of a rail that "stopped showing what I watch": the viewer
need only have opened one film in each of their genres and not gone back.

### R-38 — Medium — a film on a shelf you watch is filed as a "discovery" while still
labelled "Because you watched …"

`aurora-core/src/recommend.rs:229` (`rank`)

```rust
// Anything that matched their taste outranks anything that did not, whatever the
// arithmetic says, so the split is by kind rather than by score.
let (matches, discoveries): (Vec<_>, Vec<_>) = scored
    .into_iter()
    .partition(|(score, _, _)| *score > DISCOVERY_CEILING);
```

The comment says the split is by kind. The code splits by score, and the two disagree
exactly where it matters.

This panel sends **no genre field for films at all** — confirmed across all 27,494 of
them — so a film's only description is the provider's category, and `CATEGORY_WEIGHT` is
0.20, which is `DISCOVERY_CEILING` to the digit. A film on a shelf the viewer watches,
with nothing else going for it, scores exactly 0.2000, and `> DISCOVERY_CEILING` is
false:

```
category match, no rating, no year, not new      score 0.2000  discovery  "Because you watched B"
category match, rating 0.0 (as the panel sends)  score 0.2000  discovery  "Because you watched B"
category match, rating 7.0                       score 0.4620  MATCH      "Because you watched B"
no match at all, rating 9.5, brand new           score 0.1433  discovery  "Highly rated"
```

So it is placed by the discovery pass, after everything that did match, competing with
titles that have nothing to do with the viewer — while the poster underneath it still
reads *"Because you watched B"*, because `explain` is reached through the matched path.
The label and the placement contradict each other.

**3,812 of 27,494 films on this subscription have no rating**, so for one film in seven a
category match is the only signal there is, and every one of them lands on the boundary.

### R-39 — Info — what the panel actually sends for films and series

Not a defect. Recorded because R-25, R-37 and R-38 all turn on it.

```
films   27,494   rating > 0: 23,682      no rating: 3,812
                 genre field: DOES NOT EXIST on the row at all
                 a year in the name: 27,356
series   8,086   rating > 0: 6,877
                 genre present: 7,839     56 distinct genre strings
                 top: drama 3,676, comedy 2,009, crime 1,405, documentary 1,145
```

So series carry genres and films do not. Every genre-based mechanism in `recommend.rs`
is working from series alone, or from TMDB — which, per [R-25], matches only on an exact
normalised title. For films, the recommender is running on provider categories and
ratings, which is what `CATEGORY_WEIGHT` was added for and is the right call; the
consequence is that `GENRE_WEIGHT`, at 0.45, is the largest weight in the scoring and is
zero for 27,494 of the 35,580 recommendable titles.

### R-40 — Low — an out-of-range `around_local_minute` makes a series rule match everything

`aurora-core/src/dvr.rs:248` (`SeriesRule::matches`)

```rust
let diff = (minute_of_day - target as i64).abs().min(
    1440 - (minute_of_day - target as i64).abs(),
);
if diff * 60 > self.time_slack_secs { return false; }
```

The wrap-around arithmetic is right for a target inside a day. `around_local_minute` is
a public `u16` on a `Deserialize` struct, though, and nothing bounds it to `0..1440`. At
`target = 2000` the second term is negative, `min` picks it, and `diff * 60 > slack`
compares a negative number — so the time restriction passes for every programme and the
rule records all day.

A rule written by the UI will be in range. A rule row written by an older build, or
hand-edited, will not be, and the failure is a disk filling up rather than an error.

### R-41 — Low — `expand_rules` recomputes the programme's match key once per rule

`aurora-core/src/dvr.rs:263` / `:119`

`matches` calls `title::match_key(&programme.title)` itself, and `expand_rules` calls
`matches` for every (programme, rule) pair, so the same title is normalised once per
rule. Measured against all **546,992** real programme titles from this guide:

```
match_key over each title once   497ms   (909ns per call)
 1 unrestricted series rule     ~0.50s of match_key alone
 5 unrestricted series rules    ~2.49s
10 unrestricted series rules    ~4.98s
```

A rule with `channel_id: Some(..)` or `new_only: true` exits before the key is computed,
so this only bites on unrestricted rules — which is what `SeriesRule::new` produces by
default. Hoisting the key to once per programme makes it 0.5s whatever the rule count.
Not urgent at 909ns a call; recorded with the real number so it is not guessed at later.

### R-42 — Info — `largest_fitting` reports a certainty it does not have

`aurora-core/src/mosaic.rs:99`

With `limit: None`, `Budget::check` returns `Unknown`, `is_refused()` is false, and so
every layout "fits" — the function returns `Grid3x3`. Its documentation says "the largest
layout that fits", and for an M3U provider, which the same file calls "the ordinary case
rather than the exotic one", it has no idea whether nine streams fit.

`Budget::Unknown` itself is handled honestly and the UI says so in words. This one
accessor quietly converts "unknown" into "yes, nine".

The rest of `mosaic.rs` checks out: each layout's `rects()` returns exactly `tiles()`
rectangles, the proportional `edge()` makes neighbours share a boundary exactly so no
seam can appear, `cell()` clamps a reversed pair rather than wrapping a `u32`, and
`OnePlusFive` tiles its 3×3 grid without overlap or gap.

### R-43 — Info — `margin * 2` in the PiP geometry can wrap before the guard runs

`aurora-core/src/pip.rs:106`

```rust
let Some(room_w) = width.checked_sub(margin * 2) else { return ZERO };
```

`checked_sub` guards the subtraction; `margin * 2` is unguarded, and `margin` is a public
`u32`. Above `u32::MAX / 2` it wraps to a small number in a release build and the guard
then passes on a window that has no room at all. `Geometry` does not derive
`Deserialize`, so this depends on where `margin` comes from in `aurora-app` — to be
confirmed when the review reaches it. `saturating_mul` removes the question.

Otherwise this module holds up under the arithmetic that matters. The corner offsets
cannot underflow: `tile_w <= room_w = width - 2*margin`, so `width - margin - tile_w >=
margin`, and the same for the vertical. A `width_fraction` of NaN does not panic —
`f32::clamp` only panics on NaN *bounds*, and the subsequent `as u64` saturates to 0,
which `MIN_WIDTH` then lifts. The comment at `:118` explaining why `max`-then-`min` is
used instead of `clamp` is correct and is the kind of thing worth not undoing.

---

## aurora-core: complete

All 26 files read line by line. Findings R-01 to R-43, of which:

| Severity | Count | Numbers |
|---|---|---|
| High | 5 | R-01, R-17, R-22, R-25, R-37 |
| Medium | 7 | R-05, R-12, R-18, R-21, R-32, R-33, R-36, R-38 |
| Low | 11 | R-02, R-03, R-04, R-06, R-07, R-08, R-13, R-19, R-20, R-26, R-34, R-40, R-41 |
| Info / context | rest | R-09…R-11, R-14…R-16, R-23, R-24, R-27…R-31, R-35, R-39, R-42, R-43 |

Three of the five High findings were invisible to every existing check: the test suite
passes, clippy is clean at `-D warnings`, and none of them is a type error. Each was
found by running the production code over the real subscription and comparing the result
with what the code's own comments claim it does. In four cases — R-22, R-25, R-37 and
R-38 — a comment states the correct behaviour and the code beside it does something else,
which suggests the comments were written from the design and not from the output.

## aurora-db

### R-44 — Low — the `skip_markers` CHECK has no room for the `convention` source

`aurora-db/src/schema.rs:263` and `aurora-core/src/markers.rs:82`

```sql
source TEXT NOT NULL CHECK (source IN ('chapters','user','learned'))
```

`MarkerSource` has four variants. `Convention` — tier 4, the credits-from-the-clock
marker — is not in the CHECK, and `markers::record` inserts `marker.source.as_str()`
without restriction, so recording one would fail at runtime with a constraint violation.

Nothing records one today: `library_record_skip` hardcodes `MarkerSource::User` and
`set_chapter_markers` writes the literal `'chapters'`. But `MarkerSource::parse` accepts
`"convention"` specifically so the value can round-trip through this column, and
`record` is a public function whose signature says any marker may be stored. The
constraint and the enum disagree, and the compiler cannot see it.

### R-45 — Medium — an unrecognised marker source becomes one the app may skip without
asking

`aurora-db/src/repo/markers.rs:51`

```rust
source: MarkerSource::parse(&source).unwrap_or(MarkerSource::Learned),
```

`MarkerSource::may_auto_skip()` is `!matches!(self, Convention)` — `Learned` returns
**true**. So a `source` value this build does not recognise is read back as a marker that
the per-show "always skip" preference is allowed to fire automatically.

`aurora-core/src/markers.rs:122` is explicit about why that is the wrong direction:

> A conventional credits marker is a button, and pressing it is the viewer agreeing with
> the guess — auto-skipping one would mean a clock silently cutting the last minute of an
> episode whose runtime was wrong, which is how somebody loses the final scene of a
> finale.

The safe fallback for an unknown value is the least trusted source, not the second most
trusted. Reachable as soon as a newer build writes a source this one does not know —
including the `convention` of [R-44], which is the exact value whose auto-skip is being
guarded against.

**And the UI's guard is the same string, so it inherits the fault.**
`src-ui/src/hooks/useEpisodeAids.ts:116` decides automatic skipping with
`activeMarker.source !== 'convention'`. A row whose source this build cannot parse is
serialised to the UI as `'learned'`, which passes that test — so the marker is skipped
without being asked, which is precisely what both guards exist to prevent. There is no
second line of defence: the host's fallback and the UI's check are the same decision made
twice from the same wrong value.

### R-46 — Medium — the guard that stops Aurora moving your library aside is half dead
and untested

`aurora-db/src/lib.rs:140`

```rust
if failure.contains("newer version of Aurora") || failure.contains("schema") {
```

This decides whether a database that would not open is *refused* or *renamed and
replaced*, and it decides it by substring-matching an error message.

- **The first condition matches nothing.** `"newer version of Aurora"` occurs exactly
  once in the whole repository: in the line above. No error this codebase can produce
  contains that phrase.
- **The second is doing all the work by coincidence of wording.** `DbError::SchemaTooNew`
  reads *"database is newer than this build (schema v9999, supported v11) — update
  Aurora TV or restore a backup"*, and it is the word `schema` in the parenthesis that
  saves the library. Reword that message — drop the version parenthetical, say "format"
  instead — and the next launch against a newer database renames the user's library and
  starts an empty one.
- **No test covers this branch.** `migrate.rs` has
  `refuses_a_database_from_a_newer_build`, but it exercises `run()`. `lib.rs`'s own tests
  cover intact, corrupt, truncated, sidecars and repeated failures — not this.

Tested against the real function, the behaviour today is right:

```
schema from a newer build      -> Err: database is newer than this build (schema v9999…)
schema page overwritten        -> REPLACED (library moved aside)
truncated to half its length   -> REPLACED
not a database at all          -> REPLACED
a data page overwritten        -> INTACT, row still readable
```

So this is a latent fragility rather than a current defect — but what it guards is the
destruction of favourites, watch progress and recordings metadata, which the function's
own doc comment says is "not a trade this should make on its own". Matching on
`DbError::SchemaTooNew` as a variant costs nothing and cannot drift.

The same line has a second consequence worth stating: a **migration bug** — a future
migration with a SQL error in it — produces `DbError::Migration`, whose text contains
"schema" only if the underlying SQL error happens to. If it does not, a broken release
moves every user's library aside on first launch and reports it as corruption.

### R-47 — Low — changing a recording rule's padding re-schedules everything it already
scheduled

`aurora-db/src/schema.rs:392`

```sql
CREATE UNIQUE INDEX idx_recordings_unique ON recordings(channel_id, start, title);
```

The comment above it says this is what makes rule expansion idempotent across EPG
refreshes, and it is — as long as `start` does not move. `start` is the **padded** start,
and `pre_padding_secs` is a per-rule setting. Raise a rule's pre-padding from 60 to 120
seconds and every airing it has already scheduled gets a new `start`, so the next refresh
inserts a second row for each one rather than updating it: two recordings of the same
programme, two connections wanted, and on a one-connection line (see R-32) one of them
failing.

The row already carries `air_start`, the unpadded airtime, which is the stable identity
of "this airing on this channel" and does not move when padding changes.

### R-48 — Info — ruled out: the planner's statistics are not stale

Worth recording as checked, because `aurora_db::analyze`'s own doc comment describes an
85-second browse query and the call site in `open` is guarded by
`if !has_statistics(&conn)` — which is true only on a database that has never been
analysed, i.e. a fresh and empty one. That would leave every library permanently
described by the statistics of an empty table, reproducing exactly the symptom the
function exists to prevent.

It does not, because `aurora-ingest/src/sync.rs:547` calls `analyze` at the end of every
import apply, after all rows are written and after `reclassify`. The statistics are
re-measured at the only moment the row counts change by six figures, which is right.

### R-49 — High — every refresh adds another complete copy of the guide, for ever

`aurora-ingest/src/sync.rs:468`, `aurora-db/src/repo/epg.rs:34`,
`aurora-db/src/schema.rs:88`

The EPG write path is:

```rust
for source in fetched.epg {
    epg_repo::upsert_channels(db, &source.channels)?;
    for batch in &source.programmes {
        epg_repo::insert_batch(db, batch)?;      // plain INSERT
    }
}
```

`insert_batch` is an unqualified `INSERT` — no `ON CONFLICT`, no preceding delete — and
`epg_programmes` has **no unique constraint** (its two indexes are both non-unique). So
nothing stops the same airing being stored again.

Nothing ever removes one either. `epg::prune_before` and `epg::clear_channels` both
exist, and `prune_before` carries the comment *"README §4.4: prune old programmes on a
schedule so the DB does not grow unbounded"* — **neither has a caller outside its own
test.** Grepped across the whole workspace: every reference is in `repo/epg.rs` or its
`mod tests`.

Running the real write path three times, exactly as `sync.rs` runs it:

```
after refresh 1: epg_programmes rows 3, grid_slice returns 3 rows for 3 distinct airings
after refresh 2: epg_programmes rows 6, grid_slice returns 6 rows for 3 distinct airings
after refresh 3: epg_programmes rows 9, grid_slice returns 9 rows for 3 distinct airings

the guide's rows for bbc1.uk:
   id  1  start 1000  "Programme 0"
   id  4  start 1000  "Programme 0"
   id  7  start 1000  "Programme 0"
   …
```

`grid_slice` is the guide grid's only query, and it hands the UI one row per copy.

**The scale on this subscription.** `providers.rs:960` sets `REFRESH_AFTER_SECS = 24 *
60 * 60` and `main.rs:65` sweeps for due providers every 30 minutes, so this happens by
itself once a day. One copy of this guide is:

```
547,166 programmes
  12,728,575 bytes of titles
  92,080,284 bytes of descriptions
  ~128 MB of row payload per copy, before index overhead
```

So the library database grows by roughly **128 MB a day**, and after a week the guide
shows seven identical entries in every slot, the "now and next" banner picks whichever
copy SQLite reaches first, and `grid_slice` returns seven times the rows for every paint.

Three separate things are missing and the fix wants all three: a unique constraint on
`(channel_id, start, title)` so a re-import updates rather than appends, a call to
`clear_channels` or an upsert in the import, and a caller for `prune_before` on the
schedule its own documentation promises.

### R-50 — Low — only the first of a channel's display names is stored

`aurora-db/src/repo/epg.rs:85`

```rust
stmt.execute(params![c.id, c.display_names.first(), c.icon])?;
```

`EpgChannel::display_names` is a `Vec<String>` because XMLTV allows several — the
alternative spellings are exactly what fuzzy name matching needs. The column is singular,
so all but the first are dropped at the boundary.

It does not affect EPG matching today, and this is worth stating because it bears on
[R-22]: `sync.rs:485` builds the `EpgIndex` from the **in-memory parse**
(`EpgIndex::build(&epg_channels)`), not from the table, so the matcher does see every
display name. The measurement in R-22 used all of them and was right to.

What is lost is any *later* use of those names — a re-match without a re-fetch, or a
"which EPG channel is this?" picker in Settings — which can only ever see one spelling.

### R-51 — Low — three columns the parser fills are never read back

`aurora-db/src/repo/epg.rs:114` (`SELECT`), `:15` (`ProgrammeRow`)

`epg_programmes` stores `icon`, `credits` and `star_rating`. `ProgrammeRow` has fields
for none of the first two, and the `SELECT` fetches `star_rating` as column 11 and then
never reads it — `map_row` goes straight from `rating` at 10 to `flags` at 12.

So a guide entry's own artwork, its cast list and its star rating are written on every
import and cannot reach the UI. On this subscription there is no such data to lose
(see [R-31]), which is presumably why nobody noticed; on a guide that carries it, the
work is being done and discarded.

### R-52 — High — the A–Z letter bar sends 97% of the library to the letter "E"

`aurora-db/src/repo/library.rs:83` (`browse_filters`), `:194` (`list_movies`)

Three different expressions describe "the title" in the same query, and no two of them
agree:

| what | expression |
|---|---|
| what the viewer **sees** | `COALESCE(custom_title, tmdb_title, title)`, then `shown()` strips the provider's language prefix |
| what the **letter bar** filters on | `COALESCE(custom_title, title)` — no `tmdb_title`, no prefix stripping |
| what the **A–Z sort** orders by | bare `title` — neither |

`browse_filters` is explicit that this must not happen:

> The letter is matched against the title as shown, which is the same expression the A–Z
> sort orders by — otherwise the bar would filter on one name and the list would be
> sorted by another, and "W" could land among the Vs.

It is not the same expression, and this subscription writes **27,416 of 27,494** film
names with a leading language code. Loading all 27,494 real names through the real
`upsert_movies`, `reclassify` and `list_movies`:

```
what each letter-bar button returns:
   #:1  A:9  B:8  C:4  D:7  E:26796  F:14  G:1  H:5
   I:0  J:0  K:0  L:5  M:3  N:1  O:0  P:5  Q:599
   R:6  S:11  T:13  U:2  V:0  W:4  X:0  Y:0  Z:0

the same films by the first letter of their DISPLAYED title:
   #:370  A:2038  B:1800  C:1357  D:1464  E:579  F:969  G:746  H:1124
   I:733  J:508  K:482  L:1223  M:1651  N:615  O:537  P:945  Q:45
   R:883  S:2265  T:5372  U:526  V:198  W:806  X:31  Y:161  Z:66
```

"E" returns 26,796 films because almost every raw name begins `EN ★ …`; "Q" returns 599,
which are the `QFR ★ …` ones. Every other button returns single digits, and **eight
letters — I, J, K, O, V, X, Y, Z — return nothing at all**, in a library that holds 733
films beginning with I and 482 beginning with K. Press "T" for the 5,372 films starting
with T and get 13.

Every film is matched by *some* button, so nothing is unreachable — it is reachable only
under a letter that has nothing to do with its name, which is worse than missing: the
bar looks like it works.

**The sort is a separate and much milder matter.** Ordering by the raw `title` is nearly
right here by accident, because almost every row carries the *same* prefix, so sorting by
`EN ★ Alien` ≈ sorting by `Alien`. Measured over the whole sorted list, **18 adjacent
pairs of 27,493 are out of order** — the places where a different prefix interleaves:

```
"Andromeda  - 2022"
"ElGrandeToto - OLM Souissi Mawazine - 2025"     <- raw title is "AR ★ ElGrandeToto …"
"Aries Spears Hollywood, Look I'm Smiling  - 2011"
```

That one is on the **first page** of Movies sorted A–Z.

The fix is one expression used in all three places. `lang::strip_language_prefix` is Rust
and not available to SQLite, which is presumably how the three drifted apart — so this
wants either a stored display-title column (`reclassify` already walks every row and
would be the place to fill it) or a registered SQL function. A stored column also makes
the letter filter and the sort indexable, which neither is today.

### R-53 — Medium — the app says dropped channels were "flagged"; nothing flags them

`aurora-db/src/repo/channels.rs:119` (`stale`), `:128` (`list`),
`src-ui/src/features/setup/SetupWizard.tsx:682`

After a refresh the wizard tells the viewer:

> `${report.channelsMissing} channels your provider no longer lists were kept and flagged.`

`channels::stale` is documented as *"README §4.6 keeps them visible with a badge rather
than deleting them out from under the user's favorites"*, and the "kept" half is true —
nothing deletes them. The "flagged" half is not implemented anywhere:

- `channels::stale` has exactly one caller, `sync.rs:340`, which uses it only to set
  `report.channels_missing` — a number. It does not mark the rows.
- `channels::list` selects no staleness column, and `ChannelRow` has no field for one.
  Confirmed by constructing a row and printing it:

```
ChannelRow { id, name, number, logo, group, tvg_id, epg_channel_id,
             quality, hidden, is_radio, has_catchup, lang, favorite }
```

- Nothing in `src-ui` reads a per-channel staleness flag; the only consumer of
  `channelsMissing` is that one sentence.

So a channel the provider has dropped sits in the list looking exactly like a working
one, and the only sign is a tune that fails with a provider error. The viewer has been
told explicitly that these are marked, which makes the absence worse than if nothing had
been claimed — they will look for a badge that does not exist.

Either the badge or the sentence has to change. The data is already there: `last_seen_at`
is on the row and the provider's last refresh time is on `providers`.

### R-54 — Info — ruled out: the cast list's `ORDER BY` is a no-op but the order is right

`aurora-db/src/repo/library.rs:107`

```sql
(SELECT group_concat(p.name, char(31))
   FROM credits c JOIN people p ON p.tmdb_id = c.person_id
  WHERE c.item_kind = 'movie' AND c.item_id = movies.id AND c.is_cast = 1
  ORDER BY c.ord)
```

This is an aggregate with no `GROUP BY`, so it returns one row and the trailing
`ORDER BY c.ord` sorts a single-row result — it cannot influence the order
`group_concat` accumulates in. The comment two lines down even says "`group_concat`
cannot be ordered portably". SQLite 3.44 added `group_concat(x, sep ORDER BY …)`; this
is not that.

Tested rather than assumed: credits inserted out of billing order (ord 2, 0, 1) come back
`["Top Billed", "Second Billed", "Third Billed"]` — correct — **and still correct after
`DROP INDEX idx_credits_item`**, so it is not the index that is saving it. I could not
construct a plan that gets it wrong.

Recorded as unspecified-but-working rather than as a defect: the order is not guaranteed
by anything the SQL says, and a future SQLite is free to change it. Making the `ORDER BY`
real costs one edit.

### R-55 — Low — `reclassify` says it is a fallback; it is the only thing that classifies

`aurora-db/src/repo/filtering.rs:250`

> Recompute `lang_code` and `quality_rank` for everything.
>
> **Ingestion classifies as it writes**, so this exists for the two cases ingestion
> cannot cover: a library that predates the columns, and a build whose classifier has
> changed its mind.

Ingestion does not. Neither write path mentions either column:

- `library::upsert_movies` inserts `(provider_id, provider_key, title, match_key, year,
  quality, group_title, url, poster, added_at, rating, last_seen_at)`.
- `channels::upsert_batch` inserts `(… name, match_key, number, logo, group_title,
  tvg_id, shift_minutes, quality, language, country, is_radio, catchup_*, last_seen_at)`.

So `lang_code` and `quality_rank` are set **only** by `reclassify`, which `sync.rs:542`
calls at the end of every import. That is correct and necessary — but it means the import
has an undocumented ordering dependency that the comment actively denies, and the
sibling `reclassify_if_stale` (which skips the work when the classifier version has not
changed) would be the *wrong* call there. Its only caller is
`aurora-ingest/examples/import.rs`.

Two consequences worth stating, both currently benign:

- An import interrupted after the upserts and before `reclassify` leaves every row with
  `lang_code = NULL` and `quality_rank = 0`. "Hide other languages" tolerates NULL by
  design, so nothing vanishes — but `hide_untagged` would hide the entire library, and
  duplicate collapsing with every rank at 0 falls back to "lowest id wins" rather than
  "best copy wins".
- `CLASSIFIER_VERSION` is 4 and the comment documents only versions 3 and 2. Whatever
  changed in 4 is not written down.

### R-56 — Medium — search is inside its budget today and R-49 takes it out

`aurora-db/src/repo/search.rs:133` (`grouped`), `:161` (the programme query)

Two structural costs, both proportional to the library rather than to the result set:

1. `grouped` calls `visible_ids` three times per search — once each for channels, films
   and series — and each one loads **every visible id** into a `HashSet`. With
   `hide_duplicates` on, that is the expensive correlated `NOT EXISTS` over the whole
   table, three times, per keystroke. The comment says "search is a small result set by
   construction, so this costs one extra query"; the result set is small, the filter set
   is the library.
2. The programme search is `p.title LIKE '%term%'` over `epg_programmes`, which no index
   can serve, so it is a full scan of the guide per keystroke.

**Measured on a real-sized library** (27,494 films, 6,698 channels, 546,992 programmes,
five successive keystrokes, after `ANALYZE`):

```
no library filters                19.4 ms per keystroke
hide duplicates on                41.6 ms
english only + hide duplicates    44.8 ms
```

README §10 asks for sub-100 ms search, so as it stands this passes, and the three
`visible_ids` scans are affordable.

**What breaks it is [R-49].** Each refresh appends another copy of the guide, and the
programme scan is linear in that. Simulating successive days:

```
day 1:    546,992 programmes ->  19.9 ms per keystroke
day 2:  1,093,984            ->  26.2 ms
day 3:  1,640,976            -> 152.9 ms      <- past the budget
day 4:  2,187,968            -> 113.3 ms
day 5:  2,734,960            -> 145.4 ms
and with the library filters on, at day 5:  241.1 ms
```

So within three days of ordinary use the command palette is over its stated budget, and
by day five it is twelve times slower than on the first day. Fixing R-49 fixes this; the
two should be worked together, and this is the measurement that says search needs no
other change once it is.

### R-57 — Medium — a shelf with an accent in its name contributes nothing to the rail

`aurora-db/src/repo/recommend.rs:183`

```rust
"{} WHERE hidden = 0 AND LOWER(group_title) = ?1{visible} {ORDER} LIMIT ?2"
```

The bound parameter is `aurora_core::recommend::normalise(category)`, which is Rust's
`trim().to_lowercase()` — Unicode-aware. SQLite's built-in `LOWER()` folds **ASCII
only**. For any category whose name contains a non-ASCII uppercase letter the two
disagree and the equality can never hold.

This subscription has two of them, out of 145 categories:

```
'CA ✪ QUEBEC NOËL'
   rust   'ca ✪ quebec noël'
   sqlite 'ca ✪ quebec noËl'
'CA ◉ TÉLÉRÉALITÉS ET VARIÉTÉS'
   rust   'ca ◉ téléréalités et variétés'
   sqlite 'ca ◉ tÉlÉrÉalitÉs et variÉtÉs'
```

Measured through the real `candidates()` with the real film names filed under the real
category names:

```
EN ✪ BOX OFFICE                 ->   50 candidates
EN ✪ ACTION                     ->   50 candidates
US ★ CINEMANIA                  ->   50 candidates
CA ✪ QUEBEC NOËL                ->    0 candidates
CA ◉ TÉLÉRÉALITÉS ET VARIÉTÉS   ->    0 candidates
```

The decorative `✪` and `★` are unaffected — `LOWER` leaves non-letters alone — so it is
specifically the accented letters that break it. The consequence is narrow and sharp: a
viewer who watches Québécois films gets a rail built from every shelf **except** the one
they watch, and nothing reports a problem. Taken with [R-39] — films carry no genres at
all here, so the category bucket is the only retrieval that can work for them — those
two shelves produce no film recommendations whatsoever.

The genre path takes the same `LOWER`, and there it is currently safe: none of this
panel's 65 genre strings contains a non-ASCII uppercase letter.

The cost of `candidates()` itself is fine. Every bucket is a full table scan with a sort
— there is no index on `group_title`, and `LOWER()` would defeat one anyway — but
measured over 27,494 films with the module's own constants (`TASTE_BUCKETS = 3`,
`PER_BUCKET = 600`, `EXPLORE = 600`):

```
no filters                       30.8 ms   2,400 candidate rows
hide duplicates                  57.4 ms   2,307
english only + hide duplicates   60.1 ms   2,294
```

Worth knowing before anyone "optimises" it; the defect here is correctness, not speed.

### R-58 — Low — an age limit the code cannot parse becomes "no limit"

`aurora-db/src/repo/profiles.rs:59`

```rust
max_age: max_rating.as_deref().and_then(parental::min_age),
```

`max_rating` is stored by `create` and `set_limits` as `max_age.map(|a| a.to_string())` —
a bare number — and `min_age` reads every bare number back correctly, so the round trip
works today (checked for 0, 7, 12, 13, 16, 17, 18).

What is wrong is the direction of failure. `min_age` returns `None` for anything it does
not recognise, and `None` here means **unrestricted**: `may_watch` passes
`profile.max_age.map(AgeLimit)` to `is_allowed`, which returns `true` for every
certification when the limit is `None`. So a `max_rating` this build cannot parse — one
written by a different build, or a certification string rather than an age — silently
turns a kids profile into an unrestricted one.

Same shape as [R-45]: the fallback for an unreadable value should be the restrictive
reading, not the permissive one. In both cases the code reads as if the author was
thinking about the happy path only.

For the record, the PIN throttle below it is sound: the lockout is checked before
verification so a correct PIN is also refused while locked; `failures` is not reset when
a lockout expires, so every further wrong guess costs another 60 seconds, which caps
guessing at one attempt a minute. `pin.rs` is explicit that this is "a lock that keeps a
nine-year-old out of the horror section, not a security boundary", and the implementation
matches what is claimed. `minutes_watched_since` uses SQLite's two-argument `min()`,
which is the scalar function and not the aggregate — correct, and easy to misread.

### R-59 — Low — thirty films keep a year in their search key — **this finding was
first written as High and 99%, and that was wrong**

`aurora-ingest/src/enrich.rs:196`, `aurora-ingest/src/sync.rs:362`

**The correction first.** The original version of this finding claimed that 99% of films
reach TMDB with the year still in the title, and attributed the library's near-total lack
of metadata to it. That is false. I measured `strip_language_prefix(provider_name)` —
but the provider's name is not what is stored. `sync.rs:362` runs
`title::clean_movie_title` at ingest and stores *its* output, with the year lifted into
its own column:

```
provider  "EN ★ Inception - 2010"
stored    title "EN Inception"   year Some(2010)
searched  "Inception"                       -> scores 1.000 against TMDB's "Inception"
```

Re-measured through the real order — `clean_movie_title` at ingest, then
`strip_language_prefix` at enrichment — over all 27,494 real film names:

```
match_key still contains a 4-digit year:  30 of 27,494  (0.1%)
```

Not 27,334. The pipeline works for the other 27,464.

**What is left.** Thirty films share one shape, where the year sits against a bracketed
quality tag and `clean_movie_title` leaves it behind:

```
provider  EN ★ The Academy - 2025[4K]
stored    EN The Academy 2025
searched  The Academy 2025
key       theacademy2025          -> scores 0.000 against "The Academy"
```

`- 2025[4K]` with no space before the bracket is the form that defeats it. Thirty titles
on this subscription, each permanently marked `nomatch` after the first attempt.

**What was right in the original and still is:** `enrich.rs` uses `Query::new` on an
uncleaned string where `tmdb::Query::from_stream_name` exists and would clean it. For the
stored titles that is almost always a no-op, which is why it has not shown; for these
thirty it is the difference.

**A third correction to this finding.** The original also said "there is no
`CLASSIFIER_VERSION` equivalent for enrichment". There is.
`aurora-db/src/repo/enrichment.rs:166` defines `QUERY_VERSION`, `:173`
`forget_stale_nomatches` deletes every `nomatch` row when it changes, and
`main.rs:289` calls it from the maintenance thread at launch. It was added for exactly
this situation and its doc comment records the evidence — "of 60 sampled titles this
library had recorded as `nomatch`, 0 could be found under the provider's string and 59
under the stripped one".

So the correct instruction for whoever fixes the thirty is narrower and easier than what
I first wrote: change the query *and bump `QUERY_VERSION`*, and the existing machinery
re-asks. I found this by reading `main.rs` and following the call, which is the same way
the two earlier errors in this finding were caught — forward from the caller, not from
the function.

**On my own method.** This is the second finding in this review to be wrong the same way:
[R-21] replayed `EpgIndex::build` over the raw XMLTV instead of the parser's output, and
this one replayed the enrichment query over the provider's name instead of the stored
title. Both times the code I was reading was correct and the stage *before* it was the
one I had not read yet. Both were caught by continuing to read forward — `xmltv.rs:94`
in one case, `sync.rs:362` in the other — rather than by rechecking the measurement.
The lesson for the rest of this review, and for the fixes: a measurement of one function
is only as good as the knowledge of what actually calls it, and "I reproduced it" is not
the same as "I reproduced what the app does".

---

## aurora-ingest

### R-61 — High — `redact` misses the URL shape this panel uses for every live channel

`aurora-ingest/src/http.rs:443`

```rust
if !redacted && segments.len() == 4 && looks_like_stream_id(segments[3]) {
    segments[1] = "***";
    segments[2] = "***";
}
```

`looks_like_stream_id` requires `<digits>.<ext>`:

```rust
let Some((id, ext)) = segment.rsplit_once('.') else { return false };
```

This panel's M3U serves live channels with **no extension**. Fetched and counted from the
real 51 MB playlist:

```
240,269 URLs in the playlist
  6,699 whose last path segment contains no '.'
  6,699 of the form http://host/<user>/<pass>/<id>   -- one per live channel
```

Run through the real `redact`:

```
LEAKS    the m3u's live form, no extension
         http://64582429.max-jbnott.online/G22R37R4/47732430/95471
clean    the same with .ts
         http://64582429.max-jbnott.online/***/***/95471.ts
clean    built by xtream::stream_url
         http://64582429.max-jbnott.online/live/***/***/95471.ts
clean    a movie URL, player_api query form, the tokenised 302 target,
         the userinfo form, timeshift.php          -- all redacted correctly
```

Every other shape is handled. The one that is not is the one 6,699 of this
subscription's channels use, and it is the shape the codebase already knows about —
`aurora_core::catchup::split_xtream_stream_url` parses it, and the comment immediately
above this branch says *"Many panels serve `http://host/<user>/<pass>/<id>.ts` with no
marker at all … Without this the commonest stream URL there is went into the log
intact."* The branch was written for the no-marker case and still requires an extension.

This is F-15's class of defect, narrowed rather than closed. The stored
`channel_sources.url` for every live channel on this line is a string that `redact`
returns unchanged.

### R-62 — High — the player logs mpv's output with no redaction at all

`aurora-player/src/mpv.rs:322` and `:444`

```rust
tracing::warn!("playback failed: {described}");          // :322
tracing::debug!("mpv: {line}");                          // :444
```

`described` is mpv's message plus the last `LOG_CONTEXT_LINES` (8) lines of mpv's own
log, joined. Neither site passes anything through `http::redact` — and `redact` is never
called anywhere in `aurora-player`; grepped, there is not one reference to it in the
crate.

`LOG_LEVEL` is `"warn"`, so only mpv's warnings and errors are captured, not its verbose
"Opening …" lines. **What I could not establish here is whether a given libmpv build
puts the URL in a warn-or-above line** — mpv's own `Failed to open <url>` is emitted at
error level, which this filter would capture, but confirming the exact text needs a real
failing stream and a build of mpv, which this review did not run. So treat the severity
as "the mechanism is unredacted and the likely text contains the URL", not as a measured
leak.

What *is* measured: the real profile's log holds no credentials today.

```
%LOCALAPPDATA%\tv.aurora.player\aurora.log    lines containing the password: 0
%LOCALAPPDATA%\tv.aurora.player\aurora.log.1  lines containing the password: 0
```

(Checked read-only, by counting matches; nothing was printed or modified.)

Taken with [R-61] the two compound: even if these two call sites were routed through
`redact`, the bare form would still pass through it unchanged. Both need fixing, and
`redact` needs the extension requirement dropped — a three-segment path after the host
whose last segment is all digits is a stream URL whether or not it carries `.ts`.

`PRIVACY.md` states plainly that credentials "do not appear in a log" and that "every URL
written to the log goes through `http::redact` first". Neither half is true of the player.

### R-63 — High — pasting a panel URL with the wrong character in it kills the app

`aurora-ingest/src/source.rs:110` (`urldecode`)

```rust
if b[i] == b'%' && i + 2 < b.len() {
    if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {   // <- byte-slices a &str
        out.push(v as char);                                     // <- byte as Latin-1
```

Two defects in four lines.

**It panics.** `&s[i+1..i+3]` slices by byte index without checking a character boundary.
A 3- or 4-byte UTF-8 character immediately after a `%` splits mid-character:

```
a 3-byte character right after a %   -> PANICKED
    end byte index 3 is not a char boundary; it is inside '€' (bytes 1..4 of string)
a 4-byte character right after a %   -> PANICKED
    end byte index 3 is not a char boundary; it is inside '😀' (bytes 1..5 of string)
```

`src-native/Cargo.toml:50` sets `panic = "abort"`, so this is not an error the wizard can
report — the process goes. The input is a URL **pasted into the setup wizard**, which is
the first thing a new user does, and the trigger is a `%` followed by a non-Latin
character.

**It mangles every non-ASCII password even when it does not panic.** `out.push(v as char)`
and `out.push(b[i] as char)` both reinterpret a UTF-8 byte as a Latin-1 code point:

```
password=caf%C3%A9   ->  "cafÃ©"      (should be "café")
password=café        ->  "cafÃ©"      (should be "café")
```

So a provider whose password contains any accented character cannot be set up by pasting
the URL. The stored secret is wrong, the panel answers 401, and
[R-32]/`neterr` reports "Your provider rejected these credentials" — which is true and
sends the viewer to retype a password that was already correct.

**The entry point, confirmed.** `aurora-app/src/providers.rs:69` is
`#[tauri::command(async)] pub fn providers_detect`, and its first act is
`parse_pasted_xtream(&args.text)`. The setup wizard calls it on what the viewer pastes,
so the panic is one IPC call from the first screen of a first run — not reachable only
through some internal path. Combined with [R-72], in a release build that is the process
gone before the wizard can report anything.

**The correct implementation already exists in this repository.**
`aurora-core/src/m3u.rs:392` is the same function written properly: it accumulates into a
`Vec<u8>`, tests the two hex digits as bytes rather than slicing the `&str`, and finishes
with `String::from_utf8_lossy`. It cannot panic and it decodes UTF-8 correctly. There are
two copies of this function and the one on the first-run path is the broken one.

### R-64 — Medium — the "streaming" EPG import holds the whole guide in memory

`aurora-ingest/src/sync.rs:700` (`fetch_epg`), `:137` (`Fetched`)

```rust
// Two passes would mean two downloads, so channels are collected in memory (a few
// thousand small rows) while programmes stream straight through to SQLite.
let mut programme_batches: Vec<Vec<Programme>> = Vec::new();
…
|progs: &[Programme]| { programme_batches.push(progs.to_vec()); Ok(()) },
```

Every batch is pushed onto a `Vec` and kept. Nothing streams to SQLite — `fetch` returns
the batches and `apply` writes them afterwards, which is the whole point of the
fetch/apply split (the database lock is not held during the download). The batching is
real and the sink is written correctly; it is the caller that collects.

Measured by running the production parser and sink over the real documents and sampling
the process's working set:

```
the real 178 MB guide, 546,912 programmes held as Vec<Vec<Programme>>
   parsed in 1.16 s      working set 262 MB      peak 263 MB

the real 51 MB playlist, 237,170 entries held as Vec<PlaylistEntry>
   parsed in 0.62 s      working set 159 MB      peak 173 MB
```

`Fetched` holds `live`, `movies`, `episodes`, `series` **and** `epg` in one struct, so
both sit in memory together: about **420 MB of parsed data** at the peak of a refresh on
this subscription, before the WebView2 processes and everything else.

README §16 budgets "Idle memory (library loaded, not playing) ≤ 500 MB" and requires the
XMLTV import to be "incremental" against a **500 MB** document — 2.8× this one, which on
these numbers would hold roughly 735 MB for the guide alone.

The module header for `epg.rs` says "nothing here collects the whole document", and that
is true of `epg.rs`. It is not true of the pipeline.

Incidentally confirming [R-04]: the real playlist parse reports **3,099 skipped with 0
warnings**. Since an unusable URL always logs a warning and a duplicate never does, all
3,099 are duplicates — exactly the conflation R-04 describes, and a viewer told "3,099
skipped" has no way to learn that none of them was an error.

### R-65 — Info — two sharpenings from the Xtream client

`aurora-ingest/src/xtream.rs`

**[R-33] is narrower than first written, and the contrast makes it clearer.** This client
has its own `urlencode` (`:236`) which percent-encodes every byte outside the unreserved
set, byte-wise, so it is UTF-8-correct — and `api_url` and `xmltv_url` both use it. Only
`stream_url` (`:193`) skips it, by delegating to `aurora_core::xtream::stream_url`, which
interpolates the credentials raw. So within one file the API requests are encoded
correctly and the playable URLs are not. The correct encoder is four lines away from the
call that needs it.

**`provider_failure` offers a Retry button on a failure it has marked non-retryable**
(`:210`): `retryable: false` with `actions: vec![ErrorAction::Retry, ErrorAction::OpenSettings]`.
Same shape as [R-13] — the `actions` list and the `retryable` flag are set independently
and can disagree. Worth fixing in the same pass.

### R-66 — High — a failed recording writes the username and password into the log

`aurora-ingest/src/recorder.rs:159`, `aurora-core/src/neterr.rs` (the `Unknown` branch)

This is the certain version of [R-62]. [R-36] already noted that the recorder classifies
with `e.to_string()` instead of the shared `classify_reqwest`. The consequence is worse
than a vague message, and `http.rs:363` says why in its own words:

> That omission is the point. `reqwest`'s Display is `error sending request for url
> (<the whole URL>)`, and every Xtream URL contains `username=`.

`cause_chain` exists specifically to keep that string out of the classifier. The recorder
passes it in. Traced through the real code with this subscription's real credentials:

```
recorder.rs:159 passes:
  error sending request for url (http://64582429.max-jbnott.online/G22R37R4/47732430/95471)

NetFailure::classify -> Unknown
  message: "Something went wrong with that request"

and the Unknown branch does:  tracing::warn!("unclassified network failure: {l}")
so aurora.log gets:
  unclassified network failure: error sending request for url
    (http://64582429.max-jbnott.online/g22r37r4/47732430/95471)
```

No branch of `classify` matches that text — it contains none of the keywords — so it
lands in `Unknown`, which is the one branch that logs its input verbatim.

And `redact` would not save it even if it were applied, because of [R-61]:

```
redact(...) -> error sending request for url (http://…/G22R37R4/47732430/95471)
   still leaks: true
```

**How often this fires:** on a line that allows one connection, a recording that starts
while anything is playing fails ([R-32]). So the ordinary case — recording a programme
while watching another channel — is the case that writes the password to disk.

Four separate defects line up to make it happen, and each one alone would have stopped
it: the recorder not using `classify_reqwest` ([R-36]), `classify`'s fallback logging its
raw input, `redact` not knowing the extension-less form ([R-61]), and the player never
calling `redact` at all ([R-62]). That is worth noting in its own right — the redaction
story has four places to be right and is right in one of them.

The user-facing `cause` for this failure is also the string with eighteen spaces in it
([R-12]).

### R-67 — Low — the stall detector cannot detect the stall the code says is characteristic

`aurora-ingest/src/recorder.rs:73` (`Progress::is_stalled`),
`aurora-app/src/dvr.rs:369` (`note_stalls`)

```rust
pub fn is_stalled(&self, now: i64, limit_secs: i64) -> bool {
    let last = self.last_write();
    last > 0 && now - last > limit_secs
}
```

`last_write` is set only on a successful write, so a recording that has received nothing
has `last_write == 0` and the `last > 0` guard makes it permanently not-stalled:

```
a recording that has received nothing at all:
   bytes 0  last_write 0
   is_stalled(now, 60)     = false
   is_stalled(now, 600)    = false
   is_stalled(now, 86400)  = false
```

The case this misses is the one `build_client`'s own comment calls out two hundred lines
earlier: *"a provider which accepts the connection and then sends nothing holds `start`
for the whole window"*. That recording shows as `Recording` with no progress for its
entire window, `report.stalled` never mentions it, and only `reap` resolves it at the end
with "the provider sent nothing".

The outcome is eventually right; what is missing is the signal while it is happening,
which is the only thing `note_stalls` exists to provide. Using `started_at` as the
baseline when `last_write` is zero is the whole fix.

### R-68 — Medium — the recommendation rail throws away Gemini's explanation; the
assistant does not

`aurora-ingest/src/gemini.rs:212` vs `aurora-ingest/src/chat.rs:235`

`http::post_json_with_status` exists precisely for this, and its doc comment says so:

> Gemini answers a rejected request with `400` and a JSON envelope whose `error.message`
> says precisely what was wrong … and `post_json` throws all of that away, because
> `send_with_retry` turns a non-2xx into a `NetFailure` before anything reads the
> response. The assistant reported every one of them as "Your provider didn't respond".

`chat.rs` was changed to use it. `gemini.rs` — the recommendation rail — still calls
`post_json`. So `parse_reply`'s error branch, which reads `error.message` and maps "api
key" onto `Unauthorized` + `OpenSettings`, is unreachable for any HTTP error status; it
can only fire if Gemini returns 200 with an error envelope, which it does not.

The two paths side by side, on the envelope Gemini really sends for a bad key:

```
the chat path (post_json_with_status)
   code     Unauthorized
   message  "Gemini refused the request"
   cause    "API key not valid. Please pass a valid API key."
   actions  [OpenSettings]

the recommendation path (post_json)
   code     BadRequest
   message  "That request was rejected"
   cause    "The server would not accept it as sent. Nothing about retrying will
             <18 spaces> change that; the log has what it said."
   actions  [TryAnotherSource, OpenSettings]
```

So a viewer with an expired or mistyped key is told the assistant's key is invalid and,
on the home screen, that "that request was rejected" — with a "Try another source"
button ([R-13]) and the mangled whitespace ([R-12]). One line changes it.

The rest of `gemini.rs` is sound: the key travels in `x-goog-api-key` rather than the
query string for the stated reason (a URL is the thing that gets logged, and `redact`'s
denylist did not include a bare `key`), the response schema is enforced server-side, and
the library lookup is what stops an invented title reaching the screen.

### R-69 — High — the library database does contain credentials, and PRIVACY.md says it
does not

`aurora-ingest/src/sync.rs:840` (`provider_key`), `:733` (`write_channel_sources`),
`docs/PRIVACY.md`

PRIVACY.md is unambiguous:

> They are **not** stored in the library database. They live in Windows Credential
> Manager, and the database keeps only a reference to the entry. That means a copy of
> `library.db` — a backup, an export, a file handed to somebody for a bug report —
> contains no credentials.

Two columns contradict it.

**`channel_sources.url`, for every entry, necessarily.** `write_channel_sources` inserts
`entry.url`, and `entry.url` is the playable stream URL — which carries the username and
password in its path, because that is how Xtream addresses a stream. This is true whether
the provider was added as Xtream (the URL is built by `xtream::stream_url`) or as an M3U
(the URL came from the playlist). There is no version of a playable URL for this panel
that does not contain the credentials, so this part is not a bug in the code — it is a
false claim in the document.

**`channels.provider_key`, for M3U-sourced live channels, unnecessarily.**
`provider_key` extracts a stable id from the URL, but only when it finds a `/live/`,
`/movie/` or `/series/` marker. This panel's playlist has no marker on live channels, so
it falls through to `path.to_string()` — the whole URL:

```
http://…/live/G22R37R4/47732430/95471.ts     -> "live95471"        clean
http://…/movie/G22R37R4/47732430/1284015.mkv -> "movie1284015"     clean
http://…/G22R37R4/47732430/95471             -> the entire URL     leaks

over the real playlist: 6,698 of 237,170 entries get a provider_key
containing the password
```

That one is avoidable: the same three path segments that `redact` should recognise
([R-61]) and that `catchup::split_xtream_stream_url` already parses would give
`live95471` here too.

**It also breaks identity.** `provider_key` is half of `UNIQUE (provider_id,
provider_key)`. For those 6,698 channels the key contains the password, so the day the
provider rotates it every live channel is a new row: the old ones go stale, the new ones
start fresh, and anything keyed to a channel id — favourites, EPG manual mappings, saved
mosaic layouts, recording rules — points at rows nobody will see again.

**What is not true:** no feature of the app exports the database. `logging::export_to`
copies log files only. So this is a document that overstates its guarantee plus a
needless leak into a key, not an automatic disclosure — but the document names exactly
the scenario ("a file handed to somebody for a bug report") that makes it matter, and
somebody who read it would hand the file over believing it was safe.

---

## aurora-player

### R-70 — High — rewinding more than twelve seconds kills playback twelve seconds later

`aurora-player/src/watchdog.rs:122` (`observe`), `aurora-player/src/mpv.rs:692` (`seek`)

`Watchdog::observe` treats the playhead as a high-water mark:

```rust
let advanced = position_secs > self.last_position + PROGRESS_EPSILON_SECS;
if position_secs > self.last_position {
    self.last_position = position_secs;
}
```

`last_position` only ever increases. After a **backwards** seek the playhead is behind it,
so `advanced` stays false until playback climbs back past where it was before the seek —
and `mpv::seek` does not call `watchdog.reset()`. `reset()` has exactly two callers,
both on load/stop (`mpv.rs:658`, `:670`).

So a backwards seek of more than `DEFAULT_STALL_TIMEOUT_SECS` (12 s) worth of content
produces a `Stalled` verdict 12 seconds later, and `pump` turns every verdict into
`self.fail(reason)` — status `Error`, and `playback::tick` rolls over to another source.

Driven against the real `Watchdog`:

```
playing normally, playhead advancing
   t=   1s pos=   1.0s -> Healthy      … steady

a small backwards seek (the playhead soon passes the old mark)
   t=   8s pos=  13.0s -> Healthy      … no fault, correctly

a thirty-minute live rewind (pos 1804s -> 0s)
   t=  17s pos=  12.0s -> Stalled   reason "the stream stopped sending data: timed out"
```

Twelve seconds after the rewind, playback is marked dead and restarted.

This is README §7.6's headline feature — pause and rewind live TV — and ordinary
scrubbing in a film. The reason it has not been obvious is the threshold: a nudge back of
a few seconds recovers before the timer expires, so only a *real* rewind triggers it.

The module header is explicit that the watchdog exists for "a provider that behaves —
accepts the connection, answers 200, and then sends nothing". A viewer rewinding is the
opposite of that, and the watchdog cannot currently tell them apart because it has no
notion of a discontinuity it was told about.

`seek` calling `watchdog.reset()` is the fix, and it is the same one-line change the two
existing callers already make for the same reason — `mpv.rs:669` even says so: *"the
position left by the stopped stream is still the watchdog's last known one."* A seek
leaves exactly that behind.

**Why no test caught it.** `watchdog.rs` was written to be pure and testable precisely
because `mpv.rs` is `#![cfg(windows)]` and "cannot be tested on CI's Linux runner at
all" — its own words. But the watchdog is *driven* only from `MpvBackend::pump`, and
`NullBackend`, which every test and the Linux dev server use, has no watchdog at all. So
the module's twelve unit tests cover `observe` with positions the test chooses, and
nothing exercises the one sequence the app produces: load, play, seek backwards, keep
observing. A test for that needs no mpv — it is three calls to `observe` — and would
have failed on the first run.

### R-71 — High — the complete set of places a credential can reach the log

`aurora-app/src/playback.rs:415`, `aurora-player/src/mpv.rs:321`, `:416`, `:443`,
`aurora-core/src/neterr.rs` (the `Unknown` branch)

[R-61], [R-62] and [R-66] each found one site. This is the closed set, from two sweeps:
one mechanical over every `tracing!`/`println!` in `src-native` that mentions a URL
without `redact`, and one by hand over every log statement in `aurora-player`, whose
variables (`described`, `line`, `message`) the mechanical pass cannot judge.

**Certain, with the credential-bearing value identified:**

1. `playback.rs:415` — `tracing::info!("reloading {} at …", session.url, …)`.
   `session.url` is `movies.url`, the playable stream URL, which carries `/<user>/<pass>/`
   in its path. Fires whenever a film or episode's stream dies and is retried, at **info**
   level, three times per failure. `redact` *would* handle this shape — it has `/movie/`
   and an extension — so this one is a missing call, not a gap in `redact`.
2. `neterr.rs` `Unknown` branch — `tracing::warn!("unclassified network failure: {l}")`
   logs its raw input. Fed by `recorder.rs:159`, which passes reqwest's `Display`
   including the whole URL. See [R-66].

**Probable, text not established:**

3. `mpv.rs:321` — `tracing::warn!("playback failed: {described}")`, where `described` is
   mpv's message plus its last eight log lines.
4. `mpv.rs:443` — `tracing::debug!("mpv: {line}")`, every captured line.
5. `mpv.rs:416` — `tracing::debug!("mpv reported {message} …")`.

   `LOG_LEVEL` is `"warn"`, so only mpv's warnings and errors are captured. mpv's own
   `Failed to open <url>` is an error-level message and would be; confirming the exact
   text needs a failing stream against a real libmpv, which this review did not run.

**Not a leak, checked:** `updates.rs:908` logs `RELEASES_URL`, a constant.
`mpv.rs:631` logs an option *name* and mpv's error, not the value.

`aurora-player` contains no reference to `redact` at all — the crate that handles the
one URL that always carries credentials is the crate with no redaction in it.

The fix has three parts and needs all three, because each covers a different site:
`redact` must stop requiring an extension ([R-61]); `playback.rs` and the three `mpv.rs`
sites must call it; and `neterr::classify`'s fallback must not log its raw input — or
must redact it first, which is cheap and covers every future caller.

Measured state today: the real profile's `aurora.log` and `aurora.log.1` contain **zero**
lines matching either credential. Nothing has fired yet.

**And there is a button to share them.** `aurora-app/src/logging.rs:177` is
`logs_export`, a Tauri command that copies both log generations into
`<data>/logs-export` and returns the path so the UI can show it — its doc comment says
the point is to put them "somewhere a person can attach them to a message". So the
sequence is: a recording fails, the password goes into `aurora.log`, and the viewer is
invited to export that file and send it to whoever is helping them.

That is the scenario `PRIVACY.md` says is safe:

> Nor do they appear in a log. Every URL written to the log goes through `http::redact`
> first…

Both halves of that sentence are the thing to fix.

---

## aurora-app

### R-72 — High — the panic safety net does not exist in the shipped binary

`aurora-app/src/supervise.rs:58`, `src-native/Cargo.toml:50`

`supervise.rs` exists to stop one panicking tick from ending a background thread for the
rest of the session. Its own header states the stakes:

> A panic in any of them ended that thread, permanently, and said nothing. … What a
> viewer would see is an OSD frozen on whatever it last showed, failover that stopped
> working, or — worse, because there is no symptom until the programme is gone — a DVR
> that silently never records anything again.

It is built on `catch_unwind`. The release profile sets `panic = "abort"`, under which
`catch_unwind` cannot catch anything: the process is gone before it returns.

Demonstrated with the same code shape under both profiles:

```
debug build (panic = unwind)
   tick 1 (healthy)      supervised returned true
   tick 2 (panics)       panic hook ran: … a tick blew up
                         the test thread panicked and was restarted
                         supervised returned false
   tick 3 — if this line prints, the loop survived      <- printed

release build (panic = abort, as shipped)
   tick 1 (healthy)      supervised returned true
   tick 2 (panics)       panic hook ran: … a tick blew up
   exit code: 127                                       <- tick 3 never ran
```

The panic *hook* does run first, so `log_panics` still writes the location and payload to
`aurora.log` — that half works, and it is the half that makes a report diagnosable. What
does not work is the recovery. In the shipped build a panicking tick takes the whole
application, which is strictly worse than the frozen-OSD symptom the module was written
to replace.

`supervised` has six call sites — maintenance, player, mosaic, DVR, update, playlist
refresh — and none of them is protected in release.

**Why the tests pass.** `cargo test` builds with the test profile, which unwinds. So
`a_panicking_tick_does_not_end_the_loop` can only ever pass, and can never exercise the
configuration that ships.

**This reframes every panic in this review.** [R-17] (a `strip from name` rule on a
Turkish title) and [R-63] (a pasted URL with a 3-byte character after a `%`) are caught
and logged in a debug build and fatal in a release one — the opposite way round from what
a reader of `supervise.rs` would assume. The same applies to
`recorder.rs:117`'s `join().unwrap_or_else(…)` branch, which handles "a panicked writer
thread has already lost the recording" and cannot run in release either.

Either the profile drops `panic = "abort"` — which costs some binary size and a little
speed, and makes the module do what it says — or `supervise.rs` and the comments around
it should stop claiming a recovery that does not happen. The first is almost certainly
right: six background threads whose panics are currently fatal are a worse trade than
unwinding tables.

**And seven of twelve background threads are not wrapped at all**, so their panics are
unprotected even in a debug build:

```
file              line  supervised?
dvr.rs             284  NO     the recorder's own writer thread
lib.rs              51  NO     catch_up_in_background
main.rs            243  NO     the window-reveal thread
main.rs            274  yes    maintenance
main.rs            310  yes    player + mosaic heartbeat
main.rs            334  yes    DVR scheduler
main.rs            353  yes    update check
main.rs            373  yes    playlist refresh
metadata.rs        428  NO     artwork warming
metadata.rs        525  NO     enrich_in_background
series.rs          210  NO     the episode sweep
updates.rs         503  NO     the update download
```

The five that are wrapped are the five `main.rs` starts directly; every thread spawned
from a module is not. Two of those matter in the same way the module header describes —
`enrich_in_background` and the episode sweep are long-running passes whose silent death
leaves a library permanently half-enriched with nothing said. And `metadata.rs:428`'s
artwork thread holds entries in `services.warming`, which it removes at the end: a panic
there leaves those URLs marked "being fetched" for the rest of the session, so those
posters are never retried.

So the picture is: in debug, five threads recover and seven do not; in release, none do.

### R-73 — Open question — parallel function calls are encoded as separate model turns

`aurora-app/src/assistant.rs:912`, `aurora-ingest/src/chat.rs:156` (`contents_of`)

`parse_reply` collects `calls: Vec<ToolCall>`, so the model may ask for several lookups in
one reply, and `send` handles them in a loop: push `Turn::ToolCall`, run it, push
`Turn::ToolResult`, repeat. `contents_of` then encodes each `ToolCall` as its own
`{"role":"model","parts":[{functionCall}]}` and each result as its own
`{"role":"user","parts":[{functionResponse}]}`, producing an alternating sequence.

Gemini's documented shape for parallel calls is one `model` content holding *several*
`functionCall` parts, answered by one `user` content holding the matching
`functionResponse` parts. The alternating form is a different shape.

**I could not establish whether the API rejects it.** Doing so means a live request with
the viewer's own key and quota, which this review did not spend. It is recorded because
the precedent is strong: the `thoughtSignature` defect that `chat.rs:76` documents at
length was a 400 caused by exactly this kind of shape mismatch, and the error it produced
("Function call is missing a thought_signature in functionCall parts") named the part
structure. A model that makes two lookups in one turn is the untested path through this
loop.

Cheap to settle either way: ask the assistant something that needs two lookups at once
and read the log. If it is wrong, the fix is to group a reply's calls into one turn
rather than interleaving them.

The rest of the loop holds up. `turn_with_retries` retries only `retryable` failures and
retries at the *turn* level, which is correct for the two cases the HTTP client cannot
see — a 200 carrying an `error` envelope, and a reply with no candidate. Partial work is
kept when a later round fails, so a question that got two lookups in and then lost the
connection still answers. And the claim in the module header is true by construction:
`ChatItem`s only carry ids that a previous tool result handed out, so an invented title
can reach the prose but never become something with a play button.

---

## src-ui

### Mechanical: ESLint, run for the first time

`eslint.config.mjs` was added during this review and had never executed — the repo's own
`npm install` fails on an npm 11.13 bug (`Cannot read properties of null (reading
'matches')`), which is presumably why. Installed into a scratch tree and run against
`src-ui/src` and `shared`:

```
8 problems (6 errors, 2 warnings) across 69 files
```

For a codebase that has never been linted that is a good result, and six of the eight are
not defects. Assessed individually rather than taken at face value:

| Finding | Verdict |
|---|---|
| `App.tsx:182` `useEffect` missing dependency `profile` | **Not a defect.** The array is `[profile.load]`, which is the zustand action — a stable reference. Depending on `profile` instead would re-run the effect on every store change. The rule is being conservative; the code is more precise than the rule. |
| `ErrorBoundary.tsx:29` unused `eslint-disable` | Trivial; the directive predates the config. Remove it. |
| four × `jsx-a11y/no-autofocus` (`Primitives.tsx:422`, `MosaicPage.tsx:444`, `PlaylistPage.tsx:482`, and one more) | **Deliberate and right here.** Each is a search or filter field inside a dialog the viewer just opened; on a remote-driven app, focusing it is the difference between typing and hunting for the field. The rule's objection is about surprising a screen-reader user on a web page. |
| `fixtures.ts:170` unnecessary escape `\[` | Trivial, in a test fixture. |
| `PlayerOverlay.tsx:141` click handler with no keyboard listener | **This one is a real defect** — see R-74. |

### R-74 — Medium — once the OSD hides, no key brings it back

`src-ui/src/features/player/PlayerOverlay.tsx:118` (`bump`), `:142`,
`src-ui/src/hooks/useHotkeys.ts:81`

`bump` makes the OSD visible and restarts a 3,200 ms hide timer. It is wired to exactly
two things:

```tsx
onMouseMove={bump}
onClick={bump}
```

and `visible` gates the whole control layer — `{visible && (…)}` at `:195`, so the
controls are unmounted, not merely faded.

Keyboard and remote input is handled elsewhere, by a global `window` listener in
`useHotkeys`, and **nothing in it calls `bump`**. Grepped: `bump` appears five times in
the file and nowhere else in `src-ui`.

So after 3.2 seconds of no mouse movement: pressing volume-up changes the volume with no
indicator, seeking moves the playhead with no scrub bar, pause pauses with no sign of it,
and channel-up zaps with no channel banner. The action happens; the feedback does not.
With a mouse it is a small annoyance — one twitch restores everything. On a television
driven by a remote, which README §1 names as the target, the OSD becomes unreachable:
there is no pointer to move.

ESLint found this as `jsx-a11y/click-events-have-key-events` on the overlay div, which is
the right complaint for the wrong reason — the fix is not a handler on that div but a
`bump()` in the hotkey path, since that is where remote input actually arrives.

### R-75 — Low — the Browse grid is the one list that is not virtualized, and it grows
without bound

`src-ui/src/features/browse/BrowsePage.tsx:117`, `src-ui/src/hooks/usePages.ts:63`

README §16 lists "virtualization for every list and the guide grid" under **"Techniques
that are required, not optional"**. Checked across the UI:

```
LivePage        useVirtualizer  ✓   (comment: "twenty-two thousand DOM…")
GuidePage       useVirtualizer  ✓   both axes
GroupSidebar    useVirtualizer  ✓
PlaylistPage    useVirtualizer  ✓
RecordingsPage  no              —   bounded set, defensible
BrowsePage      no              ✗
```

Browse instead pages with an `IntersectionObserver` sentinel, and `usePages` appends:

```ts
setItems((prev) => (at === 0 ? page : [...prev, ...page]));
```

There is no cap and nothing unmounts. `PAGE` is 120 and `CatalogCard` renders about 20
elements, so each page adds ~2,400 nodes: twenty pages of scrolling is ~48,000, and the
27,494 films on this subscription would be ~550,000 if somebody scrolled to the end.

**Proportionately:** the posters do carry `loading="lazy" decoding="async"`
(`Primitives.tsx:202`), so §16's image requirement is met and off-screen artwork is not
decoded — which is what would otherwise have made this a memory problem rather than a
DOM one. And a viewer has to scroll a long way to feel it. So this is a real gap against
an explicit requirement with a modest practical cost, not a crisis — recorded at Low for
that reason, and because the other four lists show the pattern the codebase already
prefers.

I have not measured a frame rate here: doing it properly needs the UI running against
the host, which this review did not drive. The arithmetic above is the honest extent of
the claim.

### R-76 — Low — the OSD shows the provider's language tag; every list strips it

`aurora-app/src/window.rs:185` and `:220`, `src-ui/.../PlayerOverlay.tsx:215`

`resolve_playback` selects the bare `title` column and puts it in `LoadOptions.title`,
which becomes `PlayerState.title`, which `PlayerOverlay` renders raw. Every list goes
through `library::shown` (`strip_language_prefix`) and the player does not:

```
provider : EN ★ Inception - 2010
stored   : EN Inception
lists    : Inception
the OSD  : EN Inception        <- the tag is still on it

provider : AR ★ الأرجنتين Vs Spain
lists    : الأرجنتين Vs Spain
the OSD  : AR الأرجنتين Vs Spain
```

Two smaller things in the same two queries: `SELECT url, title` ignores
`custom_title`, so a film the viewer renamed plays under the provider's name, and it
ignores `tmdb_title`, so an enriched film plays under the filing string rather than what
it is actually called. `MOVIE_SELECT` in `repo/library.rs` already has the right
expression — `COALESCE(custom_title, tmdb_title, title)` — eleven lines of SQL away.

### R-77 — Low — a doc comment has lost two clauses, and the sweep that found it was
mostly noise

`aurora-app/src/providers.rs:1061`

```rust
/// A playlist on this machine, or on a box in the cupboard, really can refuse in
/// twenty milliseconds. Telling those people their antivirus is at fault would be a
/// And a panel on the internet is not local, including the ones whose addresses look
/// The message has to point at the thing that is actually in the way, and name the
/// host so it can be allowed.
```

Two clauses are gone. "would be a" ends nowhere — presumably "…would be a lie" or
"…a mistake" — and "whose addresses look" ends nowhere, presumably "…look local". The
paragraph explains why `locally_blocked` is only reached after a specific timing test, so
what is missing is the reasoning for a diagnosis that otherwise accuses the viewer's
antivirus. Same family as [R-12]: text lost to hand-wrapping, in prose this codebase
relies on.

**On the sweep.** I looked for a comment line ending in a dangling function word
followed by a line starting with a capital. It returned **35 hits, of which 32 were
false positives** — ordinary wraps whose next line happens to begin with a proper noun:

```
…state when asked, and the / UI's OSD is driven by…          (fine)
…separated by / U+2605, and without it…                      (fine)
…absent from the / UPDATE list…                              (fine)
…would be a / And a panel on the internet…                   (broken)
```

A 9% hit rate is a bad check, and worth recording as such: it found two real defects only
because 35 results is few enough to read. The lesson matches [R-12]'s — a mechanical
sweep is a way of *narrowing where to look*, and the judgement has to stay with the
reading. Had this returned 350 hits I would have believed the summary and found nothing.

### R-78 — Medium — typing a radio station's number on the keypad finds nothing

`aurora-app/src/commands.rs:83` (`channels_by_number`)

```rust
let Some(id) = channels::find_by_number(&db, args.number)? else { return Ok(None) };
Ok(channels::list(&db, &channels::ChannelFilter::default())?
    .into_iter()
    .find(|c| c.id == id))
```

`find_by_number` finds the channel. Then the row is fetched by listing **every** channel
through the default filter and searching it — and `ChannelFilter::default()` has
`radio_only: false`, which `channels::list` turns into `AND channels.is_radio = 0`. So a
radio station is found and then discarded.

Demonstrated with one TV channel and one radio station:

```
keypad 101 (a TV channel)
   find_by_number        -> Some(1)
   commands.rs:83 result -> Some("BBC One")
   channels::get result  -> Some("BBC One")
keypad 198 (a radio station)
   find_by_number        -> Some(2)
   commands.rs:83 result -> None            <- found, then thrown away
   channels::get result  -> Some("BBC Radio 4")
```

**This exact bug is already documented and already fixed elsewhere.**
`repo/channels.rs:142` introduces `get()` precisely for it:

> It was wrong: `ChannelFilter::default()` excludes radio, so a radio station could not
> be tuned at all, and a recording on a channel the viewer had hidden failed with
> "unknown channel". And it was slow: reading one channel's name sorted and materialised
> the whole table — twenty-two thousand rows on a real subscription, per zap, per
> recording start, and once per row of the Live TV list.

`window.rs:124` says the same in one line — "`channels::get`, not
`channels::list(default)`" — and `commands.rs:182` and `:199` both use `get`. This one
call site was missed, and it carries the second half of the old bug too: it materialises
and sorts every channel row to answer for one, on every keypad entry.

`channels::get(&db, id)` is the whole fix, and the hidden-channel half comes with it.

### R-79 — Info — one NULL in `categories` would empty the whole guide

`aurora-db/src/repo/epg.rs:95` (`map_row`), `aurora-db/src/schema.rs:96`

```rust
let categories: String = r.get(7)?;     // the column is `categories TEXT`, nullable
```

`epg_programmes.categories` has no `NOT NULL` and no default. `map_row` reads it as a
`String`, so a NULL yields `InvalidColumnType` — and because `grid_slice` collects with
`collect::<Result<Vec<_>,_>>()`, that one row fails **the entire query**. The guide would
be blank, not missing one entry.

Not reachable today: `insert_batch` is the only writer and always passes
`serde_json::to_string(&p.categories)`, which is `"[]"` for an empty list. I found it by
accident — a probe of mine inserted rows without the column and the guide query died
with exactly that error, which is a fair demonstration of what a future migration or a
second write path would cause.

`r.get::<_, Option<String>>(7).unwrap_or_default()` costs nothing, and the row beside it
already does `serde_json::from_str(&categories).unwrap_or_default()` — so the function
already intends to tolerate a bad value, and only the NULL case escapes that intent.

### R-80 — Info — verified fine: the guide loads every channel, and that is affordable

`aurora-app/src/commands.rs:106` (`epg_grid_slice`), `GuidePage.tsx:140`

The UI asks with `channelIds: []`, so the host lists **all** channels and queries the
guide for all of them, then the UI filters by group in JavaScript. That looked like the
same shape as the mistake `channels::get` was written to fix ([R-78]), so I measured it
on the real data:

```
6,698 channels, 546,992 programmes

a two-hour window:   channels::list(all)  5.1 ms (6,698 rows)
                     epg::grid_slice      7.9 ms (2,662 programmes, 6,698 placeholders)
                     total               13.0 ms

a six-hour window:   total               15.1 ms
```

Comfortably inside any reasonable budget, and the 6,698 bound parameters are far below
SQLite's 32,766 limit. The `idx_prog_channel_time` index is doing its job. Recorded so
that nobody rewrites this on suspicion — and noting that [R-49] multiplies the programme
count, which is the one thing that would change the answer.

### R-81 — Low — four `_ =>` fallbacks that all fail towards the permissive answer

A cluster, found separately and worth fixing together. Each takes an unrecognised stored
or supplied value and resolves it to the *least* restrictive reading:

| where | unknown value becomes | why it matters |
|---|---|---|
| `aurora-db/src/repo/markers.rs:51` | `MarkerSource::Learned` | `Learned.may_auto_skip()` is true, so the app skips without asking ([R-45]) |
| `aurora-db/src/repo/profiles.rs:59` | `max_age: None` | no age limit at all, on a kids profile ([R-58]) |
| `aurora-db/src/repo/dvr.rs:25` | `RecordingState::Scheduled` | a corrupt `recording` row is started again |
| `aurora-app/src/commands.rs:723` | `progress::ItemKind::Movie` | recording #5's position would be written as movie #5's |

The last is not reachable today — `useWatchProgress.ts:22` guards with
`if (p.itemKind !== 'movie' && p.itemKind !== 'episode') return false`, so only those two
ever arrive. But `watch_progress`'s primary key is `(profile_id, item_kind, item_id)`, so
if a future caller did send `recording`, it would silently overwrite the progress of the
film with the same id — which is the sort of corruption nobody traces back.

Three of the four are one-line changes to the restrictive variant. The fourth wants a
rejection rather than a default, since there is no safe guess for "which table is this id
in".

### R-82 — Medium — `library.refreshed` is emitted the one way Tauri rejects, and the
test written to catch that does not list it

`aurora-app/src/providers.rs:929`, `aurora-app/src/lib.rs:102` and `:139`

`lib.rs` has a long doc comment on `emit()` explaining a bug that made **every** host
event undeliverable:

> Tauri 2 rejects any event name containing a `.` … So `emit` returned
> `Err(IllegalEventName)` every single time. … Every call site was written
> `let _ = app.emit(…)`, on the reasonable-sounding grounds that a dropped event must
> never fail the work that produced it. The result was that a *systematically* dropped
> event looked exactly like a quiet one.

The fix was `aurora_app::emit`, which translates `player.state` into `player:state` and
logs a failure instead of discarding it. Seventeen call sites use it.

One does not:

```rust
let _ = app.emit(
    "library.refreshed",
    &serde_json::json!({ … }),
);
```

Direct `app.emit`, dotted name, result discarded — the original bug exactly, in the file
that calls `emit()` correctly four lines earlier for `ingest.progress`.

**And the guard test cannot see it.** `lib.rs:139`'s `EVENTS` list exists so that "a new
one added without a line here is a new one nobody has checked". It has 10 entries; 13
events are emitted:

```
emitted but NOT in the EVENTS list: library.refreshed, series.listings, series.listingsDone
listed but not emitted:             (none)
```

So the two tests that assert every event survives translation never see the one event
that does not get translated.

**Current impact is small**, which is why it has gone unnoticed: `shared/ipc.ts:1516`
declares the payload but nothing in `src-ui` subscribes to `library.refreshed` — grepped,
the only occurrence is the type declaration. So an event nobody listens for is failing to
send. The defect is that the mechanism and its test both have a hole in exactly the same
place, and the next event added this way will matter.

I found this by reading `lib.rs`'s `EVENTS` list and noticing three events missing from
it. My first mechanical sweep for direct dotted emits returned **zero** — the regex was
single-line and the event name sits on the line after `.emit(`. Third time in this review
that a one-line regex has missed a real defect ([R-12], [R-77]); the multi-line version
found it immediately.

### R-83 — Medium — the episode sweep re-asks shows it cannot list, up to 200 times

`aurora-app/src/series.rs:130` (`sweep_in_background`), `:65` (`sweep`),
`aurora-db/src/repo/library.rs:505` (`series_needing_episodes`)

The sweep's queue is "shows with no episodes stored":

```sql
WHERE series.hidden = 0 AND NOT EXISTS (SELECT 1 FROM episodes e WHERE e.series_id = series.id)
ORDER BY … LIMIT ?1
```

and its loop stops when `batch.asked == 0`. `asked` counts shows *attempted*, not shows
*resolved*. So a show that the provider will not list — `fetch_episodes` returning `Ok(0)`,
or failing — writes no episodes, stays in the queue, and is returned by the very next
batch. Since the ordering is deterministic, the tail of a sweep re-asks the same
unanswerable remainder on every pass, until `SWEEP_MAX_BATCHES` (200) runs out.

With N unanswerable shows left, that is up to **200 × N requests to the viewer's own
panel** — the same host their video comes from, on a line that ([R-32]) answers HTTP 555
when it has one connection too many.

**What I measured, and what I did not.** I sampled 25 of this subscription's 8,086 shows
through `get_series_info` and applied `fetch_episodes`'s own filter (an `id` parseable as
`u32` and an `episode_num`):

```
of 25 sampled shows: 25 usable, 0 with no episodes, 0 with none usable
shows that would make fetch_episodes return Ok(0): 0
```

So on this panel the `Ok(0)` path is rare — I did not observe it once. The *failure* path
is the more likely trigger and I did not try to provoke it: `sweep` counts a provider
error as `failed` and leaves the show in the queue exactly the same way, so any transient
refusal during a sweep puts that show into the re-ask loop.

**The asymmetry is the tell.** Metadata enrichment has a whole table for this —
`enrichment` records `matched` / `nomatch` / `failed` with an `attempted_at`, precisely
so that "we already asked and the answer was no" is not re-asked for ever ([R-59]'s
`QUERY_VERSION` machinery exists on top of it). The episode sweep has no equivalent and
infers its queue from the absence of rows, which cannot distinguish "not asked yet" from
"asked, and there is nothing".

### R-84 — Medium — the recommendation cache stops noticing new viewing after 40 titles

`aurora-app/src/gemini.rs:66` (`Cache::history_len`), `:71` (`watched_for_prompt`),
`:240`

The cache is reused when:

```rust
now - c.generated_at < FRESH_FOR_SECS && c.history_len == watched.len() && …
```

and the field is documented as the mechanism that makes watching something refresh the
rail:

> How many things had been watched when this was asked. A changed count is a changed
> viewer, and worth asking again for.

But `watched` is not the history — it is the prompt:

```rust
history.into_iter().take(HISTORY_FOR_PROMPT)      // HISTORY_FOR_PROMPT = 40
```

So `watched.len()` is `min(history, 40)`, and once a viewer has watched forty things it
is **40 for ever**. From then on `c.history_len == watched.len()` is always true and the
only things that refresh the rail are the six-hour clock and an explicit refresh.

The doc comment on `FRESH_FOR_SECS` leans on this explicitly — "The cache is also keyed
on how much has been watched since, so finishing something gets a new list without
waiting for the clock" — and that stops being true at the fortieth title. A viewer who
finishes a series and goes back to the home screen gets the same rail, built before they
watched it, for up to six hours.

The fix wants a key that keeps changing: the untruncated history length, or the
`updated_at` of the most recent item, or a hash of the top forty titles. The last also
catches *re*-watching, which a count never can.

**Also worth noting, in the same two functions:** `watched_for_prompt` sends
`w.title` straight from the database, which is the stored title with the provider's
language tag still on it. So the model is told the viewer watched "EN Inception" and
"AR الأرجنتين Vs Spain". Resolution afterwards is fine — `find_by_title` goes through
`match_key`, which strips it — so this costs prompt quality rather than correctness, and
`lang::strip_language_prefix` is the same one-call fix used in `library::shown` and
`enrich.rs`.

### R-85 — Low — pressing volume-down at zero shows an error

`src-ui/src/App.tsx:448`, `aurora-app/src/commands.rs:622`

```tsx
onVolume: (delta: number) =>
  invoke('player.setVolume', { volume: (ui.player?.volume ?? 70) + delta })
    .catch(report('Could not change the volume')),
```

No floor. `VolumeArgs::volume` is a `u32`, so at zero the arithmetic sends `-5` and serde
refuses it:

```
volume  10 -> sends {"volume":5}   -> ok, volume 5
volume   5 -> sends {"volume":0}   -> ok, volume 0
volume   0 -> sends {"volume":-5}  -> REJECTED: invalid value: integer `-5`, expected u32
```

`.catch(report('Could not change the volume'))` turns that into a notification, so
holding ArrowDown to mute something ends with an error toast — for an action that had
already finished successfully. The top end is handled: `set_volume` does `volume.min(200)`,
so ArrowUp past the maximum is silently clamped. Only the bottom is missing, and
`Math.max(0, …)` is the whole fix.

The OSD slider at `PlayerOverlay.tsx:301` sends `Number(e.target.value)` from a range
input, which cannot go below its own `min`, so this is reachable only from the keyboard —
which is also the path that cannot see the OSD it is changing ([R-74]).

### R-86 — High — My List ticks never load, and switching profile shows the other
profile's list

`src-ui/src/state/marks.ts:64` (`load`)

```ts
load: async (profileId) => {
  try {
    const marks = await invoke('lists.marks', { profileId });
    // Still the profile that was asked about: switching profile mid-flight must not
    // apply one person's list to another's screen.
    if (get().profileId !== profileId) return;
    set({ myList: new Set(marks.myList), liked: new Set(marks.liked) });
  } catch (e) { … } finally {
    set({ profileId, loaded: true });
  }
}
```

`get().profileId` is the profile whose marks are **currently loaded**, not the one
selected. It is set in the `finally`, *after* the guard. So the guard compares the
request against a value the request itself is about to write, and it is wrong both times
it matters. Transcribed and run:

```
initial:                      profileId null   myList []
after the FIRST load(1):      profileId 1      myList []          <- result discarded
after a SECOND load(1):                        myList [movie:10, movie:7]
after load(2) — switching:    profileId 2      myList [movie:10, movie:7]
```

**Three consequences:**

1. **The first load always throws its answer away.** `profileId` starts `null`, so
   `null !== 1` is true and the function returns before storing anything. `App.tsx:197`
   calls `load` once per `profileId` change, so for the chosen profile that *is* the only
   load — and every plus on every card renders as a plus, every thumb as unset, for
   titles that are on the list. This is the bug the file's own header is about
   ("both buttons existed on three screens and did nothing") in a new form: the state is
   fetched correctly and then dropped.

2. **It works on the second call**, which is why it survives: anything that reloads makes
   it correct, so it is invisible to whoever is testing a flow rather than a cold start.

3. **Switching profile applies the previous profile's list.** `load(2)` while 1 is loaded
   returns early, and `finally` then sets `profileId: 2` while leaving profile 1's
   `myList` and `liked` in place. Profile 2 sees profile 1's ticks — exactly what the
   comment says the guard prevents.

The shell also waits on `loaded` before painting (`App.tsx:522`), and `loaded` is set in
the `finally`, so there is no boot hang — that part is right, and the comment "however it
finished" is accurate.

The fix is to compare against the profile the *app* has selected rather than the one this
store last stored, or to drop the comparison for a request generation counter — the same
pattern `usePages` and `Playback::generation` already use for exactly this race.

### R-87 — Medium — every appearance setting is forgotten on restart

`src-ui/src/state/ui.ts:61`, `src-ui/src/features/settings/SettingsPage.tsx:109`

The Appearance section offers four choices — theme (dark / oled / light / contrast),
interface density (desktop / TV), animations, hover previews — and each handler writes
only to the in-memory store:

```ts
onClick={() => ui.setTheme(t)}          // set({ theme }) and applyDom(...)
onToggle={ui.toggleAnimations}
```

`useUi` is a plain `create<UiState>` with no `persist` middleware, and its initial state
is hardcoded: `theme: 'dark'`, `density: 'desktop'`, `hoverPreviews: true`. Grepped
across `src-ui`: no `persist`, no `localStorage`, and no settings command for any of
them. There is no load at boot because there is nothing to load from.

So picking the OLED theme, or TV density, lasts until the window closes. **Density is the
one that matters**: its own hint says "TV mode enlarges everything for a 10-foot UI", and
a 10-foot UI is what README §1 describes the app as being for. Somebody running Aurora on
a television has to set it on every launch.

Everything else in Settings persists properly — `library.setFilters`,
`timeshift.setSettings`, `metadata.setKey`, `metadata.setConcurrency` all go to the host
— so this is the one panel that does not, and the `settings` table plus
`repo::settings::{get,set}` it would use already exist and are used by those four.
Nothing new is needed on the host beyond a command pair; `localStorage` would also do,
since these are per-machine display preferences rather than library data.

### R-88 — Info — the guide's group list counts radio stations the guide then hides

`aurora-db/src/repo/channels.rs:77` (`groups`), `:128` (`list`)

`channels::groups` filters on `hidden = 0` and the library filter; `channels::list`
additionally appends `AND channels.is_radio = 0` for any filter that is not
`radio_only`. So the guide's group sidebar counts radio stations and the grid beside it
does not — and a group made up only of radio would appear with a count and open onto
nothing.

That is the shape `groups`' own doc comment says it exists to prevent:

> Filtered here as well as in `list`, because these counts are the sidebar: a group
> offering 482 channels that opens onto 120 is two screens disagreeing about what the
> library holds.

It filters by the library filter and not by `is_radio`.

Not reachable on this subscription: `m3u::parse` sets `is_radio` only from an explicit
`radio="true"` attribute, and of the real playlist's 6,038 live entries **zero** carry
one — so there are no radio rows for the two queries to disagree about. Recorded because
the next playlist that marks its radio stations properly will show it, and because the
fix is one clause in a query that already has the rest of them.

### R-89 — Low — the guide's programme lookup has a fallback key that can only ever be
wrong

`src-ui/src/features/guide/GuidePage.tsx:303`

```tsx
const progs = slice?.programmes[ch.epgChannelId ?? String(ch.id)] ?? [];
```

`slice.programmes` is keyed by `epg_programmes.channel_id` — the XMLTV channel id, a
string the provider chose. `String(ch.id)` is the channel's SQLite row id. The two are
different namespaces, so the fallback looks up a key that does not describe the thing
being asked about.

Usually that just returns `[]`, which is the right answer for a channel with no guide
mapping — so it works by accident. The case where it does not is an XMLTV id that happens
to be a bare number equal to some channel's row id: that channel, which has **no** EPG
mapping, would be drawn with whichever channel's programmes sit under that numeric key.

**Measured as not reachable here:** of this guide's 4,827 distinct non-empty channel ids,
**zero** are purely numeric. But the fallback runs for **4,874 of 6,699** live channels —
every one with no `epg_channel_id` — so it is on the hot path for three quarters of the
grid, and it is one numeric provider id away from showing the wrong channel's listings.

`ch.epgChannelId ? slice?.programmes[ch.epgChannelId] ?? [] : []` says what is meant and
cannot collide.

### R-90 — Low — the whole UI re-renders four times a second while anything is playing

`src-ui/src/App.tsx:97`, `:541` (the `handlers` memo), `src-ui/src/hooks/useZapper.ts:45`,
`src-ui/src/features/settings/SettingsPage.tsx:19`

`useUi()` with no selector subscribes to every field of the store. The host's player
heartbeat calls `setPlayer` four times a second (`PLAYER_TICK` is 250 ms), so every
component that does this re-renders at 4 Hz for the whole time something is playing —
including `App`, which is the shell around everything.

App.tsx:494 already documents the trap, for the banner effect:

> `ui` cannot be a dependency. `useUi()` subscribes to the whole store, so every
> unrelated write produces a new snapshot — and while the player is open the host's
> heartbeat writes `setPlayer` four times a second. The effect re-ran on each one…

The lesson was applied to that one effect — `hideBanner` is taken with a selector — and
not to the three remaining whole-store subscriptions. One consequence is visible in the
same file: `handlers` is `useMemo(..., [ui, …])`, so it is rebuilt 4×/s, and
`useHotkeys`'s effect depends on it, so the global `keydown` listener is removed and
re-added four times a second. That is not a correctness bug — cleanup and setup run in
the same task, so no key event can fall between them — but it is a listener churn nobody
intended, and it keeps the whole handler closure graph alive.

Five call sites already use the selector form (`useUi((s) => s.x)`), so the pattern is
established; these three are the ones that were not converted.

### R-91 — Low — Live TV's "now playing" never updates while you are looking at it

`src-ui/src/features/live/useNowNext.ts:63`

The hook remembers what it learned and deliberately never re-asks:

```ts
const asked = useRef<Set<number>>(new Set());
…
const wanted = visibleIds.filter((id) => !asked.current.has(id));
```

which is right for the problem it was written for — one IPC call per row per scroll. But
"now and next" is a question about the clock, and nothing here has a clock. There is no
TTL on `asked`, no timer, and no invalidation of `known`. The only thing that refreshes
it is unmounting the page.

So a viewer who opens Live TV at 17:55 and is still browsing at 18:30 sees the 17:30
programme listed as what is on. `GuidePage` has exactly the tick this needs —
`setInterval(() => setNow(…), 30_000)` with the comment "The 'now' line has to actually
move (README §7.1)" — and Live TV has no equivalent.

Bounded by navigation: the route is lazy, so leaving and coming back remounts the hook
and clears both caches. Programmes run 30–60 minutes, so the window in which it is wrong
is the ordinary length of a browse.

Clearing `asked` and `known` for the *visible* ids on a timer, or stamping each answer
with the `stop` it already carries and dropping it when that passes, both fit the
existing shape — the second needs no timer at all, since `Programme.stop` is already in
the payload.

### R-92 — Info — the artwork answer cache grows for the life of the session

`src-ui/src/hooks/useAssetSrc.ts:23` and `:32`

`answered` and `refused` are module-level and never shrink. `watchWarms` deletes only the
*absences* (`if (!local && !refused.has(url)) answered.delete(url)`), so every URL that
resolved to a local copy is held until the window closes, as are all refusals.

One entry per distinct artwork URL seen. With posters and backdrops that is up to two per
title, so browsing a large library in one sitting accumulates tens of thousands of string
pairs — and [R-75]'s unvirtualised browse grid makes "browsing a lot in one sitting" the
easy case. A few megabytes rather than a leak that matters, and the batching it enables
is worth more than the memory; recorded because nothing bounds it and a `Map` with no
ceiling is the kind of thing that is fine until the library is ten times bigger.

### R-93 — Info — the inlay hook forces a layout read on every scroll event in the page

`src-ui/src/hooks/useInlaidPicture.ts:109`

```ts
window.addEventListener('scroll', push, true);   // capture: every scroll in the document
```

`push` calls `measure`, which calls `getBoundingClientRect` — a synchronous layout read.
With `capture: true` this runs for every scroll event anywhere in the page, including the
guide grid's own continuous scroll, and there is no `requestAnimationFrame` coalescing or
throttle.

The `same()` guard does stop a duplicate `preview.place` from going out, and the guide's
preview `aside` sits outside the grid's scroll container so its rectangle does not
actually move when the grid scrolls — which is why this has not shown up as a stutter.
What remains is a forced layout per scroll event on the hottest scrolling surface in the
app.

The capture listener is there for a real reason, stated in the comment: `ResizeObserver`
does not fire when an element *moves* without resizing. Wrapping `push` in a
`requestAnimationFrame` (one measure per frame at most) keeps that and drops the rest.

### R-94 — Medium — a scheduled recording's countdown can read "in 1h 60m"

`src-ui/src/lib/format.ts:189`

```ts
const h = Math.floor(secs / 3600);
const m = Math.round((secs % 3600) / 60);
if (secs < 86400) return m ? `in ${h}h ${m}m` : `in ${h}h`;
```

`h` is floored and `m` is **rounded**, so the two disagree whenever the leftover is in the
last half-minute of an hour. Measured over every second in the hour-to-day window:

```
secs=7170 -> in 1h 60m
seconds in [3600,86400) that render "Xh 60m": 690
worst case: in 23h 60m
```

690 seconds of each day's worth of offsets — 11.5 minutes in every 24 hours of lead time.
`RecordingsPage.tsx:395` is the only caller, on the scheduled list, so a recording booked
between 1 h 59 m 30 s and 2 h 00 m 00 s away reads "in 1h 60m" until the next tick moves
it on. The 23 h case is worse than cosmetic: "in 23h 60m" should be the `dayLabel` branch.

`runtime()`, three functions above, has the same job and gets it right (`floor` for both).
`Math.floor((secs % 3600) / 60)` is the fix, and it makes `m ? … : …` behave: at
secs=7190 the honest answer is "in 1h 59m", not "in 2h".

### R-95 — Low — "left" with nothing in front of it on a nearly-finished card

`src-ui/src/lib/format.ts:165` with `:138`

```ts
export function remaining(positionSecs: number, durationSecs: number): string {
  const mins = Math.round(left / 60);
  return `${runtime(mins)} left`;   // runtime(0) === ''
}
```

`runtime` returns the empty string for zero — deliberately, so a title with no known
runtime renders nothing — and `remaining` concatenates it anyway:

```
20s left -> " left"
29s left -> " left"
30s left -> "1m left"
```

Rendered at `CatalogCard.tsx:350`, under the progress bar on a Continue Watching card.

Reachable range, from the rule that puts the row there: `progress.rs:163` keeps a row
while `completed = 0`, and `save` sets `completed` at `EPISODE_COMPLETE_RATIO = 0.95` /
`MOVIE_COMPLETE_RATIO = 0.92`. So a card is on the rail only while more than 5% (episode)
or 8% (movie) is left, and fewer than 30 seconds of that needs a duration under 600 s or
375 s respectively. Short items only — not measured against the catalogue, so the bound is
from the code, not from the data. Worth a floor of "<1m left" regardless, since `runtime`
cannot distinguish "zero minutes" from "unknown" and `remaining` has no business asking it
to.

### R-96 — Low — `bytes()` prints "1024 KB" and "1024 B"

`src-ui/src/lib/format.ts:175`

The carry loop tests `v >= 1024` before the rounding that `toFixed`/`Math.round` then
applies, so a value just under a unit boundary carries after it is too late:

```
1048575      -> 1024 KB     (should be 1.0 MB)
1023.6       -> 1024 B
1073741824   -> 1.0 GB      (correct)
```

Shown on the recordings storage line (`RecordingsPage.tsx:180`), the artwork cache
(`MetadataPanel.tsx:232`) and the timeshift buffer (`TimeshiftPanel.tsx:125`). The window
is narrow — the last 0.05% of each unit — but the storage line is read while deciding
whether there is room, which is exactly when a number one unit too small is unhelpful.

### R-97 — Low — a second notice restarts the first one's dismiss timer

`src-ui/src/components/NoticeStack.tsx:20`

```ts
useEffect(() => {
  const timers = notices.map((n) => window.setTimeout(() => dismissNotice(n.id), DISMISS_AFTER_MS));
  return () => timers.forEach((t) => window.clearTimeout(t));
}, [notices]);
```

The effect depends on the whole `notices` array, so every change tears down **all** timers
and starts them again from zero. A toast that has been up for seven of its eight seconds
goes back to eight the moment another arrives.

`errors.ts` caps the stack at three and dedupes by title, so this cannot grow without
bound — but it does not bound the *time*. A sequence of distinct failures arriving more
often than every eight seconds, which is what a provider refusing artwork during a browse
produces, keeps the oldest toast on screen for as long as the sequence lasts.

Each notice owning its own timer — keyed on `n.id`, which already exists and is stable —
makes the eight seconds mean eight seconds.

### R-98 — High — the detail modal reads a film's resume position out of the *mock*, and the mock has fabricated rows in it

`src-ui/src/components/DetailModal.tsx:9` and `:130`

```ts
import { getProgress, invoke } from '@/ipc';
...
const prog = item.kind === 'movie' ? getProgress('movie', item.id) : null;
```

`getProgress` is not a command and not a transport. It is declared in exactly one place —
`src-ui/src/ipc/mock.ts:2516` — and reaches `@/ipc` through `export * from './mock'`
(`ipc/index.ts:125`). It reads a module-level `Map` that only the mock's own command
handlers write (`mock.ts:1956`, `:2016`), and `invoke()` routes to those handlers only
when `__TAURI_INTERNALS__` is absent (`ipc/index.ts:56`).

`DetailModal` is the only production file that imports a mock state reader; every other
one of the 37 files importing from `@/ipc` takes `invoke` and the event subscriptions.

So on Windows the map is never written. **But it is not empty**, because `mock.ts:94`
seeds it at module-evaluation time:

```ts
const progress = new Map<string, Progress>();
for (const p of fx.seedProgress) { ... progress.set(`${p.kind}:${p.id}`, { ... }); }
```

`ipc/index.ts` imports `./mock` statically, so that loop runs in every build, release
included. `fixtures.ts:407` seeds `movies[3]`, `movies[11]`, `movies[27]`, `movies[52]`
and `makeMovie(i + 1)` gives `movies[i].id === i + 1`, so the seeded keys are
`movie:4, movie:12, movie:28, movie:53` and `episode:3, episode:41`.

Measured, with a Tauri host present and the mock transport therefore unreachable:

```
isNativeHost() === true
  movie:1   -> null
  movie:4   -> {"itemKind":"movie","itemId":4,"positionSecs":3325,"durationSecs":9780,...}
  movie:12  -> {"itemKind":"movie","itemId":12,"positionSecs":3706,"durationSecs":5220,...}
  movie:28  -> {"itemKind":"movie","itemId":28,"positionSecs":1087,"durationSecs":9060,...}
  movie:53  -> {"itemKind":"movie","itemId":53,"positionSecs":3422,"durationSecs":7440,...}
  movie:100 -> null
fabricated movie rows visible on a native host: [4,12,28,53]
```

Two consequences, both of them wrong in the shipped app:

1. **For whichever four real films hold rowids 4, 12, 28 and 53** — and with 27,494 films
   imported, they all exist — the modal draws a progress bar, prints "55:25 of 2:43:00"
   (`:214`) and labels the button "Resume from 55:25" (`:222`) for a title the viewer has
   never opened. The duration shown is the *fixture* film's runtime, unrelated to the real
   one.
2. **For every other film**, `prog` is null, so the progress bar at `:210` and the resume
   label at `:221` never appear at all — however much of it the viewer has actually seen.
   The file's own header comment advertises a "resume CTA"; for films it is dead.

Pressing the button is still correct: `App.tsx:261` calls `resumeAt`, which goes through
the real `progress.get` command (`App.tsx:72`). So the button's label and the bar above it
disagree with what pressing it does — in both directions.

This is precisely the failure mode `ipc/index.ts:44` warns about in prose ("98 green
journeys the whole time"): in a browser the mock *is* the transport, so every end-to-end
journey sees a working resume CTA, and no amount of running them can show this.

The fix is `useCommand('progress.get', { profileId, kind: 'movie', id })` — the command
exists, is typed, and is already called from `App.tsx` — and then deleting `getProgress`
from `mock.ts` so nothing can reach for it again. `isInMyList` and `isFavorite` beside it
(`mock.ts:2510`, `:2513`) are the same hazard with no current caller.

### R-99 — Medium — "% match" is a mock-only field, so it never appears in the real app

`src-ui/src/components/DetailModal.tsx:337`, `shared/ipc.ts:62`

```ts
/** Percentage match computed locally from viewing history (README §8.4). */
match?: number;
```

It is computed nowhere. The only assignments in the repository are `fixtures.ts:299` and
`:328` (`match: int(72, 99)`), and `MovieRow` / `SeriesRow` in
`aurora-db/src/repo/library.rs:10` have no such field, so serde cannot produce one.
Confirmed in a browser against the mock — the dialog shows one "% match" line — and by the
absence of any producer on the host side.

`item.match !== undefined` therefore guards a branch that is dead in every real build.
Same class as [R-98] and the same reason it survived: the screenshots and the journeys
both run against the fixtures.

Either compute it — the recommender already builds a profile from history
(`aurora-core/src/recommend.rs`), so the number is derivable, though see [R-37] for the
state of that scoring — or delete the field, the fixture values, and the branch. What it
must not stay is a documented feature that exists only in the mock.

### R-100 — Low — every title claims closed captions

`src-ui/src/components/DetailModal.tsx:361`

```tsx
<Badge tone="neutral">CC</Badge>
```

Unconditional, with no backing data anywhere: nothing in the schema, the playlist parse or
enrichment records whether a title has subtitles. Confirmed rendered once per dialog.

Xtream's `get_vod_info` does return a subtitle list per title, so this is answerable — but
until it is asked, a badge asserting captions on all 27,494 films is a claim the app
cannot support. The honest options are to drop it or to key it on something real.

### R-101 — Medium — a show's resume label survives onto the next show

`src-ui/src/components/DetailModal.tsx:98`

```ts
useEffect(() => {
  if (item.kind !== 'series') { setLabel(null); return; }
  let live = true;
  void (async () => {
    ...
    const ep = eps.find((e) => e.id === point.episodeId);
    if (!live || !ep) return;                 // no setLabel(null)
```

The effect never clears `label` on the way *in* for a series, and the `!ep` path returns
without clearing it either. `DetailModal` is one mounted component whose `item` prop
changes, so `Hero` is not remounted between titles.

Two visible results:

* Closing a show you are partway through and opening a different one shows the **first
  show's** label — "Resume S02E05 from 41:12" — on the second show's Play button for as
  long as the two commands take. The button is live during that window.
* If the resume point names an episode the listing does not contain (the host answers
  `progress.resumePoint` from `watch_progress`, which carries no foreign key — see
  `repo/progress.rs:134` on the deliberate absence of a cascade), the stale label never
  clears at all.

`setLabel(null)` as the first statement of the effect fixes both. The same effect gets the
cancellation right (`live`), which is what makes the omission look like an oversight rather
than a choice.

### R-102 — Medium — `aria-modal="true"` on three dialogs, none of which hold focus

`src-ui/src/components/DetailModal.tsx:66`, `features/search/CommandPalette.tsx:128`,
`features/mosaic/MosaicPage.tsx:414`

No focus trap, no initial focus, no `inert`/`aria-hidden` on the background, and no focus
restoration on close. A sweep for `focus()`, `activeElement` and `inert` across `src-ui`
returns three initial-focus calls (`CommandPalette.tsx:67`, `SetupWizard.tsx:309`,
`Primitives.tsx:422` — the last is a `Select` filter, not a dialog) and nothing else.

Measured in Chromium, opening the detail modal from a rail card:

```
focus right after opening: DIV[The Silent Tide, 2019] inDialog=false
tab trail from the open modal:
  BUTTON[Play The Silent Tide]            inDialog=false
  BUTTON[Add The Silent Tide to My List]  inDialog=false
  BUTTON[I like The Silent Tide]          inDialog=false
  BUTTON[More info]                       inDialog=false
  DIV[The Velvet Circuit, 2025]           inDialog=false
  BUTTON[Play The Velvet Circuit]         inDialog=false
  ... (12 of 12 outside the dialog)
of 12 tabs, landed outside the dialog: 12
focus after Escape: BUTTON[Add [SPANISH] La Casa del Lago to My List]
```

Every one of twelve Tab presses lands on a control **behind the scrim** — including Play
buttons on other titles, which are reachable by keyboard and will start playback while a
modal is over them. Nothing in the dialog is reachable by Tab at all from the opening
state. After Escape, focus is left wherever the walk ended rather than on the card that
opened the modal, so a keyboard user loses their place in the rail.

The palette is no better, despite focusing its input:

```
palette tab trail:
  BODY inDialog=false
  A    inDialog=false   (the nav rail)
  ... 8 of 8 outside the dialog
```

`aria-modal="true"` is an assertion to assistive technology that the rest of the document
is unavailable. Here it is false, and a screen-reader user is told the background is inert
while the keyboard walks straight into it. One shared hook — focus the dialog on mount,
cycle Tab within it, restore the opener on unmount — covers all three, and the opener is
already in hand at each call site.

### R-103 — Low — a per-show preference that fails to save stays switched on

`src-ui/src/components/DetailModal.tsx:486`

```ts
const update = (patch: Partial<SeriesPrefs>) => {
  const next = { ...prefs, ...patch };
  setLocal(next);
  invoke('library.setSeriesPrefs', { profileId, seriesId, prefs: next })
    .catch(report('Could not save that preference'));
};
```

`setLocal` runs first and nothing undoes it. On a rejection the viewer gets a toast *and*
a switch showing the state that was not saved, and `local` shadows `data` (`:480`) until
`seriesId` changes (`:482`) — so re-opening the same show from the same modal session
keeps showing the wrong value.

The optimistic update is right; `.catch` should put `prefs` back as well as report. These
three toggles are what the player's skip logic reads, so the divergence is not only
cosmetic: "Always skip intros" can look enabled for a whole viewing session while the host
has never been told.

### R-104 — High — the "Details" tab is five invented rows, identical for all 27,494 films

`src-ui/src/components/DetailModal.tsx:788`

```ts
const rows: [string, string][] = [
  ['Title', item.title],
  ['Year', String(item.year ?? '—')],
  ['Genres', item.genres.join(', ') || '—'],
  ['Certification', item.certification ?? '—'],
  ['Quality', item.quality ?? '—'],
  ['Container', 'matroska (mkv)'],
  ['Video', 'HEVC Main 10 · 3840x2160 · 23.976 fps'],
  ['Audio', 'E-AC-3 5.1 @ 640 kbps · AAC 2.0'],
  ['Subtitles', 'SRT (eng), ASS (eng SDH)'],
  ['Source', 'example.com'],
];
```

The first five come from the item. The last five are string literals. Every title in the
library — all 27,494 films and 8,086 shows — reports the same container, the same codec,
the same resolution, the same frame rate, the same audio layout, the same subtitle tracks,
and `example.com` as its source.

Worse than [R-100]'s single CC badge for three reasons:

* It is a whole panel, under a heading that says **Details**, which is exactly where a
  viewer looks to find out whether a stream is actually 4K before committing to it.
* The five real rows sit immediately above the five invented ones in the same `<dl>`, with
  identical styling, so nothing distinguishes the data from the placeholder. `Quality`
  comes from the playlist and can legitimately read `SD` on the line above a `Video` row
  claiming 3840x2160.
* `Source` says `example.com` on a screen that exists to tell the viewer where a title
  came from — and the app does know: `provider_key` is on the row and
  `library.alternates` already surfaces `a.provider` two sections up (`:301`).

Partly answerable, partly not. `Source` and `Container` are derivable now. Resolution,
codec, frame rate, audio layout and subtitle tracks are known only once mpv has the file
open — `aurora-player` holds the handle and mpv exposes `video-params`, `audio-params` and
`track-list` — so for an unplayed title the honest answer is to omit the row rather than
to invent it. Xtream's `get_vod_info` also returns much of this per title.

### R-105 — High — "More Like This" shows the same nine films for every title

`src-ui/src/components/DetailModal.tsx:762`

```ts
function Similar() {
  const { data } = useCommand('library.movies', { sort: 'rating', limit: 9, offset: 12 }, []);
```

`Similar` takes no props. It does not receive the item, so it cannot be about it. The query
is a constant — the highest-rated films, skipping the first twelve, nine of them — with an
empty dependency array, so it is literally the same nine titles under every title in the
library.

It is also the **default tab for films**: `:41` sets `tab` to `'similar'` for anything that
is not a series. So the first thing the detail view shows for any film is a grid of nine
unrelated titles presented as being like it. For a *series*, the same tab shows films.

The `offset: 12` is the tell — it is there so the grid does not repeat whatever the home
screen's top-rated rail is showing. That is a layout concern standing in for a missing
feature.

There is no item-to-item similarity capability on the host: `library.recommended`
(`shared/ipc.ts:946`) is profile-based, and nothing in `aurora-core/src/recommend.rs` or
`aurora-db/src/repo/recommend.rs` answers "like this one". But the honest minimum needs no
new capability — `library.movies` already accepts `genre` (`shared/ipc.ts:951`), and the
item's genres are in hand at the call site. Passing `genre: item.genres[0]`, dropping
`offset`, and filtering out `item.id` turns a fabrication into a defensible answer in one
line.

### R-106 — Medium — an episode's watch-progress bar is invisible unless it has artwork

`src-ui/src/components/DetailModal.tsx:691`–`:716`

```tsx
<div style={{ position: 'relative', borderRadius: 'var(--r-sm)', overflow: 'hidden' }}>
  {ep.still && <img ... />}
  <div style={{ position: 'absolute', inset: 0, ... }}><Icon name="play" ... /></div>
  {partway > 0 && <div style={{ position: 'absolute', left: 0, right: 0, bottom: 0, height: 3, ... }}>
```

The `<img>` is the only in-flow child. Both of the others are `position: absolute`, so
when `ep.still` is null the container has no intrinsic height at all — it is a grid cell
148px wide and 0px tall, with `overflow: hidden` clipping everything inside it.

So for any episode without a still:

* the play affordance over the thumbnail does not appear, and
* **the resume bar does not appear either** — `partway` is computed correctly at `:659`
  from real host data (`progress.forSeries`), rendered, and then clipped to nothing.

That is the part that matters. The episode list is where you find the one you were partway
through, and the only indication of it other than the tick is this bar.

Episode stills come from TMDB enrichment and nothing else, so `ep.still` is null for every
episode of every show until a TMDB key is configured and the metadata sweep has reached
that show — which is the state a fresh library is in, and the permanent state for the
shows enrichment never matches.

Derived from the layout rather than measured in a browser: the fixtures all carry stills,
so this does not reproduce against the mock. An `aspectRatio: '16/9'` and a background on
the container — which it needs anyway, as an empty 148px gap is its own small defect —
gives the box a height whether or not the image arrives.

Beside it, in the same rows: the hover highlight at `:666` is set imperatively on
`onMouseEnter`/`onMouseLeave` with no `onFocus`/`onBlur` counterpart, so an episode row
never highlights under keyboard navigation. Minor on its own, and the same omission as
[R-102] seen from a different angle.

### R-107 — Info — opening any film asks the host for episode progress on series 0

`src-ui/src/components/DetailModal.tsx:37`

```ts
const watch = useSeriesProgress(item?.kind === 'series' ? item.id : 0);
```

`useSeriesProgress` fires `progress.forSeries` from an effect with no guard on the id
(`:587`, `:596`), so every film opened costs one round trip asking which episodes of
series `0` have been watched. The answer is empty and harmless, and `setVersion` then
re-runs `useSeriesResume`, which returns immediately for a non-series.

One wasted command per film opened, which on a library browsed with the keyboard is a
steady trickle. Recorded rather than ranked: the pattern — a hook called unconditionally
with a sentinel id — is the same one `useCommand`'s fourth `enabled` argument exists to
avoid, and `useEpisodes` two functions down (`:546`) already uses it correctly.

### R-108 — High — `providers.max_connections` is never written, so every connection budget in the app is wrong

`src-native/crates/aurora-app/src/providers.rs:609` and `:719`

The column exists (`aurora-db/src/schema.rs:27`), is read in three places, and is **written
by no production code path**. Every `INSERT INTO providers` / `UPDATE providers` in the
workspace:

```
providers.rs:609   INSERT INTO providers (id, name, kind, base_url, username,
                                          credential_ref, enabled, created_at)
providers.rs:719   UPDATE providers SET name = ?2, kind = ?3, base_url = ?4, username = ?5
providers.rs:748   UPDATE providers SET credential_ref = NULL WHERE id = ?1
providers.rs:759   UPDATE providers SET credential_ref = ?2 WHERE id = ?1
providers.rs:913   UPDATE providers SET last_refresh_at = ?2 WHERE id = ?1
```

None of them mentions it. The only statement in the repository that does is a unit-test
fixture (`aurora-app/src/mosaic.rs:892`). So `max_connections` is NULL for every provider
on every real install, permanently.

The value is not unavailable — it is fetched and then discarded. `aurora-ingest/src/xtream.rs:142`
parses `user_info.max_connections` out of the `player_api.php` auth response into
`ProviderStatus`, and `providers.rs:488` copies it into the `ValidationResult` sent to the
UI. It is displayed once, during setup, and never stored.

Three things read the column, and all three therefore get the wrong answer:

**1. The DVR runs two simultaneous recordings on every account.**

```rust
// services.rs:134
/// The tightest connection limit across enabled providers, which is what actually caps
/// simultaneous recordings — exceeding it gets the line cut, not queued.
fn max_connections(db: &Connection) -> Option<usize> {
    db.query_row("SELECT MIN(max_connections) FROM providers
                  WHERE enabled = 1 AND max_connections > 0", ...)
```

NULL rows fail `max_connections > 0`, so `MIN` is NULL, so the function returns `None`,
so `services.rs:89` falls back to `DEFAULT_MAX_CONCURRENT = 2`. On this panel —
`max_connections: 1`, and HTTP **555** is what it answers when the limit is exceeded, as
established earlier in this review — the second concurrent recording cannot work. The
comment above the query describes exactly the failure its own result guarantees.

The `Skipped` state and the "too many recordings at once for this subscription" reason at
`dvr.rs:240` are unreachable on a one-connection line: `busy >= 2` is never true before
the provider has already cut the line.

**2. The warning strip written for a one-connection line never appears on one.**

`ConnectionBudget.tsx:194` takes `allowed` from `dvr.storage.maxConcurrent`, which is
`services.dvr.max_concurrent()` — 2 — and returns null unless `busy >= allowed`. With one
recording in flight on a single-stream account, which is the precise case its own header
comment is about ("on a single-stream account a recording in flight means the next thing
you press cannot play, and until now the only way to find that out was to press it and
watch one of the two die"), `1 >= 2` is false and the strip stays hidden.

Its closing line — "Visible only while it is true, which on a two-stream account with one
recording is never" — is accidentally true of every account.

**3. Multi-view offers six tiles on a line that allows one.**

`aurora-app/src/mosaic.rs:228` runs the same query, and `Budget::check` maps `None` to
`Budget::Unknown` (`aurora-core/src/mosaic.rs:208`). `is_refused()` is false for
`Unknown`, so no layout is ever refused and `largest_fitting` always returns the largest.
`aurora-core/src/mosaic.rs:16` names this very line as the case that makes every layout
impossible.

The comment above the query is also false:

```rust
// aurora-app/src/mosaic.rs:224
/// The same query `Services` uses to cap simultaneous recordings, asked again here
/// rather than cached: a provider's limit is rewritten by every refresh that reads
/// `player_api.php`, and a mosaic opened after one should use the new number.
```

No refresh rewrites it. Asking again costs a query and returns NULL every time.

**Why it was never noticed.** `fixtures.ts:396` sets `maxConnections: 2` and `mock.ts:1200`
filters on it, so in the browser the whole budget mechanism works on declared limits and
every journey exercises the `Fits`/`Exceeds` paths. On Windows it is `Unknown` forever.
Same family as [R-98] and [R-99]: the mock supplies a field the host does not.

Two further consequences worth separating out:

* `repo/providers.rs:51` hardcodes `active_connections: None`, and `SettingsPage.tsx:69`
  guards on `p.maxConnections != null`, so the "0/2 connections" line in Settings is dead
  code for a second reason as well as the first.
* Even once the column is written, `services.rs:89` reads it **once**, in `Services::new`
  at `main.rs:181`, and nothing recomputes it. On a first run there is no provider at all,
  so the DVR would still be capped at the default for the whole of the session in which
  setup happens. `Mosaic::limit()` re-queries per check and is right to; `Dvr` needs the
  same, or `max_concurrent` has to become a lookup rather than a field.

The fix is small and in one place: carry `status.max_connections` from the validation that
`providers_add` already performs into the INSERT, write it again on each refresh beside
`last_refresh_at` at `:913`, and make the DVR read the limit per tick.

### R-109 — Medium — "Record what's on" stores a window it did not record, and the comment explaining why is wrong

`src-ui/src/features/player/PlayerOverlay.tsx:61`

```ts
// Start from the beginning of the programme: the buffer may well hold the part
// already shown, and a recording that begins where you happened to press the
// button is half a programme.
airStart: programme?.start ?? now,
airStop: programme?.stop ?? now + UNGUIDED_RECORDING_SECS,
```

The timeshift buffer is not consulted. `RecordRequest` (`aurora-ingest/src/recorder.rs:28`)
carries `url`, `dest`, `window_secs`, `user_agent`, `referrer` — nothing about a buffer —
and `dvr.rs:350` fills it with `window_secs: (rec.stop - now).max(1)`, a fresh HTTP stream
from the moment the recording starts. The recorder has no reference to the timeshift
buffer at all.

So pressing Record fifty minutes into an hour-long programme produces a ten-minute file,
stored with `air_start` at the programme's start. `RecordingsPage.tsx:275` then renders it
as `20:00–21:00`. The stored `duration_secs` is honest — `dvr.rs:166` computes
`(now - started_at)` — so the same card shows a ten-minute duration against a one-hour
window, with nothing to explain the gap.

The scheduling itself works: `repo::due` picks the row up on the next tick because
`start <= now`, and `abandon_missed` only fails rows whose `stop` has also passed. It is
the recorded extent and the stated extent that disagree.

Either record from `now` and say so, or say in the notice how much of the programme has
already gone — "Recording the last 10 minutes of Match of the Day" is an honest sentence
and the current one is not. The buffer *could* be prepended, which is what the comment
imagines, but that is a feature and not a comment.

Note also that on a one-connection line this button starts a second connection while the
viewer is watching, which the provider answers with 555 — and [R-108] is why nothing
warns about it here.

### R-110 — High — every button on the player's error panel is inert

`src-ui/src/features/player/PlayerOverlay.tsx:809`

```tsx
{error.actions.map((a) => (
  <button
    key={a}
    style={{ ... }}
  >
    {LABELS[a] ?? a}
  </button>
))}
```

No `onClick`. Not a no-op handler, not a disabled attribute — no handler at all. All four
actions the host can offer are drawn as buttons and do nothing when pressed:

```
retry            -> 'Retry'
tryAnotherSource -> 'Try another source'
reportBroken     -> 'Report stream as broken'
openSettings     -> 'Open settings'
```

This is the panel whose own doc comment reads *"README §17: a human sentence, a likely
cause, and at least one action."* It delivers the sentence and the cause and then fails on
the third. It is also the worst place in the app for it: the panel only appears when a
stream has died, so the one screen a viewer reaches while something is already wrong
offers them four ways out and honours none.

The actions are not decorative in the host's model. `aurora-core/src/neterr.rs:293`–`:308`
chooses them per failure class, with care — there is a comment at `:300` explaining why a
400 gets `TryAnotherSource` but *not* `ReportBroken`, "the source is not what is wrong."
That reasoning is then rendered as four identical dead buttons.

Three of the four are implementable with commands that already exist:

* **Retry** — `channels.tune` for live, `player.play` for VOD. Both are what the viewer
  would otherwise do by hand.
* **Try another source** — the data and the policy are both already built.
  `channel_sources` carries `priority`, `fail_count`, `last_ok_at` and `last_fail_at`
  (`aurora-db/src/repo/sources.rs`), `aurora_core::sources::rank` orders them, and
  `playback.rs:447 recover()` walks them automatically up to `MAX_ROLLOVERS`. The panel
  appears once that walk is spent — and `recover`'s own comment says what the button
  should do: *"The next deliberate tune re-reads the order, by which time the failures
  just recorded will have moved the good one to the front."* So re-tuning is a real
  action with a real chance of working, not a placebo.
* **Open settings** — `navigate('/settings')`; the router is already in the tree.

`reportBroken` has no host command behind it, so it is the one that needs either an
implementation or removal from the vocabulary.

The structural reason it was missed is visible in the signature: `ErrorPanel({ error })`
takes nothing else, so it has no callback to call. Its parent has `onClose` and `onGuide`
passed in and `invoke` imported, and every other control in the file calls `invoke`
directly — so this is the one component in `PlayerOverlay.tsx` that could not act even if
it wanted to.

Third instance of this exact pattern in the codebase, and the file documents the other
two: `PlaybackPanel`'s header (`:640`) — *"`player.setSpeed` and `player.setAspect` are
implemented on the host … and no screen had ever called either. The button that should
have opened them had no handler"* — and `tests-e2e/lists.spec.ts`'s header on My List. A
sweep for `<button` and `<IconButton` with no `onClick` is worth doing as a standing check;
see the methodological note under [R-12].

### R-111 — Low — the scrub handle can stay stuck where it was dropped

`src-ui/src/features/player/PlayerOverlay.tsx:425`

```ts
useEffect(() => {
  if (dragged === null || committedAt.current === 0) return;
  const arrived = Math.abs(value - dragged) <= SEEK_SETTLED_SECS;
  const gaveUp = Date.now() - committedAt.current > SEEK_SETTLE_TIMEOUT_MS;
  if (arrived || gaveUp) { setDragged(null); committedAt.current = 0; }
}, [value, dragged]);
```

`SEEK_SETTLE_TIMEOUT_MS` is checked only when the effect re-runs, and the effect re-runs
only when `value` or `dragged` changes. `committedAt` is a ref, so `commit()` causes no
render of its own.

So the four-second escape hatch depends on the host continuing to report a *changing*
position — and `playback.rs:335` deliberately suppresses an emit when the state is
identical to the last one:

```rust
let mut last = self.last.lock();
if last.as_ref() == Some(&state) { return None; }
```

When the position stops moving and the seek did not land, nothing re-runs the effect,
`gaveUp` is never evaluated, and the handle sits at `dragged` indefinitely while the host's
real position is ignored. The display is then wrong in a way that only another drag clears.

Narrow in practice: a live stream's position keeps advancing, and a successful seek
satisfies `arrived` on the next tick. The reachable case is a seek that is refused or
ignored while the position is static — a paused film, or a stream that has stopped
delivering — which is a fair description of when somebody scrubs hard enough to care.

The comment above `SeekBar` names the exact scenario the timeout is for ("A seek can be
refused"), so the intent is right and only the trigger is wrong. A `window.setTimeout`
armed in `commit` and cleared on arrival does not depend on the host saying anything.

**The sweep [R-110] asks for, run.** Every `<button>`, `<Button>` and `<IconButton>` in
`src-ui/src/**/*.tsx`, brace-balanced to the end of its opening tag, with no `onClick`,
`onPointerDown`, `onMouseDown`, `type="submit"` or spread props:

```
candidate interactive elements with no handler: 2
  src-ui/src/features/live/LivePage.tsx:361      <button>        (false positive)
  src-ui/src/features/player/PlayerOverlay.tsx:810  <button key={a} ...>
```

`LivePage.tsx:361` is the word `<button>` inside a comment — *"A div with a button role
rather than a `<button>`"*. So one real hit, and it is [R-110]. Unlike the sweep recorded
under [R-12], this check has been shown to catch a known instance before being trusted:
the `PlayerOverlay` line was found by reading, and the sweep found it independently.

That leaves the inert-control defect confined to [R-110] among plain handlers. It does not
cover controls that *have* a handler which does nothing useful — My List's
`onClick={(e) => e.stopPropagation()}`, the shape `tests-e2e/lists.spec.ts` was written
for — and no mechanical check will.

### R-112 — Medium — on Live TV a letter filter stays applied after the bar that set it is disabled

`src-ui/src/features/live/LivePage.tsx:87` with `:331`

The A–Z bar is greyed out unless the list is in alphabetical order:

```tsx
<LetterBar selected={letter} onSelect={setLetter} disabled={order !== 'name'} />
```

but the filter it set is applied regardless of `order`:

```ts
if (letter) {
  const first = (c: Channel) => c.name.trim().charAt(0).toUpperCase();
  rows = letter === '#' ? rows.filter((c) => first(c) < 'A' || first(c) > 'Z')
                        : rows.filter((c) => first(c) === letter);
}
```

So: press **A–Z**, pick **T**, press **Number**. The list is still only the T channels,
the count beside the heading still reflects them, and the bar that would clear it is now
`pointerEvents: 'none'` (`LetterBar.tsx:42`). There is no other control for it — the
"Clear search" button exists only inside the no-matches empty state (`:267`), which needs
`visible.length === 0`, so it is unavailable in exactly the case where something *is*
showing but most of the group is missing.

`BrowsePage` gets this right and is the proof that the shape was understood: `:204` renders
the active letter as a removable chip — *"The active letter, where it can be cleared. The
bar itself is at the far edge"* — beside the other filter chips. Live TV has the same bar,
the same `disabled` rule, and no chip.

Either clear `letter` when `order` leaves `'name'`, or give the screen the chip `BrowsePage`
already has. The first is one line and matches what the greying is trying to say.

Two smaller things in the same two files:

* `LetterBar` disables itself with `opacity: 0.35` and `pointerEvents: 'none'` and nothing
  else — no `disabled` attribute, no `aria-disabled`. `pointer-events` does not remove an
  element from the tab order and does not block activation by keyboard, so all 27 letter
  buttons remain reachable by Tab and will fire `onSelect` on Enter while looking inert;
  assistive technology is not told they are unavailable either. The surrounding `title`
  explains the state to a mouse user only.
* The `#` test is `first(c) < 'A' || first(c) > 'Z'` on an upper-cased first character, so
  every channel whose name begins outside ASCII A–Z — Cyrillic, Arabic, `É` — lands in
  `#`. Defensible, but it is a *third* definition of a title's first letter in this
  codebase, after the two the host disagrees about in [R-52]. Whatever that rule becomes,
  these three have to be the same one or the bar means something different on each screen.

### R-113 — Medium — the grid view's tile width is measured once and never again

`src-ui/src/features/live/LivePage.tsx:102`

```ts
const measure = useCallback((el: HTMLDivElement | null) => {
  scrollRef.current = el;
  if (el) setWidth(el.clientWidth);
}, []);
const perRow = view === 'grid' ? Math.max(1, Math.floor((width || 1200) / TILE_MIN_W)) : 1;
```

A ref callback fires when the element attaches and when it detaches — not when it resizes.
There is no `ResizeObserver` and no `resize` listener, so `width` is whatever the scroller
measured on mount and `perRow` is derived from it for the life of the mount.

The comment above it states the requirement it does not meet: *"Measured rather than
assumed: the grid template is `auto-fill` over a 150px minimum, and the virtualiser has to
agree with it about how many fit."* When they disagree, the CSS lays out one number of
tiles per row and `rowCount`/`virt` assume another, so the virtualiser's total height is
wrong and rows are positioned at the wrong offsets — tiles overlap or the list ends early.

The column's width does change in normal use: resizing or maximising the window, and the
group sidebar beside it, whose effect on this very column is called out thirty lines below
— *"the moment the group sidebar took 250px off this column and the longer channel names
began to wrap."*

Mitigated by two accidents rather than by design: the scroller only mounts when
`visible.length > 0` (`:275`), so the *first* measurement happens with content present and
is correct; and switching between List and Grid does not remount it, so a stale width
survives that switch too. `ROW_H`/list view is unaffected because `perRow` is 1 there.

A `ResizeObserver` on the scroller, which `useInlaidPicture.ts:109` already sets up
elsewhere in this codebase, is the fix.

### R-114 — Low — the grid view drops the favourite and catch-up affordances the list has

`src-ui/src/features/live/LivePage.tsx:477`

`ChannelRow` (list) renders the catch-up badge and the heart (`:451`, `:452`), and takes
`onToggleFavorite`. `ChannelTile` (grid) takes only `channel`, `guide` and `onTune`, and
renders neither. So switching Live TV to Grid silently removes the only way to favourite a
channel from that screen, and hides which channels have catch-up.

Nothing says the two views are meant to differ. The empty state for favourites tells the
viewer where the control is — *"Press F while watching, or use the heart on any channel,
to add it here"* (`:248`) — and in Grid there is no heart on any channel.

The tile also renders `data?.now?.title ?? '—'` where the row says `'No guide data'`, so
the same channel reads as unknown in one view and as a dash in the other. With 4,874 of
6,699 channels carrying no `epg_channel_id` on this subscription, the dash is what most of
the grid shows.

### R-115 — High — a viewer with reduced motion enabled gets no buttons on any rail card

`src-ui/src/components/CatalogCard.tsx:96` with `src-ui/src/state/ui.ts:64`

```ts
function enter() {
  if (!animations) return;      // <- returns before setExpanded(true)
  setExpanded(true);
  onExpand?.(index);
  ...
}
```

Every action a card offers lives inside `{expanded && …}` (`:233`–`:299`): **Play**, **My
List**, **I like this**, **Remove from Continue Watching**, and **More info**. With
`animations` false, `expanded` never becomes true, so none of them is ever rendered — on
hover or on focus, since `onFocus={enter}` is the same function.

And `animations` is not opt-in. It is derived from the operating system:

```ts
// state/ui.ts:64
animations: !window.matchMedia?.('(prefers-reduced-motion: reduce)').matches,
```

So anyone who has turned animation effects off in Windows — Settings → Accessibility →
Visual effects, a routine setting for motion sensitivity and on slower machines — gets
this from first launch, with nothing on screen to explain it.

Measured in Chromium with the media feature emulated, same build, same fixture, hovering
the first card of the Recently Added rail:

```
AUDIT [no-preference] buttons on the card while hovered: 5
AUDIT [no-preference] they are: ["The Silent Tide, 2019","Play The Silent Tide",
                                 "Add The Silent Tide to My List","I like The Silent Tide",
                                 "More info"]
AUDIT [no-preference] buttons on the card while focused: 5
AUDIT [no-preference] "remove from Continue Watching" present: 1

AUDIT [reduce]        buttons on the card while hovered: 1
AUDIT [reduce]        they are: ["The Silent Tide, 2019"]
AUDIT [reduce]        buttons on the card while focused: 1
AUDIT [reduce]        "remove from Continue Watching" present: 0
```

One button instead of five, and the one that remains is the card itself. Across the home
screen and both browse grids, that is every rail card in the app.

What survives: clicking the card still calls `onOpen`, and Space still calls `onPlay`
(`:135`–`:138`) — so a title can be opened and played, by someone who knows that Space
does that. What does not survive: My List, the thumbs-up, and **removing something from
Continue Watching**, whose long justification at `:54`–`:61` is explicit that the rail
"stops being about what you are watching" without it. [R-98]'s detail modal is then the
only route to My List and the thumbs-up, and [R-102] means that route is unreachable by
keyboard too.

An accessibility preference that removes functionality is the inverse of what the
preference is for. The expansion and the actions are separate concerns: `enter` should
always set `expanded`, and `animations` should only decide the `transition` durations —
which it already does independently at `:147` and `:238`, both of which read
`animations ? x : 0` and are correct.

**A related latent trap, for whoever fixes [R-87].** `applyDom` (`ui.ts:54`) is called
only from `setTheme`, `setDensity` and `toggleAnimations` — never at store creation. So
`<html>` carries no `data-theme`, `data-density` or `data-motion` until the viewer touches
a setting; measured above as `data-motion=undefined` in both runs. It is harmless today
because the defaults line up with the bare selectors: `tokens.css:39` is
`:root, :root[data-theme='dark']`, `data-density='tv'` (`:112`) is the only density
override, and the reduced-motion rules are duplicated inside an
`@media (prefers-reduced-motion: reduce)` block (`app.css:128`) precisely so they apply
without the attribute. The moment settings are persisted and restored, a saved `light`
theme or `tv` density will not be applied on load. `applyDom` wants calling once at
start-up regardless.

### R-116 — Low — the shared primitives, read line by line: four small gaps and nothing larger

`src-ui/src/components/Primitives.tsx` (524 lines)

Recorded as one entry because each item is minor and they are all in the component every
screen renders through, so they are worth fixing together.

**`Select` is the strongest thing in the file and is nearly complete.** The trigger carries
`aria-label`, `aria-expanded` and `aria-haspopup="listbox"` (`:391`–`:393`), the popover is
`role="listbox"` (`:408`), rows are `role="option"` with `aria-selected` (`:503`–`:504`),
Escape closes it and an outside `mousedown` closes it (`:355`–`:367`) — *"the one thing
every popover must do and the one thing hand-rolled ones forget."* What is missing:

1. **No arrow-key navigation.** Having declared `listbox`/`option`, the component owes
   Up/Down/Home/End and `aria-activedescendant`; a screen-reader user told this is a
   listbox will press Down and nothing will happen. Tab does work, because the options are
   real `<button>`s — which is a legitimate pattern on its own, but not the one the roles
   announce. Either add the keys or drop to a plain menu of buttons.
2. **Focus is not moved into the popover on open, nor returned to the trigger on close.**
   Same omission as [R-102], in the control that replaced every native `<select>` in the
   app, so it is on four screens at once.
3. **The group heading is a bare `<div>` inside the `listbox`** (`:443`). A listbox's
   children should be options or `role="group"`; an unlabelled `div` there is ignored at
   best and announced as a stray node at worst. `role="presentation"` on it would do.

**`ProgressBar`** (`:131`) declares `role="progressbar"` with `aria-valuenow/min/max` and
**no accessible name** — no `aria-label`, no `aria-labelledby`. A screen reader announces
"44 percent" with nothing saying of what. On `CatalogCard` (`:211`) and in the detail
modal's hero (`DetailModal.tsx:212`) it is decorative beside text that already says the
same thing, so `aria-hidden` is the honest answer there; where it is the only indication,
it needs the name.

**`IconButton`** (`:67`) has no disabled treatment. `Button` above it is careful —
`cursor: rest.disabled ? 'not-allowed' : 'pointer'` and `opacity: 0.45` (`:36`–`:37`) —
and `IconButton` hardcodes `cursor: 'pointer'` and full opacity, so a disabled icon button
would look and feel enabled while doing nothing. No current caller passes `disabled`
(swept: none across `src-ui`), so this is latent rather than live; it is recorded because
the next caller to need it will not think to check.

**Checked and correct, so nobody re-opens them:** neither `Button` nor `IconButton` sets
`type="button"`, which would make them submit buttons inside a form — and the only `<form>`
in the UI (`AssistantPage.tsx:344`) calls `preventDefault()` and marks its own button
`type="submit"`, so there is no stray-submit hazard. `TextField`'s `hasValue`
(`:262`) uses `??` rather than `||`, so an empty string correctly hides the clear button.
`Poster` (`:188`) has the placeholder, `loading="lazy"`, `decoding="async"` and an
`onError` fall-back to the remote URL. `FIELD` being exported with the reason stated
(`:230`–`:241`) is the right answer to the drift it describes.

### R-117 — Medium — opening Recordings pulls the entire channel catalogue to label a handful of rows

`src-ui/src/features/dvr/RecordingsPage.tsx:35`

```ts
const { data: channels } = useCommand('channels.list', {}, []);
...
const byName = useMemo(() => {
  const m = new Map<number, Channel>();
  for (const c of channels ?? []) m.set(c.id, c);
  return m;
}, [channels]);
```

`channels.list` takes `group`, `favoritesOnly` and `profileId` and nothing else
(`shared/ipc.ts:1096`), so `{}` means every channel. The map is used for one thing: giving
each recording row a channel name (`:112`, `:135`, `:150`).

`Recording` carries `channelId` and no channel name or logo (`shared/ipc.ts:586`), so the
page has no cheaper option as the contract stands.

Measured against the live panel — `get_live_streams` plus `get_live_categories`, projected
into exactly the `Channel` shape `shared/ipc.ts:12` declares:

```
channels: 6699
Channel[] as JSON (utf-8): 1,804,009 bytes = 1.72 MB
  with a logo URL: 6,623  (98%), average 51 chars
  distinct groups: 67
```

1.72 MB serialised, crossing the Tauri bridge, built into a `Map` of 6,699 entries, to
label as few as three rows — on a page whose `onDvrTick` subscription refreshes the other
four commands but not this one, so at least the cost is paid once per mount rather than
per tick.

**A correction to a comment, not to the code.** `LivePage.tsx:77` says `channels.list`
"on a real library is 6.3 MB of JSON". On this subscription it is 1.72 MB. The larger
figure is consistent with the 22,121-channel subscription `useNowNext.ts:6` cites from
`docs/ROADMAP.md` — 1.72 MB × (22,121 / 6,699) ≈ 5.7 MB — so the comment is almost
certainly about that account rather than being wrong. Recorded so that nobody fixes a
performance problem against the wrong number.

The same fetch confirms an earlier figure in this review from an independent source:
**1,825 of 6,699 channels carry an `epg_channel_id`**, i.e. 4,874 do not, which is what
[R-21]/[R-22] were measured against.

Either `dvr.list` should join the channel name — the host already has both tables and the
row is already denormalised (`title`, `subTitle`, `season`, `episode` are all copied from
the guide) — or `channels.list` needs an id filter. The first is less work and removes the
command entirely from this page.

### R-118 — High — nothing in the app can play a recording

`src-ui/src/features/dvr/RecordingsPage.tsx:343` and `src-ui/src/App.tsx:759`

```tsx
// RecordingsPage.tsx:343
<Button size="sm" variant="primary" icon="play" onClick={() => onPlay?.(rec)}>
  Play
</Button>
```

```tsx
// App.tsx:759
<Route path="/recordings" element={<RecordingsPage />} />
```

`onPlay` is optional (`:26`) and the only mount supplies nothing, so `onPlay?.(rec)` is a
no-op. The Play button on every completed recording does nothing when pressed.

It is not a missing prop. **There is no command that could be passed.** Checked across the
whole contract and the host:

* `MediaKind = 'live' | 'movie' | 'episode'` (`shared/ipc.ts:9`), so `player.play` cannot
  name a recording.
* The `dvr.*` surface is `schedule, list, cancel, delete, setKeep, setWatched, conflicts,
  rules, createRule, deleteRule, setRuleEnabled, reminders, addReminder, removeReminder,
  storage` — fifteen commands, none of which plays anything.
* No command anywhere takes a file path. `Recording.filePath` (`shared/ipc.ts:604`) is
  carried to the UI and never used by it.
* `grep Recording src-native/crates/aurora-app/src/playback.rs` is empty. The playback
  layer has no concept of a recording.

So the DVR schedules, records, reaps, detects stalls, enforces a storage quota, honours
Keep, resolves conflicts by priority, runs series rules, writes a reason for a short
recording, and reports progress on a tick — and a viewer cannot watch any of it. The only
way to the file is Explorer.

The design clearly intended otherwise. `watch_progress.item_kind` allows `'recording'` in
the schema's `CHECK` constraint (`aurora-db/src/schema.rs:184`), `ItemKind::Recording`
exists and round-trips through `progress.rs:27`/`:182`, and `shared/ipc.ts:151` carries
`'recording'` in the progress `itemKind` union. Resume support for recordings is modelled
end to end; playback is not. `MediaKind` and `ItemKind` disagree on exactly this one value,
and the narrower one is the one playback uses.

The shortest honest route is `player.play` accepting `kind: 'recording'`, resolving
`filePath` host-side and loading it as a local file — mpv needs no network path for it —
with `resumeAt('recording', id, profileId)` reusing the progress row the schema already
allows. `App.tsx` then passes a `play` for it exactly as it does for the other kinds.

Note that this is a different shape from [R-110]: that button has no handler at all and a
mechanical sweep finds it. This one *has* a handler, and the handler is an optional-call on
a prop nobody passes — invisible to both the type checker and the sweep, because
`onPlay?.()` is valid TypeScript and a legitimate pattern everywhere else.

**Beside it, in the same rows:** `act` (`:291`) is

```ts
const act = async (fn: () => Promise<unknown>) => { await fn(); onChanged(); };
```

with no `.catch`. A failed `dvr.delete`, `dvr.setKeep`, `dvr.cancel` or `dvr.setWatched`
rejects into `installGlobalErrorHandlers` (`lib/errors.ts:110`), so the viewer does get a
toast — but it reads **"Something went wrong"** rather than the specific sentence every
other screen produces via `report('Could not …')`, and `onChanged()` never runs so the row
keeps showing the state that was not saved. Four controls, including the irreversible one,
are the only places in the UI that fall through to the generic handler.

### R-119 — Low — a series rule whose recordings all failed still reads "N recorded"

`src-ui/src/features/dvr/RecordingsPage.tsx:522`

```tsx
<span style={{ ... }}>{rule.scheduled} recorded</span>
```

`scheduled` is a count of rows in any state:

```sql
-- aurora-db/src/repo/dvr.rs:434
(SELECT COUNT(*) FROM recordings WHERE rule_id = r.id)
```

with no `WHERE state = …`. The Rust field's own doc comment says so — *"How many recordings
this rule has scheduled or made"* (`:426`) — and `shared/ipc.ts:634` carries it through
unqualified. `RecordingState` has five values (`scheduled, recording, completed, failed,
skipped`), and the label claims the fourth-and-fifth are the third.

So a rule for a show on a channel that keeps dying, with five failed attempts and nothing
on disk, reports "5 recorded" on the Series rules tab — and [R-108] makes `skipped` the
expected outcome for the second concurrent recording on this subscription, so this is not
an exotic case. The information to say it properly is already in the row: `scheduled` is a
total, not a completion count, and the word for it is "scheduled" or "tracked".

**And the uncaught-rejection shape from [R-118] is the whole file, not one function.** Six
call sites invoke a host command from a click and never catch:

```
:291  act()           — delete, keep, setWatched (used by RecordedRow)
:401  dvr.cancel      — Stop / Cancel on a scheduled recording
:435  dvr.delete      — Dismiss on a failed recording
:510  dvr.deleteRule  — Delete rule
:527  dvr.setRuleEnabled — Pause / Resume
```

Every one rejects into `installGlobalErrorHandlers` and produces **"Something went
wrong"**, and in each case the `onChanged()` that would re-read the list never runs, so the
row keeps showing the state that was not saved. `RecordingsPage.tsx` is the only screen in
`src-ui` that imports `invoke` without also importing `report` — every other one pairs
them. One `.catch(report('…'))` per call site brings the page in line with the rest of the
app, and the sentences are easy: "Could not cancel that recording", "Could not delete that
recording".

### R-120 — High — the wizard asks "What should Aurora import?" and throws the answer away

`src-ui/src/features/setup/SetupWizard.tsx:57` and `:124`

```ts
const [content, setContent] = useState({ live: true, vod: true, series: true });
...
const runImport = useCallback(async () => {
  setStep('importing');
  const { id } = await invoke('providers.save', { draft });
  setReport(await invoke('providers.refresh', { providerId: id }));
  setStep('done');
}, [draft]);                                   // <- `content` is not even a dependency
```

`ContentStep` draws three switches under the heading **"What should Aurora import?"** —
Live TV ("Channels and the programme guide"), Movies ("On-demand films"), Series ("Shows,
seasons and episodes") — and `content` is read nowhere except by that component's own
rendering and its `none` guard (`:527`). Every reference in the file:

```
:57   useState
:166  passed to ContentStep
:517  its prop type
:522  the row labels
:527  const none = !live && !vod && !series
:538-:558  the switch rendering
```

`runImport` never touches it. `DraftProvider` has no content fields
(`shared/ipc.ts:487`), and neither `providers.save` nor `providers.refresh` accepts any
(`:1448`, `:1449`). So unchecking Movies and Series still imports 27,494 films and 8,086
shows — which on this subscription is nearly all of the import's cost, and skipping it is
the entire reason the step exists.

**The host already implements it.** `aurora-ingest/src/sync.rs:96` declares

```rust
pub struct SyncOptions {
    ...
    pub import_live: bool,
    pub import_vod: bool,
    pub import_series: bool,
```

honoured where it matters:

```rust
// sync.rs:247
MediaKind::Live    if options.import_live   => out.live.push(entry),
MediaKind::Movie   if options.import_vod    => out.movies.push(entry),
MediaKind::Episode if options.import_series => out.episodes.push(entry),
```

and `SyncOptions::new` leaves all three at `true` (`:116`–`:118`). `providers.rs:889`
builds one and sets only `password`, so nothing can ever change them.

Implemented on the host, drawn in the UI, disconnected at the IPC boundary — the same
shape as [R-110] (speed and aspect), [R-108] (`max_connections`) and the My List bug
`tests-e2e/lists.spec.ts` was written for. Three fields on `providers.refresh` close it.

There is a second-order cost: a viewer who deliberately unchecks Movies on a slow
connection waits through the whole VOD import anyway, and then sees a `DoneStep` reporting
27,494 movies they asked not to have.

### R-121 — High — the final screen overstates guide coverage, and the host already knows the real number

`src-ui/src/features/setup/SetupWizard.tsx:641`

```ts
const coverage =
  report.epgMatched + report.epgUnmatched.length > 0
    ? Math.round((report.epgMatched / (report.epgMatched + report.epgUnmatched.length)) * 100)
    : 0;
```

`epgMatched` is a complete count. `epgUnmatched` is **capped at 200**:

```rust
// aurora-ingest/src/sync.rs:514
None => {
    if report.epg_unmatched.len() < 200 {
        report.epg_unmatched.push(row.name.clone());
    }
}
```

The cap is sensible — nobody wants 4,000 channel names over the IPC bridge — but the UI
uses the list's *length* as the unmatched total. Past 200 unmatched, the denominator stops
growing while the numerator keeps going, so the percentage is wrong in the flattering
direction and converges on 100%.

On this subscription that is the ordinary case, not an edge: 1,825 of 6,699 channels carry
an `epg_channel_id` (measured independently in [R-117]), and the fuzzy pass adds 655
([R-22]). So roughly 2,480 match and roughly 4,200 do not — and the screen computes
`2480 / (2480 + 200) = 93%` where the truth is `2480 / 6699 = 37%`.

Then it says, underneath: *"The rest can be mapped by hand in Settings"* — describing four
thousand channels as "the rest" of 7%.

**The host computes the correct figure and logs it instead of returning it:**

```rust
// sync.rs:520
let coverage = epg_match::coverage(&index, rows.iter().map(|r| (r.name.as_str(), r.tvg_id.as_deref())));
tracing::info!(matched = coverage.matched, total = coverage.total, "EPG coverage {:.1}%", coverage.percent());
```

`coverage.total` is every row, so `coverage.percent()` is exactly the number the screen
wants. It goes to `aurora.log` and no further — `grep coverage shared/ipc.ts` is empty, and
`SyncReport` has no field for it.

Two one-line fixes, either of which is enough: put `coverage` on `SyncReport` and render
it, or add an `epgUnmatchedTotal: usize` beside the capped list so the UI can divide by the
right number. The second is the smaller change and also fixes the sentence, since "4,219
channels can be mapped by hand" is the honest version.

### R-122 — Medium — "You can keep using Aurora while this runs", said by a full-screen modal with no way out

`src-ui/src/features/setup/SetupWizard.tsx:624` with `:138`

```tsx
<p style={{ ... }}>
  Large subscriptions can take a minute. You can keep using Aurora while this runs.
</p>
```

The wizard's root is

```tsx
position: 'fixed', inset: 0, zIndex: 400, background: 'var(--bg)'
```

— opaque, full-viewport, above everything. `ImportingStep` renders a spinner, a phase
label, a progress bar and that sentence, and **no button at all**: no Back, no Skip, no
Cancel, no "do this in the background". `onSkip` exists and is wired into the header for
the other steps but not this one. So for the duration of the import the viewer can do
nothing except watch it, which is the exact opposite of what the paragraph says.

The claim is not idly made — it is true of the *host*. `providers.rs:894` goes out of its
way to download with no database lock held, specifically so the rest of the app keeps
working during a refresh, and `RefreshButton.tsx` uses the same `ingest.progress` events
from inside Settings where the app genuinely does stay usable. It is only this screen that
blocks, and only because it is a modal that does not offer to step aside.

Measured against the real catalogue, "a minute" is also optimistic: the XMLTV parse alone
is 1.04 s for 178 MB (recorded earlier in this review) on top of a 51 MB playlist, three
`player_api` calls, 237,170 entries classified, and the search index rebuilt.

A "Continue in the background" button that calls `onFinished()` and leaves the progress to
`RefreshButton`'s existing surface would make the sentence true. Failing that, the sentence
should go.

**And `providers.detect` runs on every keystroke.** `SourceStep`'s textarea calls
`onPaste(e.target.value, byPaste)` from `onChange` (`:320`), which is `detect` (`:75`),
which does `await invoke('providers.detect', { text })` with **no try/catch**. Typing a
60-character address by hand is 60 IPC round trips, and any one rejection surfaces as
"Something went wrong" from the global handler (`lib/errors.ts:110`) in the middle of
typing. Cheap to fix — debounce, or only detect on paste and on blur, which is what the
`fromPaste` flag already distinguishes.

This matters more than a wasted round trip, because of what is on the other end:
`providers_detect` (`aurora-app/src/providers.rs:69`) calls `parse_pasted_xtream`, which
calls the `urldecode` of [R-63] — the one that byte-slices a `&str`. That function is
therefore reached **per keystroke on the first field of the first screen a new viewer
sees**, which is a considerably larger exposure than [R-63] was recorded with. See the
addendum below.

### R-63 — addendum — the exact boundary of the paste panic, and a silent credential corruption beside it

Measured with a probe crate outside the repository linking `aurora-ingest` and calling the
production `parse_pasted_xtream` on `http://panel.example.com/get.php?username=alice&password=<tail>`:

```
plain password                     -> parsed
percent at the end       ("p%")    -> parsed       (the `i + 2 < b.len()` guard holds)
percent + one ASCII      ("p%2")   -> parsed
valid %20                          -> parsed
percent + 2-byte (e-acute)         -> parsed       (slice boundaries happen to align)
percent + 3-byte (en dash)         -> *** PANICKED ***
percent + 3-byte (curly apostrophe)-> *** PANICKED ***
percent + 3-byte (euro)            -> *** PANICKED ***
percent + 4-byte (emoji)           -> *** PANICKED ***
smart-quoted whole url             -> parsed
```

So the panic needs `%` followed within two bytes by a **3- or 4-byte** UTF-8 character.
`%` at the end of the string is safe, and so is `%` followed by a 2-byte character — both
of which I had expected to fail, so the guard at `source.rs:115` is doing more than it
looks like it is.

The realistic trigger is a URL pasted out of a chat client or mail client that has applied
smart quotes: `…&password=abc%’def`. Combined with [R-122]'s finding that
`providers.detect` runs **on every keystroke of the wizard's first field**, and with
[R-72]'s `panic = "abort"` in the release profile, the consequence in a shipped build is
that the window disappears mid-setup with nothing written anywhere.

**The same probe found a separate defect that is quieter and probably worse.** `out.push(v as char)`
treats each decoded byte as a Unicode scalar, which is Latin-1, not UTF-8:

```
caf%C3%A9  -> decoded to "cafÃ©"  (5 chars; should be "café", 4)
p%C3%A4ss  -> decoded to "pÃ¤ss"  (5 chars; should be "päss", 4)
```

So **any subscription whose password contains a non-ASCII character is silently corrupted
on paste**. The wrong string is filled into the masked password field, saved to Windows
Credential Manager by `providers.save`, and sent to the panel — which rejects it. The
viewer sees a credentials failure for a password they pasted correctly, with no way to see
what went wrong, because the field is masked and the mangling is invisible at a glance
(`cafÃ©` in a password box is five dots).

A correct implementation already exists in this workspace at `aurora-core/src/m3u.rs:392`
— it decodes into a byte buffer and runs `String::from_utf8_lossy` at the end, which gets
both cases right. `source.rs:110` should call it rather than keep its own.

One more thing about that addendum, which changes how it should be read. The correct
version at `aurora-core/src/m3u.rs:392` is not merely a better implementation that happens
to exist — its doc comment names **both** of the defects measured above, in those words:

> `byte as char` is a Latin-1 cast, so `%C3%A9` decoded to `Ã©` instead of `é`, in a
> string that goes back out as an HTTP header.
>
> Decoding into a `Vec<u8>` and interpreting the result as UTF-8 once at the end removes
> both: there is no `&str` index to get wrong, and the bytes a provider escaped are
> reassembled before anything decides what characters they are.

So this was found, understood, written up and fixed once — in `aurora-core` — and the
identical twelve-line function in `aurora-ingest/src/source.rs:110` was left as it was.
[R-63] is not a bug nobody has noticed; it is a fix that reached one of two copies. That is
worth knowing before fixing it a second time: the right move is to export the
`aurora-core` one and delete the other, so there is no third copy to miss.

### R-123 — Medium — multi-view: the whole screen's premise is defeated by [R-108], plus two smaller gaps

`src-ui/src/features/mosaic/MosaicPage.tsx` (467 lines, read in full)

This file has the clearest statement of intent in the codebase:

> **The warning is not a footnote.** A 3×3 is nine simultaneous streams and the
> subscription this project was measured against allows one — so on that line no layout
> here will ever work, and a picker that let you choose one and then failed four times
> would be telling you the same thing four times more slowly. The limit is shown per
> layout, the ones that cannot open say so on the button, and where no provider declares
> a limit at all — every M3U playlist, so the common case — it says that instead of
> pretending to know.

Every one of those promises is kept in this file and broken by [R-108]. Because
`providers.max_connections` is never written, `Mosaic::limit()` always returns `None`,
`Budget::check` always returns `Unknown`, and so:

* `budgetLine` (`:100`) always takes the `unknown` branch and returns `blocked: false`.
* `line.blocked` is never true, so the "largest that fits" sentence at `:302` and the
  "no arrangement fits on this line" sentence at `:307` are both unreachable.
* `selectionBlocked` (`:144`) requires `verdict === 'exceeds'`, so it is always false, so
  the Open button (`:345`) is never disabled for budget reasons and the warning at `:355`
  never shows.
* On this one-connection subscription, every layout including 3×3 is offered with the
  sentence *"your provider has not said how many it allows"* — which is false; it said
  one, during setup, and nothing stored it.

So the screen written specifically to avoid "telling you the same thing four times more
slowly" does exactly that. Nothing in `MosaicPage.tsx` is wrong; it is downstream of a
column nobody writes. Recorded here because fixing [R-108] is what makes this file work,
and because it is the sharpest demonstration of what that one missing `INSERT` column
costs.

**Two gaps that are this file's own:**

1. **`SaveLayoutDialog` cannot be dismissed with Escape** (`:401`). It has
   `role="dialog"`, an `aria-label` and `autoFocus` on the input — and no `keydown`
   handler and no click-away on the backdrop. The only way out is the Cancel button. Every
   other dismissible surface in the app handles Escape: `DetailModal.tsx:47`,
   `CommandPalette`, and even the `Select` popover (`Primitives.tsx:360`). It also has no
   `aria-modal`, unlike the other two dialogs in [R-102] — which here is the more honest
   markup, since nothing makes the background inert in any of the three.

2. **Deleting a saved layout is one click with no confirmation** (`:390`, `:219`). A saved
   layout holds an arrangement and up to nine channel choices and exists nowhere else;
   `mosaic.deleteLayout` is called straight from the icon button. `RecordingsPage` is
   careful about precisely this shape twice over — a two-step for deleting a recording
   (`:296`–`:303`) and for deleting a series rule (`:471`–`:478`), each with a written
   justification about the delete control sitting next to the reversible one. Here the
   delete sits next to **Open**, which is worse than next to Keep.

**Checked and correct:** the `picked` truncation on a smaller layout (`:135`) with its
reason; `filled` counting only non-null tiles so an empty tile costs no connection
(`:139`, `:142`); `channelOptions` built once per channel list rather than per tile
(`:164`, with the nine-copies note); `LayoutThumb`'s cell arithmetic against
`MOSAIC_TILES`; and all three failure paths (`open`, `openSaved`, `deleteSaved`) catching
and naming what failed — which is what `RecordingsPage` does not do ([R-119]).

### R-124 — Medium — a typo in a channel number silently discards the renumbering

`src-ui/src/features/playlist/PlaylistPage.tsx:358`

```tsx
<EditableCell
  value={row.number == null ? '' : String(row.number)}
  onCommit={(next) => onPatch({ number: Number(next) || 0 })}
/>
```

`0` is not a neutral value here — it is the sentinel that **clears** the override. Both
sides agree on that, deliberately:

```rust
// aurora-db/src/repo/playlist.rs:269
if let Some(number) = patch.number {
    // Channel 0 does not exist, so it is how the editor says "back to the
    // provider's number".
    let value = if number == 0 { None } else { Some(number) };
```

```ts
// ipc/mock.ts:1691 — "An empty string or a zero clears the override, exactly as the host reads them."
if (patch.number !== undefined) edit.number = patch.number === 0 ? null : patch.number;
```

So `Number(next) || 0` maps **every** unparseable input onto "discard my renumbering":
`Number('12abc')` is `NaN`, `NaN || 0` is `0`. Measured in a browser against the playlist
editor:

```
AUDIT cell label: Number for Meridian News   text: 201
AUDIT input value: 12abc
AUDIT cell after: 201
AUDIT page error text: []
```

A viewer who had renumbered that channel to 5, then mistyped while editing it, is back to
201 with nothing said. `||` also swallows a deliberate `0`, and `Number('')` is `0`, so
clearing the field resets too — which is probably the intent, but it is the same code path
as the typo and so cannot be distinguished from it.

A control edit in the same run confirms the path is otherwise sound: a **name** edit on the
adjacent cell committed and persisted ("AUDIT RENAMED"), so this is specific to the
coercion and not to `EditableCell` or `playlist.update`.

`Number.isInteger(n) && n > 0 ? n : undefined` — leaving the patch field absent, which
`Patch` already treats as "no change" (`playlist.rs:269` is an `if let Some`) — keeps the
typo from doing anything, and an explicit "Use the provider's number" action can own the
`0` sentinel instead of a parse failure owning it.

**Two things I measured expecting a defect and did not find, recorded so they are not
re-investigated:**

* **The checkboxes do have a focus ring.** `.aurora-field:focus { outline: none }`
  (`app.css:66`) removes the UA ring, and I expected nothing to replace it on a native
  checkbox, which has no author border for `border-color` to act on. It is replaced
  globally: `app.css:41` is `:focus-visible { outline: none; box-shadow: var(--focus-ring) }`.
  Measured on both the select-all and a row checkbox:
  `boxShadow: "rgb(11,11,15) 0 0 0 2px, rgb(124,92,255) 0 0 0 5px"` — a two-tone ring that
  survives being drawn over artwork, which is what the comment above it claims.
* **`.aurora-field` on a checkbox is harmless.** The class carries only `:hover` and
  `:focus` border colours and a `::placeholder` rule (`app.css:65`–`:67`); the padding,
  background and border live in the inline `FIELD` object, which the checkboxes do not
  spread. So the two `<input className="aurora-field" type="checkbox">` uses
  (`:192`, `:344`) pick up nothing that distorts a native checkbox.

### R-125 — Low — "Hide all 27,494 matching" is one click with no confirmation

`src-ui/src/features/playlist/PlaylistPage.tsx:229`

```tsx
{ids.length === 0 && total > rows.length && (
  <Button size="sm" variant="secondary" disabled={busy}
    onClick={() => void act(() => invoke('playlist.hideMatching', { ...query, hidden: true }))}>
    Hide all {total} matching
  </Button>
)}
```

Its condition — nothing selected, and more matches than are on screen — is exactly the
state the screen is in when you arrive, before touching anything. On this library that
means arriving at the Movies tab and finding a button offering to hide all 27,494 films,
one click away, with no second step.

It is recoverable: `show: 'hidden'` lists them again and `playlist.reset` takes ids. But
the recovery is per-selection in pages of 300, so undoing it by hand is ninety-two pages
of select-all-and-reset. There is no "show all matching" counterpart to the bulk hide.

The button does state the number, which is the single most important thing about it, and
this screen is otherwise careful. But `RecordingsPage` two-steps the deletion of **one**
recording with a written justification about the control sitting next to a safer one — and
here the control sits where a freshly opened screen puts the pointer. A confirm step, or a
matching "Show all N matching", brings it in line.

**Checked and correct in the same file:** the virtualiser's `ROW_H` agreement with the row
(`:273`, `:340`); `switchKind` clearing the selection and the group so a selection cannot
leak across lists (`:90`); `toggleSelected`'s `if (!next.delete(id)) next.add(id)`;
`EditableCell`'s Enter-commits / Escape-abandons / blur-commits contract (`:475`–`:490`),
which matches its doc comment exactly; `languageName` deferring to `Intl.DisplayNames`
with a fallback rather than carrying a translation table (`:31`); and `act` catching and
surfacing the message inline at `:242` — this screen does what [R-119] says
`RecordingsPage` fails to do.

### R-126 — Critical — the provider's password is stored in plain text in `library.db` hundreds of thousands of times, and `docs/PRIVACY.md` says it is not

`src-native/crates/aurora-db/src/schema.rs:72`, `:118`, `:166`
`src-native/crates/aurora-core/src/xtream.rs:406`

Xtream stream URLs carry the credentials in the path, by design of the protocol:

```rust
// aurora-core/src/xtream.rs:416
MediaKind::Live    => format!("{base}/live/{username}/{password}/{stream_id}.ts"),
MediaKind::Movie   => format!("{base}/movie/{username}/{password}/{stream_id}.{ext}"),
MediaKind::Episode => format!("{base}/series/{username}/{password}/{stream_id}.{ext}"),
```

Those strings are then stored verbatim, in three tables:

```
schema.rs:72    channel_sources.url   TEXT NOT NULL
schema.rs:118   movies.url            TEXT NOT NULL
schema.rs:166   episodes.url          TEXT NOT NULL
```

On this subscription that is 6,699 channel sources and 27,494 movies, plus one row per
episode of 8,086 series — several hundred thousand plaintext copies of the username and
password in a single SQLite file.

**`docs/PRIVACY.md:16` states the opposite, and states it as a feature:**

> They are **not** stored in the library database. They live in Windows Credential
> Manager, and the database keeps only a reference to the entry. That means a copy of
> `library.db` — a backup, an export, a file handed to somebody for a bug report —
> contains no credentials.

Each sentence is true of the `providers` table and false of the library. `providers` does
keep only `credential_ref` (`providers.rs:609`), which is what the paragraph was written
about — but `library.db` as a whole is the thing it names, and the three columns above are
in it. So the specific reassurance the document offers about the specific artefact a user
would hand over is wrong: **a `library.db` sent with a bug report hands over the
subscription.**

The project clearly understands this risk elsewhere. The paragraph immediately following
is about `http::redact`, which exists to keep exactly this URL shape out of the log and
which [R-61] found holes in:

> Every URL written to the log goes through `http::redact` first … and also handles the
> form that has no markers at all: `http://host/<user>/<pass>/12345.ts`, which is how most
> panels address a stream. That second case was a real leak once (F-15).

So the shape is known, the leak has happened before, and the defence was applied to the
log and not to the database.

**It also crosses the IPC boundary into the WebView.** `EpisodeRow` derives
`Serialize` with `rename_all = "camelCase"` and includes `url` (`library.rs:834`–`:844`),
`episodes_for` SELECTs it (`library.rs:882`), and two commands return it:

```
commands.rs:448   library_episodes  -> Result<Vec<library::EpisodeRow>>
commands.rs:754   PlaybackAids { next_episode: Option<library::EpisodeRow>, .. }
```

So opening any show in the detail modal, or playing any episode, sends the provider's
password into the renderer — once per episode. `MovieRow` has no `url` field, so films do
not leak this way; `channel_sources` is read only into Rust (`window.rs:123`). Episodes
are the whole of the IPC exposure, and they are enough.

This is invisible from the TypeScript side, which is why it has survived: `shared/ipc.ts`'s
`Episode` interface declares `{id, seriesId, season, episode, title, overview, still,
runtime_mins, airDate}` and **does not declare `url`**, so no UI code references it, `tsc`
never mentions it, and nothing in the contract review would show it. It arrives in the
payload regardless. Anything that can read the renderer — an injected script, devtools, a
WebView2 crash dump, `ErrorBoundary`'s `console.error(error, info.componentStack)` firing
inside the episode list — can read it.

**Four separate things to fix, and they are independent:**

1. **Stop storing it.** The URL is derivable: `base_url` is on the provider row, the
   credentials are in Credential Manager, and `stream_url(base, user, pass, kind, id, ext)`
   already builds it from a `stream_id`. Storing `stream_id` and the extension instead of
   the URL removes the leak from the database entirely, and makes rotating a password a
   provider-row update rather than a rewrite of 300,000 rows. For M3U providers, whose
   URLs are whatever the playlist said and cannot be reconstructed, the honest options are
   to keep them and say so, or to store them with the credential substring replaced by a
   reference the same way the log does.
2. **Stop sending it.** `library_episodes` should return a shape without `url` — the UI has
   never used it, so this costs nothing. `#[serde(skip)]` on the field is a one-line change
   that fixes both commands.
3. **Correct `docs/PRIVACY.md`.** Until (1) is done, the paragraph at `:16` has to say that
   the library database contains stream URLs and that on an Xtream provider those carry the
   credentials — because somebody is going to read that paragraph and attach `library.db`
   to an issue.
4. **Add the `Episode` contract fields that are missing while you are there.**
   `shared/ipc.ts:596` declares `seriesId` and `airDate`; `EpisodeRow` has neither, so both
   are `undefined` in every real build. Nothing reads them today, so this is latent rather
   than live — but it is the same defect as [R-99], and `tsc --strict` will keep passing
   for whoever writes `ep.airDate` next.

**On the mechanical check that found this.** The first field-drift sweep I ran compared
each TS field name against the whole text of `src-native/**/*.rs` and reported zero
drifted fields — which is worthless, because it would have passed `match` too: `match` is
a Rust keyword and appears in every file. Replacing it with a per-interface diff of
`shared/ipc.ts` interfaces against the specific Rust struct that serialises into each one
caught `Movie.match` and `Series.match` — the known instance from [R-99] — and then found
`Episode`, which is how this entry exists. Of 38 interfaces paired, 6 flagged; two were my
pairing being wrong (`Programme` pairs with `ProgrammeRow`, not
`aurora_core::model::Programme`; `PlaylistEntry` with a view struct), and five interfaces
could not be paired by name at all and still need checking by hand (`PlaybackAids`,
`TimeshiftWindow`, `TimeshiftSettings`, `DvrStorage`, `LibraryFilters`). The lesson from
[R-12] holds exactly: a check that has not been shown to catch a known instance is not
evidence.

### The IPC contract, checked exhaustively — two real drifts, both already recorded

Method: parse every `export interface` in `shared/ipc.ts` (64 of them, 410 fields), parse
every `pub struct` in `src-native/crates/**/*.rs` with its field names and `serde(rename)`
attributes, pair each interface with the Rust struct sharing the most fields — pairing by
overlap rather than by name, so a renamed struct cannot hide a drift — and diff.

```
interfaces examined : 64
  field sets matching the host exactly : 59
  with TS fields the host cannot send  : 5
  no Rust struct shares a single field : 0
```

Of the five, **two are real** and both are already written up above:

```
DRIFT Movie   <- MovieRow     TS only: match                     -> [R-99]
DRIFT Series  <- SeriesRow    TS only: match                     -> [R-99]
DRIFT Episode <- EpisodeRow   TS only: airDate, seriesId
                              Rust only: url                     -> [R-126]
```

**Three are the check's own blind spots, resolved by hand:**

* `UpdateStatus` flagged `current`, `latest`, `available` as TS-only. They come from
  `#[serde(flatten)] pub check: UpdateCheck` (`aurora-app/src/updates.rs:47`), and
  `UpdateCheck` (`aurora-ingest/src/updates.rs:113`) declares all three. The script does
  not follow `serde(flatten)`. Not a defect.
* `Recording` flagged `liveBytes`. It is set by a view wrapper at `dvr.rs:417`/`:430` from
  `services.dvr.bytes_so_far(...)`, not by the core `Recording` struct the script paired
  it with. Not a defect.
* `Programme` and `PlaylistEntry` flagged under the earlier name-based pairing and are
  clean under overlap pairing: they serialise from `ProgrammeRow` (14/14) and
  `PlaylistRow` (11/11).

So the contract is sound apart from `match` and `Episode`. That is worth stating plainly
because the earlier note in this review recorded "the IPC contract (no drift)" on the
strength of a command-name and arity check, which says nothing about fields — and the two
defects it missed are [R-99], a documented feature that exists only in the mock, and
[R-126], the credential leak. A field-level check was the missing one.

### R-121 — addendum — the app already shows the correct coverage on another screen, and the right shape exists in the codebase

Two things found while reading `shared/ipc.ts` make [R-121] considerably stronger.

**1. Settings shows the honest number, from a command built for it.**

```tsx
// src-ui/src/features/settings/SettingsPage.tsx:177
{stats.epgCoverage.matched} of {stats.epgCoverage.total} channels
{' '}({Math.round((stats.epgCoverage.matched / stats.epgCoverage.total) * 100)}%)
```

`library.stats` returns `epgCoverage` from `aurora-db/src/repo/library.rs:572`:

```sql
total   = SELECT COUNT(*) FROM channels WHERE hidden = 0
matched = SELECT COUNT(*) FROM channels
          WHERE hidden = 0 AND epg_channel_id IS NOT NULL AND epg_channel_id <> ''
```

A real total and a real matched count. On this subscription that is **1,825 of 6,699 =
27%** ([R-117] measured the 1,825 independently against the panel).

So the same library is reported as **~93% covered on the first-run screen and 27% in
Settings**, by two different arithmetic expressions over two different payloads. Whichever
a viewer sees first is the one they will believe.

**2. The non-truncating shape already exists, with the hazard written on it.**
`EpgCoverage` (`library.rs:562`) is:

```rust
pub struct EpgCoverage {
    pub total: i64,
    pub matched: i64,
    /// A few names, to make "what is missing" answerable without a query tool. Capped:
    /// a playlist with nine thousand unmatched channels should not ship all nine
    /// thousand to the renderer to be printed in one line.
    pub unmatched: Vec<String>,
}
```

— a sampled list **beside** a true total, with `UNMATCHED_SAMPLE = 12` and a comment
explaining precisely why the sample must not be mistaken for the count. `SyncReport`
(`aurora-ingest/src/sync.rs:58`) carries `epg_matched: usize` and
`epg_unmatched: Vec<String>` capped at 200, and **no total** — the one shape the comment
above warns against — and `DoneStep` then does the division the missing field would have
prevented.

The fix is now obvious and small: give `SyncReport` an `EpgCoverage`, or just its `total`,
and have `DoneStep` divide by that. The arithmetic is already written correctly thirty
lines away in `SettingsPage`.

**One smaller thing in the correct version**, for completeness: `SettingsPage.tsx:178`
divides by `stats.epgCoverage.total` with no zero guard, so a library with a provider that
imported no live channels — which `LivePage`'s `emptyReason` (`:534`) exists specifically
to explain, so it is a case the project knows happens — renders "NaN%".

### The host's security surface, reviewed — sound, and notably so given [R-126]

Worth stating as its own entry because [R-126] is a credential leak, and the obvious next
question is whether the renderer can reach anything else. It cannot.

**The capability set is as small as Tauri allows.** `capabilities/default.json` in full:

```json
{ "identifier": "default", "windows": ["main"],
  "permissions": ["core:default", "core:window:allow-start-dragging"] }
```

No `fs`, no `shell`, no `http`, no `dialog`, no `process` plugin — so the UI genuinely can
only reach the system through this app's own typed commands, which is what its description
claims. The three 2,292-line files under `gen/schemas/` are the *authoring* schema — the
catalogue of every permission Tauri could grant — generated from the plugin set, not a
record of what is granted. They are not reviewable prose and nothing in them affects the
granted set; `capabilities/default.json` above is the whole grant.

**The CSP is restrictive where it counts.** `tauri.conf.json`:

```
default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline';
frame-src https://www.youtube-nocookie.com https://www.youtube.com;
img-src 'self' data: https: http: asset: http://asset.localhost;
media-src 'self' https: http:
```

`script-src 'self'` with no `unsafe-inline` and no `unsafe-eval` is the important one, and
it is right. `style-src 'unsafe-inline'` is unavoidable given that the UI styles almost
everything with inline `style` objects. `img-src`/`media-src` admitting `http:` is forced
by the providers, which publish artwork and streams over plain HTTP.

**The asset protocol is scoped correctly, and this is the part I expected to find wrong.**
`tauri.conf.json` has `assetProtocol.enable = true` with `scope = []`, which denies
everything statically. The grant happens at runtime, narrowly:

```rust
// main.rs:192
let folder = services.artwork.dir().to_path_buf();
if let Err(e) = app.asset_protocol_scope().allow_directory(&folder, true) { ... }
```

`services.artwork.dir()` is the artwork cache subdirectory, **not** the data directory — so
`library.db`, which [R-126] shows contains the provider password, is not readable from the
WebView through `http://asset.localhost/`. Had that grant been one level higher, [R-126]
would have been remotely exploitable rather than locally embarrassing. The comment explains
why the grant cannot live in the manifest (the folder differs between a portable and an
installed copy) and why failing to grant it is not fatal, and `metadata.rs:384` documents
the degradation path. Recursive (`true`) over a cache of image files is fine.

**Dependency advisories.** `cargo audit` against `src-native/Cargo.lock`, 512 crates:

```
0 vulnerabilities
8 warnings: 7 unmaintained/unsound, 1 yanked
  RUSTSEC-2024-0370  proc-macro-error         unmaintained
  RUSTSEC-2025-0075/0080/0081/0098/0100  unic-*  unmaintained (5 crates)
  RUSTSEC-2024-0429  glib 0.18.5              unsound (VariantStrIter)
  yoke-derive 0.8.3                           yanked
```

`pnpm audit --audit-level moderate`: **no known vulnerabilities**.

None is actionable as a security matter: all are transitive, the `glib` unsoundness is in
the GTK stack that only builds on Linux and this is a Windows-only product, and the
`unic-*` family is unmaintained rather than vulnerable. The yanked `yoke-derive` is the one
worth a `cargo update`, since a yanked version will eventually stop resolving in CI.

### R-126 — addendum — the same leak is understood, named and regression-tested, on the other path only

`src-native/crates/aurora-app/src/assistant.rs:1519`

```rust
/// Nothing a tool returns may carry a stream URL: the request goes to Google.
#[test]
fn no_tool_result_contains_a_stream_url() {
    ...
    assert!(!text.contains("example.com") && !text.contains("http"),
            "{name} leaked a URL: {text}");
}
```

It drives all six read tools — `search_library`, `browse_library`, `title_details`,
`library_digest`, `watch_history` — and asserts that no tool result reaching the model
contains `http`. So the exact hazard in [R-126] was identified, given a one-line
justification, and locked down with a named regression test.

It was locked down for the Gemini boundary and not for the IPC boundary. The two paths even
share the query: `assistant.rs:506` calls `library::episodes_for(db, id, None)` — the same
function whose `EpisodeRow.url` is the leak — and is safe only because it projects the
fields it wants explicitly:

```rust
// assistant.rs:511 — an explicit json! body, so `url` cannot escape
result: json!({ "kind": "series", "id": s.id, "title": s.title, "year": s.year,
                "rating": s.rating, "genres": s.genres, "overview": s.overview,
                "seasons": seasons.len(), "episodes_in_library": episodes.len() }),
```

whereas `commands.rs:448` returns `Result<Vec<library::EpisodeRow>>` and lets serde emit
the whole struct. Same data, same call, two serialisation styles: the one that names its
fields is safe and the one that derives is not.

That is the strongest argument for fixing [R-126] at the struct — `#[serde(skip)]` on
`EpisodeRow::url`, so neither style can leak it — rather than at each call site. And the
test above is the model for the missing one: an equivalent assertion over the Tauri command
results would have caught it.

### R-127 — Low — the assistant can show a card for a title it never looked up

`src-native/crates/aurora-app/src/assistant.rs:18` and `:616`

The module header states the invariant that makes the cards trustworthy:

> Every card the viewer sees came from `Tool::ShowTitles`, and the ids it takes are **ids
> this module handed out in a previous tool result**. A title the model made up has no id,
> so it cannot become a card.

What is enforced is one step weaker. `ShowTitles` resolves each id against the library:

```rust
// Resolved here, not trusted: an id the model invented, or one from a row that has
// since been removed by a refresh, must not reach the UI as a card with a play button
// that cannot work.
match resolve_item(db, kind, id, note) {
    Some(item) => items.push(item),
    None => refused.push(json!({ "kind": kind, "id": id })),
}
```

`resolve_item` checks the row **exists**, not that it was handed out. So `show_titles` with
`{"kind":"movie","id":1,"note":"a slow-burn thriller you'll like after Heat"}` — ids the
model can simply guess, and low integers always resolve on a 27,494-film library —
produces a real, playable card carrying a model-written note about a film it never
retrieved and knows nothing about.

The consequence is not a broken play button, which is what the inline comment guards
against and does prevent. It is that the *note* — the one sentence the system prompt
insists on, "a short note saying why it suits THIS person" — can be attached to an
arbitrary title. The card looks exactly as trustworthy as a researched one.

Closing it is cheap, because the data already exists: `run_tool` returns the ids it hands
out, so a per-`send` set of issued `(kind, id)` pairs, with `ShowTitles` refusing anything
not in it, makes the header's claim true. The `not_in_library` channel for telling the
model what was refused is already built.

### The assistant, reviewed — the design holds, and this is where the care went

`src-native/crates/aurora-app/src/assistant.rs` (1,682 lines)

Recorded because it is the largest hand-written file in the host and the only one that
sends anything off the machine, so "nothing found" is worth as much as a finding.

The three bounds the header promises are real and enforced: `MAX_ROUNDS = 6` on the
ask-and-lookup loop, `MAX_CALLS = 14` across a message, `MAX_ROWS = 25` per lookup — and
there is a test asserting the row cap holds through a real round
(`:1505`–`:1516`). `KEEP_TURNS = 40` trims the stored conversation.

Not persisting tool calls is correct and the reason given is the right one: Gemini requires
a call and its response as a matched pair, so trimming a history that contains them by age
is how it becomes invalid. Looking the facts up again is cheap and current.

What goes to Google matches `docs/PRIVACY.md` claim for claim: the digest is counts, shelf
names and genres (`digest_line`, `:711`); the tools return titles, years, ratings, genres
and overviews; the system prompt (`:686`) carries no identifier of any kind. No `url`, no
`username`, no `password`, no `credential` appears anywhere in the file outside test
fixtures — verified by sweep as well as by reading.

The system prompt's last line is also the right instruction for an assistant with no
actuators — *"Never claim to have started playback, changed a setting, or recorded
anything. You can offer titles; the person presses play."* — and it is backed by
construction rather than by hope: there is no tool that does any of those things.

One open item from the earlier part of this review stays open: **[R-73]**, whether Gemini
rejects parallel function calls encoded as separate model turns, still needs a live request
with the viewer's key and has deliberately not been spent.

### R-128 — Low — the updater's two loopback checks can be bypassed with URL userinfo, and the defence against it is written down one branch away

`src-native/crates/aurora-ingest/src/updates.rs:308` and
`src-native/crates/aurora-app/src/updates.rs:168`

The `Releases` branch of `is_allowed_asset_url` is right, and its comment names the attack
class precisely:

```rust
/// The trailing slash in the prefix is load-bearing: without it
/// `https://github.com.example.invalid/...` and `https://github.com@example.invalid/...`
/// both pass.
AssetOrigin::Releases => {
    url.starts_with(&format!("https://github.com/{repo}/releases/download/"))
}
```

The `/` immediately after `github.com` is what makes `github.com@evil` fail, and there is
a `url.contains("..")` guard above it. That branch is sound.

The `Loopback` branch, four lines below, is a prefix list — and the trailing-slash trick
that saves the branch above is not available to it, because a loopback URL legitimately
continues with a port:

```rust
// Loopback only, and with the port separator, so `http://127.0.0.1.evil/` and
// `http://localhost.evil/` are still refused.
AssetOrigin::Loopback => ["http://127.0.0.1:", "http://localhost:", "http://[::1]:"]
    .iter()
    .any(|prefix| url.starts_with(prefix)),
```

The comment's claim is true as far as it goes — `127.0.0.1.evil` has no colon there — and
incomplete. With a colon and then userinfo, the host is somewhere else entirely. Measured
against RFC 3986 authority parsing:

```
value                                          passes loopback check  actual host
http://127.0.0.1:8080/                         True                   127.0.0.1
http://127.0.0.1:8080@evil.example/            True                   evil.example
http://localhost:1234@evil.example/api         True                   evil.example
http://127.0.0.1:0@198.51.100.7:80/            True                   198.51.100.7
http://evil.example/                           False                  evil.example
```

`test_api_base` (`aurora-app/src/updates.rs:168`) uses the same three-prefix list for the
same purpose and has the same hole. Its comment states the property it is trying to hold:

> **Loopback only.** An updater that can be pointed anywhere by an environment variable is
> a way to serve somebody a signed-looking binary, because the digest that verifies the
> download comes from the same answer that named it.

That reasoning is exactly right, which is why the check not holding matters.

**Why this is Low and not higher.** Reaching either site requires `AURORA_UPDATE_API` to be
set in the app's environment, which means the attacker already runs code as that user — a
threat the comment explicitly puts out of scope ("useless to anyone who is not already
running code on it"). And the `Releases` branch still gates the asset URL in a normal
build, so redirecting the API alone does not get a binary executed: `download_asset`
(`:344`) refuses an asset URL that is not under
`https://github.com/<repo>/releases/download/`, and refuses outright when the release
publishes no SHA-256 at all:

```rust
let Some(want_sha) = expected.sha256.as_ref() else {
    return Err(refuse("This release does not publish a checksum",
        "Aurora will not install an update it cannot verify. ..."));
};
```

So this is a weakness in a defence-in-depth layer, not an exploitable path. It is recorded
because the layer exists specifically to be robust, the author demonstrably understands the
`@` form — they wrote the comment about it — and the fix is to compare a parsed host
instead of a string prefix, in both places, which also lets the two copies become one.

### The updater, reviewed — the parts that matter are right

`src-native/crates/aurora-app/src/updates.rs` (1,262 lines) and the `aurora-ingest` half

Recorded because this is the one subsystem that writes an executable and then runs it, so
"nothing found" is worth stating. Apart from [R-128]:

* **Nothing installs unverified.** `refusal` (`:652`) refuses unless
  `status == DownloadStatus::Ready`, and `Ready` is only reachable through
  `fetch_verified`. There is a named test for it:
  `nothing_installs_until_something_verified_is_on_disk` (`:938`).
* **The release must publish a digest.** No checksum, no download — stated in the refusal
  text the viewer actually sees, with the release page offered as the alternative.
* **The asset URL is pinned to this repository's releases**, with a `..` guard, because the
  file "is about to be written to disk and executed."
* **`RELEASES_URL` is a constant, deliberately not the API's `html_url`** (`:37`): *"a
  command that opens whatever URL it is handed is a way to make the app launch something
  else."*
* **The staged path never crosses to the UI.** `Download::path` carries
  `#[serde(skip)]` (`:88`) with the reason given — *"the install command finds the file
  from here rather than being handed a path, so there is no path to get wrong"* — and a
  test, `the_path_is_never_sent_to_the_ui` (`:1014`).
* **A recording in progress postpones the install** (`:673`), in a sentence, with a test
  (`:968`). "A recording is a thing that cannot be redone. The update can wait."
* **The swap happens in `main` before Tauri starts** (`:749`), when the app holds no
  library, log or video surface open, with a filesystem breadcrumb because the logger does
  not exist yet at that point.

That `#[serde(skip)]` is also the direct answer to [R-126]: the same attribute, for the
same reason, applied here to a path and not there to a stream URL.

### R-129 — Low — two rapid tile changes can each pass a budget check the other invalidates

`src-native/crates/aurora-app/src/mosaic.rs:481`

`set_tile` takes and releases `self.session.lock()` three separate times: once to read the
layout and whether the tile was empty (`:485`), once inside the budget check to count
filled tiles (`:499`), and once to stop the outgoing instance (`:523`). The budget decision
and the build that acts on it are not under one lock.

`#[tauri::command(async)]` means two `mosaic.setTile` calls can be in flight at once, so two
clicks on different empty tiles in quick succession can both read `filled = n`, both
compute `n + 1 <= limit`, and both proceed — landing at `n + 2`. On a line with exactly
enough connections for the layout that is one stream over, which is the single thing this
module exists to prevent.

Holding the lock across the check and the build closes it. The surrounding code is
otherwise careful about exactly this class of problem and says so — the outgoing instance
is stopped *before* the replacement is built, with the reason written down:

> The old instance goes before the new one is built, so the two never hold a connection at
> the same time — on a line with exactly enough for the layout, building first would be one
> over the limit for as long as the load takes.

So the single-tile overlap was thought about and the two-call overlap was not.

Not currently reachable, for an unhappy reason: [R-108] means `self.limit()` is always
`None`, `Budget::check` always returns `Unknown`, and no budget check in this file can
refuse anything. It becomes reachable the moment [R-108] is fixed, which is why it is
recorded now rather than later.

### Multi-view's host half, reviewed — careful, and its premise is defeated upstream

`src-native/crates/aurora-app/src/mosaic.rs` (1,530 lines)

The module header is the clearest statement in the codebase of what [R-108] costs:

> A 3×3 mosaic is nine simultaneous streams. The subscription this project was measured
> against allows **one**. A provider does not queue the surplus, it refuses it — or cuts a
> stream already running, including a recording in flight — so opening optimistically and
> letting the tiles fail would spend somebody's DVR to draw eight error messages.
>
> Where no provider declares a limit — which is every M3U playlist, so the common case
> rather than the odd one — the attempt is allowed and the UI says it may not work.

The second paragraph is the assumption [R-108] breaks: `Unknown` was designed as the M3U
case, and because `providers.max_connections` is never written it is the *only* case, on
Xtream too. Everything built on top of it — `Budget::Exceeds`, `refusal()`, `largest_fitting`,
the per-layout warnings in `MosaicPage` — is unreachable. See [R-108] and [R-123].

What the file itself gets right, apart from [R-129]:

* `open` closes an existing session first, with the reason stated, so two mosaics can never
  both hold connections (`:302`).
* Every tile failure is contained: *"A channel that was deleted by a refresh, a channel
  with no sources, and a stream the provider refuses are three ordinary things, and one of
  them must not take the other eight tiles down with it"* (`:340`), and `TileView.error`
  carries the reason per tile.
* Audio lands on the first tile that actually has a stream, not on tile 0 unconditionally
  (`:325`) — *"focusing an empty tile would mean a mosaic with a picture and no sound"*.
* `channel_at` deliberately does **not** close the mosaic when promoting a tile (`:552`):
  the caller tunes first and closes on success, so a failed promotion leaves the picture up.
* `TileFactory` is injected so the budget, geometry, audio-focus and no-sources logic can
  be tested with `NullBackend`s instead of only ever running on Windows with a live
  subscription — which is why this module has real coverage and [R-72]'s supervised
  threads do not.

### R-130 — High — one guide refresh moves 14.8 MB across the IPC bridge, and every toolbar press does it again

`src-ui/src/features/guide/GuidePage.tsx:138` with `src-native/crates/aurora-app/src/commands.rs:106`

```ts
const { data: slice, loading } = useCommand(
  'epg.gridSlice',
  { from, to, channelIds: [] },   // [] means every channel
  [from, to],
);
```

`channelIds: []` is read by the host as "all of them":

```rust
// commands.rs:115
let wanted: Vec<channels::ChannelRow> =
    if args.channel_ids.is_empty() { all } else { all.into_iter().filter(...).collect() };
```

and `to = from + WINDOW_HOURS * 3600` with `WINDOW_HOURS = 24`. So every fetch is *every
channel* and *every programme in a 24-hour window*, in one reply.

Measured against the live panel — the real 165 MB XMLTV and the real channel list, projected
into exactly the `GuideSlice` shape `commands.rs:98` serialises:

```
channels with an epg id: 1,549 distinct (of 6,699 channels)
programmes in the file: 529,638, covering 6.0 days
programmes overlapping a 24h window on a channel we have: 35,576
  programmes half of the payload: 13,735,695 bytes = 13.10 MB
  channels half (measured in R-117):                  1.72 MB
TOTAL one epg.gridSlice reply:                       14.82 MB
```

**14.8 MB per fetch**, serialised by serde, passed through Tauri's IPC, parsed by
`JSON.parse` in the WebView, and held in a React state object.

And it is re-fetched on every change to `from`, which is what all eight navigation controls
do (`:398`–`:412`): **Now**, **−24h**, **−1h**, **+1h**, **+24h**, and three prime-time
buttons, each going through `jumpTo` → `setFrom`. Walking forward an hour at a time across
an evening is a 14.8 MB round trip per press.

Two things make the waste larger than the number suggests:

* **The virtualiser renders about a dozen rows.** `rowVirt` has `overscan: 6` over
  `ROW_H = 64`, so on a 950px-tall window roughly 14 of 6,699 channel rows are ever in the
  DOM. Over 99% of the payload is parsed and then never read.
* **The category filter is client-side.** `:144` does
  `group ? all.filter((c) => c.group === group) : all`, so choosing "UK | SPORTS" out of 67
  groups narrows what is drawn and not what is fetched — the next `from` change still pulls
  all 6,699 channels and all 35,576 programmes.

**A correction to an earlier entry in this review.** It recorded the guide slice as verified
fine at "13–15 ms". That measurement was the SQLite query, and it stands — `grid_slice` with
an index on `(channel_id, start)` really is that fast. It says nothing about serialisation,
transport or parse, which is where the 14.8 MB goes. A fast query and a huge reply are
independent, and only one of them was measured.

The fix is the argument that already exists and is never used: the UI knows which channel
rows the virtualiser has mounted — `LivePage`/`useNowNext` does exactly this, passing
`visibleIds` for the visible window and caching what it learns (`useNowNext.ts:27`). Passing
`channelIds` for the mounted rows plus overscan turns 14.8 MB into roughly 30 KB. The group
filter belongs on the query for the same reason. Note that [R-91] is the other half of this
trade — `useNowNext` fetches narrowly and then never refreshes — so the two screens have
opposite bugs and the right answer is between them.

### R-131 — Info — a hidden element rendering a timestamp to satisfy a linter

`src-ui/src/features/guide/GuidePage.tsx:460`

```tsx
<span style={{ display: 'none' }}>{from}</span>
```

Last line of `TimeHeader`, whose props are `{ slots, from }` and which uses only `slots`.
The element exists so `from` counts as used. It renders a Unix timestamp into the DOM on
every guide paint, inside the sticky header, where it is invisible but present — and it is
in the accessibility tree's text content unless a screen reader honours `display: none`,
which they do, so the practical cost is nil.

Recorded because the honest fix is to delete the prop, and because a reader hunting for why
a timestamp appears in the markup will not guess that it is there for a lint rule.

### R-132 — Medium — five guide features are dead against this provider's guide, measured over 529,638 programmes

`src-ui/src/features/guide/GuidePage.tsx:574`, `:670`, `:671`, `:877`–`:888`,
`src-ui/src/features/player/ChannelBanner.tsx:61`

Counted directly in the live 165 MB XMLTV:

```
<category          0 occurrences
<episode-num       0 occurrences
<premiere          0 occurrences
<new/>             0 occurrences
<star-rating       0 occurrences
<credits           0 occurrences
<rating            0 occurrences
<sub-title         0 occurrences
<desc        529,638 occurrences
<title       529,638 occurrences
```

Title and description, for every one of 529,638 programmes, and nothing else. So on this
subscription:

* **`genreTone` always returns the default.** `:575` colours a block by matching "sport",
  "movie", "news", "kid" or "document" in `p.categories`, which is always `[]`, so every
  block in the grid is `var(--surface)`. The function's doc comment is *"Genre colour
  coding with a legend (README §7.1)"* — and there is no legend anywhere in the file
  (`grep -n legend` finds only that comment), so the colour coding is inert and the legend
  was never built.
* **The "New" badge never appears** (`:670`, `:877`): `p.isNew` comes from `<premiere>` /
  `<new/>`, neither of which occurs.
* **The "Live" badge never appears** (`:671`, `:878`, and `ChannelBanner.tsx:61`).
* **The "S1E2" suffix never appears** (`:683`): `p.season`/`p.episode` come from
  `<episode-num>`.
* **The programme rating badge never appears** (`:880`), and `prog.subTitle` is always null
  so the sub-title line at `:887` never renders — which also means `recordNow`'s
  `subTitle: programme?.subTitle ?? null` (`PlayerOverlay.tsx:64`) is always null and every
  recording is filed under its title alone.

None of this is a bug in the code: each is correctly guarded with `&&`, so nothing breaks
and nothing is drawn. It is dead surface, and it matters for two reasons. First, the grid is
a wall of identically-coloured blocks where the design intends genre colour as the primary
scanning aid. Second, it is five more instances of the pattern this review keeps finding —
a feature that works against the fixtures (`fixtures.ts` populates categories, `isNew` and
`isLive`) and cannot work against the data.

Worth distinguishing from a near-miss: `App.tsx:432` and `:467` also branch on `isLive`, but
that is `PlayerState.isLive`, set by the player backend for a live stream. Those branches
are live and correct; only the `Programme.isLive` uses are dead.

**A figure in a comment that does not match the data.** `:608` says:

> On a real subscription this is most of it: EPG coverage measured 42.8% on the panel this
> was built against, so whichever end of the list you open is likely to be blank.

Measured on this panel today, three different ways, none of which is 42.8%:

```
channels with a non-empty epg_channel_id : 1,825 of 6,699 = 27.2%
distinct epg ids among them              : 1,549
channels with any programme in a 24h window : 1,198 of 6,699 = 17.9%
```

The conclusion the comment draws is right and if anything understated — the "No guide data
for this channel" strip it justifies is correct and valuable. Only the number is off, and
since 42.8% may well be a different panel or an earlier import, this is recorded as a figure
to re-derive rather than as an error. The honest source for it is
`EpgCoverage` (`library.rs:562`), which computes it from the database; see the [R-121]
addendum.

### R-133 — High — "Record series" creates a rule that can never record anything

`src-ui/src/features/dvr/useDvrMarks.ts:128`

```ts
await invoke('dvr.createRule', {
  title: p.title,
  channelId: anyChannel ? null : ch.id,
  newOnly: true,          // hardcoded, with no UI to change it
});
```

`newOnly` gates every match:

```rust
// aurora-core/src/dvr.rs:218
if self.new_only && !programme.is_new {
    return false;
}
```

and `is_new` has exactly two sources in the whole workspace, both in the XMLTV parser:

```
aurora-core/src/xmltv.rs:205   p.is_new = true;     // <new/>
aurora-core/src/xmltv.rs:345   p.is_new = true;     // <premiere>
```

There is no Xtream-API fallback and no other writer. [R-132] counted both elements in the
live 165 MB feed: **`<new/>` 0 occurrences, `<premiere>` 0 occurrences, across 529,638
programmes.**

So `programme.is_new` is `false` for every programme this subscription publishes, every
rule created from the guide has `new_only = true`, and `SeriesRule::matches` returns `false`
at its second check — always. **No series rule created through the UI will ever record an
episode.**

Everything around it works, which is what makes it hard to notice:

* `dvr.createRule` succeeds and the rule is stored.
* The guide button flips to `variant="primary"` and reads "Stop recording series"
  (`GuidePage.tsx:952`–`:956`), because `ruleFor` finds the rule by title.
* `RecordingsPage`'s Series rules tab lists it, with its channel and "New episodes only"
  (`:493`), and a count of `0 recorded` — which [R-119] shows is a count of *all* rows for
  the rule, so zero there really does mean nothing was ever even scheduled.

The viewer gets every signal that a series recording is set up and no episode ever lands in
Recordings. Nothing logs a reason, because nothing failed.

`newOnly` is not reachable from any UI: `grep newOnly src-ui` finds exactly two hits — this
hardcoded `true`, and the read-only label in `RecordingsPage`. So there is no way to create
a rule that works, and no way to repair one that does not.

Three fixes, in order of how much they are worth:

1. **Default `newOnly` to `false`.** It is the only value that can work against a feed with
   no `<new/>` markers, which per [R-132] is this provider and, on the evidence of what
   XMLTV feeds typically carry, most of them. A rule that records repeats is a far better
   failure than a rule that records nothing.
2. **Offer it.** The rule model supports `newOnly`, `weekdays`, `aroundLocalMinute` and
   `keepEpisodes` (`shared/ipc.ts:621`), `RecordingRule` renders all four, and the UI can
   set none of them. A "Record series…" dialog would make the whole feature reachable.
3. **Say when a rule cannot match.** The host knows both facts — the rule wants new
   episodes, and no programme in the database has `is_new` set — so the Series rules tab
   can say "this rule needs programmes your guide does not flag as new" instead of "0
   recorded".

This compounds with [R-118]: the DVR cannot play back what it records, and its series rules
never fire in the first place.

### R-134 — Medium — the whole app re-renders four times a second while anything is playing, and the diagnosis for it is thirteen lines away

`src-ui/src/App.tsx:97` and `:481`

```ts
const ui = useUi();          // no selector: subscribes to the entire store
```

`useUi()` with no selector subscribes `App` to every field of the store — `player`,
`banner`, `digits`, `detail`, `paletteOpen`, `paletteQuery`, `catalogVersion`, `theme`,
`density`, `animations`, `hoverPreviews`, `use24h`. `App` renders the whole route tree, so
any write to any of those re-renders the mounted page.

The author measured the rate and wrote it down — in the comment on the *banner* effect,
thirteen lines below the dependency array that still has the problem:

> `ui` cannot be a dependency. `useUi()` subscribes to the whole store, so every unrelated
> write produces a new snapshot — and while the player is open the host's heartbeat writes
> `setPlayer` **four times a second**. The effect re-ran on each one, cleared the pending
> timeout and started a fresh five seconds, so the deadline was never reached and the
> banner stayed on screen for the whole programme.

That fix is correct and complete for the banner: `hideBanner` is taken as a stable action
and the effect depends on `bannerUntil` rather than on `ui` (`:507`–`:512`). The same root
cause is untouched two places:

1. **`handlers`** (`:389`–`:481`) lists `ui` first in its dependency array, so the memo is
   rebuilt on every heartbeat, and `useHotkeys(handlers, …)` depends on `[h, enabled]`
   (`useHotkeys.ts:83`) — so the global `keydown` listener is removed and re-added roughly
   four times a second for as long as something is playing. React runs the cleanup and the
   setup back to back in one flush, so this does **not** drop keypresses; it is pure churn,
   not a correctness bug.
2. **`App` itself** re-renders at the same rate, and with it `<Routes>` and the mounted
   page. The route elements are rebuilt as fresh JSX each time with inline closures —
   `onPreview={(ch) => tune(ch, { reveal: false })}`, `onSearch={(q) => ui.setPalette(true, q)}`
   (`:749`, `:753`) — so the page cannot bail out on reference equality either. On the guide
   that means re-running `rowVirt.getVirtualItems()` and rebuilding every visible row's JSX
   four times a second, on top of the 14.8 MB slice [R-130] leaves in state.

`App` genuinely needs `ui.player` (for `showingPicture`), `ui.detail`, `ui.banner`,
`ui.paletteOpen` and `ui.digits`, so it cannot avoid re-rendering on a heartbeat entirely.
What it can avoid is re-rendering on the other seven fields, and `handlers` can avoid being
rebuilt at all: every action it calls (`pushDipgit`, `setPalette`, `openDetail`,
`showBanner`) is a stable zustand action reachable through `useUi.getState()`, which is the
pattern `closePlayer` (`:215`) and the banner effect already use. Selector subscriptions
(`useUi((s) => s.player)`) for the five fields it reads would finish it.

Recorded as Medium rather than High because nothing is incorrect — it is wasted work on the
hottest path in the app, in a file that already contains the correct analysis of why.

### R-135 — Info — two contradictory doc comments on the same function, about error handling

`src-ui/src/App.tsx:271`–`:291`

`playCatchup` carries two stacked doc blocks. The first:

> Not routed through the zapper: the zapper tunes a channel to its live edge, and this is
> the opposite request. **The error is surfaced rather than swallowed** because the three
> ways catch-up can refuse … are things the viewer can act on.

and immediately after it, the second:

> The refusal is deliberately **not** turned into a toast here: the Guide catches it and
> prints it beside the programme it is about … So this rethrows. A caller that does not
> handle it is caught by the global `unhandledrejection` backstop rather than by silence.

The code matches the second one — `playCatchup` has no `catch`, and
`GuidePage`'s `InfoPane` catches it into `catchupError` and renders it beside the button
(`GuidePage.tsx:921`–`:934`). So the behaviour is right and documented right; the first
block is a superseded version of the same comment that was never deleted, and it says the
opposite thing about the one decision that matters.

Worth a line because both paragraphs read as deliberate, and the next person to touch
catch-up error handling has to work out which of the two the code actually does.

### R-136 — Low — a 12/24-hour clock preference that nothing reads, nothing sets, and no caller can reach

Three places implement half of it and nothing joins them up:

```ts
// state/ui.ts:14, :66   — declared and initialised
use24h: boolean;
...
use24h: true,

// lib/format.ts:5       — honoured, if anyone passed it
export function clockTime(unix: number, use24h = true): string {
  const d = new Date(unix * 1000);
  if (use24h) return `${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
  const h = d.getHours() % 12 || 12;
  return `${h}:${pad2(d.getMinutes())} ${d.getHours() < 12 ? 'AM' : 'PM'}`;
}
```

Swept across `src-ui`:

* `use24h` appears in exactly two files — the store declaration and `format.ts`'s parameter
  name. No component reads it, and there is no `setUse24h`/`toggle24h` action beside
  `setTheme`, `setDensity`, `toggleAnimations` and `toggleHoverPreviews`.
* **Zero** of the twenty-odd `clockTime(...)` call sites pass a second argument, so the
  `use24h` default of `true` always wins and the 12-hour branch is unreachable code.

So every time in the app — the guide's time header and programme blocks, Live TV's now/next,
the channel banner, the OSD clock, the recordings list — is 24-hour, permanently, with a
store field and a function branch standing by. For a viewer in a 12-hour locale that is the
whole of the app's time display in a format they do not use, and the fixtures are
US-flavoured ("US ★ QVC HD"), so the intended audience includes them.

**And `TopBar` does not use `clockTime` at all.** `App.tsx:968` formats inline:

```tsx
{String(clock.getHours()).padStart(2, '0')}:{String(clock.getMinutes()).padStart(2, '0')}
```

— a fourth copy of the same two lines, which is how a preference added to `format.ts` would
still miss the most prominent clock in the interface. The fix is three small things in one
go: a `toggle24h` action, `clockTime(t, use24h)` at the call sites (or a `useClock()` hook
that closes over the setting), and `TopBar` calling it like everything else. Note this sits
on top of [R-87]: even once set, the preference would not survive a restart.

### R-137 — Low — a window resize re-places the inlaid picture without re-clamping it

`src-native/crates/aurora-app/src/pip.rs:128` with `:204` and `:211`

`set_inlay` clamps the rectangle a page asks for, and says why at length:

> Clamped rather than trusted. A page measures its own layout, and a box that is
> mid-animation, mid-scroll or momentarily zero-sized will produce a rectangle that is
> partly outside the window — and **a surface placed outside its parent is not a small
> mistake on Windows, it is a child window nobody can see and no amount of re-measuring
> brings back.**

```rust
pub fn set_inlay(&self, rect: Option<Rect>) -> Result<PipView> {
    let ((w, h), _, _) = self.snapshot();
    *self.inlay.lock() = rect.and_then(|r| clamp_to_window(r, w, h));   // clamped
    self.apply()
}
```

`apply` then places the stored rectangle verbatim:

```rust
} else if let Some(rect) = inlay {
    player.place(rect)?;        // no clamp
```

and `relayout` — called from the window's `Resized` handler for every resize, PiP on or
off — goes straight to `apply`:

```rust
pub fn relayout(&self, width: u32, height: u32) -> Result<PipView> {
    self.set_window(width, height);
    self.apply()
}
```

So the rectangle is validated against the window size at the moment the page measured it,
and re-applied against every later size without re-validation. Dragging the window
smaller while the guide's preview is inlaid places the surface at a rectangle that is now
partly or entirely outside the client area — which is precisely the state the comment on
`set_inlay` calls unrecoverable.

In practice the page rescues it: `useInlaidPicture` holds a `ResizeObserver` and a capture
`scroll` listener (see [R-93]), so a resize changes the box's measured rect and a fresh
`preview.place` arrives with a valid one. That makes this a window of a frame or two rather
than a permanent black box — and it also means one of the two statements has to be wrong,
since the comment says re-measuring does not bring it back. Both readings point at the same
fix: clamp inside `apply`, which is already the single place that touches the backend's
geometry and already holds the size it needs. `set_inlay` can then stop clamping, leaving
one copy of the rule.

### Picture-in-picture, reviewed — the best-documented file in the codebase

`src-native/crates/aurora-app/src/pip.rs` (832 lines)

Apart from [R-137], nothing. Worth recording because of what it gets right:

* **`snapshot()` exists to prevent a specific deadlock, described in full** (`:86`–`:106`):
  a struct literal holding a `parking_lot` guard from one field while a second field's
  getter re-locked the same mutex. Non-reentrant, so it deadlocked permanently, on the main
  thread, from the window's `Resized` handler — *"That is what shipped in 1.0.1: a window
  that drew, a UI that mounted, and then no command, event or asset request ever answering
  again — 'Opening your library…' forever."* One function now takes both locks, and every
  public method goes through it.
* **The precedence between PiP and an inlay is stated once, in `apply`** (`:140`), with the
  reasoning: a corner beats an inlay because PiP is a mode the viewer chose and an inlay is
  a page asking opportunistically.
* **`toggle` and `set_enabled` roll the flag back if the surface will not move** (`:161`,
  `:174`), so the UI never draws a small frame around a full-window picture.
* **`clamp_to_window` is total**, computes edges before sizes so nothing underflows, and
  returns `None` for a rectangle with no area left — with the reason it has a test per
  branch: *"the failure it prevents is invisible … the symptom is 'the preview is black'
  with nothing wrong anywhere a log would look."*
* **PiP is refused while a mosaic is open**, in words, rather than silently
  (`:276`–`:283`).
* The module header gets the distinction from a mosaic exactly right: *"It is the same
  stream, moved. No second instance, no second connection, nothing to refuse — which is …
  the reason there is no budget check anywhere in here."* That is also why [R-108] does not
  reach this file.

### R-138 — Low — changing the timeshift folder orphans whatever is in the old one

`src-native/crates/aurora-app/src/timeshift.rs:153`

```rust
if let Some(folder) = &args.folder {
    let trimmed = folder.trim();
    if trimmed.is_empty() {
        settings::clear(db, settings::GLOBAL, FOLDER_KEY)?;
    } else {
        settings::set(db, FOLDER_KEY, &trimmed.to_string())?;
    }
}
```

The setting is rewritten and nothing is done about the old directory. `bytes_on_disk`
(`:97`) and `clear` (`:113`) both read the folder from settings, so after the change they
describe the *new* folder only:

* **Settings reports 0 B** for a buffer that may hold up to `MAX_BYTES` on the old path.
* **"Empty the buffer" cannot reclaim it** — `timeshift_clear` resolves `folder(&db, …)`
  and clears that, so the old files are unreachable from the UI for ever.

`TimeshiftPanel.tsx` offers the folder as a free-text field with Apply and Reset
(`:95`–`:115`) and says nothing about it, so the viewer who moves the buffer to another
drive leaves a gigabyte behind with no indication it exists. `TimeshiftPanel.tsx:125`
renders `bytes(settings.bytesOnDisk)` beside a "Empty it" button, which will read 0 B and
free nothing.

Either clear the old folder on the way out — the buffer is a cache, so deleting it is
always safe, and `clear` already refuses files mpv has open for the right reason — or say
in the panel that the old location keeps its files. Clearing is the better answer and is
two lines: resolve the folder before writing the setting, and call `clear` on it after.

### Verified against the real panel: four concurrent listing requests are fine

`src-native/crates/aurora-app/src/series.rs:32` — `SWEEP_CONCURRENCY: usize = 4`

Recorded because it looks like it should be a problem and is not. The episode-listing sweep
makes four concurrent `get_series_info` calls to the viewer's own panel, and this
subscription advertises `max_connections: 1` and answers **HTTP 555** when that is
exceeded — established earlier in this review by holding one stream open. The comment
justifies the number on latency and on not competing with playback, and says nothing about
the connection limit.

Measured against the live panel, four real series ids:

```
SEQUENTIAL                       CONCURRENT, 4 at once
  31554 -> 200 in 0.342s           31554 -> 200 in 0.275s
  44329 -> 200 in 0.418s           45916 -> 200 in 0.274s
  45916 -> 200 in 0.321s           44329 -> 200 in 0.393s
   5733 -> 200 in 0.536s            5733 -> 200 in 0.556s
```

All eight succeeded, and concurrency made them no slower. So **`max_connections` governs
streams, not `player_api.php` calls** — four concurrent API requests are served normally —
and the comment's "a listing came back in about 0.3s" is confirmed to the tenth of a
second. `SWEEP_CONCURRENCY = 4` is safe and no change is needed.

This also refines the earlier HTTP 555 finding in this review: the 555s correlated with
rapid *requests*, and the distinction that matters is between the API and the streams. The
API tolerates parallelism; the stream endpoints are what the limit counts.

**A note on how this nearly went wrong.** The first run of this test reported one `200` and
seven `000`s, which reads exactly like a panel serving one request and dropping the rest —
and I was a paragraph away from writing it up as a High finding. `curl --show-error` gave
the real reason: `URL rejected: Malformed input to a URL function`. The id list had been
written by Python on Windows, so `tr '\n' ' '` left a carriage return inside each URL. The
one id that "succeeded" was the last one, which had no trailing `\r`. A result that confirms
a hypothesis is the one most worth re-running.

### R-139 — High — the provider sends every episode's runtime and plot, the parser reads them, and the import throws them away — which is why Up Next and Skip Credits do not fire

`src-native/crates/aurora-db/src/repo/library.rs:708` with
`src-native/crates/aurora-app/src/series.rs:176`

```rust
pub struct NewEpisode {
    pub season: u16,
    pub episode: u16,
    pub title: Option<String>,
    pub url: String,
    pub still: Option<String>,
}
```

Five fields. The `episodes` table has nine (`schema.rs:156`):

```sql
id, series_id, season, episode, title, overview, still, runtime_mins, air_date, url, added_at
```

so `overview`, `runtime_mins` and `air_date` are never written by an import.

**The data is there and already parsed.** `aurora-core/src/xtream.rs:303`–`:315` defines the
episode's `info` block with a lenient deserializer per field:

```rust
#[serde(default, deserialize_with = "lenient_string")]
pub plot: Option<String>,
#[serde(default, alias = "movie_image", alias = "cover_big", deserialize_with = "lenient_string")]
pub movie_image: Option<String>,
#[serde(default, deserialize_with = "lenient_u32")]
pub duration_secs: Option<u32>,
```

and there is a test fixture carrying `"duration_secs":2700` (`:551`). Measured against the
live panel, one real series (`get_series_info`, 30 episodes):

```
episode info keys: audio, bitrate, duration, duration_secs, movie_image,
                   plot, rating, releasedate, season, tmdb_id, video
  with movie_image : 30/30 (100%)
  with a duration  : 30/30 (100%)   duration_secs: 3563, duration: "00:59:23"
  with a plot      : 29/30  (96%)
```

`series.rs:176` then builds a `NewEpisode` from exactly three of those — `title`,
`container_extension` (into the URL) and `movie_image` — and `plot` and `duration_secs` go
nowhere.

**Why that matters more than two blank fields.** `library_playback_aids`
(`commands.rs:885`) is built on the stored runtime as its fallback, and says so:

> The player's own number first, because it is the file in front of us. But a provider's
> VOD stream very often reports no duration — mpv has nothing to read it from until the
> whole thing is buffered, and on some streams never — and the consequence was that an
> episode with no chapters and no learned markers had no Up Next either, so autoplay never
> fired on an ordinary library. **The episode's own runtime, which enrichment or the
> provider supplied, is the fallback.**

```rust
let duration = if args.duration_secs > 0.0 {
    Some(args.duration_secs)
} else {
    library::episode_runtime_secs(&db, args.episode_id)?   // reads episodes.runtime_mins
};
```

The provider never supplied it to the database, so the fallback only works once TMDB
enrichment has written `runtime_mins` — which needs a TMDB key and a sweep that has reached
that episode. On a build you compile yourself there is no key at all
(`docs/PRIVACY.md`), and on a fresh install the sweep has reached nothing.

So on exactly the case the comment describes — a provider VOD stream that reports no
duration — `duration` is `None`, `up_next_at(&resolved, 0.0, 45.0)` has nothing to work
from, and **Up Next, autoplay-next and Skip Credits are all dead**. That is the bug the
comment says was fixed, and the fix is complete except for the one line that would have
stored the number.

It is also the precise scenario the mock was built to reproduce. `mock.ts:2536` on
`setSilentDurationMock`:

> Not a hypothetical: a provider's VOD stream very often reports no duration … That is the
> state in which Skip Credits and autoplay were dead, and it was invisible here because
> every fixture plays with a known runtime. **The host answers `library.playbackAids` from
> the episode's *stored* runtime in this case, which the mock already mirrors** — so
> flipping only the player's number reproduces the real asymmetry rather than a made-up
> one.

The mock mirrors a stored runtime that the host, against a real import, does not have. So
the test seam built to catch this asserts the fixed behaviour against fixture data that has
the field, and the real path does not.

**Two smaller consequences of the same omission**, both in the episode list of the detail
modal: `ep.overview` (`DetailModal.tsx:736`) and `runtime(ep.runtimeMins)` (`:726`) are
blank for every Xtream-imported episode, so the list is title-and-still only until
enrichment runs — while the panel sent a plot for 96% of them.

The fix is four lines: `overview` and `runtime_mins` on `NewEpisode`, filled from
`ep.info.plot` and `ep.info.duration_secs / 60`, and carried into the `upsert_episodes`
INSERT. `air_date` is available too, from `info.releasedate`.

### R-106 — correction — episode stills are not TMDB-only, so the zero-height box is narrower than recorded

[R-106] states that an episode's progress bar is clipped to nothing when `ep.still` is
null, and justified its reach like this:

> Episode stills come from TMDB enrichment and nothing else, so `ep.still` is null for every
> episode of every show until a TMDB key is configured and the metadata sweep has reached
> that show.

**That is wrong.** `series.rs:183` fills `still` from `ep.info.movie_image`, which the
parser accepts under three aliases (`movie_image`, `cover_big`), and the live panel supplies
it for **30 of 30** episodes in the sample above — served from TMDB's own CDN by the
provider, which is why it looked like enrichment.

The layout defect is unchanged and still real: a container whose only in-flow child is a
conditional `<img>` has no height, so when `still` *is* null the play affordance and the
resume bar are both clipped away. What changes is when that happens — not "every episode
until enrichment", but an M3U library, whose episodes are built from the playlist and carry
no image, plus whatever individual episodes a panel has no artwork for. Still worth fixing,
and still worth an `aspectRatio` on the box; not the universal blank it was recorded as.

Recorded as its own entry rather than edited into [R-106] because the original reasoning is
the useful part: I inferred the field's only source from the UI's type and from enrichment
code, without checking what the importer wrote — the same mistake as the one noted under
[R-59], which was trusting a function without reading its caller.

### R-140 — Medium — the log export exists to be handed to somebody, and [R-61] says what is in it

`src-native/crates/aurora-app/src/logging.rs:45`, `:170`

The whole point of this module's second half is getting the log to another person:

> **And there was no way to hand it to anyone:** the path is inside `%LOCALAPPDATA%`, which
> is not somewhere a person goes.

`logs_export` copies both generations into `<data>/logs-export` and returns the paths so the
viewer can be shown where to find them. The design is careful — the destination is fixed
rather than chosen, *"a command that writes wherever it is told is a way to overwrite
something"* — and `docs/PRIVACY.md` makes the matching promise about the contents:

> Nor do they appear in a log. Every URL written to the log goes through `http::redact`
> first … and also handles the form that has no markers at all:
> `http://host/<user>/<pass>/12345.ts` … That second case was a real leak once (F-15).

[R-61] measured that `redact`'s `looks_like_stream_id` requires a file extension, and that
6,698 of this playlist's 6,699 stream URLs have none — so they pass through unredacted. So
the feature built for sharing the log, and the sentence promising the log is safe to share,
are both on the wrong side of a redaction that does not fire.

This is not a new defect; it is [R-61] plus the fact that there is a one-click button whose
purpose is to produce a copy of the file for someone else, and [R-126] plus the fact that
`library.db` sits in the same folder a viewer is being pointed at. Recorded separately
because fixing `redact` is urgent for a different reason than fixing it for tidiness: the
app actively encourages handing the output over.

Worth checking the export before closing [R-61]: a log from a real session, grepped for the
username, is a two-minute test and the only one that settles it.

**And a second stale comment pair, the same shape as [R-135].** `export_to`'s doc says:

> The destination is **the viewer's own choice of folder**, which is the whole point — a
> path under `%LOCALAPPDATA%` is not somewhere anyone is going to go looking.

while `logs_export`, its only non-test caller, says:

> **Deliberately not a path the UI chooses:** a command that writes wherever it is told is a
> way to overwrite something.

The command is right and the helper's comment describes a design that was abandoned. Two
files now carry a superseded paragraph next to the one that replaced it ([R-135] is the
other); worth a sweep for the pattern rather than two one-line fixes.

### `logging.rs`, reviewed — clean, and tested for the right thing

`src-native/crates/aurora-app/src/logging.rs` (281 lines). Apart from [R-140]'s comment
pair, nothing. Two generations rather than a rolling set, with the reason; `rotate`
ignores rename failures deliberately; `existing` is order-stable; `export_to` copies rather
than moves, *"the app is still writing to one of them"*, and has a test asserting it.

The test for the bug it replaced is the one worth noting — *"`File::create` truncated on
every launch, so relaunching to report a crash was what destroyed the record of it"* — and
`only_two_generations_are_kept` asserts there are exactly two files left, not just that the
two it wants exist.

`app_about` treating a missing `THIRD-PARTY-NOTICES.md` as `tracing::error!` rather than a
shrug is correct and the reason given is the right one: *"a build that cannot show the
licence notices it is obliged to carry is one that should not be distributed."*

### R-61 — addendum — settled against the real playlist: every live stream URL leaks, and the difference is one character

`src-native/crates/aurora-ingest/src/http.rs:411`, `:472`

**What the real playlist actually contains.** The first 300 KB of
`get.php?type=m3u_plus&output=ts` from the live panel, 240,349 URL lines, shapes counted
with the credential segments masked before printing:

```
 128,839  http://<HOST>/series/<USER>/<PASS>/N.mkv
  67,342  http://<HOST>/series/<USER>/<PASS>/N.mp4
  14,089  http://<HOST>/movie/<USER>/<PASS>/N.mkv
  13,408  http://<HOST>/movie/<USER>/<PASS>/N.mp4
   9,786  http://<HOST>/series/<USER>/<PASS>/N.avi
   6,699  http://<HOST>/<USER>/<PASS>/N            <- every live channel

segments after the scheme: {5: 233,650,  4: 6,699}
last-segment extension:   {.mkv: 142,928, .mp4: 80,750, .avi: 9,825, (none): 6,699, .mpg: 108}
```

So VOD and series carry a `movie`/`series` marker and an extension. **Live carries neither**
— four segments, no marker, no extension — and that is all 6,699 channels, which is the
only kind of URL a viewer's session opens repeatedly.

**What `redact` does with each.** Run against the production function with synthetic
credentials (`alice99` / `s3cret`):

```
shape                                                redact() output
live, as the real playlist writes it (no extension)  http://panel.example.com:8080/alice99/s3cret/12345
                                                     >>> LEAKS the credentials
live, as docs/PRIVACY.md assumes it (with .ts)       http://panel.example.com:8080/***/***/12345.ts        ok
series, as the real playlist writes it               http://panel.example.com:8080/series/***/***/67890.mkv ok
movie, as the real playlist writes it                http://panel.example.com:8080/movie/***/***/67890.mp4  ok
the host's own live builder (xtream.rs:416)          http://panel.example.com:8080/live/***/***/12345.ts    ok
get.php, query form                                  .../get.php?username=***&password=***&type=m3u_plus    ok
xmltv.php, query form                                .../xmltv.php?username=***&password=***                ok
```

Six of seven are right. The one that is not is the one this subscription uses for every
live channel, and **the only difference between the leaking case and the safe one is the
`.ts`**:

```rust
// http.rs:411
fn looks_like_stream_id(segment: &str) -> bool {
    let Some((id, ext)) = segment.rsplit_once('.') else {
        return false;                       // <- no extension, so not a stream id
    };
    ...
}
// http.rs:472 — the marker-less fallback, gated on it
if !redacted && segments.len() == 4 && looks_like_stream_id(segments[3]) {
```

`docs/PRIVACY.md` names this exact form and puts an extension on it:

> and also handles the form that has no markers at all: `http://host/<user>/<pass>/12345.ts`,
> which is how most panels address a stream. That second case was a real leak once (F-15).

The fix for F-15 was written against the form with an extension; the panel serves it
without one. Requiring the extension is what reopens it. A segment of digits in the fourth
position of a four-segment URL is already enough to identify it — dropping the `ext`
requirement, or accepting an all-digit segment with or without a suffix, closes it without
widening what else gets redacted.

This moves [R-61] from "plausible by inspection" to confirmed, raises [R-140] with it — the
export button exists precisely to hand this file to someone — and it is the third place in
this review where the credentials escape by a route the project believed was closed, after
[R-126] (the database and the IPC boundary) and the `aurora-ingest` copy of `urldecode`
([R-63]).

No real credential was printed at any point in this test: the URL shapes were masked before
display, and the `redact` run used synthetic values.

### R-141 — High — the documented way to stop sending your watch history to Google does not work in a release build

`src-native/crates/aurora-app/src/gemini.rs:96`, `:329`, with `docs/PRIVACY.md:52`

`docs/PRIVACY.md` is explicit, in the section it introduces as **"This is the one worth
reading"**:

> **It is on by default in an official build.** … it means *your viewing history leaves the
> machine by default*, and you should know that rather than discover it.
>
> **To stop it: clear the key in Settings → Assistant. With no key, both features say they
> need one and nothing is sent.**

`gemini.rs:16` repeats it: *"Clearing the key in Settings stops it."*

It does not. Clearing deletes the viewer's key and the resolver falls back to the key
compiled into the build:

```rust
// :329 — "Empty clears it."
if key.is_empty() {
    let _ = services.credentials.delete(CREDENTIAL_KEY);
}

// :96 — which is then resolved like this
pub(crate) fn resolve_key(services: &Services) -> Option<String> {
    services.credentials.get(CREDENTIAL_KEY).ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .or_else(|| built_in().map(str::to_string))   // <- the compiled-in key
}
```

`BUILT_IN_KEY` is `option_env!("AURORA_GEMINI_KEY")` (`:42`) and a release is compiled with
it (D19, and the same file's header says so). So in the only build an ordinary viewer has:

* "Remove key" removes **their** key, not the feature.
* `key_available` stays `true`, so the home screen's rail goes on asking Google the first
  time it opens with anything in the watch history.
* Their titles, years, genres, completion ratios and likes keep leaving the machine.

**There is no other off switch.** Swept across the host, the contract and the UI: no
`gemini.enabled`, no `assistant.enabled`, no rail-level toggle, no setting of any kind. The
only states reachable are *your key* and *the built-in key*. Off is not one of them, unless
you compile the app yourself.

**The UI is honest after the fact, which makes this a documentation-and-design gap rather
than a deception.** `GeminiStatus.key_is_built_in` is computed as
`credentials.get(...).is_err() && built_in().is_some()` (`:315`), and `GeminiPanel.tsx:77`
renders:

> Using the key built into this build. Entering your own replaces it, and carries your own
> rate limit rather than sharing one.

So a viewer who removes their key and then reads the panel is told the truth. A viewer who
follows the privacy document's instruction and closes Settings is not — and the privacy
document is the artefact written specifically for the person who wants this off.

The fix is a switch. The plumbing is almost all there: one setting read by
`key_available`, defaulting to on to preserve the trade the built-in key buys, with the
panel's existing "Remove key" becoming "Turn this off" when the key is built in. Then
`docs/PRIVACY.md`'s paragraph becomes true, and the `AiRail` and `AssistantPage` already
handle the no-key state because that is what a self-built binary does.

Until then the document has to say what actually happens: that removing your key falls back
to the built-in one, and that the only way to stop it in an official build is to not use the
home screen's AI rail or the assistant.

### The Gemini recommender, otherwise reviewed

`src-native/crates/aurora-app/src/gemini.rs` (374 lines). Apart from [R-141]:

* **What leaves the machine matches the claim.** The prompt carries watched titles with
  years, genres, completion and liked-flag, capped at `HISTORY_FOR_PROMPT = 40`; nothing
  identifies the viewer, the provider or the subscription. Same conclusion as the
  assistant's review above, reached the same way.
* **The answer is resolved against the library and anything absent is dropped** (`:20`–
  `:24`), so an invented title cannot become a playable card — the same property
  `Tool::ShowTitles` has in the assistant, and here it is genuinely by lookup rather than
  by trust. See [R-127] for the one place that property is weaker than its comment.
* **The key is in the credential store**, never the database, so it is not in a backup of
  `library.db` — which, after [R-126], is worth noting as the thing that *was* done
  correctly for a secret in this codebase.
* `FRESH_FOR_SECS = 6 * 60 * 60` with the cache additionally keyed on `history_len`, so
  finishing something invalidates it without waiting for the clock. Sound.
* `built_in()` trims and rejects a blank key *"because CI sets the variable
  unconditionally"* (`:91`) — which is the kind of detail that is only ever learned by
  having been bitten.

### R-73 — refinement — the mechanism is confirmed; only the API's verdict is still open

`src-native/crates/aurora-ingest/src/chat.rs:156` with
`src-native/crates/aurora-app/src/assistant.rs:912`

[R-73] was recorded as needing a live request. The code side can be settled by reading, and
it is worse-shaped than the original note assumed.

Gemini returns parallel function calls as **one** `model` content with **several**
`functionCall` parts. `ChatReply.calls` is a `Vec<ToolCall>`, so the reply is received
correctly. What happens next is that each call becomes its own turn, interleaved with its
own result:

```rust
// assistant.rs:912
for call in &reply.calls {
    working.push(Turn::ToolCall { name, args, thought_signature });
    match run_tool(db, profile_id, call, now) { Ok(ran) => working.push(Turn::ToolResult { .. }), .. }
}
```

and `contents_of` emits one content per turn, with the role alternating:

```rust
// chat.rs:162
Turn::ToolCall { .. } => json!({ "role": "model", "parts": [part] }),
Turn::ToolResult { .. } => json!({ "role": "user", "parts": [{ "functionResponse": .. }] }),
```

So two parallel calls are sent back as four contents —
`model[callA] · user[respA] · model[callB] · user[respB]` — describing a conversation in
which the model made one call, was answered, then made another. What actually happened was
one model turn containing both calls, which the API's own shape expects to be answered by
one `user` content carrying both `functionResponse` parts.

Two reasons this is more than pedantry:

1. **The API validates the shape of these parts.** The code already carries the scar:
   `ToolCall::thought_signature` exists because dropping it produced *"Function call is
   missing a thought_signature in functionCall parts. This is required for tools to work
   correctly"* — a 400 that *"was every tool-using question in the app failing on its second
   round"* (`chat.rs:78`–`:88`). A validator strict enough to reject a missing per-part
   signature is one worth not feeding a regrouped history to.
2. **Each signature is bound to the part it came from.** Splitting one content's parts
   across two contents moves each signature into a content that did not produce it.

What remains genuinely unknown is only whether the API refuses this or tolerates it. The
test that settles it is narrow: one request whose system prompt invites two independent
lookups — "how many films do I have, and what is on now?" maps onto `library_digest` and
`whats_on_now` — then inspect whether `reply.calls.len() > 1`, and if so whether the
*following* round returns 200. Still deliberately not spent, since it needs the viewer's
key and a real quota.

Worth fixing either way, because the correct encoding is also the simpler one: group a
reply's calls into a single `model` content and their results into a single `user` content.
That needs `Turn::ToolCall`/`ToolResult` to carry a `Vec`, or a grouping pass in
`contents_of` that merges adjacent same-role function parts. The second is a dozen lines
and leaves `assistant.rs` alone.

Note what is *not* wrong here: `MAX_CALLS` being enforced inside the same loop means a
refused call still gets a `ToolResult` carrying an explanation rather than being dropped
(`:916`) — *"a model that asked for a fifteenth lookup needs to know why it did not get
one, or it asks again"* — so the call/response pairing the API requires is maintained even
on the bound. That is the part that would most easily have been got wrong.

### R-139 — addendum — nothing enriches episodes either, so the runtime fallback is dead permanently, not temporarily

[R-139] said the stored-runtime fallback "only works once TMDB enrichment has written
`runtime_mins`". There is no such enrichment. The defect is unconditional.

**`episodes.runtime_mins` is written by nothing but test fixtures.** Every statement in the
workspace that inserts into `episodes`:

```
library.rs:816     INSERT INTO episodes (series_id, season, episode, title, url, still, added_at)   <- production
library.rs:2237    INSERT INTO episodes (series_id, season, episode, url, runtime_mins)             <- #[cfg(test)] mod runtime_tests
playback.rs:1356   INSERT INTO episodes (id, series_id, season, episode, title, url)                <- test
progress.rs:394    INSERT INTO episodes (id, series_id, season, episode, url, added_at)             <- test
progress.rs:642    INSERT INTO episodes (id,series_id,season,episode,url)                           <- test
recommend.rs:322   INSERT INTO episodes (id, series_id, season, episode, title, url)                <- test
```

The one production path is `upsert_episodes`, and its `ON CONFLICT` clause touches only
three columns:

```sql
ON CONFLICT (series_id, season, episode) DO UPDATE SET
  title = COALESCE(excluded.title, episodes.title),
  url   = excluded.url,
  still = COALESCE(excluded.still, episodes.still)
```

so `runtime_mins`, `overview` and `air_date` are not written on insert and not written on
update.

**And enrichment cannot fill them**, because it has no concept of an episode:

```rust
// aurora-db/src/repo/enrichment.rs:19
pub enum ItemKind { Movie, Series }
```

Two variants. `enrich.rs`'s `plan` builds its work list from those two, `fetch_one` searches
TMDB for a film or a show, and `apply` writes to the movie or series row. No code path
anywhere looks up an episode.

So `episode_runtime_secs` returns `None` for every episode on every real library, for ever
— and so does its own fallback, which is a nicely-judged median over the show's other
episodes (`library.rs:2193`) but reads the same empty column:

```sql
SELECT e.runtime_mins FROM episodes e
 WHERE e.series_id = (SELECT series_id FROM episodes WHERE id = ?1)
   AND e.id != ?1 AND e.runtime_mins > 0
```

**The giveaway is the test helper.** `mod runtime_tests` opens with

```rust
fn add_episode(conn: &Connection, series_id: i64, number: u16, runtime: Option<i64>) -> i64 {
    conn.execute("INSERT INTO episodes (series_id, season, episode, url, runtime_mins) ...
```

— so the fallback, including the median-of-siblings branch, is unit-tested against runtimes
the tests insert directly, and the importer has no way to produce them. Same shape as
[R-98], [R-99] and [R-120]: a feature verified against data that only the test or the mock
can supply.

Upgrades the consequence in [R-139] from "until enrichment runs" to **always**: on any
install, for any episode whose stream reports no duration, `duration` is `None`, so
`up_next_at(&resolved, 0.0, 45.0)` has nothing to work with and **Up Next, autoplay-next
and Skip Credits never fire**. The two fixes are independent and both small — store
`duration_secs` at import ([R-139]), and either extend enrichment to episodes or accept
that the provider is the only source.

### R-142 — [R-115] is a spec violation, not a judgement call

`README.md:712`

README §12 states exactly what Reduce Motion is for:

> A global **Reduce Motion** setting (and respect for the OS setting) disables
> **hover-preview autoplay, parallax, and non-essential transitions.**

Three things, all of them motion. [R-115] measured what it actually does: with
`prefers-reduced-motion: reduce`, a rail card goes from five buttons to one, losing Play,
My List, I-like-this, More info and Remove-from-Continue-Watching.

So the setting does the three things the spec asks **and** removes the card's entire action
set, which the spec does not ask for and which is not motion. `docs/ROADMAP.md` lists
"reduce-motion" among Phase 10's delivered items, which is true of the three and not of the
fourth.

The settings copy understates it too. `SettingsPage.tsx:140`:

> **Animations** — Disables hover previews, trailer autoplay and transitions.

A viewer reading that hint has no way to know the toggle also takes the buttons away, and a
viewer whose OS already has reduced motion set never sees the hint at all because they
never turned anything on.

### R-143 — Medium — the in-app keyboard reference omits six working bindings, and `F` does the wrong thing

`src-ui/src/features/settings/SettingsPage.tsx:201`, `src-ui/src/hooks/useHotkeys.ts`,
`README.md:786`

Settings → Keyboard lists ten rows. `useHotkeys` implements eighteen bindings. The six that
work and are not listed:

```
Escape       back / close modal
J            seek -10s
L            seek +10s
F            fullscreen
P            picture-in-picture
I            info — channel banner on live, detail panel on a film
```

Two of those are the only documentation their own implementations believe exists:

* `App.tsx:456` on `onInfo` — *"The key was bound, **documented in the hotkeys sheet**, and
  called an empty function."* It is not in this list, and there is no other sheet:
  README §14.1 promises *"a searchable, printable shortcuts cheat sheet in-app (`?` opens
  it) and in `docs/SHORTCUTS.md`"*, and neither exists — `?` is unbound and the file is
  absent.
* `P` reaches 832 lines of `pip.rs`, and the only way to discover the key is to read the
  source.

**`F` is bound against the spec.** README §14.1 assigns `F11` to fullscreen and `F` to
"Favorite current item". The implementation binds `f` to fullscreen (`useHotkeys.ts:72`),
binds `F11` to nothing, and has no favourite-current-item action at all. So a viewer
following the README's table presses `F` expecting to favourite what they are watching and
goes fullscreen instead — on live TV, where favouriting is a real feature with a real
button elsewhere in the app, and where `LivePage.tsx:248` tells them *"Press F while
watching … to add it here."* **That sentence is wrong**: `F` goes fullscreen, and the
favourites empty state is the place a viewer is most likely to read it.

That makes it a defect rather than unbuilt scope: two parts of the app disagree about what
`F` does, and the one that describes it is the one that is wrong.

**Remapping is claimed as shipped and is not built.** README §14.1's heading is *"implement
all; make every binding remappable in Settings"*, and `useHotkeys.ts:2` says *"Every
binding here is remappable in Settings in the shipped product; this hook is the single
place they are interpreted."* Settings → Keyboard is a read-only `<dl>`. The hook's claim
is about a product that does not exist yet; `docs/ROADMAP.md` is honest about the same
scope ("keyboard map" delivered under a **Partial** phase), so the comment is the only
place that overstates it.

The rest of §14.1 is plainly unbuilt scope rather than defect, and `ROADMAP.md` says so —
`Ctrl+,`, `Ctrl+←/→`, `,`/`.` frame step, `[`/`]`/`\` speed, `S`/`A` track cycling,
`V`/`Z` aspect and zoom, `R` record, `Ctrl+S` screenshot, `C` channel overlay, the guide's
arrow navigation, `Home`, type-ahead, §14.2's remote learning wizard and §14.3's gamepad.
Not counted as findings. Listing the six that *do* work is a one-line change and is worth
doing before any of that.

### R-144 — Info — `docs/ROADMAP.md` says picture-in-picture is not built; it is

`docs/ROADMAP.md`, Phase 10

> **Partial:** themes, TV density, reduce-motion, keyboard map, command palette, and
> **multi-view** … **PiP**, sleep timer, tray and backup/restore **not built**.

PiP is built. `aurora-app/src/pip.rs` is 832 lines, six commands are registered in
`main.rs:545`–`:550` (`pip_state`, `pip_toggle`, `pip_set_enabled`, `pip_set_corner`,
`preview_place`, `preview_clear`), four are declared in `shared/ipc.ts:1219`–`:1224`,
`src-ui/src/features/player/PipTile.tsx` renders the tile, `App.tsx:595` clips the shell
around it, and `useHotkeys` binds `P` to it.

Worth a line because this roadmap is otherwise the most reliable status document in the
repository — it is candid about zap time being over budget, about the recording file
assertion never having gone green, about `cache-dir` not existing, and about nothing being
code-signed. One stale "not built" against a subsystem that is built undersells it and
would mislead anyone planning work from it.

The same entry is right that sleep timer, tray and backup/restore are absent; I checked all
three.

**[R-140] and [R-126], as one screen.** `DiagnosticsPanel.tsx` is where both leaks are
handed over, and it says so in as many words:

```tsx
<Button size="sm" icon="stack" onClick={exportLogs}>Export logs</Button>
<span>Copies this run and the one before it, for a bug report.</span>
...
Data folder: <code>{data.dataDir}</code>
```

So the one screen built for "send this to somebody" offers (a) an export of the logs, which
[R-61] now proves carry the username and password of every live stream opened, and (b) the
path to the folder holding `library.db`, which [R-126] proves carries them several hundred
thousand times. Neither sentence hints at it, and `docs/PRIVACY.md` tells the viewer both
are safe.

Nothing on this screen is wrong in itself — it is a good panel, and the
`libraryWasReplaced` and `videoEngine.rendersVideo` warnings are exactly the two things a
"no picture" or "my library is empty" report needs. It is worth recording only because it is
the place the two upstream defects actually reach a person, and because once they are fixed
this screen is the one that should carry a line saying what the export contains.

**And the correct pattern for [R-141] is one panel away.** `MetadataPanel.tsx:107` shows
the Remove button only when the key is the viewer's own:

```tsx
{status?.hasKey && !status.keyIsBuiltIn && (
  <Button size="sm" variant="ghost" onClick={() => void saveKey(null)}>Remove</Button>
)}
```

so with a built-in TMDB key there is no "Remove" offering to do something it cannot do, and
the copy underneath explains what the built-in key means. `GeminiPanel` offers "Remove key"
in the same situation and silently falls back to the compiled-in one. TMDB needs no off
switch — nothing about the viewer leaves — so the asymmetry is defensible for the button and
not for the privacy document that promises an off switch exists.

### R-145 — Medium — Settings → Refresh shows raw enum names for four of the nine import phases

`src-ui/src/features/settings/RefreshButton.tsx:19`

```ts
const PHASE_LABEL: Record<string, string> = {
  downloading: 'Downloading',
  importingChannels: 'Channels',
  importingMovies: 'Movies',
  importingSeries: 'Series',
  importingEpg: 'Guide',
  indexing: 'Indexing',
  done: 'Done',
};
```

The nine phases the host actually emits, cross-checked three ways — the `IngestPhase` union
(`shared/ipc.ts:535`), the `Phase::*` variants constructed in `aurora-ingest/src/sync.rs`,
and `SetupWizard`'s own map — are:

```
authenticating  fetchingPlaylist  importingChannels  importingMovies  importingSeries
fetchingEpg     matchingEpg       indexing           done
```

So this map has **two keys that are not phases** (`downloading`, `importingEpg`) and is
**missing four that are** (`authenticating`, `fetchingPlaylist`, `fetchingEpg`,
`matchingEpg`). The render falls through to the raw value:

```tsx
{PHASE_LABEL[progress.phase] ?? progress.phase}
```

so a viewer pressing Refresh in Settings watches the label read **`authenticating`**, then
**`fetchingPlaylist`**, then Channels / Movies / Series, then **`fetchingEpg`**, then
**`matchingEpg`**, then Indexing, Done. Four of the nine are camelCase identifiers, and on
this subscription they are not the quick ones: fetching a 165 MB guide and matching it
across 6,699 channels is the longest stretch of the 23.7-second refresh this file's own
comment cites.

**`SetupWizard.tsx:19` already has the complete, correct map**, in sentences:

```ts
const PHASE_LABEL: Record<IngestPhase, string> = {
  authenticating: 'Checking your subscription',
  fetchingPlaylist: 'Fetching your channel list',
  importingChannels: 'Importing channels',
  importingMovies: 'Importing movies',
  importingSeries: 'Importing series',
  fetchingEpg: 'Downloading the TV guide',
  matchingEpg: 'Matching the guide to your channels',
  indexing: 'Building the search index',
  done: 'Finished',
};
```

Note the type annotation, which is the whole mechanism: `Record<IngestPhase, string>` is
exhaustiveness-checked, so the wizard's map cannot drift. `RefreshButton` declares
`Record<string, string>`, which accepts any key and requires none — so `tsc --strict` passes
over two invented keys and four omissions without a word. Changing the annotation would have
caught this at compile time, and exporting one map from one place prevents the next copy.

### R-146 — Info — `IconName` advertises a closed union and is `string`

`src-ui/src/components/Icon.tsx:49`

```ts
export type IconName = keyof typeof ICON_PATHS | string;
```

`keyof typeof X | string` collapses to `string` — TypeScript widens the union, so the
`keyof` contributes nothing. Every `<Icon name="…" />` and every `icon:` prop in the app is
therefore unchecked: `<Icon name="plya" />` compiles, as does any other misspelling, on a
type whose name and shape both suggest otherwise.

Nothing is broken today. Swept across `src-ui/**/*.tsx`: **38 icons defined, 32 referenced
by literal, 0 referenced that are not defined.** And the lookup degrades deliberately:

```ts
const d = ICON_PATHS[name] ?? ICON_PATHS.info!;
```

so an unknown name draws the info glyph rather than an empty `<svg>` — wrong but visible,
which is the better of the two failures.

Dropping `| string` makes it a real union and the 32 call sites get checked. The only reason
it would have been added is to allow a computed name, and the two computed sites in the app
(`PlayerOverlay.tsx:266` `playing ? 'pause' : 'play'`, `:293`
`player.muted ? 'volumeOff' : 'volume'`) are ternaries over literals, which a closed union
accepts.

*A limitation of my own sweep, stated because it affects the numbers:* the "referenced"
count matches quoted literals after `name=`/`icon=`/`icon:`, so it does not see names built
by a ternary or a variable. Six icons looked unreferenced — `cast, keyboard, pause, star,
stop, volumeOff` — and at least `pause` and `volumeOff` are used by the two ternaries above,
so that list is not a dead-code list and should not be acted on as one. The
referenced-but-undefined count of zero is the reliable half.

**The sweep [R-145] suggested, run.** Every `Record<string, …>` lookup table in `src-ui`
and `shared`, checked against the union it actually keys:

```
Icon.tsx:4          ICON_PATHS   <- IconName      38 defined, 0 missing (and IconName is `string`, above)
Primitives.tsx:104  tones        <- Badge tone    5 of 5 present
PlayerOverlay.tsx:785 LABELS     <- ErrorAction   4 of 4 present
RefreshButton.tsx:19 PHASE_LABEL <- IngestPhase   2 invented, 4 missing  -> [R-145]
fixtures.ts:191     PROG_BY_GROUP                 keyed by arbitrary group names — correct
shared/ipc.ts:180,190,224,346,1003                keyed by `movie:12` / EPG ids — correct
```

So one live defect, two latent-but-currently-complete (`tones` and the OSD's `LABELS`,
either of which would silently render an unstyled badge or a raw action name if a union
member were added), and the rest correctly keyed by arbitrary strings. The six exhaustive
tables that use `Record<Union, …>` — `SIZES`, `variants`, `counts`, `SkipButton`'s `LABELS`,
`SetupWizard`'s `PHASE_LABEL`, `MOSAIC_TILES` — cannot drift, which is the pattern the other
three should adopt.

### R-147 — Medium — the palette's arrow-key highlight is never scrolled into view, and never announced

`src-ui/src/features/search/CommandPalette.tsx:149`, `:230`, `:177`

The results list is a 52vh scroller (`:177`) and arrow keys move a numeric `cursor`:

```tsx
if (e.key === 'ArrowDown') { e.preventDefault(); setCursor((c) => Math.min(c + 1, flat.length - 1)); }
```

which is rendered purely as a background colour on the matching row:

```tsx
background: cursor === idx ? 'var(--surface-hover)' : 'transparent',
```

Three things follow, and the first is the one a viewer hits:

1. **No `scrollIntoView`.** With six groups each showing four hits plus a "Show N more"
   row, the list is routinely longer than 52vh — on this subscription a common word matches
   channels, both guide groups, movies and series at once. Arrowing down past the fold
   moves the highlight to a row that is not on screen, so the panel appears to stop
   responding while Enter opens something the viewer cannot see. README §12 asks for the
   opposite in as many words: *"Focus is never lost, never invisible, and always scrolled
   into view."*
2. **Focus never moves, so assistive technology is never told.** The `<input>` keeps focus
   throughout — correctly, because that is where typing continues — but the rows are plain
   `<button>`s with no `aria-selected`, the container has no `role="listbox"`, and the input
   has no `aria-activedescendant`. A screen-reader user pressing Down hears nothing change.
   The `role="dialog" aria-modal="true"` at `:128` is the same assertion [R-102] measured as
   false here, so the palette's keyboard story is: arrows move an invisible highlight, Tab
   leaves the dialog entirely.
3. `setCursor` is also reset only when the *count* changes — `useEffect(() => setCursor(0),
   [flat.length])` (`:107`) — so a new query returning the same number of hits leaves the
   highlight on row N of a completely different list.

All three are the same fix: give the input `aria-activedescendant`, give the rows ids and
`aria-selected`, and call `scrollIntoView({ block: 'nearest' })` on the highlighted row when
`cursor` changes. The `flat`/`offset` arithmetic that keeps the keyboard index and the drawn
rows in step is already careful and correct (`:102`, `:206`, with the reason written down),
so the hard part is done.

### R-148 — Low — three smaller things in the palette

`src-ui/src/features/search/CommandPalette.tsx`

**A slow search can overwrite a newer one** (`:75`). The debounce clears its timer on
cleanup, but the in-flight `invoke` is not tracked:

```tsx
const t = window.setTimeout(() => {
  invoke('search.query', { text }).then(setResults).catch(report('Search failed'));
}, 90);
return () => window.clearTimeout(t);
```

If the request for "the" takes longer than the request for "them" that follows it, the
older results land last and win. Narrow — search measured 19–45 ms earlier in this review
against a 90 ms debounce — but [R-56] notes [R-49] pushes search past 100 ms by day three,
which is where the two cross. `useSeriesResume` (`DetailModal.tsx:100`) has the `let live =
true` guard this needs, and `subscribe()` in `ipc/subscribe.ts` has the same pattern; this
is the one async read in the UI without it.

**The exit animation cannot run** (`:111`). `if (!open) return null;` sits above the
`<AnimatePresence>`, so when `open` goes false the whole subtree unmounts synchronously and
the `exit={{ opacity: 0 }}` on `:116` never plays. Harmless, and worth knowing before
someone spends time wondering why the palette closes without a fade: the `AnimatePresence`
has to live outside the early return, or in `App.tsx` which already renders this component
unconditionally.

**People are labelled with a heart** (`:42`). `{ key: 'people', label: 'People', icon:
'heart' }` — and the same glyph means "favourite" on `LivePage`'s channel rows
(`:469`), in the recordings list's Keep control (`RecordingsPage.tsx:348`) and in Live TV's
group sidebar. `ICON_PATHS` has 38 icons and no person; adding one is a path string.

### R-149 — Medium — auto-skip fires on the first episode of a session and never again

`src-ui/src/features/player/SkipButton.tsx:31` with `src-ui/src/App.tsx:876`

```tsx
// Remember which regions we already auto-skipped, so seeking back in does not
// immediately yank the viewer forward again.
const autoSkipped = useRef(new Set<string>());

useEffect(() => {
  if (!marker || !autoSkip) return;
  const key = `${marker.kind}:${marker.startSecs}`;
  if (autoSkipped.current.has(key)) return;
  autoSkipped.current.add(key);
  onSkip(marker, true);
}, [marker, autoSkip, onSkip]);
```

The guard is right and the reason for it is right. The key is not: it identifies a region
by kind and start second, with nothing about *which episode*. And the component is never
remounted between episodes —

```tsx
// App.tsx:876, inside {playerOpen && ( … )}
<SkipButton marker={episode.activeMarker} autoSkip={episode.autoSkip} onSkip={episode.skip} />
```

no `key`, and autoplay-next keeps `playerOpen` true as it moves from one episode to the
next. So `autoSkipped` accumulates for the whole viewing session.

**Why the keys collide rather than merely might.** `autoSkip` is deliberately restricted to
markers that are not conventional guesses (`useEpisodeAids.ts:116`:
`activeMarker.source !== 'convention'`), which leaves `chapters`, `user` and `learned`. The
`learned` source is the feature's own best case — markers derived from where the viewer
skipped in *earlier episodes of this show* — and a value learned from a show's episodes is
by construction the same number for all of them. A show whose intro is learned at 42 s has
`intro:42` on every episode, so the set hits on episode two and auto-skip silently stops.
Chapter markers from a provider's MKVs have the same property for the same reason.

The result is a per-show preference — "Always skip intros", set in the detail modal
(`DetailModal.tsx:494`) — that works once and then appears to have turned itself off. There
is nothing on screen to explain it, and the manual Skip button still appears, so it reads as
the setting not having saved.

One line either way: `key={episode.episodeId}` on the `SkipButton` in `App.tsx`, or put the
episode id in the ref key. The first is better, because it also discards the memory when the
viewer moves to a different show, and `useEpisodeAids` already resets its own `dismissed`
state on an episode change (`:69`) — this is the one piece of per-episode state that does
not.

### R-150 — Info — four stale comment pairs, each contradicting the fix beside it

A pattern rather than four separate defects, and worth one sweep. In each case a paragraph
describing the old behaviour survived the change that replaced it, and reads as current:

1. **`App.tsx:271`** — two stacked doc blocks on `playCatchup`, the first saying *"The error
   is surfaced rather than swallowed"* and the second *"The refusal is deliberately **not**
   turned into a toast here … So this rethrows."* The code does the second. ([R-135])
2. **`logging.rs:45`** — `export_to`'s *"The destination is the viewer's own choice of
   folder, which is the whole point"* against its only caller's *"Deliberately not a path
   the UI chooses: a command that writes wherever it is told is a way to overwrite
   something."* ([R-140])
3. **`SurfaceHole.tsx:16`** — *"Putting them in front would cover the page … **it is what
   `PipTile` does**, and it is why the small picture has never shown anything."* Present
   tense, and `PipTile` no longer does it: its own header (`:10`) explains the four bands
   were removed and that the hole is now cut by `holeClipPath`, and `App.tsx:595` applies
   that clip. So the file that owns the mechanism describes the caller as still broken.
4. **`useHotkeys.ts:2`** — *"Every binding here is remappable in Settings in the shipped
   product"*, where Settings → Keyboard is a read-only list. ([R-143])

These matter more in this codebase than they would in most, because the comments *are* the
design record — they carry measurements, failure histories and the reasoning behind
non-obvious choices, and this review has leaned on them throughout and found them accurate
far more often than not. Four that describe a superseded state is the cost of that style,
and the fix is to delete the superseded paragraph rather than to add a third one.

Not counted among these: comments that are *factually* off rather than stale — the 42.8%
EPG figure ([R-132]), the 6.3 MB `channels.list` payload ([R-117]) — and the one case where
the comment is right and the code is wrong, which is most of the rest of this review.

### R-151 — Medium — the hero billboard rotates under a keyboard user's hands

`src-ui/src/components/HeroBillboard.tsx:86`

```tsx
<section
  aria-label="Featured"
  onMouseEnter={() => setPaused(true)}
  onMouseLeave={() => setPaused(false)}
>
```

Pausing is hover-only. There is no `onFocus`/`onBlur` anywhere in the file — swept — and no
pause control of any kind. Meanwhile the billboard carries three large focusable buttons:

```tsx
<Button variant="primary" size="lg" icon="play"  onClick={() => onPlay(item)}>   // :155
<Button ... aria-pressed={myList.has(...)}>                                       // :161
<Button variant="secondary" size="lg" icon="info" onClick={() => onOpen(item)}>   // :170
```

So a viewer who tabs to **Play** and pauses to read the synopsis gets twelve seconds
(`ROTATE_MS`) before `setIndex` advances — thirty-two if a trailer started — and the button
they are focused on is now Play for a different film. The focused element is not remounted,
so focus stays put and the label changes under it. Pressing Enter plays something they did
not choose.

The same applies to anyone reading with a screen reader: `aria-label="Featured"` is a plain
`<section>` with no `aria-live` and no `role="region"`-plus-controls contract, so the
content changing is neither announced nor stoppable.

WCAG 2.2.2 asks for a way to pause, stop or hide anything that moves for more than five
seconds; the dot indicators at `:193` let a viewer *jump* but not stop. Two small changes
cover it: `onFocus`/`onBlur` beside the existing mouse handlers — which is the same one-line
omission as `DetailModal`'s episode rows ([R-106]) and `SelectRow`'s hover
(`Primitives.tsx:516`) — and honouring `animations === false` by not rotating at all, which
would also make the Reduce Motion behaviour match what README §12 asks for rather than what
[R-115] measured.

The rest of the rotation logic is careful: it stops entirely below two items (`:73`), resets
`muted` on every change with the reason given (*"a home screen that suddenly makes noise
about a film nobody asked about"*), gives a title with a trailer longer on screen than one
without, and crossfades rather than handing over — with a comment explaining that
`mode="wait"` left half a second of empty billboard.

**[R-112] — sharper evidence.** `BrowsePage` does not merely offer a chip to clear the
letter; it clears it automatically the moment the sort stops being alphabetical:

```ts
// BrowsePage.tsx:83
const alphabetical = sort === 'title';
useEffect(() => {
  if (!alphabetical) setLetter(undefined);
}, [alphabetical]);
```

with the reasoning stated above it — *"picking 'W' while sorted by year would narrow to the
Ws and then order them by year, which is a result nobody asked for from a control that looks
like an index."* `LivePage` has the identical `disabled={order !== 'name'}` on the same
component and no such effect, so the filter stays applied and unreachable. The fix for
[R-112] is literally these three lines, moved.

### R-152 — Medium — after a PIN lockout the pad stays locked until the viewer backs out, and the countdown it shows is frozen

`src-ui/src/features/profiles/ProfilePicker.tsx:101`

```tsx
const isLocked = lockedUntil > Date.now() / 1000;
const secondsLeft = Math.max(0, Math.ceil(lockedUntil - Date.now() / 1000));
```

Both are computed during render from `Date.now()`, and nothing schedules a re-render. There
is no `setInterval`, no `setTimeout` on `lockedUntil`, and no other state that changes while
the pad is locked. So once `lockedUntil` is set:

* **Every key stays disabled.** `PinKey disabled={isLocked}` covers 0–9 and backspace
  (`:168`–`:177`), so there is nothing left to press that could trigger a re-render — the
  only enabled control is **Back** (`:180`).
* **The countdown does not count.** `{message}{isLocked ? ` (${secondsLeft}s)` : ''}`
  (`:159`) renders the value from the moment of the refusal and never updates, so it reads
  "(60s)" for as long as the pad is open.

`PIN_LOCKOUT_SECS = 60` and `MAX_PIN_FAILURES = 5` (`aurora-db/src/repo/profiles.rs:15`), so
the honest behaviour would be a minute's wait with a ticking number. What a viewer gets is a
dead keypad showing a static "(60s)", and the only way to make it live again is to press
Back and re-enter the profile — at which point `lockedUntil` resets to 0 locally, the keys
re-enable, and the host refuses again until the real minute is up. So the recovery path is
undiscoverable and, when found, confusing.

The host side is correct throughout: the attempt state is per scope, failures are counted
and cleared on success, and `lockedOut.until` is a real timestamp the UI could count from.
`App.tsx:507` already demonstrates the pattern this needs — a `setTimeout` to the stored
deadline rather than a fixed interval, with the comment explaining why counting from "now"
each render was wrong.

**And `submit` has no `catch`** (`:104`):

```tsx
const outcome: PinOutcome = await invoke('profiles.verifyPin', { profileId: profile.id, pin });
```

A rejection — the database locked, the profile deleted in another window — skips both the
success path and `setDigits('')`, so the pad is left showing four filled dots with the
generic "Something went wrong" toast from `installGlobalErrorHandlers` and no indication
that the PIN was never checked. Every other screen in the app pairs `invoke` with `report`;
this is the second that does not, after [R-119]'s `RecordingsPage`.

**One smaller thing:** `PinOutcome` in `shared/ipc.ts:470` includes a `{ ok: null }` member
that the host cannot produce — serde renders the unit variant as the string `"ok"`, which is
what `:110` correctly checks for. Harmless dead type surface, but it invites the next reader
to write `'ok' in outcome`, which would never match.

### R-153 — Low — three of the four "Settings → X" pointers name a section that does not exist

The eleven section headings `SettingsPage.tsx` actually renders:

```
Providers · Appearance · Artwork and metadata · Recommendations · Filtering
Library · Pause live TV · Updates · About · Diagnostics · Keyboard
```

Every place in the app or the docs that sends a viewer to a named section:

| Pointer | Where | Section it names | Exists? |
|---|---|---|---|
| "Settings → Metadata and recommendations" | `AssistantPage.tsx:276` | — | **No.** Two sections were merged into one name. The Gemini key is under **Recommendations**. |
| "Settings → Assistant" | `docs/PRIVACY.md:52` | — | **No.** There is no Assistant section; the key is under **Recommendations**. |
| "Settings → Filters" | `LivePage.tsx:546` | — | **No** — the heading is **Filtering**. |
| "Settings → Updates" | `docs/PRIVACY.md:72` | Updates | Yes. |

Small individually. Two of them matter more than their size:

* `AssistantPage`'s is the empty state a viewer sees **when the assistant will not work**,
  so it is read by exactly the people who need to find the right screen, and it names a
  section that is not in the list.
* `PRIVACY.md`'s is the opt-out instruction, which [R-141] already shows does not work in a
  release build. So a viewer trying to stop their watch history going to Google is sent to a
  section that does not exist, and the thing they are told to do there would not have
  stopped it.

`Section`'s own doc comment (`SettingsPage.tsx:226`) explains that each heading is an
addressable region precisely so things can be found; the headings are the names to use.

### R-154 — High — the test that would have caught five of this review's findings exists, and runs against one interface out of sixty-four

`src-native/crates/aurora-app/tests/contract.rs:194`

```rust
/// The UI reads `PlayerState` on every frame, and the host has to send all of it.
///
/// `itemKind` was declared here, answered by the mock, and never sent by the host. The
/// UI gates Skip Intro, Skip Credits and Up Next on `player.itemKind === 'episode'`
/// (`useEpisodeAids`), so all three worked in every browser journey and none of them
/// could ever appear on Windows. Exactly the failure the command test above exists for,
/// one layer down: **the mock is generous and the host is the one that ships.**
#[test]
fn the_player_state_the_host_sends_is_the_one_the_contract_declares() {
    let declared = declared_fields(&contract, "PlayerState");
    let sent = serde_json::to_value(aurora_player::PlayerState::default()).expect("serializes");
    let missing: Vec<&String> = declared.iter().filter(|f| !sent.contains_key(*f)).collect();
    assert!(missing.is_empty(), "the UI reads these and the host never sends them: {missing:?}");
    let extra: Vec<&String> = sent.keys().filter(|k| !declared.contains(k)).collect();
    assert!(extra.is_empty(), "the host sends these and the contract does not declare them: {extra:?}");
}
```

That comment is a precise statement of the defect class this review kept finding, and the
test is the right shape for catching it: it parses the declared field names out of
`shared/ipc.ts`, serialises the Rust struct, and checks **both** directions — fields the UI
reads that the host never sends, and fields the host sends that the contract never
declares.

It is applied to `PlayerState`. `shared/ipc.ts` declares **64** interfaces.

Run against all of them — which is what the field-level sweep earlier in this review did by
hand — the same two assertions catch:

| Finding | Which assertion would have caught it |
|---|---|
| [R-99] `Movie.match`, `Series.match` — a documented feature that exists only in the mock | `missing` |
| [R-126] `EpisodeRow.url` — the provider's password crossing into the WebView | **`extra`** |
| [R-126] `Episode.airDate`, `Episode.seriesId` — declared, never sent | `missing` |

and the same *reasoning*, applied one layer out to command arguments rather than result
fields, is what [R-120] (`providers.refresh` cannot carry the wizard's content choice) and
[R-139] (`NewEpisode` drops `plot` and `duration_secs`) are.

So this is not a missing idea. The idea is written down, with its history, in the file whose
job it is. It covers one interface because `PlayerState` was the one that bit, and nothing
generalised it afterwards.

**Generalising it is small.** `declared_fields(contract, interface)` already takes the
interface name as a parameter — it was written to be called more than once. The test becomes
a loop over a table of pairs:

```rust
for (iface, json) in [
    ("PlayerState",     to_value(aurora_player::PlayerState::default())),
    ("Movie",           to_value(library::MovieRow::default())),
    ("Series",          to_value(library::SeriesRow::default())),
    ("Episode",         to_value(library::EpisodeRow::default())),
    ("Channel",         to_value(channels::ChannelRow::default())),
    ("Recording",       …),
    ("Programme",       to_value(epg::ProgrammeRow::default())),
    …
] { /* the same two assertions */ }
```

The only work is `#[derive(Default)]` on the row structs that lack it, and writing down the
pairing — which this review has already done for all 64 (see the IPC-contract entry above,
including the three that pair by field overlap rather than by name and the two blind spots:
`serde(flatten)` and view wrappers).

The payoff is specific: a `#[serde(skip)]` forgotten on a credential-bearing field becomes a
red test rather than a leak, and a field the mock answers and the host does not becomes a
red test rather than a feature that works in all 154 Playwright journeys and in none of the
shipped builds.

### R-155 — Medium — there is no ESLint, and five `eslint-disable` comments suppress rules nothing runs

Checked four ways: `package.json` has no `eslint` or `jsx-a11y` devDependency and no `lint`
script (`dev, build, preview, typecheck, test, shots`), there is no config file of any
flavour in the repository, and `.github/workflows/ci.yml` has no lint step. The only
TypeScript check anywhere is `tsc --noEmit`.

Meanwhile five files carry suppressions for rules that no installed tool evaluates:

```
src-ui/src/components/ErrorBoundary.tsx   eslint-disable-next-line no-console
src-ui/src/features/live/LivePage.tsx     eslint-disable-next-line react-hooks/exhaustive-deps
src-ui/src/features/live/useNowNext.ts    eslint-disable-next-line react-hooks/exhaustive-deps
src-ui/src/hooks/useCommand.ts            eslint-disable-next-line react-hooks/exhaustive-deps
src-ui/src/hooks/usePages.ts              eslint-disable-next-line react-hooks/exhaustive-deps
```

Each of those four `exhaustive-deps` suppressions is accompanied by a good reason —
`useNowNext`'s *"`key` is the identity of the window; the array itself is new every
render"*, `usePages`'s `deps` passthrough — so they are not covering anything up. They are
evidence that the code was written expecting a linter, and that the linter never arrived.

What that costs, against this review's own findings: `jsx-a11y` would flag the
`click-events-have-key-events` and `no-static-element-interactions` cases behind
[R-102]'s dialogs and [R-151]'s hover-only pause; `react-hooks/exhaustive-deps` would flag
nothing new, since the four real cases are already suppressed deliberately. So a linter is
worth having mostly for the a11y rules, which is where this review found the most repeated
problems ([R-102], [R-106], [R-116], [R-147], [R-151], [R-152]).

`tsc --strict` is clean and was verified so earlier in this review, which is the stronger
half of the job and is being done.

### R-156 — Info — a direct push to `main` runs no tests, and this repository has had them

`.github/workflows/ci.yml:16`

The trade is deliberate and written down in full, which is why this is Info rather than a
finding against the design:

> A pull request gets the checks that can fail fast … A merge builds the thing … It used to
> run on every push to every branch — twice over while a PR was open, since `push` and
> `pull_request` both fired.
>
> What this trades away, stated plainly: `cargo check` proves the Windows code compiles, not
> that it links … A break there now surfaces on main instead of on the PR. A red merge
> leaves the previous release in place, so the cost is a stale installer rather than a
> broken one.

The gap the comment does not cover is a commit that reaches `main` **without** a pull
request. Both test jobs are gated on `if: github.event_name == 'pull_request'`, and the only
job that runs on `push` to `main` is the installer build — which does not run
`cargo test`, `tsc`, `vitest` or Playwright. So a direct commit to `main` is published as a
release having had nothing but a successful link.

That is not hypothetical here: this repository's recent history includes
`Add files via upload` and `Delete Ledger.xcodeproj.zip`, which are GitHub web-UI commits
straight to the branch. The reasoning behind the trade is about avoiding duplicate runs
between `push` and `pull_request`, which is the right thing to avoid; the fix that keeps
both is to gate on `github.event_name == 'pull_request' || github.ref != 'refs/heads/main'`,
or simply to require a PR for `main`.

### R-157 — High — every one of the 154 end-to-end journeys runs against a fully-enriched library, which no real library ever is

`src-ui/src/ipc/fixtures.ts:276` against `aurora-db/src/repo/library.rs:32`

What an import actually writes for a film — the complete `NewMovie` struct:

```rust
pub struct NewMovie {
    pub provider_key: String,
    pub title: String,
    pub match_key: String,
    pub year: Option<i32>,
    pub quality: Option<String>,
    pub group: Option<String>,
    pub url: String,
    pub poster: Option<String>,
    pub added_at: Option<i64>,
    /// The provider's own score out of ten. Enrichment overwrites it when TMDB has
    /// an opinion; until then it is the only rating the library has.
    pub rating: Option<f32>,
}
```

Ten fields. What `makeMovie` hands every journey:

```
id, title, year, quality, poster, backdrop, logoArt, overview, runtimeMins,
rating, certification, genres, cast, match, addedAt, lang, trailerKey
```

So the fixtures supply **eight fields no import writes** — `backdrop`, `overview`,
`runtimeMins`, `certification`, `genres`, `cast`, `trailerKey`, and `match` (which nothing
writes ever, [R-99]). Those eight come only from TMDB enrichment, which needs a key and a
sweep that has reached the title.

**This is not a transient first-run state.** `docs/ROADMAP.md` records enrichment against
the real service: *"TMDB was called with a real key for the first time, **matching 39 of 80
titles** from a panel's catalogue."* So after a complete sweep roughly half the library
still has none of those eight fields, permanently — and [R-139]'s addendum shows episodes
never get `runtime_mins`, `overview` or `air_date` from any source at all.

Which means the unenriched state is not an edge case the suite happens to miss. It is the
majority state of a real library, and **no journey has ever rendered it.** That single fact
is the common cause behind five findings above:

| Finding | What it is, restated as "the fixtures hid it" |
|---|---|
| [R-99] | `match` is in the fixtures and in no import, so "% match" renders in every journey and in no build |
| [R-100] | the unconditional `CC` badge is never contradicted, because nothing in the fixtures lacks metadata |
| [R-104] | the Details tab's five invented rows sit beside five real ones that are always populated |
| [R-106] | the zero-height still box needs `ep.still` to be null, and every fixture episode has one |
| [R-132] | `categories`, `isNew`, `isLive`, `rating`, `subTitle` are populated in the fixtures and absent from 529,638 real programmes |

And the same shape explains [R-98] (`getProgress` reads a map only the mock writes) and
[R-108] (`maxConnections: 2` in `fixtures.ts:396`, a column no production code writes).

**What to do about it is cheap and specific.** The mock already has three seams for exactly
this kind of thing — `failNextMock`, `hangMock`, `setSilentDurationMock` — each added after
a real failure the fixtures could not express, and each with a comment saying so.
`setSilentDurationMock`'s is the template:

> Not a hypothetical: a provider's VOD stream very often reports no duration … That is the
> state in which Skip Credits and autoplay were dead, and it was invisible here because
> **every fixture plays with a known runtime**.

A fourth seam — call it `unenrichedMock()`, or a `?unenriched` query parameter beside
`?video` and `?setup` — that strips those eight fields from the fixture rows would let the
existing journeys run twice: once against an enriched library and once against the one a
viewer actually has on the day they install it. Several of the findings above would have
turned red the first time it ran.

A cheaper half-measure, worth doing regardless: make *some* fixture rows unenriched.
`makeMovie` already varies quality, year and cast; having one film in six carry
`backdrop: null, overview: null, genres: [], cast: [], trailerKey: null` would put the
unenriched card on screen in every journey that renders a rail, at no cost to the others.

### R-158 — Low — Tailwind is a build dependency that generates nothing the app uses

`package.json`, `vite.config.ts:8`, `src-ui/src/styles/app.css:2`, `docs/DECISIONS.md` D5

The pipeline is fully wired: `tailwindcss` and `@tailwindcss/vite` in the dependencies, the
plugin in `vite.config.ts`, `@import 'tailwindcss'` at the top of `app.css`, and an
`@theme` block exposing four tokens:

```css
@theme {
  --color-bg: var(--bg);
  --color-surface: var(--surface);
  --color-accent: var(--accent);
  --color-text: var(--text);
}
```

Those four exist so that `bg-bg`, `text-accent` and friends resolve. Nothing writes them.
Every `className` in the entire UI, counted:

```
  16  aurora-field          3  no-scrollbar         1  aurora-switch
   3  text-shadow-hero      1  skeleton             1  aurora-page
                            1  boot-sweep           1  aurora-btn
```

Eight distinct classes, all hand-authored in `app.css`. **Zero Tailwind utility classes**,
and the app styles everything else with inline `style` objects reading
`var(--token)` directly.

D5's rationale is *"One source of truth, themeable at runtime by swapping a `data-theme`
attribute — which is what the OLED / Light / High-Contrast themes need."* That is true and
it is achieved entirely by `tokens.css`, which declares `:root[data-theme='oled']`,
`[data-theme='light']` and `[data-theme='contrast']` as plain CSS custom properties. The
`@theme` bridge adds nothing to it, because the bridge's purpose is to let Tailwind
*utilities* see the tokens and there are no utilities.

**One caveat before removing it**, and the reason this is Low rather than a confident
"delete it": `@import 'tailwindcss'` pulls in Preflight, Tailwind's CSS reset. Some of the
app's appearance may rest on it — `app.css` declares `* { box-sizing: border-box }` itself
on the next line, which suggests the author did not assume Preflight for that one, but
margins on headings, list styling and form-control normalisation would all change. So the
honest version of this finding is: the dependency earns nothing *deliberate*, and removing
it is a small visual-diff exercise rather than a no-op.

Worth doing because it is two packages, a Vite plugin and a CSS import on the critical path
of every build, in service of four variables nothing reads.

### R-141 — addendum — a green end-to-end test asserts the opt-out that a release build does not have

`tests-e2e/gemini.spec.ts:71`

```ts
test('removing the key takes the rail away again', async ({ page }) => {
  const panel = await settings(page);
  await panel.getByLabel('Gemini API key').fill('test-key');
  await panel.getByRole('button', { name: 'Use this key' }).click();
  await expect(panel.getByRole('button', { name: 'Get recommendations now' })).toBeVisible();

  await panel.getByRole('button', { name: 'Remove key' }).click();
  await expect(panel.getByText(/No key yet/)).toBeVisible();

  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await page.waitForTimeout(900);
  await expect(page.getByRole('region', { name: RAIL })).toHaveCount(0);
});
```

This asserts, step by step, precisely what `docs/PRIVACY.md` promises: remove the key, the
panel says there is none, and the rail that sends your history to Google is gone.

It passes, and it is green on every run, because **the mock has no built-in key.**
`BUILT_IN_KEY` is `option_env!("AURORA_GEMINI_KEY")` (`gemini.rs:42`) — resolved by the Rust
compiler at build time, with no counterpart in `mock.ts`, which keeps a single
`let mockGeminiKey = ''`. So in a browser, removing the key really does leave no key.

In the build a viewer installs, removing it leaves the compiled-in one:
`resolve_key` falls through to `built_in()`, `key_available` stays true,
`keyIsBuiltIn` becomes true, the panel switches to *"Using the key built into this build"*,
and the rail stays exactly where it was.

So the test does not merely fail to catch [R-141] — it **certifies the opposite of what
ships**, under a name that states the promise. It is the clearest instance in this review of
[R-157]'s shape: the mock is more generous than the host in one direction, and here it is
more *restrictive* in the other, which is the harder case to notice.

What would catch it: the mock needs a built-in key to simulate. A `?builtInKey` seam beside
`?video` and `?setup` — the same mechanism `hangMock` and `setSilentDurationMock` already
use — would let this test run twice and the second run would fail today.

### R-133 — addendum — and a green test asserts the flag that makes series rules inert

`tests-e2e/dvr.spec.ts:211`

```ts
await page.getByRole('button', { name: 'Record series' }).click();
await expect(page.getByRole('button', { name: 'Stop recording series' })).toBeVisible();

// The new rule is on the rules screen.
await page.goto('/#/recordings');
await page.getByRole('tab', { name: /Series rules/ }).click();
await expect(page.getByText('New episodes only').first()).toBeVisible();
```

The last assertion is the `newOnly: true` that [R-133] shows prevents the rule from ever
matching a programme on this subscription, because `<new/>` and `<premiere>` occur zero
times in 529,638 real programmes and `is_new` has no other source. The test treats it as the
expected outcome and checks for it by name.

It cannot do otherwise against a mock: there is no scheduler tick, no XMLTV, and nothing
that could observe a rule matching or failing to match. What it asserts — the button flips,
the rule is listed, the label is right — is all true. The thing that matters, that a rule
eventually records an episode, is not expressible here and is not covered anywhere else
either: `aurora-core`'s 19 rule tests cover `SeriesRule::matches` including the `new_only`
branch (`dvr.rs:294` builds programmes with an explicit `is_new: bool`), so the unit tests
prove the gate works and the end-to-end test proves the gate is set. Nobody asked whether
real data can satisfy it.

That is the gap worth naming: every layer tested the mechanism correctly, and the question
"does any programme this provider sends have `is_new` set?" belongs to none of them. It is
answerable in one line against a fetched guide, which is how this review answered it.

### R-139 — addendum — the test written for exactly this case asserts the fallback by name, and passes because the mock has what the host does not

`tests-e2e/skip-and-next.spec.ts:274`

This test exists *because* fixtures hide reality. Its own header says so:

> **The case an ordinary library is actually in: a stream that never says how long it is.**
>
> mpv reads a duration out of the container, and a provider's VOD stream frequently gives
> it nothing to read … Both the host and `upNextVisible` were taught to cope, **by falling
> back to the episode's stored runtime**, and it made no difference: `useEpisodeAids` would
> not *ask* until the player reported a duration, so `aids` stayed null for the entire
> episode and Skip Credits, Skip Intro and autoplay were dead together.
>
> **Every other test here plays a fixture with a known runtime, which is why none of them
> saw it.**

And the body forces the real condition, checks the premise, and then asserts the fallback
explicitly:

```ts
await page.evaluate(() => { window.__auroraSilentDuration(true); });
await playEpisodeWithoutChapters(page);

const state = await page.evaluate(/* player.state */);
expect(state.durationSecs).toBe(0);                       // the premise: it genuinely does not know

const upNextAt = await page.evaluate(/* library.playbackAids with durationSecs: 0 */);
expect(upNextAt, 'the host should place Up Next from the stored runtime').not.toBeNull();

await expect(page.getByRole('button', { name: 'Skip Credits' })).toBeVisible();
await expect(page.getByText(/Up next/i).first()).toBeVisible();
```

The assertion message is **"the host should place Up Next from the stored runtime"**. That
is the exact mechanism [R-139] shows does not exist: `episode_runtime_secs` reads
`episodes.runtime_mins`, and nothing in production writes that column — not the importer,
whose `NewEpisode` has no such field while the panel sends `duration_secs` for 100% of
episodes, and not enrichment, whose `ItemKind` is `Movie | Series` with no episode variant.

It is green because `mock.ts`'s `library.playbackAids` reads the *fixture* episode's
`runtimeMins`, and `fixtures.ts` gives every episode one. The mock's own comment
acknowledges the dependency — *"The host answers `library.playbackAids` from the episode's
**stored** runtime in this case, which the mock already mirrors"* — and the thing it mirrors
is a column the host never fills.

So the author found the failure, reasoned about it correctly, built a test seam for the half
of the condition that lives in the player (`__auroraSilentDuration`), wrote a test that
names the other half in its assertion, and shipped with that other half unimplemented. Of
everything in this review this is the clearest case of [R-157]: the fixtures were generous
in precisely the place the test was trying to be strict.

**The seam this needs already has three siblings.** Adding a fourth that strips
`runtimeMins` from the fixture episodes — `__auroraSilentRuntime()`, next to
`__auroraSilentDuration` — turns this green test red, and the red is [R-139].

### R-115 — addendum — the existing suite would catch this with one line of configuration

`tests-e2e/smoke.spec.ts:35` already asserts the thing [R-115] breaks:

```ts
test('hovering a rail card expands it with quick actions', async ({ page }) => {
  const rail = page.getByRole('region', { name: 'Recently Added' });
  const card = rail.getByRole('button').filter({ hasText: '' }).nth(2);
  await card.hover();
  await page.waitForTimeout(1100);
  await expect(rail.getByRole('button', { name: /^Play / }).first()).toBeVisible();
});
```

Under `prefers-reduced-motion: reduce` that `Play …` button does not exist — measured in
[R-115] as five buttons dropping to one — so this test fails. It passes only because
Chromium's default is `no-preference` and `playwright.config.ts` does not set it.

Playwright takes `reducedMotion` in the `use` block, so a second project in the config:

```ts
projects: [
  { name: 'default', use: {} },
  { name: 'reduced-motion', use: { reducedMotion: 'reduce' } },
],
```

runs all 202 journeys both ways. Today the second project would fail on this test and on
`lists.spec.ts`'s My List journeys, which also reach for the card's buttons — and that is
exactly the right failure, because those buttons are the ones a reduced-motion viewer
genuinely cannot reach.

Worth noting alongside [R-157]: that entry argues the fixtures hide the *data* a real
library has, and this one is the same argument about the *environment* a real viewer has.
Both are one config seam away from being covered by journeys that already exist.

**[R-115] — and one existing test already walks straight through it.**
`tests-e2e/trailers.spec.ts:96`:

```ts
test('turning animations off stops trailers starting by themselves', async ({ page }) => {
  await page.goto('/#/settings');
  await page.getByRole('switch', { name: /Animations/i }).click();   // animations OFF

  await page.goto('/#/series');
  const card = page.getByTestId('catalog-card').first();
  await card.hover();
  await page.waitForTimeout(1200);
  await expect(card.getByTestId('trailer-frame')).toHaveCount(0);
});
```

This is the only test in the suite that hovers a card with `animations === false`. It
passes — and it passes for the wrong reason. With animations off, `enter()` returns before
`setExpanded(true)`, so the card does not expand at all: there is no trailer frame, and
there is also no Play, no My List, no thumbs-up and no More info. The test concludes
"trailers do not autostart" where the truth is "the card no longer responds to hover."

That is the hazard of asserting an absence. `toHaveCount(0)` cannot distinguish *the one
thing was correctly suppressed* from *nothing happened at all*, and here it is the second.
One line added to this test:

```ts
await expect(card.getByRole('button', { name: /^Play / })).toBeVisible();
```

turns it red today, and the red is [R-115] exactly. Together with the `reducedMotion`
project suggested above, the suite needs no new journeys to cover this — it needs one
positive assertion next to an existing negative one.

**`motion.spec.ts` is the near-miss beside it.** It turns the same switch off and asserts
that CSS animation durations collapse (`.aurora-page` to `0.001s`), which is the half that
works. It never navigates back to a screen with cards. So the suite has one test that
checks the correct consequence of the switch and one that walks past the incorrect one.

### R-159 — High — five green end-to-end tests certify behaviour a shipped build does not have

Each of these is a well-written test. Each passes on every run. Each asserts something that
cannot happen on a real install, because the fixture supplies what the host does not. They
are listed together because the pattern is the finding — individually they read as thorough
coverage.

| Test | What it asserts | Why a real build cannot do it |
|---|---|---|
| `gemini.spec.ts:71` "removing the key takes the rail away again" | Remove key → "No key yet" → the AI rail disappears | `BUILT_IN_KEY` is `option_env!` at compile time with no mock counterpart, so removing the viewer's key falls back to the compiled-in one and the rail stays — [R-141] |
| `skip-and-next.spec.ts:274` "the aids work on a stream that never reports a duration" | `expect(upNextAt, 'the host should place Up Next from the stored runtime').not.toBeNull()` | `episodes.runtime_mins` is written by nothing but test fixtures — not the importer, not enrichment — so the fallback is always `None` — [R-139] |
| `multiview.spec.ts:97` over-limit journeys | "Needs 5", "allows 2", "1 recording using the line", "3 streams is more than your provider allows", Open disabled, "no arrangement fits" | `providers.max_connections` is never written, so `Budget::check` always returns `Unknown`, `is_refused()` is never true and nothing is ever disabled or refused — [R-108] |
| `dvr.spec.ts:211` "Record series becomes the undo action" | the rule is listed with "New episodes only" | `newOnly: true` plus zero `<new/>` markers in 529,638 real programmes means the rule matches nothing, ever — [R-133] |
| `trailers.spec.ts:96` "turning animations off stops trailers starting by themselves" | `expect(card.getByTestId('trailer-frame')).toHaveCount(0)` | passes because the card does not expand **at all** with animations off, so every button is gone too — [R-115] |

Four distinct mechanisms produce this, and they are worth separating because the fixes
differ:

1. **The fixture declares a field the host never writes.** `maxConnections: 2`
   (`fixtures.ts:396`), `match` (`:299`, `:328`), every episode's `runtimeMins`. Fixed by
   [R-154]'s generalised contract test, which compares declared fields against what the
   host actually serialises in **both** directions.
2. **The mock cannot model a compile-time fact.** `BUILT_IN_KEY` is resolved by `rustc`;
   `mock.ts` has a mutable string. Fixed by a `?builtInKey` seam like the three that already
   exist.
3. **The assertion is an absence.** `toHaveCount(0)` cannot distinguish "suppressed
   correctly" from "nothing happened". Fixed by pairing every negative assertion with a
   positive one — one line in `trailers.spec.ts`.
4. **The question belongs to no layer.** `aurora-core` proves `SeriesRule::matches` honours
   `new_only`; the journey proves the flag is set; nobody asks whether real data can satisfy
   it. Fixed only by running something against a real feed, which is what
   `aurora-ingest/examples/probe.rs` exists for and what this review did.

The suite is 202 journeys over 5,341 lines and it is genuinely good — `ipc-wire.spec.ts`
catches the argument-nesting bug no mock could see, `boot-stuck.spec.ts` needed two new
seams to reach a state nothing else could, `icons.spec.ts` measures glyph geometry rather
than eyeballing it, and `errors.spec.ts` exists because 23 of 27 `invoke` calls had no
`.catch`. Every one of those was written after a real failure. The five above are the same
instinct applied to a fixture that was more generous than the host, and the pattern is only
visible once you line them up.

**[R-63] — and the hostile-input file that does not reach it.**
`aurora-core/tests/hostile_input.rs` is 351 lines of exactly the right tests: a
50,000-entry playlist inside a time budget, every way a playlist has been seen to be
broken "none of which may cost more than the entry it is on", invalid UTF-8, a lone CR, a
NUL mid-name, a single enormous line with no newline, both sides of a DST transition
normalised to UTC, half-hour and three-quarter-hour zones, and a large guide streamed
rather than buffered.

It tests `aurora-core`. The `urldecode` that panics on `%` followed by a 3- or 4-byte
character is in **`aurora-ingest`**, whose only coverage of `parse_pasted_xtream` is three
happy-path assertions in the function's own module (`source.rs:194`, `:195`, `:212`) plus
one empty-credentials case. So the crate with a dedicated hostile-input suite is not the
crate with the hostile-input bug, and the correct `urldecode` — the one whose comment names
both defects — is the one that *is* covered, in `aurora-core`.

Moving the function to `aurora-core` (which [R-63]'s addendum recommends anyway, to delete
the duplicate) puts it behind this file at the same time.

**[R-129] — the same concurrency hazard is understood and soak-tested, for tuning.**
`aurora-app/tests/stress.rs:208`:

```rust
/// The same thing from several threads, which is what the IPC surface actually is:
/// every Tauri command runs on its own thread, so nothing stops two tunes overlapping.
#[test]
fn concurrent_zapping_never_deadlocks_or_wedges() {
```

alongside `two_thousand_zaps_always_end_on_the_channel_last_asked_for`, whose invariant is
*"whatever the sequence, the channel playing at the end is the last one asked for, and the
session agrees."* So `#[tauri::command(async)]` meaning "two of these can be in flight" is
written down, soaked at 2,400 concurrent tunes across 8 threads, and asserted.

`Mosaic::set_tile` takes and releases the same lock three times across a budget check and
the build that acts on it, and has no equivalent test. The reasoning exists one file away;
[R-129] is where it was not applied.

### R-160 — Info — every test count in `README.md` understates the suite, and the two docs disagree

Counted against the repository as it stands:

| Claim | Where | Documented | Actual | Method |
|---|---|---|---|---|
| E2E journeys | `README.md` status table | 114 | **202** | `playwright test --list` |
| Playwright runs | `docs/ROADMAP.md` | 154 | **202** | same |
| `aurora-core` tests | `README.md` | 288 | **350** | `#[test]` / `#[tokio::test]` attributes |
| `aurora-db` tests | `README.md` | 224 | **291** | same |
| `aurora-ingest` tests | `README.md` | 213 | **242** | same |
| `aurora-player` tests | `README.md` | 21 | **32** | same |
| `aurora-app` tests | `README.md` | 116 | **209** | same |

All six Rust figures and both Playwright figures are low, and the two documents disagree
with each other as well as with the code (114 against 154 for the same suite). Totals: 976
documented, 1,326 present.

**One caveat on the Rust numbers**, because the method is not exact: counting `#[test]`
attributes includes anything behind `#[cfg(windows)]` and anything `#[ignore]`d, so the
actual column is an upper bound on what a given `cargo test` run executes. The Playwright
number is exact — it is the runner's own enumeration. The direction is the same either way
and the gap is too large to be accounted for by platform gating alone.

Harmless in itself, and recorded for one reason: these counts are the project's own evidence
for the claims beside them in the status table, and this review has leaned on documented
figures repeatedly — correcting the 6.3 MB `channels.list` payload ([R-117]), the 42.8% EPG
coverage ([R-132]), and the "39 of 80" TMDB match rate that turned out to be accurate and
load-bearing ([R-157]). Numbers in these two files get believed. These ones are stale rather
than wrong, which is the easier problem, and `playwright test --list` and `cargo test` print
the real ones.

### R-161 — Medium — the unit-test layer covers two files, and its own config names the gap that is still open

`vitest.config.ts`

The config exists because there were no unit tests at all, and its comment says which ones
were missing:

> `pnpm test` used to answer "No test files found, exiting with code 1", because there were
> none: **not for `shared/ipc.ts`, not for the formatters, not for the zapper's
> arithmetic.** The 84 Playwright journeys all run against the *mock* transport, which is
> exactly why a host that never sent `Channel.favorite` and a CSP that blocked every
> provider logo were both invisible from here.

Enumerated by the runner, `pnpm test` now covers **two files**:

```
src-ui/src/ipc/subscribe.test.ts   7 tests — wireEventName, and subscribe()'s teardown races
src-ui/src/lib/errors.test.ts      … messageOf, notify
```

So of the three gaps the comment names, one was closed (`subscribe`, which is neither of
them) and **the formatters and the zapper's arithmetic are still untested**. That is not an
abstract omission — it is why three findings in this review are live:

* [R-94] `untilLabel` renders "in 1h 60m" for 690 of every day's seconds, because `h` is
  floored and `m` is rounded. A table-driven test over the hour-to-day range is four lines
  and would have failed on the first run.
* [R-95] `remaining()` returns `" left"` with nothing in front of it under 30 seconds,
  because it concatenates `runtime(0)`, which is the empty string by design.
* [R-96] `bytes()` prints "1024 KB" and "1024 B", because the carry test happens before the
  rounding.

All three are pure functions of their inputs, in a file with no dependencies, which is the
easiest thing in the repository to test and the thing the config was written to point at.
`useZapper`'s arithmetic — digit entry, last-channel, Ch+/Ch− wrapping — is the same shape
and also still uncovered.

Two smaller notes on the config, both correct as written: `environment: 'node'` with the
reason given (nothing under test needs a DOM, and adding jsdom "to run four listeners would
be a dependency for its own sake"), and the deliberate exclusion of `tests-e2e` so a unit
run does not start a browser. The `include` glob is `*.test.ts` only, not `*.test.tsx`, so a
future component test has to be named `.ts` or it is silently not collected.

### R-139 — addendum — the four test layers, and the gap between them

There is a real-host harness (`tests-host/`, ~3,500 lines of Python driving the release
binary over WebDriver against a real SQLite file and a real panel), and its `resume.py`
scenario exists for a neighbouring problem:

> **Whether a film reports a position and a duration — which is what three features need.**
> Continue Watching is empty on a real subscription. `saveProgress` refuses to write unless
> `durationSecs > 0` **and** `positionSecs > 0`, so a player that reports neither saves
> nothing, ever, and the rail can only be empty. **Up Next hangs off the same numbers:** it
> appears when the position is near the end, which needs an end to be near.

So the harness knows the duration matters. What it asserts is:

```python
assert state.get("durationSecs", 0) > 0, …   # resume.py:110
assert state.get("positionSecs", 0) > 0, …   # :115
assert rows > 0, …                            # a watch_progress row was written
```

— the case where the player **does** report a duration. And
`grep -rn "playbackAids\|upNext\|runtime_mins" tests-host/` returns **nothing**, so the
fallback is not exercised on a real host at all.

Laid out across all four layers, the condition in [R-139] falls between every one of them:

| Layer | What it covers | Why it misses this |
|---|---|---|
| `aurora-core` markers tests | `up_next_at` arithmetic **given** a duration | never asked where the duration comes from |
| `aurora-db` `runtime_tests` | `episode_runtime_secs`, including the median-of-siblings branch | its helper inserts `runtime_mins` directly (`library.rs:2237`), which no importer can |
| `tests-e2e/skip-and-next.spec.ts:274` | player reports **no** duration → the fallback must work | fixture episodes carry `runtimeMins`, so the fallback has data the host never stores |
| `tests-host/scenarios/resume.py` | real host, real library, real player | asserts `durationSecs > 0`, i.e. the opposite branch |

Each layer tested the thing it owns, correctly. The condition that is broken — *a real
library, where the player reports no duration* — is the intersection of two layers' blind
spots, and the only test aimed at it was aimed from the side that has fixture data.

This is the shape `docs/TESTING_THE_HOST.md` already names as the project's largest source
of shipped bugs (see the closing note below), one level more specific: not "the mock
answered its own question" but "every layer answered its own question, and nobody owned the
one in between."

### R-108 — addendum — the real-host harness states the refusal works, and then arranges not to test it

`tests-host/scenarios/real_panel.py`, in
`_a_mosaic_gets_the_main_surface_out_of_the_way`'s docstring:

> **One filled tile, deliberately. This line allows a single connection, so a four-stream
> mosaic is refused by `Budget::check` and correctly so; one tile is the most that can ever
> open here.**

Both halves of that sentence are wrong in the same way, on the one machine where it could
have been noticed. `providers.max_connections` is never written ([R-108]), so on this very
subscription `Mosaic::limit()` returns `None`, `Budget::check` returns `Unknown`,
`is_refused()` is false, and a four-stream mosaic is **not** refused — it is opened, and
the provider cuts the streams. "One tile is the most that can ever open here" is true of the
line and not of the software.

The scenario then opens exactly one tile, so the claim is never exercised. That is a
reasonable thing for the scenario to do — its subject is HWND z-order, not the budget, and
a four-tile attempt on a one-connection line would make the surface assertions unreliable.
But it means the belief that the budget works is recorded in the only place that runs
against a real line, and the arrangement that records it is the one that avoids checking it.

Three places now assert or assume this refusal, and none of them can observe it:

* `tests-e2e/multiview.spec.ts` asserts the refusal text, and passes because
  `fixtures.ts:396` declares `maxConnections: 2` ([R-159]).
* `MosaicPage.tsx`'s header describes the refusal as the thing "the feature is judged on"
  ([R-123]).
* `real_panel.py` states it as settled fact and opens one tile.

So the only way to see it is to read `providers.rs:609` and notice the column is not in the
`INSERT`. Which is how this review found it, and why [R-108] is the entry the other three
depend on.

**[R-118] — where the real-host harness stops.** `real_panel.py`'s
`_records_a_real_stream` is the most thorough check in the repository of a thing actually
working: it schedules through `dvr_schedule`, polls `dvr_list` until the state settles,
asserts the file exists at the reported path, asserts `os.path.getsize(path) > 64 * 1024`
with the reason — *"a file that exists and is empty is the failure this whole check is for:
the recorder opened the stream, wrote nothing, and reported success"* — then navigates to
`#/recordings` and asserts the recording is **listed**.

It never presses Play, because there is nothing to press: [R-118] shows no command can play
a recording. So the one scenario that follows a recording from a real provider all the way
to disk and onto the screen ends at exactly the point where the feature stops existing, and
nothing in it notices, because listing it is a reasonable place for a recorder test to
finish.

`docs/ROADMAP.md` is candid that the size assertion *"has not had a green run"*, so this
path is known to be unverified end to end. What is not recorded anywhere is that even a
green run would leave the recording unplayable.

### R-162 — Medium — the two tests that guard the IPC contract do not run on a plain `cargo test`

`src-native/Cargo.toml:13`

```toml
# aurora-app pulls in Tauri, whose Linux backend needs GTK and whose Windows build
# needs the MSVC toolchain. Excluding it from the default set keeps a bare
# `cargo test` working on any host; CI builds it explicitly on windows-latest.
default-members = [
  "crates/aurora-core",
  "crates/aurora-db",
  "crates/aurora-player",
  "crates/aurora-ingest",
]
```

The reasoning is sound — Tauri's Linux backend needs GTK dev packages, and a bare
`cargo test` that fails on a machine without them is worse than one that covers four
crates. But `aurora-app/tests/` is where both mechanical guards live:

```
contract.rs       every declared command is registered, both ways; PlayerState's fields;
                  the name mapping; the CSP's image schemes
command_args.rs   every #[tauri::command] names its parameter `args`; no command runs
                  on the main thread (D22)
stress.rs         2,000 zaps end on the right channel; concurrent tunes never wedge
```

So a developer running `cargo test` before pushing gets 915 tests across four crates and
**none of these**. They run under `cargo test --workspace`, which is what CI does
(`ci.yml:77`), so they are not unprotected — but the gate that catches "the UI declares a
command the host never registered" is the one a local run skips, and that defect class is
[R-154]'s whole subject.

Two ways to close it without reintroducing the GTK problem: put the parsing-only tests
(`contract.rs` reads `shared/ipc.ts` and `main.rs` as text; `command_args.rs` reads the
source files) in a crate that does not depend on Tauri, or add a `cargo test --workspace`
line to the contributing docs beside the existing `cargo test`. The first is better,
because both tests are pure text analysis and neither needs the host to build at all.

**And a small correction to [R-115]'s addendum.** That entry noted `applyDom` is never
called at store creation, so `<html>` carries no `data-theme`, `data-density` or
`data-motion` until a setting is touched. `data-theme` is in fact set statically:

```html
<html lang="en" data-theme="dark">
```

(`src-ui/index.html:2`). So the theme is correct from the first paint, and only
`data-density` and `data-motion` are absent — which remains harmless today for the reasons
given there (`data-density='tv'` is the only density override, and the reduced-motion rules
are duplicated inside an `@media` block), and remains a trap for whoever persists the
settings per [R-87].

### R-163 — Info — `docs/TESTING_THE_HOST.md` describes a worker-pool hazard that the code has already closed

`docs/TESTING_THE_HOST.md`, the section after *"What driving it for real found immediately"*:

> What is left in release is a Tauri async worker **parked for the whole length of the
> request**. Every command in this app is `#[tauri::command(async)]`, which runs it on the
> shared Tokio runtime, and four of them do blocking HTTP on that thread:
>
> | `providers_validate` | one fetch, up to 12s connect + 120s read, ×3 attempts |
> | `providers_refresh` | a whole sync — 23.7s measured on a real subscription |
> | `updates_download` | the length of a release download |
> | `artwork_prefetch` | one TMDB fetch per poster |
>
> The runtime has one worker per core, so this is contention rather than a freeze … But it
> is the wrong pool, **it is `artwork_prefetch` away from exhausting itself**, and in
> development it is simply a crash.

All four commands are still `#[tauri::command(async)]` — confirmed — and the HTTP client is
still `reqwest::blocking`. But the worker is no longer parked. `aurora-ingest/src/http.rs:47`
wraps every blocking call:

```rust
fn blocking<T>(f: impl FnOnce() -> T) -> T {
    match Handle::try_current() {
        Ok(h) if h.runtime_flavor() == RuntimeFlavor::MultiThread => tokio::task::block_in_place(f),
        _ => f(),
    }
}
```

> `block_in_place` is Tokio's answer to exactly this: it hands the worker's queue to a
> replacement thread for the duration, so the runtime keeps making progress while this
> thread sits in a `recv`.

and it is applied in the two places that cover everything: the shared send at `:334`
(`blocking(|| build().send())`, which `fetch_bytes`, `fetch_string`, `post_json` and
`fetch_reader` all route through) and the response body at `:279`, where the reader itself
is wrapped —

> `fetch_reader` hands back a stream, and the download happens as the caller reads it — so
> wrapping only the request would cover the handshake and leave the body, which is the part
> that takes minutes, blocking a worker unannounced.

Tauri's runtime is multi-thread, so the `MultiThread` arm is the one that runs in the
shipped app. The hazard is closed, including the half (the body) that the obvious fix would
have missed.

So this is a fifth instance of [R-150]'s stale-passage pattern, and the most consequential of
them: the other four describe superseded *decisions*, while this one describes a live
architectural risk that no longer exists. A reader taking it at face value would go looking
for a worker-pool problem, find `#[tauri::command(async)]` and `reqwest::blocking` exactly
where the document says they are, and conclude it was right.

The paragraph that should survive is the one after it, which is still true and still useful:
the `reqwest` guard that catches this lives behind `debug_assertions`, so *"a green release
run says the app works; a green debug run says it works for the right reason."*

**[R-120] — the one test on that step covers the one thing it does.**
`tests-e2e/setup.spec.ts:140`:

```ts
test('deselecting every content type blocks the import', async ({ page }) => {
  ...
  for (const name of ['Live TV', 'Movies', 'Series']) {
    await page.getByRole('switch', { name, exact: true }).click();
  }
  await expect(page.getByRole('button', { name: 'Import library' })).toBeDisabled();
});
```

That asserts the `none` guard at `SetupWizard.tsx:527` — which [R-120] identifies as the
**only** consumer of the `content` state besides its own rendering. So the switches are
covered for the single behaviour that is wired up (all three off disables Import) and not
for the behaviour the step exists to provide (two off means two kinds are not imported).

No test turns off *one* switch and then checks what the library holds, which is the
assertion that would fail. On a real host it is also checkable in one line through
`tests-host`: deselect Movies, import, then
`SELECT count(*) FROM movies` should be 0 — the harness's own advice is *"'The screen says 3
channels' is a claim about React; `SELECT count(*) FROM channels` is 3 is a claim about the
thing that was broken."*

### R-164 — Medium — the host harness cleans up credentials from the wrong service, so it never removes its own and is only safe by accident

`tests-host/run.py:232`, `:278`, `:366`

The harness launches the app under a **separate** credential service:

```python
env.setdefault("AURORA_CREDENTIAL_SERVICE", "AuroraTV-tests-host")   # :366
```

and `aurora-ingest/src/credentials.rs:138` honours that override (`SERVICE_ENV`), falling
back to `SERVICE = "AuroraTV"`. So every secret a scenario writes is filed under
`{key}.AuroraTV-tests-host`.

The cleanup deletes from the default service instead:

```python
CREDENTIAL_SERVICE = "AuroraTV"                     # :232
...
target = f"{key}.{CREDENTIAL_SERVICE}"              # :278
subprocess.run(["cmdkey", f"/delete:{target}"], ...)
```

Two consequences, and the first is the one that matters:

1. **The harness never removes what it created.** Its entries are under
   `AuroraTV-tests-host` and the cleanup asks `cmdkey` to delete `…AuroraTV`. So every
   scenario that adds a provider — `real_panel.py` does, on every run — leaves an orphan in
   Credential Manager for ever. That is precisely the condition `reclaim_credentials` was
   written to prevent: *"Deleting the data directory therefore leaves each run's secret
   behind in Credential Manager with nothing left that references it."*
2. **It asks to delete from the real service**, which is where the machine owner's own
   Aurora keeps its passwords. It does no damage, but not because of anything the cleanup
   does: `credential_ref(namespace, id)` is `aurora-{namespace}-provider-{id}` with a
   **random** namespace per library (`credentials.rs:29`, `:33`), so a key read out of a
   throwaway test library cannot name an entry the real install created. The protection is
   the randomness, not the guard.

The docstring shows the history, and shows the fix stopping one step short:

> It must not be tidied up by pattern. That mistake has already been made once here — keys
> used to be `aurora-provider-{row id}`, every library's first provider was
> `aurora-provider-1`, and **this harness quietly destroyed the password of the copy of
> Aurora the machine's owner actually uses.** So the keys are read out of the library that
> is about to be destroyed, which is the only way to be certain every one of them was
> created by this suite.

Both remedies — reading the key from the doomed library, and the `key.count("-") >= 3`
refusal at `:273` — are about the **key**. Neither is about the **service**, and the service
is the half that was also changed when `AURORA_CREDENTIAL_SERVICE` was introduced.

One line: delete from the service the app was actually launched with, which `driver_env`
already knows —

```python
target = f"{key}.{env.get('AURORA_CREDENTIAL_SERVICE', CREDENTIAL_SERVICE)}"
```

— and `CREDENTIAL_SERVICE` becomes the fallback it is named for rather than the value used.
Worth doing on the Linux branch too (`secret-tool clear service …`), which has the same
constant.

---

# Fixes

Worked through in batches of five, each batch on its own branch and its own pull request.
A fix is only recorded here once it has been verified by something that fails without it —
in most cases the same measurement that found the defect, re-run.

## Batch 1 — everything `docs/PRIVACY.md` promises

One claim with five holes in it. `PRIVACY.md` is the document written for the person who
wants to know what leaves their machine, and three of its sentences were false in different
directions: one about the log, one about the database, one about how to opt out.

| # | Finding | Commit | Verified by |
|---|---|---|---|
| 1 | [R-63] `urldecode` panics on `%` + a 3/4-byte character, and mangles UTF-8 to Latin-1 | `6ef91c9` | The probe that found it, re-run against the production function: 4 of 9 inputs panicked, now 0; `caf%C3%A9` was `"cafÃ©"` (5 chars), now `"café"` (4). Plus 5 unit tests on the new module and 2 on the paste parser, which had no hostile-input coverage at all. |
| 2 | [R-61] `redact` leaves every live stream URL intact | `4ba2d80` | The seven real URL shapes, re-run: 1 of 7 leaked (the one covering all 6,699 live channels), now 0 of 7 — and the four ordinary paths that must not be mangled are still returned unchanged. |
| 3 | [R-126] (IPC half) `EpisodeRow.url` carries the password into the WebView | `ed7c680` | A test that serialises a real episode row and asserts the username, password and host are absent from the JSON — asserted on the serialised form, because that is what crosses the bridge. Covers `library.episodes` and `playbackAids.nextEpisode`. |
| 4 | [R-141] the documented opt-out does not opt out | `62551e7` | `resolve_key` split into a pure `key_from(enabled, own, built_in)` and its table asserted exhaustively — including the row that previously had nowhere to exist: off yields no key for all four combinations of the other two inputs. Plus an e2e journey that turns the feature off **with a key still saved**, which is the state the old instruction could never reach. |
| 5 | `docs/PRIVACY.md` and the release notes template now say what is true | — | By reading, against the four claims. |

### What fix 3 does and does not do

[R-126] has two halves and only one is closed. The **IPC** half is: `EpisodeRow` no longer
carries `url`, so no command serialises a credential into the renderer. The **storage**
half is not: `channel_sources.url`, `movies.url` and `episodes.url` still hold
`http://host/<user>/<pass>/<id>` verbatim, so `library.db` still contains the subscription's
password several hundred thousand times.

That is a schema change — store the stream id and the extension, rebuild the URL at
playback — touching sync, playback, the recorder, the mosaic and catch-up, with a migration
over existing libraries. It is the first item of batch 2 rather than something to rush into
the end of batch 1.

So `PRIVACY.md` now says so, in those words, and tells the viewer not to attach
`library.db` to a bug report. A document that overstates the fix is worse than one that
admits the gap: Settings → Diagnostics shows them that folder.

### Two things worth noting about how these went

**The fix for [R-61] was one character.** `looks_like_stream_id` required a file extension;
the panel serves live channels without one. The defence had been written, documented in
`PRIVACY.md`, and given a regression test — against the form with a `.ts` on the end.

**Fix 4 changed shape once it was testable.** The obvious change was an early return in
`resolve_key`, which works and cannot be tested without constructing a whole `Services`.
Splitting the decision into a pure function of its three inputs made the one case that was
wrong assertable, and the test is a nine-row table rather than a sentence of reasoning. The
off state previously had nowhere in the code to be represented, which is the same reason it
could not be tested and the same reason it did not exist.
