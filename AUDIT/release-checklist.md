# Release checklist

What is left for a person, in the order it blocks 1.0. Everything still open here
needs a Windows machine, a paid account, a certificate, or a decision — none of which
this audit could supply.

Items 6 and 8 are **done**. Item 1's code is done and only its *verification* is left.
Items 2, 3, 4 and 5 are blocking and untouched; 7, 9 and 10 are the difference between
shipping and shipping well.

---

## 1. Run the Phase 0 spike — **blocking, and everything else is downstream**

**The wiring is done.** It was the gap that made every other question unanswerable:
three things were written and called from nowhere, so a run would have shown no video
for reasons unrelated to compositing. They are connected now (`feat(player): connect
the video surface and the event pump`):

- `MpvBackend::attach` and `pump` are on the `PlayerBackend` trait, which is what made
  them reachable at all — the app layer holds a `Box<dyn PlayerBackend>`.
- `main.rs` attaches the surface at setup; `WindowEvent::Resized` repositions it.
- `Playback::tick` pumps before it reads, so the OSD is driven by something.
- `"transparent": true` is set, and the OSD no longer paints over the video.

`aurora-player` type-checks for `x86_64-pc-windows-msvc`. **`aurora-app` could not be
cross-checked here** — rustls's `ring` needs an MSVC C compiler this container does
not have — so CI's Windows job is the first thing that will compile `window.rs` and
`main.rs`. Expect to fix a compile error or two there before anything runs.

What remains is the part only a machine can do. Run `cargo tauri dev` on Windows with
`mpv-2.dll` beside the exe and confirm:

1. Video renders *behind* the UI, not in a separate window.
2. The UI receives input while video plays underneath.
3. Resizing, snapping and alt-tab do not tear the two surfaces apart.
4. Zap time is under the 1.5 s budget in README §16.

If (1) or (3) fails, the fallback is a native XAML/WinUI shell for the player surface.
`aurora-core`, `aurora-db` and `aurora-player` are backend-agnostic and port unchanged.

**While you are there**, confirm the two things this audit changed that only Windows
can show. With every command now on the thread pool (F-01), the window should stay
movable and repaint throughout a full provider refresh, and the `ingest.progress` bar
should actually move — before the fix it could not, because delivering the event
needed the thread the refresh was holding. And with `pump` wired, the OSD's clock,
buffer readout and track lists should change *during* playback rather than freezing on
whatever the load set.

If there is no picture, the log says which step failed: look for `video surface
ready`, `no video surface: …`, or `no main window at setup`.

## 1b. Test the portable self-update — **new, and unproven**

A portable copy now updates itself instead of being sent to a browser (D25): it
downloads the zip, verifies the digest, unpacks it, and swaps its own files in at the
next launch. The staging, the path refusals, the apply and the rollback are all tested
on Linux, because none of it is `#[cfg(windows)]` — it is `std::fs` throughout.

**What is not tested is the claim it rests on:** that Windows lets you rename a
running `.exe`. It does, and it is how every self-updating Windows application works,
but nothing here has done it. On the machine from item 1:

1. Unzip the portable build, run it, and let it find a newer release.
2. Download, then Install and restart. It should come back as the new version with no
   installer and no Windows prompt.
3. Check `data\updates\previous\` holds the old files, and that they are gone after
   the launch *after* that.
4. **Then break it on purpose** — make the install folder read-only, or hold
   `mpv-2.dll` open — and confirm the rollback leaves a working copy of the old
   version and a line in `aurora.log` saying why. That is the path that matters; a
   failed update must never be why somebody's television stops working.

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

## 6. Dependency advisories in CI — **done, and the correction is the interesting part**

The `core` job runs `rustsec/audit-check`, `pnpm audit --audit-level high`, and
`pnpm test`, which had nothing to run until this pass.

**The Rust half of that was wrong when it was first written here.** The action defaults
to `./Cargo.lock`; this workspace keeps its lockfile under `src-native`, so the step
died with `Couldn't load ./Cargo.lock` before auditing anything — and because a failed
step skips the rest of a job, it also took `pnpm audit`, the release-tooling tests and
all 97 end-to-end journeys down with it. A checklist item that read **done** was a step
that had never once run, in a job whose green was hiding four other things.

Pointing it at the right lockfile found two **high-severity (7.5)** advisories in
`quick-xml 0.36`, both reachable from a provider's XMLTV guide: RUSTSEC-2026-0194
(quadratic parse time on duplicate attribute names) and RUSTSEC-2026-0195 (unbounded
allocation in `NsReader`). Both are fixed by the upgrade to 0.42. `cargo audit` now
exits clean, with seven informational warnings it does not fail on — five unmaintained
build-time crates, and `glib`'s `VariantStrIter` unsoundness, which is Linux GTK and
not in the shipped Windows binary.

The steps after the advisory checks now carry `if: ${{ !cancelled() }}`, so a future
advisory fails the job without taking the test results with it.

Worth adding later: `cargo deny`, which would police the licence question in item 8
automatically rather than by anyone remembering to.

## 7. Turn on the artwork cache

`aurora-ingest::artwork` downloads, evicts and reports; the UI still renders remote
TMDB URLs, so every poster is fetched from the network on each paint. Closing it needs:

1. `assetProtocol` enabled in `tauri.conf.json`, scoped to the artwork folder.
2. The image components swapped to `artwork::asset_url(...)` **with a fallback to the
   remote URL**, so a misconfigured protocol degrades to today's behaviour rather than
   breaking every image.

The CSP already allows `asset:` and `http://asset.localhost` (F-12), so that half is
done. Neither step is verifiable without running the app.

## 8. Third-party licences — **done, with one thing to record per release**

`LICENSE` (GPL-3.0), `licenses/LGPL-2.1.txt`, `licenses/GPL-3.0.txt` and
`THIRD-PARTY-NOTICES.md` are in the tree, bundled as installer resources, copied into
the portable zip, and shown in Settings → About — which reads them from beside the
executable, so what is on screen is what shipped.

**The one thing still on a person:** the notices say libmpv is LGPL-2.1+ *or* GPL-2+
"depending how the `mpv-2.dll` in this build was compiled", because the release
workflow *discovers* the shinchiro archive rather than pinning one and those builds
may or may not enable GPL-only components. Aurora is GPL-3.0-or-later so either is
compatible — but the release notes should record which archive was used, and if you
pin a specific build, replace that paragraph with the definite answer.

Do not static-link libmpv. The relinking obligation is satisfied by its being a
separate `.dll`, and `THIRD-PARTY-NOTICES.md` now states that as a constraint on the
build rather than an observation about it.

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
- The Phase 0 spike is wired (item 1), the licence obligation is met (item 8), and CI
  checks advisories on both ecosystems (item 6).
- Both kinds of build update themselves in-app (item 1b); CI publishes the portable zip
  to the release so the portable one has something to fetch.
