# Aurora for Apple TV, iPhone and iPad

The tvOS and iOS app. It's a separate project in the same repository: it **reuses**
the Windows app's shared Rust crates and **changes none of them**. The plan it follows
is [`docs/APPLE_PORT_AND_SYNC.md`](../docs/APPLE_PORT_AND_SYNC.md). This file records
where the work deliberately departs from that plan.

## Status

Phase 2 of the plan (the spike: Rust → Swift → video), plus the start of Phase 4's
screens. The app can:

- ask "Who's watching?" and check a profile's PIN (skipped at launch when there's one
  profile with no PIN, as on Windows). Wrong tries and the lockout are counted in the
  shared database, so they're the same tries Windows counts,
- add, rename and delete profiles and set, change or remove a PIN, by Windows'
  editor's rules (a kids profile starts at age 12; a PIN is 4 digits; the last
  profile stays), checked against `ProfilePicker.tsx` by `mirrors.rs`,
- add an Xtream Codes panel or an M3U playlist (the password goes in the Keychain),
- import it with progress: live TV only (the tvOS default) or with films and series,
- list channels by group, with now/next from the guide,
- keep favourite channels per profile (long-press a channel),
- show the guide grid (channels down, time across), fetching only the rows on screen
  as Windows does since R-130; choosing what's on plays the channel, choosing what
  already aired replays it where the archive allows,
- filter the guide as Windows does: group chips to tap in or out, and a Filters sheet
  with every group searchable, shown only or hidden, plus with listings, favourites,
  catch-up, radio and quality. The rules are Windows' `guideFilters.ts` in the core
  (`guide_filters.rs`), and the choice is kept on the device,
- show a channel's schedule a day at a time, and replay what already aired where the
  channel keeps an archive (catch-up, up to a week back),
- keep films and series on My List, and like them,
- open on Home, as Windows does: Continue Watching, then the personal feed the shared
  core builds (Top Picks, Because you watched…, Trending with a metadata key, the
  shelves), each rail paging as it scrolls; long-press a card for "Not interested", or
  to take it off Continue Watching,
- search channels, what's on now and later, films and series in one box,
- browse films and series by the provider's shelves, genre and first letter, sorted
  newest, A–Z, by year or by rating, with search, a page at a time and with the true
  total,
- filter the whole library as Windows does: English only, also hide what's untagged,
  and one copy per title, each with what it would hide (Settings),
- list providers in Settings with what each holds, and refresh, edit or remove one.
  Editing works like Windows: change the name, address and sign-in (the password shown
  only on request), and Check connection tries the form without saving it. When the
  address fails it tries the other scheme and the playlist, and offers whichever works.
  Pasting a `get.php` link when adding a provider fills in the address and sign-in,
- open a series on the episode to resume, list its seasons and episodes,
- play a channel, film or episode through mpv (MPVKit). A channel tries each of its
  sources in turn. Films and episodes resume, and save their position every 15 s and
  on pause or stop,
- control playback as Windows does: subtitles, audio track, speed, aspect and a sleep
  timer; Skip Intro / Recap / Credits, learned from the viewer's own skips and the
  file's chapters; Up Next at the credits; and per-series switches to always skip
  intros or recaps and to play the next episode. On Apple TV the remote jumps back 10 s
  and forward 30 s with the controls hidden, and Menu backs out one step at a time,
- keep channels in named favourite lists, rename and empty them, and reorder them,
- edit the playlist (its own Playlist tab, as Windows' nav item; Settings → Library on
  an iPhone): rename, renumber, regroup or hide channels, films and
  series, hide everything a search matches, see a title's other copies, and put any of
  it back,
- change channel from inside the player, as Windows does: channel up and down (on
  Apple TV, up and down on the remote), a number typed on a keypad (up to 4 digits,
  tuned 1.8 s after the last one), and back to the last channel. A banner names where
  it landed, or says there is no such number. The timings and the order up and down
  walk are Windows', checked by `mirrors.rs`,
- pause and rewind live TV, as Windows does: mpv's own disk cache is the buffer (in the
  app's Caches folder), up to 1 GB / 30 minutes by default and set in Settings. The bar
  runs from the oldest moment held to live, says how far behind live you are, and Back
  to live returns to the edge. The window is Windows' arithmetic (`aurora_core::timeshift`).
