# Release checklist

What is left for a person, in the order it blocks 1.0. Everything here needs a
Windows machine, a paid account, a certificate, or a decision — none of which this
audit could supply. Items 1–4 are blocking; 5 onwards are the difference between
shipping and shipping well.

---

## 1. Wire and run the Phase 0 spike — **blocking, and everything else is downstream**

Budget an hour of Win32 plumbing, then an afternoon of looking at it.

Three things are written and called from nowhere, so a run today shows no video for
reasons that have nothing to do with compositing:

- `MpvBackend::attach` (`aurora-player/src/mpv.rs`) creates the child HWND video
  renders into and hands mpv its `wid`. Nothing calls it, so mpv has nowhere to draw.
- `window::attach_video_surface` (`aurora-app/src/window.rs`) is the host half. Also
  never called, and it does not call `attach` — it makes the WebView2 background
  transparent and resizes a surface that does not exist yet.
- `MpvBackend::pump` drains mpv's event queue and is the only thing that updates
  position, tracks, buffering, errors and the timeshift window. The 250 ms heartbeat
  in `main.rs` *reads* that state; it does not produce it.

Also set `"transparent": true` on the window in
`crates/aurora-app/tauri.conf.json` — the compositing model in README §2.1 needs it and
it is currently `false`.

Then run `cargo tauri dev` on Windows with `mpv-2.dll` beside the exe and confirm:

1. Video renders *behind* the UI, not in a separate window.
2. The UI receives input while video plays underneath.
3. Resizing, snapping and alt-tab do not tear the two surfaces apart.
4. Zap time is under the 1.5 s budget in README §16.

If (1) or (3) fails, the fallback is a native XAML/WinUI shell for the player surface.
`aurora-core`, `aurora-db` and `aurora-player` are backend-agnostic and port unchanged.

**While you are there**, confirm the one thing this audit changed that only Windows can
show: with every command now on the thread pool (F-01), the window should stay movable
and repaint throughout a full provider refresh, and the `ingest.progress` bar should
actually move. Before the fix it could not, because delivering the event needed the
same thread the refresh was holding.

## 2. Code signing — **blocking for anything a stranger installs**

Nothing is signed. SmartScreen warns on every install, and D17's updater verifies the
SHA-256 GitHub published for an asset — which defends against a corrupted or
substituted *download*, not against whoever can publish a release.

Two routes, and it is a decision, not a commit:

- **An OV/EV code-signing certificate.** Buy it, put the PFX and its password in
  repository secrets, and add `signCommand` to `tauri.conf.json`'s `bundle.windows`.
  EV clears SmartScreen immediately; OV builds reputation over weeks.
