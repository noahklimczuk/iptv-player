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
- edit the playlist (Settings): rename, renumber, regroup or hide channels, films and
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
- Settings → Diagnostics and About, as on Windows. Diagnostics says when the library
  had to be started again, names the data folder and the video engine, and exports this
  run's log, with every address's login removed (`redact_log`), to share from an iPhone.
  The log is the system's: the core's own lines (`log.rs`, at Windows' `info`) and mpv's
  warnings, redacted before they are written, both go into it. About carries the
  version, the licence and the third-party notices. Those are this folder's
  `THIRD-PARTY-NOTICES-APPLE.md` (MPVKit's LGPL build, shipped as separate frameworks)
  followed by the shared file, both bundled with the app.
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

On an iPhone the tab bar holds five tabs (Home, Live TV, Guide, Films, Series), and
Search and Settings open from Home's toolbar, as in Apple's TV app. A sixth tab would go
under "More", whose navigation bar stacks on the tab's own and doubles every back
button. iPad and Apple TV keep both as tabs.

Not yet: profile age and daily limits (being fixed on Windows first; the Apple app will
follow the same rule), recordings, and syncing provider passwords (each device keeps its
own, as on Windows today).

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

As more screens arrive (guide, films, series, DVR rules), the amount of mirrored glue
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