- "On now" at the top of Home, as Windows' `OnNowTile`: the first six favourite
  channels, what each is airing with its times and progress, and a tap to tune; with no
  favourites, a line saying how to add some. Channels without a logo get Windows'
  `ChannelTile`: the number (or first letter) on one of the seven art colours, picked
  from the name the same way, so a channel is the same colour in both apps,
- the channel banner over the player, as Windows' `ChannelBanner`: on every tune, the
  channel's logo, number, name and quality tag, what is on now with its times and how
  far through, what is next (or "No guide data for this channel"), and the time, for 5
  seconds. On an upright iPhone the time is left to the status bar and the logo and
  number are a size smaller, so the name has room (Windows never draws it that narrow),
- the seek preview on iPhone and iPad (Windows' `Scrub.tsx` and `play/preview.rs`):
  dragging the bar shows a bubble with the time, the chapter or skip marker there, and on
  live TV the wall-clock moment and what was on. The picture in it comes from frames mpv
  takes of what has already played (`screenshot-raw`, 320 px wide, every few seconds
  once playback is steady), so it exists only for moments that have been on screen. The
  rules are Windows' `Frames`, copied and checked by `mirrors.rs`. Apple TV has no
  drag to preview; it gets the banner only,
- Settings in Windows' sections and order: Providers, Appearance, Sync, Parental
  controls, Artwork and metadata, Recommendations, Privacy, Filtering, Pause live TV,
  Library (the counts and guide coverage), About, Diagnostics. Departures:
  - Appearance has Windows' theme (Dark, Oled, Light, Contrast, in its colours),
    Animations and 24-hour clock, kept on the device as Windows keeps them. Interface
    density is not offered (an Apple TV is always the 10-foot UI, a phone never is),
    nor glass transparency (the system draws the glass), nor hover previews (nothing
    hovers). Every page, pushed page and sheet sits on the theme's own background
    (Dark is Windows' charcoal, not the system's black); the player keeps its own, as
    on Windows. A switch between light and dark rebuilds the tabs and any open sheet
    so the system's glass bars follow, which returns pushed pages to their tab's root.
  - Updates and Keyboard are not there: the App Store updates the app, and neither a
    remote nor a touchscreen has Windows' shortcuts.
- Settings → Diagnostics and About, as on Windows. Diagnostics says when the library
  had to be started again, names the data folder and the video engine, and exports this
  run's log, with every address's login removed (`redact_log`), to share from an iPhone.
  The log is the system's: the core's own lines (`log.rs`, at Windows' `info`) and mpv's
  warnings, redacted before they are written, both go into it. About carries the
  version, the licence and the third-party notices. Those are this folder's
  `THIRD-PARTY-NOTICES-APPLE.md` (MPVKit's LGPL build, shipped as separate frameworks)
  followed by the shared file, both bundled with the app.
- Gemini recommendations, as on Windows: a "Because of what you watch" rail on Home,
  after Continue Watching, built from the watch history and kept only where the library
  carries the title (`gemini.rs`, on the shared `aurora_ingest::gemini`). The key is the
  viewer's own (Keychain) or one compiled in from `AURORA_GEMINI_KEY`. Settings →
  Recommendations has Windows' on/off switch, which is what stops anything being sent,
  and says why the rail is missing when it is.
- The assistant, "Ask Aurora": a conversation that looks through the library and hands
  back titles to play. The loop is Windows' `assistant.rs`, copied into the core because
  the Tauri host cannot be linked, and `mirrors.rs` compares the copy with the original
  line by line. It is a tab on Apple TV and iPad, and a button on Home on an iPhone.
  "Ask about these" on the Gemini rail opens it with those titles.
- the setup wizard, Windows' four steps and wording: paste an address (a `get.php` link
  is split into address, username and password, and the box keeps the address alone),
  check it (a failed check is advice, and Continue becomes "Add it anyway"), choose what
  to import, watch it import or leave it running in the background, then see what came
  in and how much of it the guide covers. It opens on first run, before "Who's
  watching?", while there is no provider; "Skip for now" lasts until the next launch.
  Settings and Live TV open it as "Add another provider". `-AuroraSetup YES` shows it
  over a library that has providers (debug builds).
- sync with the household's other devices, Windows included (Settings → Sync, Windows'
  Sync panel and wording): set up or join with an email, a password and a passphrase
  (or the recovery code); favourites, My List, watch progress, profiles and playlist
  edits follow, end-to-end encrypted. Opening a film or episode first asks the
  household for its position, waiting at most 1.5 s, as Windows does. The sign-in and
  the vault key are in the Keychain. The loop runs while the app is in front; the
  shared pacer decides when a round is due, and the free plan's daily count and the
  slower "thrifty" pace are the same as Windows'. Needs `AURORA_FIREBASE_API_KEY` and
  `AURORA_FIREBASE_PROJECT_ID` in the environment when the core is built
  (`scripts/build-core.sh`); without them Settings says the build has no sync.

- parental controls, as Windows applies them since F-36 to F-38. The shared database's
  gate (`aurora_db::repo::parental`) already hides from every list what the profile
  watching may not see; the core (`parental.rs`, mirrored from `aurora-app` and checked by
  `mirrors.rs`) does the host's part. "Who's watching?" chooses through the core, which
  checks the profile's PIN and forgets the choice at every launch (until then the
  strictest profile applies). Playing something over the profile's age limit or in an
  adult category is refused; a locked channel or category, or a day's viewing time used
  up, asks for the parental PIN over the player and then plays. With a parental PIN set,
  adding or deleting profiles, changing someone else's PIN, limits and locks need it, and
  a kids profile (or everybody, if the household chose) is kept out of Settings: the
  page says so, and the 19 core calls behind it refuse until the PIN is entered. The
  daily limit is counted by the player once a second, warns five minutes before, and
  closes the player when it runs out. Settings → Parental controls sets the PIN, hides
  adult categories, locks Settings, hides locked content, each profile's age and daily
  limit, and what is locked; Live TV draws a padlock on locked channels, and an empty
  Movies or Series page says when it is the age limit holding back unrated titles.

