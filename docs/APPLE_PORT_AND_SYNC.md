# Apple TV / iOS port, and sync across every device

A plan, not a record of work done. Nothing here is built yet. It sits beside
`ARCHITECTURE.md` and `DECISIONS.md`. When a phase below lands, its decisions move into
`DECISIONS.md`, its status moves into `ROADMAP.md`, and this file keeps only what is
still ahead.

## The goal

1. Aurora runs on **Apple TV (tvOS)** and on **iPhone and iPad (iOS/iPadOS)**, as well as
   on Windows.
2. A viewer's **personal state is the same on every device**: profiles, favorites, My
   List, watch progress, provider logins, playlist edits and settings. That holds across
   **Windows and Apple devices alike**, and across **devices signed into different Apple
   IDs**.
3. It costs nothing to run. Sync uses a **free managed service**.

Two consequences follow from goal 2 and shape the rest of the plan:

- **iCloud is out.** CloudKit and `NSUbiquitousKeyValueStore` belong to an Apple ID and
  do not exist on Windows. Sync needs a backend of its own that no platform owns.
- **Items need IDs that mean the same thing on every device.** Today `favorites.item_id`,
  `watch_progress.item_id` and the rest are SQLite row ids. Every library numbers its
  rows from 1, so movie 4,812 on the TV is a different film from movie 4,812 on the PC.
  §3.3 replaces these with stable keys.

---

## 1. Decisions at a glance

| Question | Decision | Why |
|---|---|---|
| Interface on Apple devices | **Native SwiftUI**, with one code base for tvOS and iOS | tvOS has no WebView (no `WKWebView`, no `UIWebView`), so the React UI cannot run there at all. Tauri 2 targets iOS but not tvOS. Since the TV needs SwiftUI anyway, iOS shares it rather than keeping a third interface. |
| Domain logic on Apple devices | **The existing Rust crates**, built as an XCFramework and called from Swift through **UniFFI** | Parsers, guide matching, rules, the SQLite schema, ingestion and TMDB matching come to about 55k of the 76k lines of Rust, with ~1,000 tests. Rewriting them in Swift would fork the product's behaviour. |
| Player on Apple devices | **MPVKit** (libmpv built for iOS/tvOS), with **VLCKit** as the fallback | IPTV panels mostly serve raw MPEG-TS over HTTP, which `AVPlayer` handles badly or not at all; `AVPlayer` is reliable only for HLS. Keeping mpv means mpv's settings, its error log (`MpvBackend::fail`) and timeshift behave the same on every platform. |
| Sync backend | **Firebase: Cloud Firestore and Firebase Authentication, on the free Spark plan** | Free, managed, and never paused for inactivity. The free plan's daily quotas are far above what one household uses (§3.7). Both services have REST APIs, so one Rust client serves every platform. |
| Who talks to Firebase | **One Rust crate, `aurora-sync`, over REST**, on every platform | One merge implementation, testable on Linux CI like the rest of the core. Apple devices do not depend on the Firebase Apple SDK, whose tvOS support for Firestore is not clearly official. |
| Privacy of synced data | **End-to-end encrypted.** Google stores only ciphertext, keyed ids and timestamps. | Provider passwords and viewing history are the most sensitive things the app holds. `PRIVACY.md` promises they stay with the viewer. |
| What does *not* sync | The catalogue, the guide, artwork and recordings | Each device imports from the provider itself, as Windows does today. Uploading a 117k-film catalogue would take days of the free plan's 20,000-a-day write quota, and syncing it gains nothing. |

### Alternatives considered

- **Supabase (free plan).** It is pleasant to build on, but a free project is **paused
  after a week of low activity** and must be restored by hand from the dashboard. A
  household that goes on holiday would come back to a broken sync. The Pro plan removes
  the pause at about $25 a month, which breaks goal 3.
- **CloudKit.** Ruled out by goal 2, as explained above.
- **A self-hosted server.** It would be free and private, but you rejected running one.
  The design in §3 isolates the backend behind a `SyncTransport` trait, so moving to a
  server later is a new transport, not a redesign.
