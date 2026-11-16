# Aurora for Apple TV, iPhone and iPad

The tvOS and iOS app. It's a separate project in the same repository: it **reuses**
the Windows app's shared Rust crates and **changes none of them**. The plan it follows
is [`docs/APPLE_PORT_AND_SYNC.md`](../docs/APPLE_PORT_AND_SYNC.md). This file records
where the work deliberately departs from that plan.

## Status

Phase 2 of the plan (the spike: Rust → Swift → video), plus the start of Phase 4's
screens. The app can:

- ask "Who's watching?" and check a profile's PIN (skipped when there's one profile
  with no PIN, as on Windows). Wrong tries and the lockout are counted in the shared
  database, so they're the same tries Windows counts,
- add an Xtream Codes panel or an M3U playlist (the password goes in the Keychain),
- import it with progress: live TV only (the tvOS default) or with films and series,
- list channels by group, with now/next from the guide,
- keep favourite channels per profile (long-press a channel),
- show a channel's schedule a day at a time, and replay what already aired where the
  channel keeps an archive (catch-up, up to a week back),
- keep films and series on My List, and like them,
- search channels, what's on now and later, films and series in one box,
- browse films and series by the provider's shelves, with search,
- list providers in Settings with what each holds, and refresh one,
- open a series on the episode to resume, list its seasons and episodes,
- play a channel, film or episode through mpv (MPVKit). A channel tries each of its
  sources in turn. Films and episodes resume, and save their position every 15 s and
  on pause or stop.

Not yet: the full guide grid, creating and editing profiles, profile age and daily
limits (being fixed on Windows first; the Apple app will follow the same rule), named
favourite lists, removing a provider, library filters, recordings, and sync (Phase 3).

Verified on an Intel Mac with Xcode 26.5 and Rust 1.100:

| Check | Result |
|---|---|
| Shared crates build for `aarch64-apple-ios` (iPhone/iPad) | ✅ unmodified, including `ring`/rustls and bundled SQLite |
| Shared crates build for `x86_64-apple-ios` (simulator on Intel) | ✅ |
| Shared crates build for `aarch64-apple-tvos` (Apple TV) | ✅ on **stable** Rust. The plan's risk 1 (`ring` on tvOS) didn't happen. |
| `cargo test` in `apple/core` | ✅ 28 tests, including an import → list → favourite → search → tune → catch-up run over HTTP |
| `AuroraMobile` and `AuroraTV` build for their simulators | ✅ |
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
  `aurora-player` **by path, read-only**. It isn't a member of the `src-native`
  workspace, so `cargo` in `src-native/` never sees it, and its lockfile can't move
  the Windows build's versions.
- It doesn't depend on `aurora-app`, the Tauri host. That's Windows-only and stays so.
- Swift implements the platform pieces through traits the shared crates already define:
  `SecretStore` (Keychain) feeds `aurora_ingest::credentials::CredentialStore`, and the
  player reads `aurora_player::backend::LoadOptions::mpv_options()`.

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

Tests for the Rust side:

```sh
cd apple/core && cargo test
```

## Licensing

The app links MPVKit's **LGPL** product (`MPVKit`, not `MPVKit-GPL`). Read plan §2.6
before distributing beyond your own devices: the GPL and App Store terms are widely
considered incompatible.