- recordings, which the plan (docs/APPLE_PORT_AND_SYNC.md §2.3) left on Windows: the
  Apple app records on the device itself instead. The core (`dvr.rs`) is `aurora-app`'s
  `Dvr` and its `dvr_*` commands, line for line where it can be, over the shared schedule,
  rules, conflicts and recorder; `mirrors.rs` checks its numbers, reasons and refusals,
  and the Recordings screen's words, against Windows. The Recordings tab (from Home's
  toolbar on an iPhone) has Windows' three tabs, storage meter, clash notice, cards with
  Keep and a confirmed Delete, Coming up, the failures with their reasons, and series
  rules with Pause and a confirmed Delete. Choosing a programme in the guide or a
  channel's schedule opens Windows' info pane: Watch now, Watch from start, Record,
  Record series and Remind me, the channel's other copies by quality ("Also in"), and
  Search this title (Search, with the title typed), and the guide marks what is set to
  record. The player has
  Windows' Record what's on and the next programme's Record, and the strip that says
  when recordings hold every stream the subscription allows. A recording plays and
  resumes as a film does.

  Where it differs, and why:
  - it records while Aurora is running. Windows records while its window is open, even
    minimised; iOS and tvOS suspend an app that is not in front, so a recording due
    then is marked "Aurora was not running when this was due", as Windows marks one due
    while it was closed, and one in flight finishes with what it had.
  - when the system closes Aurora in the middle of a recording, as iOS and tvOS do
    without warning, the next start finishes it with what reached the disk, cut short
    with "Aurora was closed while this was recording" (or failed, if nothing did).
    Windows always finishes its recordings on the way out, so it never needs this.
  - recordings go in Application Support/Aurora/Recordings on an iPhone or iPad, kept out
    of the iCloud backup, and in Caches/Recordings on an Apple TV, where the system may
    empty them when space runs short: tvOS gives an app nowhere else that size.
  - an app update moves the app's container, so a recording is found by its file name
    in the recordings folder when the path it was written to has gone.
  - Stop on a recording in flight stops the stream and removes the partial file.
    Windows removes the row and leaves the stream writing until its window closes.
  - reminders are kept and shown, as on Windows, which does not fire them either.