- **Files in a WebDAV or Dropbox folder.** This needs no account system, but it gives
  slow, coarse-grained sync. Watch progress handed from the TV to the phone would lag by
  minutes.

---

## 2. The Apple app

### 2.1 Shape

```
apple/                               Xcode project (new)
  Aurora.xcodeproj
  Shared/                            SwiftUI shared by both targets
    Core/                            Swift wrappers over the UniFFI bindings
    Features/                        Home, Live, Guide, Movies, Series, Search, Settings, Player
    Design/                          tokens ported from src-ui/src/styles/tokens.css
  AuroraTV/                          tvOS target: focus engine, Siri Remote, top shelf
  AuroraMobile/                      iOS/iPadOS target: touch, PiP, AirPlay, rotation
  Frameworks/AuroraCore.xcframework  built from src-native by scripts/build-apple.sh

src-native/crates/
  aurora-service/   (new) the command bodies now in aurora-app, with no Tauri dependency
  aurora-ffi/       (new) the UniFFI façade over aurora-service, for Swift
  aurora-sync/      (new) the sync engine, used by Windows and Apple alike
  aurora-app/       Tauri host, now a thin adapter over aurora-service
```

### 2.2 Splitting `aurora-app`

`aurora-app` is about 21k lines and registers ~150 `#[tauri::command]` handlers. Most of
each handler is product logic: resolving a stream URL, failover, the home rails,
profile gating, the DVR tick. Only the entry point is Tauri. The port extracts that logic
into **`aurora-service`**, a crate that depends on no Tauri. After the split:

- `aurora-app` handlers become one-line adapters:
  `fn library_list_channels(s: State<Svc>, q: Query) -> R { s.library().list_channels(q) }`.
- `aurora-ffi` exposes the same services through UniFFI. `shared/ipc.ts` is already a
  catalogue of that surface, so the Swift API mirrors it name for name. The
  `aurora-contract` checks can extend to verify that the FFI exports cover the IPC
  commands the Apple app uses.
- Events (`player.state`, `ingest.progress`, `library.refreshed`) become a callback
  interface that Swift implements. UniFFI supports foreign-implemented traits.

This split pays for itself before any Swift exists. It is a pure refactor, the existing
Windows suites check it, and it lands first (Phase 1).

### 2.3 What ports, and what stays behind

