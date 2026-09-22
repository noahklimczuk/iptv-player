# Building Aurora TV

## What builds where

| Component | Linux / macOS | Windows |
|---|---|---|
| `aurora-core`, `aurora-db` | ✅ builds and tests | ✅ |
| `aurora-player` (trait + NullBackend) | ✅ builds and tests | ✅ |
| `aurora-player::mpv` (Win32 + libmpv) | ⚠️ `cargo check --target x86_64-pc-windows-msvc` only | ✅ |
| `aurora-app` (Tauri host) | ✅ with GTK dev packages (below) | ✅ |
| `src-ui` (React) | ✅ | ✅ |

A bare `cargo test` in `src-native/` runs the three portable crates
(`default-members` in `Cargo.toml`), so the suite runs on a host with no GTK. Install
the packages below and `cargo test --workspace` additionally builds and tests the Tauri
host:

```
sudo apt-get install -y --no-install-recommends \
  libgtk-3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev
```

**A green Linux build still does not mean the Windows app works** — the mpv/Win32
backend is `#[cfg(windows)]` and is only compiled by the Windows CI job and the
cross-check below.

## Development on Windows (the real target)

```
pnpm install
pnpm dev                      # UI on :5173
cd src-native
cargo tauri dev               # host + WebView2 + mpv
```

You need:
- **Rust** stable, MSVC toolchain (`x86_64-pc-windows-msvc`)
- **WebView2 runtime** (preinstalled on Windows 11; the installer bootstraps it on 10)
- **libmpv** — `mpv-2.dll` plus its import library. Point `MPV_SOURCE` at the SDK
  directory before building. The DLL must ship next to `aurora-app.exe`.

## Developing the UI without Windows

```
pnpm install
pnpm dev
```

The UI detects the absence of the Tauri host and falls back to an in-memory mock with
synthetic fixtures (`src-ui/src/ipc/mock.ts`), so every screen is reachable and
clickable in a plain browser. Settings shows a banner saying so. This is how the
screenshots in `screenshots/` are produced.

```
pnpm exec playwright test     # 12 journeys + screenshot capture
```

## Cross-checking the Windows code from Linux

```
rustup target add x86_64-pc-windows-msvc
cd src-native
cargo check -p aurora-player --target x86_64-pc-windows-msvc
```

This type-checks the mpv/Win32 backend without an MSVC linker, which catches most
mistakes in code you cannot run locally. It does **not** work for `aurora-app`: a
transitive C dependency needs MSVC's `lib.exe`.

## Match CI's toolchain before trusting a local clippy run

CI uses `dtolnay/rust-toolchain@stable`, which floats. A local toolchain even a few
releases behind will pass `clippy -D warnings` on code the runner rejects, because each
release adds lints — this has already cost one red build (`manual_checked_ops`, which
did not exist in 1.94).

```bash
rustup toolchain install stable --profile minimal -c clippy -c rustfmt
rustup target add x86_64-pc-windows-msvc   # for the mpv cross-check
```

`cargo clippy --version` should match what the runner prints in its Clippy step.

## Checking a real subscription

The parsers are written from the Xtream spec and tested against a local server. Every
panel is a fork of a fork, so the first real subscription is where the assumptions get
tested. `probe` runs the shipping code paths against a provider and prints what came
back:

```bash
AURORA_BASE=http://your-panel.example \
AURORA_USER=yourname \
AURORA_PASS=yourpassword \
  cargo run -p aurora-ingest --example probe
```

`AURORA_BASE` is the panel root — no `/get.php`, no query string. It only reads: nothing
is written to a database and nothing is downloaded. The password comes from the
environment, is never printed, and URLs go through `http::redact` before they reach the
output.

What to look at:

- **Auth failed.** The two lines printed are exactly what the first-run wizard would
  show. If they do not match reality, `aurora_core::neterr` needs another case.
- **EPG id coverage below ~50%.** The guide will be mostly empty; the channels need
  mapping (README §4.4).
- **Year extracted below ~40%.** Metadata matching declines without a year to separate
  candidates, so posters will be sparse.
- **No connection limit reported.** The DVR falls back to allowing two simultaneous
  recordings.
- **An endpoint that FAILED.** Panels vary in what they implement; knowing which ones
  this provider does not serve is half of supporting it.