- multi-view (README §7.4), on iPad and Apple TV: Windows' picker with the 2 × 2,
  1 + 3, 1 + 5 and 3 × 3 arrangements, a channel per tile, the connection budget and its
  refusal in Windows' words, the offer to rotate one stream round the wall (12 seconds a
  tile, the rest showing what is on now and next), and saved layouts with a confirmed
  Delete. On the wall one tile has sound; choosing a tile moves it (in a rotation,
  brings the picture to it and stays), and the bar has Windows' countdown, Resume, Save
  layout and Close. The core (`mosaic.rs`) gives the tiles' geometry, from the shared
  `aurora_core::mosaic`, their tunes, the budget and the saved layouts; `mirrors.rs`
  checks the refusal and the dwell against Windows.

  Where it differs, and why:
  - not on an iPhone, as the port's plan has it: four channels on a phone screen are
    too small to tell apart, and four decoders too hot to hold.
  - Swift keeps the session (which tile has sound, whose turn it is), because each tile
    is an mpv view the app owns. On Windows the host keeps it, because it owns the
    tiles' windows.
  - a tile's channel is picked from a searchable list, grouped as Windows' dropdown
    is, because a remote cannot type into a dropdown. The help says "tap"/"select" and
    "double-tap"/"hold" where Windows says "click" and "double-click", and the `1`–`9`
    keys are not there: a remote has no number keys.

On an iPhone the tab bar holds five tabs (Home, Live TV, Guide, Movies, Series), and
Search, Recordings and Settings open from Home's toolbar, as in Apple's TV app (multi-view is not offered there, and the playlist editor opens from Settings → Library). A sixth tab would go
under "More", whose navigation bar stacks on the tab's own and doubles every back
button. iPad and Apple TV keep both as tabs.

Not yet: syncing provider passwords (each device keeps its own, as on
Windows today).

The app targets **iOS 26 and tvOS 26 only**.

Verified on an Intel Mac with Xcode 26.5 and Rust 1.100:

