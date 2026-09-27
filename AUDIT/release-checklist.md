# Release checklist

What is left for a person, in the order it blocks 1.0. Everything still open here
needs a Windows machine, a paid account, a certificate, or a decision — none of which
this audit could supply.

Items 1, 6 and 8 are **done**: item 1 was run on a Windows machine and is reported in
`AUDIT/test-report.md` §11, with one number outside its budget. Item 3 is **mostly
answered** — a real subscription imported and drew — with catch-up, recording and logos
still open. Items 2, 4 and 5 are blocking and untouched; 1b, 7, 9 and 10 are the
difference between shipping and shipping well.

---

## 1. Run the Phase 0 spike — **done**

**Run on Windows 11 with libmpv v0.41, and it works.** The full account is in
`AUDIT/test-report.md` §11; the four questions this item asked, in order:

1. **Video renders behind the UI, not in a window of its own.** The process has one
   top-level window; mpv's surface is a `STATIC` `WS_CHILD` of it, last of two children
   in z-order. 95.4% of a sampled grid over the middle of the window changed between two
   desktop captures 1.2s apart — a still window measures 0.0% — with mpv reporting
   `1920x1080 H.264 60fps hwDecoder=d3d11va-copy`.
2. **The UI receives input while video plays underneath.** A click on the OSD changes
   what is on screen.
3. **Resizing does not tear the two apart.** After a resize the client area is
   `1220x740` and the video surface is `1220x740`.
4. **Zap time is 1.91s, against the 1.5s budget in README §16.** One measurement, on one
   machine, against a public CDN rather than a provider — so it is reported rather than
   asserted, but it is outside the budget and it is the first real number there has been.

The fallback — a native XAML/WinUI shell — is not needed. `tests-host/scenarios/
video_surface.py` is the regression test, and it runs from
`python tests-host/run.py video_surface` with a playlist in `AURORA_TEST_STREAM_M3U`.

Two things this run found that only a run could: mpv has no `cache-dir` option, so the
timeshift buffer had been going to mpv's own folder rather than the viewer's (fixed),
and `clippy -D warnings` had never linted the `#[cfg(windows)]` code and did not pass on
it (fixed). Both are in `AUDIT/test-report.md` §12.

<details>
<summary>What this item said before it was run</summary>

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
`libmpv-2.dll` beside the exe and confirm:

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

</details>

## 1b. Test the portable self-update — **done, and it found something**

A portable copy now updates itself instead of being sent to a browser (D25): it
downloads the zip, verifies the digest, unpacks it, and swaps its own files in at the
next launch. The staging, the path refusals, the apply and the rollback are all tested
on Linux, because none of it is `#[cfg(windows)]` — it is `std::fs` throughout.

**Run on Windows, and the claim holds.** A portable copy of 0.11.0 was given a staged
0.11.1 and relaunched; it renamed its own running `.exe`, applied the update and came
back as the new version:

```
   old exe 13,589,504   new exe 12,845,056
   [first run] it says it is version 0.11.0
   staged: ['.ready', 'aurora-app.exe']
   [after staging] it now says it is version 0.11.1
   log: INFO aurora_app::updates: a staged update was applied before launch: applied 1 files
   previous/      : ['aurora-app.exe']
   staged/ gone   : True
   [third run] previous/ still there: False
```

So steps 1 to 3 below are answered: the swap works, `previous/` holds the old build, and
the launch after that forgets it.

**Step 4 — breaking it on purpose — found F-34 instead.** Holding `libmpv-2.dll` open
with an exclusive handle, the way a scanner does, does not exercise the rollback at all:
the process never reaches its own code. libmpv is a load-time import, so the Windows
loader ends it with `0xC0000043` (sharing violation) — and with the DLL absent
altogether, `0xC0000135` — in both cases with **no window, no log and no message**. The
`NullBackend` fallback that `create_backend` appears to offer cannot run.

