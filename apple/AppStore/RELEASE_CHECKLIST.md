# Aurora 2.0 for Apple: release checklist

Everything between the code in `apple/` and Aurora on the App Store, in order. Items
marked **(you)** need the account holder: a signature, a payment, a legal call, or
something outside `apple/`. The rest is done in this folder, or says what is left.

## 1. Blockers that only the account holder can clear

- [ ] **(you) Licence exception for the App Store.** Aurora is GPL-3.0-or-later, and the
  GPL's terms and the App Store's are widely held to be incompatible (plan §2.6; VLC was
  pulled over it in 2011). Every commit in the repository is yours (some co-authored
  with Claude, which claims no copyright), so you can grant an exception. Add to
  `LICENSE` (outside `apple/`, so not done here) a GPL section 7 additional permission,
  for example:

  > Additional permission under GNU GPL version 3 section 7: as the copyright holder,
  > Noah Klimczuk permits distributing this program, or a work based on it, through
  > Apple's App Store and TestFlight under their usage rules, which would otherwise
  > conflict with the GPL. Corresponding source stays available at
  > https://github.com/noahklimczuk/iptv-player.

  Accept outside contributions only with a contributor agreement that keeps this
  possible. MPVKit and FFmpeg stay on their **LGPL** build (`MPVKit`, not
  `MPVKit-GPL`), and the notices travel with the app (Settings → About).
- [ ] **(you) Firebase for the public.** Every household on the App Store shares your one
  Firebase project, and its free (Spark) plan allows 50,000 reads and 20,000 writes a
  day **for the whole project**, not per household. A few dozen active households will
  use that up by mid-morning and sync stops for everyone until midnight Pacific. Before
  release: move the project to the **Blaze** (pay-as-you-go) plan with a budget alert,
  and deploy `firebase/firestore.rules`. The shared pacer and Settings → Account and sync's "free
  plan's 50,000 reads" line assume one household per project; that wording and pacing
  live in the shared crates and Windows' panel, so changing them is outside `apple/`.
  Enable **email enumeration protection** and check the password-reset email template
  in the Firebase console (Authentication → Templates).
- [ ] **(you) Keys compiled into a public binary.** Release builds compile in the TMDB,
  Gemini and Firebase keys. The Firebase web key is meant to be public (the rules
  protect the data). The **Gemini key is billed to you** for every household that turns
  recommendations on: restrict it to the Generative Language API in Google Cloud, set a
  quota, or ship without `AURORA_GEMINI_KEY` so each viewer brings their own.
- [ ] **(you) Apple Developer Program** membership (paid) for the account that will own
  the app.

## 2. App Store Connect set-up (you)

- [ ] Register the identifiers in Certificates, Identifiers & Profiles:
  `tv.aurora.app` (iOS and tvOS), `tv.aurora.app.topshelf`, and the app group
  `group.tv.aurora.shared`, enabled on both tvOS identifiers.
- [ ] Create **one** app record: platforms iOS and tvOS, bundle ID `tv.aurora.app`, name
  and SKU from `METADATA.md` (universal purchase).
- [ ] Create an App Store Connect API key (Users and Access → Integrations) for
  `scripts/testflight.sh`, and add the secrets listed in `apple/README.md` → Releasing
  if CI uploads.
- [ ] Fill in the listing from `METADATA.md`, the privacy answers from
  `PRIVACY_LABELS.md`, export compliance from `EXPORT_COMPLIANCE.md`, and the review
  notes from `REVIEW_NOTES.md`.
- [ ] Age rating questionnaire (`METADATA.md` → Age rating).

## 3. In the app (done in this release branch)

- [x] Version **2.0.0** on every target, and the build number taken from
  `CURRENT_PROJECT_VERSION`, which `testflight.sh` sets per upload (the Info.plists had
  `1.0` and `1` written in, so every upload would have been build 1).
- [x] One bundle ID for both apps (universal purchase), `tv.aurora.app`.
- [x] Privacy manifest (`Shared/Privacy/PrivacyInfo.xcprivacy`) in both apps and the Top
  Shelf: no tracking, the data types above, and the required-reason APIs (UserDefaults,
  system boot time, file timestamps).
- [x] `ITSAppUsesNonExemptEncryption = NO`, so uploads skip the export question
  (confirm `EXPORT_COMPLIANCE.md`).
- [x] **Sync: create an account in the app**, laid out for the public (Settings → Account and sync →
  Create account): what the account is, password twice, the passphrase explained, the
  privacy policy, and "Forgot the password?" by email.
- [x] **Sync: delete the account in the app** (guideline 5.1.1(v)): Settings → Account and sync →
  Delete account, with the password, removing every record, the vault and the
  Firebase account.
- [x] **Gemini asks first** (guideline 5.1.2(i)): off until the viewer agrees on Home's
  card, in Ask Aurora or in Settings.
- [x] Store listing, review notes, demo playlist, privacy policy and labels in this
  folder.
- [x] Windows' 2.0 account work (`docs/RELEASE_2.0.md` §2, PR 232): the confirmation
  email after sign-up, the same delete and reset calls from the shared crate, and the
  section's new name, Account and sync.
- [x] Follow **4c**: first-run setup ends by offering a free account for sync (first
  provider only, only where sync is built in and nobody is signed in), opening Settings at
  Account and sync. Its Done step now shows on Apple too; before, saving the provider
  closed the wizard mid-import. Windows'
  `docs/PRIVACY.md` covers Windows only, so `PRIVACY_POLICY.md` stays Apple's; keep the
  two in step.
- [x] Screenshots of the UI in `apple/AppStore/screenshots/` on this Mac (the repository's
  `.gitignore` keeps `screenshots/` out of git) (iPhone 6.9", iPad 13", Apple TV 4K), taken
  in the simulator with the demo playlist (no real channels or artwork). Upload them in
  the listing's order. Still to add: **the player**, which needs a real device on this
  Intel Mac. The film screens carry TMDB posters (the key is in this Mac's `keys.env`).

## 4. On real hardware (needs devices)

The simulator on this Intel Mac cannot draw mpv's picture, so none of these has been
seen yet. All are required before submitting.

- [ ] Playback with picture on an **Apple TV** and an **iPhone**: live, catch-up, film,
  episode, a recording, multi-view.
- [ ] Import time and zap time against a large real panel.
- [ ] Top Shelf on a real Apple TV; pairing a TV from a phone on a real network (the
  local network permission prompt).
- [ ] iOS Picture in Picture (needs mpv's software render path; not built yet).
- [ ] A TestFlight build installed from TestFlight on each device, run for a day.

## 5. Ship

- [ ] `apple/scripts/testflight.sh` → TestFlight, internal testers, then external
  (external testing triggers a short beta review: use `REVIEW_NOTES.md`).
- [ ] Submit 2.0.0 for review with "Manually release this version".
- [ ] Release; tag `apple-v2.0.0`.