| Piece | On Apple devices |
|---|---|
| `aurora-core` | Ports as is. It does no I/O and has no platform dependencies. |
| `aurora-db` | Ports. `rusqlite` with `bundled` compiles SQLite for the target, and the database lives in the app's Application Support container. |
| `aurora-ingest` HTTP, Xtream, M3U, XMLTV, TMDB, artwork | Ports. reqwest with rustls; `ring` has to build for tvOS, which the Phase 2 spike checks first. |
| `aurora-ingest::credentials` | Gets a **Keychain** backend (the `keyring` crate's `apple-native` feature, or `security-framework` directly) beside the Windows Credential Manager one. |
| `aurora-ingest::selfupdate`, `updates` | Excluded on Apple. Updates come from TestFlight or the App Store. |
| `aurora-ingest::recorder` and the DVR scheduler | **Stay on Windows.** tvOS has no general background execution, and an Apple TV's storage is a cache the system purges. Recording *rules* still sync (§3.4), so a rule made on the phone records on the PC. |
| `aurora-video::libmpv` and `::surface` (Win32 child window) | Replaced by Swift code: an `MPVKit` view hosted in SwiftUI. Swift implements the engine's `Driver` trait, so failover, the playback state machine and the error taxonomy stay in Rust. |
| Timeshift | Ports via mpv's cache. The buffer belongs in `Caches/`, and the size limit should be smaller on tvOS, where free disk is scarce. |
| Mosaic / multiview | **iPad and Apple TV 4K only**, capped by `max_connections` as on Windows. Low priority. |
| PiP | iOS gets system Picture in Picture. MPVKit needs a sample-buffer display layer for that, so it is a later spike. |
| Gemini assistant, AI rail | Port with no changes; they are HTTP. |

### 2.4 tvOS specifics

- **Focus, not pointers.** The guide grid is the hard screen: a virtualised 2D grid that
  works with the focus engine (`focusSection`, `prefersDefaultFocus`, and
  `onMoveCommand` for paging by time). The React guide's behaviour carries over: cursor
  memory, group filters and preview on focus.
- **The Siri Remote.** Click to select, swipe to scrub, Play/Pause, Menu/Back, and
  long-press for the context menu (favorite, My List, record).
- **Signing in from the sofa.** Typing an email address and passphrase with a remote is
  miserable. The first release relies on the Continuity Keyboard, which lets any
  nearby iPhone type for the TV. Phase 5 adds pairing from a signed-in phone (§3.6).
- **Top Shelf.** The Continue Watching rail, served by a Top Shelf extension reading a
  small snapshot the app writes. Optional.

### 2.5 Building Rust for Apple targets

| Target | Status |
|---|---|
| `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-ios` | Tier 2 on stable. `rustup target add` works. |
| `aarch64-apple-tvos`, `aarch64-apple-tvos-sim` | **Tier 3 on stable** at the last check. A promotion to Tier 2 (without host tools) appears in the beta docs. Until stable has it, build with nightly and `-Zbuild-std`. **Check the current status when Phase 2 starts.** |

`scripts/build-apple.sh` (new) builds each target, makes simulator fat libraries with
`lipo`, generates the Swift bindings with `uniffi-bindgen`, and packages
`AuroraCore.xcframework`. CI gains a `macos-latest` job that builds the XCFramework and
runs `xcodebuild test` against both simulators. **That job is the only place the Apple
code is compiled.** Nothing Apple-specific can build on the Linux or Windows runners.

### 2.6 Licensing — read before shipping

Aurora is GPL-3.0-or-later, and libmpv and FFmpeg are LGPL when built without GPL parts.
The GPL's terms and the App Store's distribution terms are widely considered
incompatible. VLC was pulled from the App Store over exactly this in 2011, and returned
only after relicensing. TestFlight and App Store distribution run on Apple's terms, so
before distributing beyond your own devices:

- As the copyright holder, you can grant an App Store exception or dual-license your
  own code. You cannot do that for contributions you do not own.
- MPVKit and FFmpeg must be built in their LGPL configuration. The build has to record
  that configuration, as `THIRD-PARTY-NOTICES.md` already does for `libmpv-2.dll`.

For personal use there is a simpler route: build it yourself in Xcode and install it on
your own devices. A free developer account's builds expire after 7 days. A paid account's
last a year.

---

## 3. Sync

### 3.1 Shape

```
   Windows (Tauri)        iPhone / iPad          Apple TV
   aurora-service         aurora-service         aurora-service
        │                      │                      │
   aurora-sync ◄── same crate, same merge rules ──► aurora-sync
        │                      │                      │
        └────── HTTPS (REST) ──┴──────────────────────┘
                          │
          Firebase Authentication (email + password)
          Cloud Firestore   users/{uid}/records/{recordId}_{deviceId}
                            users/{uid}/meta/vault
```

- **The local database stays the source of truth on each device.** Sync is a background
  reconciliation and never sits in front of a screen. A device that has been offline for
  a month keeps working, then merges when it reconnects.
- **`aurora-sync` exposes a `SyncTransport` trait.** `FirestoreTransport` implements it
  in production, and an in-memory transport implements it in tests. That lets the merge
  rules run as ordinary `cargo test`s that simulate three devices, clock skew and
  partitions, with no network. It also leaves room for the self-hosted server rejected
  in §1, if that ever changes.

### 3.2 Accounts

- **Firebase Authentication with email and password**, through the Identity Toolkit
  REST API (`accounts:signUp`, `accounts:signInWithPassword`) and the Secure Token
  API, which refreshes the one-hour ID token. Both endpoints are free and work
  identically from Rust on every platform.
- **One Aurora account per household.** Aurora's existing profiles (`profiles` table)
  separate the people in it, as they do today. The Aurora account is not an Apple ID or
  a Google account. It is a separate email-and-password pair, which is what makes goal 2
  hold.
- **Each device stores its refresh token** in the platform credential store: Windows
  Credential Manager or the Apple Keychain. It is never stored in SQLite.
- **Sync is opt-in.** Settings gets a new **Sync** panel to create an account or sign
  in, show this device's name, list the household's other devices and their last sync
  time, sign out, and remove this device. A device that never signs in behaves exactly as
  Aurora does today.

### 3.3 Stable identity: sync keys

Every synced row gets a **sync key**, a string that names the same thing on every device:

| Entity | Sync key |
|---|---|
| Provider | `prov:{uuid}`. The UUID is minted once, when the provider is first added, and stored in a new `providers.sync_id` column. A device adding a provider looks for an existing synced one first, matched on `kind` + normalised `base_url` + username, so the same panel added on two devices before either synced becomes one provider. |
| Channel | `ch:{provider uuid}:{provider_key}`. `provider_key` is already "stream id / URL hash, stable across refreshes". |
| Movie | `mv:{provider uuid}:{provider_key}` (already `UNIQUE(provider_id, provider_key)`). |
| Series | `sr:{provider uuid}:{provider_key}` |
| Episode | `ep:{series key}:s{season}e{episode}`. This matches the existing `UNIQUE(series_id, season, episode)`. |
| Profile | `pf:{uuid}`, held in a new `profiles.sync_id` column. |
| Rule | `rule:{rules.id}`. Rule ids are already text. Check that they are globally unique, and make them UUIDs if not. |
| Setting | `set:{scope}:{key}`, where `scope` is `global` or the profile's sync key. |

The local rows keep their integer ids. Sync translates sync key ↔ local id at the edge.
A synced reference whose target has not been imported yet on this device (the TV has not
refreshed that provider) is **parked** in a `sync_pending` table and applied after the
next import. It is never dropped. This is the same "a refresh never destroys user data"
guarantee `ROADMAP.md` already holds the importer to.

### 3.4 What syncs

| Data | Table(s) | Merge rule |
|---|---|---|
| Providers (name, kind, URL, user agent, referrer, enabled, order, max connections) | `providers` | Last writer wins, field by field. |
| Provider passwords, TMDB key, Gemini key | the credential store | Encrypted vault records (§3.5), last writer wins. |
| Profiles (name, avatar, kids flag, PIN hash, rating ceiling) | `profiles`, `parental`, `parental_locks` | Last writer wins, field by field. The PIN syncs as its Argon2 hash; a new PIN on one device applies everywhere. |
| Watch progress | `watch_progress` | Latest `updated_at` wins per item, **except that `completed` is sticky**. A "finished" from one device is not undone by another device's stale mid-film position written later from an outbox. Only an explicit "mark unwatched" clears it, and that is its own timestamped write. |
| Favorites (all named lists), My List, Not interested | `favorites`, `my_list`, `not_interested` | A set of items. An add or a removal is a timestamped element, and removals are tombstones, so a removal on the TV is not resurrected by the PC's older copy. List order uses a fractional `sort_key`, so reorders on two devices both survive. |
| Series preferences (audio/subtitle language, skip intro auto) | `series_prefs` | Last writer wins, field by field. |
| Playlist edits: rename, renumber, regroup, hide, custom logo | `channels.custom_*`, `hidden` | One record per channel holding only the override fields, last writer wins per field. |
| Rules, manual EPG mappings | `rules`, `epg_manual_map` | Last writer wins per record; deletion is a tombstone. |
| Recording rules and reminders | `recording_rules`, `reminders` | Last writer wins per record. **Only Windows acts on them** (§2.3). Apple devices show them and can create them. |
| Mosaic layouts | `mosaic_layouts` | Last writer wins per record. |
| Settings that belong to the person | `settings` | Last writer wins per key, **from an allowlist** (below). |

**Settings are allowlisted, not blocklisted.** A new setting is device-local until someone
decides otherwise. That stops a Windows-only setting (hardware decoding, the timeshift
folder, the glass transparency slider tuned for one monitor) from silently arriving on a
TV it makes no sense on. Each setting key gets a `sync: Personal | Device` flag in one
table in `aurora-core`. A test fails on any key that lacks the flag.

**Never synced:** the catalogue, the guide, artwork, search indexes, skip markers (they
are rebuilt from the community source and learned per device), `pin_attempts`
(a lockout is per device by design), recordings and their files, the timeshift buffer,
logs, and `series_listing_attempts`.

### 3.5 Encryption

The goal is that Google holds nothing it could read: no passwords, no viewing history,
not even which films are on the list.

- **The vault key.** A random 256-bit key generated by the first device. It is stored in
  Firestore only **wrapped**: encrypted under a key derived from a **sync passphrase**
  with Argon2id. `argon2` is already a workspace dependency, used for PINs. The wrap
  stores its salt and parameters beside it in `users/{uid}/meta/vault`.
- **The passphrase is separate from the account password.** The account password
  authenticates to Firebase; the passphrase never leaves the device. Every device asks
  for the passphrase once, unwraps the vault key, and keeps it in the platform credential
  store from then on.
- **Records.** Each record's payload is XChaCha20-Poly1305 (`chacha20poly1305` crate)
  under the vault key, with the Firestore document path as associated data, so a
  ciphertext cannot be moved to another record.
- **Document ids.** The id is `HMAC-SHA256(vault key, sync key)` in hex, so even the
  names of the items are hidden. The server sees only: an opaque id, a ciphertext, a
  server timestamp, a `deleted` flag and the writing device's id.
- **Recovery.** A forgotten passphrase cannot be recovered; that is what end-to-end
  means. The device's local data is untouched. On setup, Aurora shows a **recovery
  code** (the vault key, as words) to write down. **Reset sync** throws away the server
  copy and re-uploads from the device the viewer chooses.
- **Firestore security rules** restrict `users/{uid}/**` to `request.auth.uid == uid`.
  The rules ship in the repo (`firebase/firestore.rules`) with emulator tests. They are
  defence in depth; the encryption above is the real protection.

### 3.6 The protocol

**Local side (migration 18, `sync_capture` — built):**

- `sync_id` columns on `providers`, `profiles` and `recording_rules` (§3.3), minted by
  trigger on insert and backfilled for existing rows. `sync_item_keys` is a view that
  gives the `ch:` / `mv:` / `sr:` / `ep:` key of any catalogue row.
- `sync_outbox(sync_key, kind, ident, hlc, attempts)`, written **by trigger, in the same
  transaction** as the change. That makes "edited offline, app killed" safe, and a new
  write site cannot forget to enqueue. The row says *which* record changed and when;
  `ident` is its device-independent identity as JSON (`{"pf":…,"list":…,"item":…}`), and
  the record key is `{kind}:{ident}`. The payload is read from the row at push time, so
  the outbox coalesces: an hour of playback is one pending record.
- `sync_shadow(sync_key, record)`: the last merged state of each record with a clock per
  field. The field-by-field merge compares against it, and a push diffs the row against
  it to know which fields this device changed.
- `sync_state(key, value)`: `enabled`, `applying`, `hlc`, the pull cursor, this device's
  id, and the last successful sync. Capture is off until `enabled` is set, so a device
  that never signs in behaves as before; it is suspended while `applying`, so a pulled
  change is not echoed back.
- `sync_pending`: parked references (§3.3).
- **Hybrid logical clocks** for every merge timestamp, rather than wall time alone. A
  Windows PC whose clock runs two minutes fast must not win every conflict for two
  minutes. The triggers stamp `max(last + 1, now_ms << 16)`, and the engine folds every
  pulled clock into `sync_state.hlc`. The HLC is inside the encrypted payload, and merging
  is done on the client.
- Settings keys are classified `Personal` or `Device` in `aurora_core::sync_settings`;
  `aurora-contract/tests/sync_settings.rs` fails on any key the sources use that the
  table does not name.

**One copy per writer (`aurora-sync`, built).** The server stores a record as one
document *per device that wrote it*: `records/{recordId}_{deviceId}`. A device pushing a
record replaces only its own previous copy, and a reader merges every copy it pulls. A
single shared document per record would be last-*push*-wins, because the server cannot
merge ciphertext: a device pushing its version would replace a newer edit from another
device that it had not pulled yet, and that edit would be lost for good. The multi-device
tests caught exactly this (two devices editing different fields of one profile in the
same millisecond). With a copy per writer the merge is commutative, every device
converges on the join of all copies in any order, and no transactions or preconditions
are needed on the server.

**Push.** The outbox drains in batches of up to 500 writes (Firestore's limit) via
`documents:commit`. Each write sets `serverTime` with a `REQUEST_TIME` field transform.
A record is read from its row at push time and diffed against the shadow, so only the
fields this device changed carry the new clock; the shadow and outbox are updated only
after the server accepted the batch. Progress writes are **coalesced**: at most one per
item per 60 s while playing, plus one on pause, stop, end and backgrounding.

**Pull.** A `runQuery` for `serverTime > cursor` ordered by `serverTime`, paged. Each
copy is decrypted and merged by its rule (§3.4), and then the cursor advances. Copies
this device wrote itself are skipped. If the record also has an unpushed local change,
that change is folded into what is written to the row, but not into the shadow, so the
next push still sends it.

**Joining.** A device's first sync pulls before it turns capture on. A provider or
profile that exists on both sides (matched on kind + URL + username, or on name) adopts
the household's id first, so the same favourite on both devices becomes one record.

**When it runs.**

- At launch and on returning to the foreground.
- After any local change, debounced by about 3 s.
- Every 2 min while the app is in the foreground.
- On Windows, on the existing supervised maintenance thread (`supervise.rs`).
- **No push notifications and no realtime listeners.** Firestore's realtime listeners
  run over gRPC, which this design avoids, and two minutes of lag is fine for everything
  in §3.4.
- **One exception, handled by a direct pull:** opening a film or episode pulls that one
  record's progress before showing **Resume / Start over**. Stopping on the TV and
  opening the same episode on the phone therefore resumes at the right second.

**Pairing an Apple TV (Phase 5).** The TV signs in anonymously (free) and shows a QR code
holding a pairing id and an ephemeral X25519 public key. A signed-in phone scans it and
writes `pairing/{id}`: the refresh token and the vault key, sealed to the TV's key. The
TV reads the document, deletes it, and is signed in. Nothing is typed on the TV.

### 3.7 Staying inside the free plan

The Spark plan's quotas are **20,000 document writes, 50,000 reads and 20,000 deletes a
day, 1 GiB stored, and 10 GiB a month of egress**. Above the daily quotas, requests fail
until the next day; nothing is billed unless the project is upgraded to Blaze. **Keep it
on Spark.**

A heavy household, with three devices each watching 6 hours a day:

| | Per day |
|---|---|
| Progress writes (1/min while playing) | ~1,100 |
| Other edits (favorites, settings, …) | ~100 |
| Pulls: 3 devices × 30/h × 16 h awake, an empty pull costs 1 read | ~1,450 |
| Records pulled by the other two devices | ~2,400 |
| **Writes** | **~1,200 of 20,000** |
| **Reads** | **~3,900 of 50,000** |

Storage: a record is under 1 KB, and even 20,000 records (years of episode progress) is
about 20 MB of 1 GiB; with a copy per writer (§3.6) three devices make that at most
60 MB. A new device's first sync reads every copy once. Most records are written by one
device only, so that is near 20,000 reads at the very worst and fits in one day's quota.
In the pathological case where all three devices had written every one of 20,000
records it is 60,000 reads, past the 50,000 quota: the pull stops at the quota, keeps
its cursor (§3.8), and finishes the next day.

`aurora-sync` keeps a **local count of today's reads and writes**. Past 70% of either
quota it widens the poll interval and stops the 60-second progress writes, keeping only
the pause/stop write. Diagnostics shows the counts.

> The quotas were checked against Firebase's pricing page in November 2026. Check them
> again at Phase 3. Free plans change.

### 3.8 Failure behaviour

| Situation | Behaviour |
|---|---|
| Offline | The outbox grows and nothing blocks. The Sync panel says "Offline — 14 changes waiting". |
| Quota exhausted | Treated as offline until midnight Pacific time, when Firestore resets the quotas. Logged once, not per attempt. |
| Token expired or revoked | The device stops syncing and shows "Sign in again"; local data is untouched. |
| Wrong passphrase | Wrapping uses AEAD, so a wrong passphrase is detected. It is reported as such and never as "corrupt data". |
| A record that will not decrypt (written by a newer app version or corrupted) | Skipped and counted in Diagnostics, never applied. The cursor still advances, and the record is re-fetched when the app updates. |
| Two devices add the same provider before either has synced | They are merged by the fingerprint in §3.3; both sides' favorites and progress survive. |

---

## 4. Phases

Each phase ends with something that runs and is tested, as the Windows work did.

| # | Phase | Exit criteria |
|---|---|---|
| 1 | **Split `aurora-app` into `aurora-service`** | Every Windows test suite and journey passes. `aurora-service` builds and tests with no Tauri in its dependency tree. The DVR tick, failover and home rails all live there. |
| 2 | **Apple spike: Rust to Swift to video** | Built on a Mac: `AuroraCore.xcframework` for iOS, the iOS simulator, tvOS and the tvOS simulator. A bare SwiftUI app on a **real Apple TV** imports one Xtream provider, lists channels and plays one live channel through MPVKit. Measure: import time for the 117k-film panel on Apple TV hardware, and zap time against the 1.5 s budget. |
| 3 | **`aurora-sync`, Windows first** | The new migration, the outbox, HLC, the merge rules and the in-memory transport, with multi-device simulation tests (offline edits, clock skew, sticky `completed`, tombstones, parked references). Then the Firestore transport, the security rules with emulator tests, and the Sync panel in the React UI. **Two Windows installs share progress and favorites end to end.** |
| 4 | **The Apple app proper** | Home, Live, Guide, Movies, Series, Search, Player, Settings and Profiles on tvOS and iOS, with sync from day one. The guide grid works with the focus engine and passes the same scenarios as the Windows guide. |
| 5 | **Polish** | Pairing the TV from a phone, Top Shelf, iOS PiP and AirPlay, multiview on iPad/Apple TV 4K, and TestFlight builds from CI. |

Phase 3 can run **before or alongside** Phase 2. It needs no Mac and gives Windows users
multi-PC sync on its own.

## 5. Risks and open questions

1. **`ring` and the rest of the TLS stack on tvOS.** It is the likeliest build failure
   in Phase 2. The fallback is to give reqwest a TLS backend that uses Apple's
   Security.framework (`native-tls`) on Apple targets only.
2. **MPVKit on tvOS.** It is community-maintained, so check its release cadence and its
   hardware-decoding path (VideoToolbox) before committing. VLCKit (TVVLCKit) is the
   fallback. The `PlayerBackend` trait keeps either swappable.
3. **Import size on Apple TV.** A 117,587-film catalogue plus FTS5 on a device with
   limited storage that the system can purge. A **"Movies & series: off"** option per
   provider may be needed on tvOS, keeping only live TV.
4. **Firebase on Spark is Google's free plan, not a contract.** The `SyncTransport` trait
   is the exit: a Cloudflare Workers + D1 transport, or a self-hosted one, would be a
   new implementation of one trait.
5. **App Store and the GPL** (§2.6), if this is ever distributed beyond your own devices.
6. **Provider connection limits.** Sync makes it easy to start the same line on the TV
   while the PC is still playing. A `max_connections = 1` line will refuse the second
   stream. That is outside sync's control, but the error should say so plainly; the
   error taxonomy has a place for it.