- **A minisign keypair** (README §18's own suggestion): private half a repository
  secret the release workflow signs with, public half compiled into the app. Free,
  and closes the updater hole, but does nothing about SmartScreen.

Until one exists, the Download button should keep saying what it checked was a
checksum. It does.

## 3. Point it at a real subscription — **blocking for the claims in `docs/ROADMAP.md`**

Several things can only be answered with an account:

- **Series.** F-04 is fixed — an Xtream panel's `get_series` listings are now written
  instead of counted — so the Series screen should populate on the panel that used to
  show 28,693 series found and nothing in it. Confirm the count, the artwork, and that
  opening a show says its episodes are still to come rather than looking broken.
- **Catch-up (F-23, deferred).** `xtream_url` formats its timestamp in UTC; panels read
  it in their own local time, and `server_info.timezone` said `Europe/Paris` on the one
  panel probed — two hours out. `timeshift.php` 404s there, so it cannot be fixed
  against evidence. On a panel where catch-up *works*: record a programme from the
  guide, check its first frame against the advertised start, and if it is out by the
  server's offset, store `server_info.timezone` at authentication and apply it.
- **Recording.** The recorder has only ever met the test server.
- **Logos.** F-12 widened `img-src` to allow `http:`. Confirm channel logos actually
  appear now; that failure was invisible from a browser, which serves no CSP.

## 4. Decide about ARM64 (F-26, currently won't-fix)

The brief named x64 and ARM64; the workspace targets `x86_64-pc-windows-msvc` only and
CI fetches an x86_64 libmpv. ARM64 Windows runs x64 under emulation, which for a video
player means software decode and a bad time. A real ARM64 build needs an ARM64 libmpv,
which the upstream this project fetches from does not publish. Either accept x64-only
for 1.0 and say so on the release page, or find/build an ARM64 libmpv first.

---

## 5. Installer, upgrade, uninstall

None of this could run here. On a clean Windows VM:

- Install the NSIS build. Check Start-menu entry, uninstall entry, file associations if
  any.
- Install *over* an older version. Confirm `%LOCALAPPDATA%\…\library.db` survives,
  favourites and watch progress are intact, and the migration runner moves the schema
  forward rather than refusing.
- Uninstall. Confirm nothing is left in Program Files or the registry — and decide
  deliberately whether the *data* folder should go too (it holds recordings, which are
  not the installer's to delete; leaving it is probably right, but it should be a
  choice).
- Confirm the credential entries under service `AuroraTV` in Credential Manager. A
  provider deleted in-app removes its own; an uninstall currently does not sweep them.

## 6. Add `cargo audit` to CI

`pnpm audit` is clean (0 critical / 0 high / 0 moderate after the upgrades in this
pass). The Rust side was never checked — `cargo-audit` is not installed in this
container. Add a step to the `core` job:

```yaml
- uses: rustsec/audit-check@v2
  with: { token: ${{ secrets.GITHUB_TOKEN }} }
```

`cargo deny` would also catch the licence question, which matters here: the workspace
is GPL-3.0-or-later and links libmpv (LGPL). See item 8.

## 7. Turn on the artwork cache

`aurora-ingest::artwork` downloads, evicts and reports; the UI still renders remote
TMDB URLs, so every poster is fetched from the network on each paint. Closing it needs:

1. `assetProtocol` enabled in `tauri.conf.json`, scoped to the artwork folder.
2. The image components swapped to `artwork::asset_url(...)` **with a fallback to the
   remote URL**, so a misconfigured protocol degrades to today's behaviour rather than
   breaking every image.

The CSP already allows `asset:` and `http://asset.localhost` (F-12), so that half is
done. Neither step is verifiable without running the app.

## 8. Third-party licences and the LGPL obligation

The workspace is `GPL-3.0-or-later` and the shipped build links **libmpv**, which is
LGPL-2.1+ and itself links FFmpeg. The installer must carry:

- libmpv's licence text and a note that it is dynamically linked (the `.dll` shipped
  beside the exe already satisfies the relink requirement — keep it that way; do not
  static-link libmpv without re-reading the LGPL).
- FFmpeg's licence, and a statement of which build is bundled.
- The Rust crate licences. `cargo about generate` or `cargo deny list` produces the
  list; there is currently no About screen showing any of it.

Add an About section to Settings with the version, the commit, and a scrollable
licence list. The version is already available (`env!("CARGO_PKG_VERSION")`, surfaced
by `updates.check`), and `app.diagnostics` now reports the data folder — About is the
natural home for both.

## 9. Store listing / release page copy

If it goes to the Microsoft Store rather than GitHub Releases, the auto-updater (D17)
must be disabled for that build — the Store owns updating, and two mechanisms fighting
is worse than either. `can_install()` already refuses for portable copies; a Store
build needs the same treatment.

## 10. A privacy note

Worth writing even though the answer is short and good: the app talks to the
provider the viewer entered, to TMDB (with a key baked in per D19), and to GitHub for
update checks. Nothing is sent anywhere else, there is no telemetry, and credentials
live in Windows Credential Manager and never in the database, a log or an export
(verified — `http::redact` now covers the markerless `/user/pass/id.ts` form too, F-15).
Say so on the release page; people ask.

---

## Already done, so you do not have to check

- Zero compiler warnings, `clippy -D warnings` clean, `cargo fmt --check` clean.
- 776 Rust tests, 96 Playwright journeys, 12 TypeScript units, 14 release-tooling
  tests. All green — see `AUDIT/test-report.md` for the pasted output.
- Soak: 2,000 zaps always end on the right channel; 2,400 concurrent tunes across 8
  threads never wedge; 20,000 tunes grow RSS by 0 kB.
- No `todo!`, `unimplemented!` or `dbg!` anywhere in `src`; every fixture host is
  `example.com` or a loopback address.
