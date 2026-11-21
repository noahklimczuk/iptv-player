# Aurora TV 2.0 — the public release

Until now Aurora was a private application with one user (`AUDIT/release-checklist.md`
said so, and closed code signing, the store listing and the privacy note on that basis).
2.0 reverses that decision: it is published on the Microsoft Store and the Apple App
Store, for strangers. Every item that the private scoping retired is open again, and
this file is the list.

**Scope of this file:** the Windows app, the shared crates and the release pipeline. The
Apple side (everything under `apple/`) belongs to the Apple agent; the App Store items
at the end are recorded so they are not lost, not because they are done from here.

Status key: **Done** (merged and checked), **Next** (the batch in progress or the one
after it), **Open** (not started), **Yours** (needs the owner's account, money or a
decision — nothing in the repository can do it).

---

## 1. Distribution

| | Item | Status |
|---|---|---|
| 1a | **Stable and preview channels.** Every merge publishes a *prerelease*; installed copies ask GitHub for `/releases/latest`, which never points at a prerelease, so they only move when a build is promoted with the **Promote to stable** workflow (`.github/workflows/promote.yml`). | Done |
| 1b | **A Store copy leaves updating to the Store** (Store policy 10.8). `aurora-app/src/packaged.rs` asks Windows whether the process has package identity; when it does, nothing checks GitHub, downloads, stages or swaps, and Settings → Updates says the Store keeps it current. One binary for all three kinds of copy. | Done |
| 1c | **MSIX package.** `scripts/package-msix.ps1` turns the portable folder into an MSIX with the manifest in `aurora-app/msix/` and the tile set `tools/make_icons.py` draws; CI uploads it as the `aurora-tv-msix` artifact on every merge. Unsigned on purpose — Partner Center signs it. | Done |
| 1d | **Store identity.** Reserve "Aurora TV" in Partner Center, then set the repository variables `STORE_IDENTITY_NAME`, `STORE_PUBLISHER` and `STORE_PUBLISHER_DISPLAY_NAME` to the values under *Product identity*. Until then the package carries placeholders and Partner Center will refuse it. | Yours |
| 1e | **Code signing for the installer and the zip** (not the Store copy). SmartScreen warns on every unsigned download. Azure Trusted Signing is the cheapest route for an individual. Not needed for the Store. | Yours |
| 1f | **Version 2.0.0.** Pinned in the manifests in the release PR, as 1.0.0 was (`scripts/version.mjs` treats the manifests as a floor), then promoted. | Open |

## 2. Accounts and sync (public users)

The sync backend already supports sign-up (`aurora-sync/src/firebase.rs`). What a
stranger needs on top of it:

| | Item | Status |
|---|---|---|
| 2a | **A real "Create account" section**: Settings → **Account and sync**, its own entry in the Settings index. Create account / Sign in tabs; the password twice and at least eight characters; a passphrase that must differ from the password; agreement to the privacy policy (opened by the host from a constant URL). | Done |
| 2b | **Verify the email address**: a verification email goes out at sign-up, and the signed-in card asks for confirmation until Firebase says it is verified, with "send again". | Done |
| 2c | **Forgot password** — a reset email from the sign-in form, worded the same whether or not the address has an account. | Done |
| 2d | **Delete account** in the app, behind the account password: every record and the vault in Firestore, then the Firebase user, then this device signs out with its library intact. A fresh sign-in does the deleting, because Firebase refuses a refreshed token for it. Apple requires this of any app that creates accounts (guideline 5.1.1(v)); the shared crate has it, so the Apple side only needs a button. | Done |
| 2e | **Firebase plan.** The project is on Spark. Its free Firestore quota (50k reads a day) is shared by every user of the app; a public release needs Blaze with a budget alert, and the security rules reviewed for multi-tenant use. | Yours |

## 3. Privacy and policy

| | Item | Status |
|---|---|---|
| 3a | **Privacy policy for the public.** `docs/PRIVACY.md` now covers the account, the two emails, deletion and a contact route. Still wanted: a stable URL of its own (GitHub Pages) rather than a link into the repository. | Partly done |
| 3b | **The compiled-in Gemini key.** Anyone with the binary can extract it and spend on it. Before 2.0: restrict the key to the Generative Language API with a daily quota in Google Cloud, or move the assistant to a key the user supplies. | Yours (decision) |
| 3c | **Content policy.** Aurora ships with no channels, playlists or provider; the listing must say it plays the user's own subscription. Store reviewers test this — a demo M3U of legal streams in the reviewer notes avoids a rejection. `docs/store/microsoft-store.md` says so up front, and `docs/store/reviewer-demo.m3u` holds three public test streams. | Done |
| 3d | **Age rating** (IARC questionnaire in Partner Center). Parental controls exist; say so. | Yours |

## 4. Product polish for 2.0

From the roadmap's "not built" list, the two a public user will miss first:

| | Item | Status |
|---|---|---|
| 4a | **Media keys and the Windows media overlay** (System Media Transport Controls): play/pause, next/previous channel, now-playing title and artwork. `aurora-app/src/media_keys` tells Windows what is playing (channel and programme, or title; logo or poster; a position bar on films and episodes) and hands presses to the page, which does what its own keys do. Anything parent-locked or in an adult category shows only "Aurora TV" on the overlay and the lock screen. Checked from Windows' side by `tests-host/scenarios/media_keys.py`. | Done |
| 4b | **Backup and restore** of settings, favourites and profiles to a file, for people who do not use sync. | Open |
| 4c | **First-run setup offers sync** — the last step of the wizard offers a free account when sync is available and nobody is signed in, and opens Settings at **Account and sync**. Not offered when adding a second provider. | Done |
| 4d | Install, upgrade and uninstall verified on a clean Windows account (`AUDIT/release-checklist.md` item 5). | Open |

Not in 2.0, by choice: Stalker portals, drag-to-reorder, custom logos, named favourite
lists, the user-defined rules engine, tray.

## 5. Store listings

| | Item | Status |
|---|---|---|
| 5a | Microsoft Store listing text, feature list, search terms, reviewer notes — `docs/store/microsoft-store.md`. | Done |
| 5b | Screenshots at 1920×1080 from the e2e journeys (`screenshots/`), without any provider's logos or channel names. | Open |
| 5c | **Apple App Store** (Apple agent): note that the project is GPL-3.0, and the GPL's terms and the App Store's usage rules are widely held to conflict (VLC was withdrawn in 2011 over exactly this). Every copyright holder can grant an App Store exception; that needs deciding before an Apple submission. | Yours (decision) |

---

## Promoting a build

1. Merge. CI publishes `vX.Y.Z` as a prerelease with the installer, the zip, and the
   MSIX as an artifact of the run.
2. Try it (`docs/TESTING_A_BUILD.md`).
3. Actions → **Promote to stable** → tag `vX.Y.Z` (optionally a notes file). Installed
   copies see it within six hours.
4. For the Store: upload that run's `aurora-tv-msix` artifact to a new submission in
   Partner Center.

## Trying the package

The MSIX is unsigned, so Windows will not install it as it is. To try one, sign it with
a throwaway certificate whose subject equals the manifest's `Publisher` and trust that
certificate — **on a separate Windows account**, never the one with your real library:
a packaged copy reads `%LOCALAPPDATA%\tv.aurora.player` through MSIX's AppData merge,
so it would open the live profile.