| Check | Result |
|---|---|
| Shared crates build for `aarch64-apple-ios` (iPhone/iPad) | ✅ unmodified, including `ring`/rustls and bundled SQLite |
| Shared crates build for `x86_64-apple-ios` (simulator on Intel) | ✅ |
| Shared crates build for `aarch64-apple-tvos` (Apple TV) | ✅ on **stable** Rust. The plan's risk 1 (`ring` on tvOS) didn't happen. |
| `cargo test` in `apple/core` | ✅ 36 tests, including an import → list → favourite → search → guide → tune → catch-up run over HTTP, and the same run against a real Xtream panel (`tests/live_provider.rs`, skipped unless `~/.config/aurora-tv/test-provider.env` exists) |
| `AuroraMobile` and `AuroraTV` build for their simulators | ✅ |
| Runs in the simulator: opens its library, imports an M3U over HTTP, lists it, keeps it across launches | ✅ iOS 26.5 and tvOS 26.5 |
| mpv in the app: the shared `LoadOptions` applied (user agent, the 8 s live buffer), HLS opened and decoded, sound playing | ✅ tvOS 26.5 simulator |
| mpv's **picture** | ⏳ not on the simulator on an Intel Mac. There MoltenVK used to fail cleanly (`VK_ERROR_INCOMPATIBLE_DRIVER`); as of 2026-11-17 the simulator's Metal driver crashes the app while MoltenVK probes it, so Intel simulator builds don't start the picture at all (`vid=no`) and the app says "only the sound is playing". Needs a real Apple TV/iPhone, or an Apple-silicon Mac |
| Guide grid against the real Xtream panel (6,699 channels): rows fetched as they scroll in, "No guide" rows, Earlier/Later, the now line, replay marked only where the channel keeps an archive | ✅ tvOS 26.5 simulator, driven with the remote |
| Live and catch-up playback from the guide, left running | ✅ sound plays, no crash. A panel that has no archive for a programme gets "archive would not play" (MS NOW's 404s on the test panel) |
| iPhone tab bar on the iOS 26 simulator | ⚠️ draws as an empty glass shape on this Intel Mac; the same `TabView` draws correctly on tvOS. To check on a device |
| Playback on a real Apple TV, import time for a big panel, zap time | ⏳ needs hardware: the remaining Phase 2 exit criteria |

## The separation rule

**Nothing under `apple/` is built by the Windows app, and nothing outside `apple/` is
edited for the Apple app.**

```
apple/
  core/                Rust workspace of its own (own Cargo.lock, own target/)
    aurora-ffi/        UniFFI façade: the only Rust written for Apple
    uniffi-bindgen/    generates the Swift bindings
  Shared/              SwiftUI used by both apps
    Core/              AppModel, the Keychain store, generated bindings (ignored)
    Features/          screens
    Player/            MPVKit player
  AuroraTV/            tvOS entry point
  AuroraMobile/        iOS/iPadOS entry point
  project.yml          the Xcode project, as text (XcodeGen)
  scripts/
    build-core.sh      Rust → AuroraCore.xcframework + Swift bindings
    generate-project.sh  project.yml → Aurora.xcodeproj
```

- `apple/core` depends on `aurora-core`, `aurora-db`, `aurora-ingest` and
  `aurora-video` **by path, read-only**. It isn't a member of the `src-native`
  workspace, so `cargo` in `src-native/` never sees it, and its lockfile can't move
  the Windows build's versions.
- It doesn't depend on `aurora-app`, the Tauri host. That's Windows-only and stays so.
- Swift implements the platform pieces through traits the shared crates already define:
  `SecretStore` (Keychain) feeds `aurora_ingest::credentials::CredentialStore`, and the
  player applies `aurora_video::Request::settings()`, the per-file mpv settings Windows'
  video engine applies (only the `Request` half: the engine itself drives libmpv on
  Windows, and MPVKit does that here).

### Where this departs from the plan

The plan's Phase 1 moves the product logic out of `aurora-app` into a new
`aurora-service` crate that both hosts would share. That means editing the Windows host,
which this port doesn't do. Instead, `aurora-ffi` repeats the small amount of host glue
it needs: saving a provider, the refresh bookkeeping, rebuilding a stream URL, and
resolving a film or episode to its address and title. Every repeated piece names the
`aurora-app` function it mirrors. Browsing, the guide and watch progress are direct
calls into `aurora-db`, so their rules (what counts as finished, that "finished" is
sticky, which episode is next) aren't repeated at all.

Copies drift: the first draft gave live TV a 10-second buffer where Windows uses 8.
To catch that, `aurora-ffi/src/mirrors.rs` reads `aurora-app`'s sources as text and
fails when a copied value no longer matches (the live and VOD buffer sizes, the free
guides, the columns a provider is saved with, the source a catch-up replay is built
from). If the Windows app changes one of them, `cargo test`
in `apple/core` says which file to update.

As more screens arrive (guide, films, series, recordings), the amount of mirrored glue
grows. At that point it's worth deciding whether to do the plan's Phase 1 after all.

## Building

Needs Xcode, Rust (`rustup`), and XcodeGen (`brew install xcodegen`).

```sh
apple/scripts/build-core.sh          # release: every iOS and tvOS slice (slow: LTO)
apple/scripts/build-core.sh --debug  # faster, for development
apple/scripts/generate-project.sh    # writes apple/Aurora.xcodeproj
open apple/Aurora.xcodeproj
```

For day-to-day work in the simulator, build only this Mac's simulator slices. It's much
faster, but the result can't run on a real device:

```sh
apple/scripts/build-core.sh --debug --sim
xcodebuild -project apple/Aurora.xcodeproj -scheme AuroraMobile \
  -destination 'generic/platform=iOS Simulator' ARCHS="$(uname -m)" build
```

(`ARCHS` keeps `xcodebuild` from compiling the Swift and MPVKit for both simulator
architectures. Running from Xcode against a chosen simulator does the same thing.)

Then run the `AuroraTV` or `AuroraMobile` scheme. A real device needs your signing team
set in Xcode. A free account's builds expire after 7 days (plan §2.6).

On an **Intel Mac** the tvOS simulator is x86_64, and `x86_64-apple-tvos` is still a
Tier 3 Rust target. `build-core.sh` builds it with nightly and `-Zbuild-std`, installing
nightly the first time. Apple-silicon Macs only need stable.

### Trying it without tapping (debug builds)

```sh
xcrun simctl launch <device> tv.aurora.tv \
  -AuroraDemoPlaylist http://127.0.0.1:8000/demo.m3u   # imports it, live TV only
  -AuroraAutoplay YES                                  # then plays the first channel
  -AuroraMpvLog v                                      # mpv's full log, to the system log
```

The simulator reaches the Mac's `127.0.0.1`, so `python3 -m http.server` beside an
M3U is enough. None of this exists in a release build.

Tests for the Rust side:

```sh
cd apple/core && cargo test
```

## Licensing

The app links MPVKit's **LGPL** product (`MPVKit`, not `MPVKit-GPL`). Read plan §2.6
before distributing beyond your own devices: the GPL and App Store terms are widely
considered incompatible.