That matters most precisely here, where this item is looking: the one file whose loss a
bad update cannot recover from is also the one whose loss the app cannot report. The fix
is to delay-load it so the failure becomes a value; see F-34.

The staged update is not lost, which is the good half: with the lock released, the next
launch applied it and came back as 0.11.1. A refused apply leaves a working copy behind
and retries, rather than rolling forward into a broken install.

The original wording of this item follows, since its steps are still the right ones:

> **What is not tested is the claim it rests on:** that Windows lets you rename a
running `.exe`. It does, and it is how every self-updating Windows application works,
but nothing here has done it. On the machine from item 1:

1. Unzip the portable build, run it, and let it find a newer release.
2. Download, then Install and restart. It should come back as the new version with no
   installer and no Windows prompt.
3. Check `data\updates\previous\` holds the old files, and that they are gone after
   the launch *after* that.
4. **Then break it on purpose** — make the install folder read-only, or hold
   `libmpv-2.dll` open — and confirm the rollback leaves a working copy of the old
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

- **Series. Answered.** A real panel imported **22,121 channels, 117,587 films and
  28,529 series**, and Live TV, Movies and Series all drew — so F-04 holds outside a
  fixture, and "Live TV says No channels" does not reproduce. Opening a show lists its
  episodes (6 on the one opened); `episodes` is 0 straight after an import because they
  are fetched when a show is opened. Artwork is still unconfirmed — that needs item 7.
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

## 5. Installer, upgrade, uninstall — **upgrade done; install and uninstall need an admin**

**What was run.** Both bundlers produce what they should: an NSIS `.exe` (37.3 MB) and an
MSI (50.9 MB), and each carries `aurora-app.exe`, `libmpv-2.dll`, `LICENSE`,
`licenses\GPL-3.0.txt`, `licenses\LGPL-2.1.txt` and `THIRD-PARTY-NOTICES.md` — so
item 8's obligation travels with the binary in both. (The NSIS build names the licence
`LICENSE.txt` and the MSI names it `LICENSE`; harmless, since Settings -> About reads
only `THIRD-PARTY-NOTICES.md`, which both spell the same way.)

**The upgrade bullet is answered.** A 0.11.1 build was installed over an existing
0.11.0 with a real library behind it — 22,121 channels, 117,510 films, 28,529 series and
453,072 programmes, imported from a live panel — plus a favourite and a watch position
seeded first, because both tables were empty and an upgrade cannot be shown to preserve
what is not there:

```
  ok channels           22,121 ->   22,121      ok epg_programmes    453,072 ->  453,072
  ok movies            117,510 ->  117,510      ok providers               1 ->        1
  ok series             28,528 ->   28,528      ok settings                5 ->        5
  ok episodes              143 ->      143      ok favorites               1 ->        1
                                                ok watch_progress          1 ->        1
  favourite : ('channel', 'US * QVC HD')     watch progress : ('movie', 1, 421, 5400)
  schema user_version: 8 (unchanged - this version pair needs no migration)
```

Nothing was lost, and the new build opened the existing library without the recovery
path firing.

**What still needs an administrator.** The machine this ran on has a standard user
account, and Tauri's NSIS build requests elevation before it parses `/CURRENTUSER`, so
every silent install attempt ended in `The operation was canceled by the user` — UAC
asking for credentials that a standard user does not have. The MSI installs per-user
without elevation (`msiexec /i ... /qn MSIINSTALLPERUSER=1 ALLUSERS=2`, exit 0, into
`%LOCALAPPDATA%\Programs\Aurora TV` with an HKCU uninstall entry), which is how the
upgrade above was done — but the per-machine NSIS path, the Start-menu integration it
creates, and the uninstall are all still unrun. **They need an admin account**, not
another machine.

Still to do, then, on a clean Windows VM with administrator rights:

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
"depending how the `libmpv-2.dll` in this build was compiled", because the release
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
