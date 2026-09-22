# IPTV Player — Build Prompt

This repository contains **one artifact: a super prompt**. It is a complete, self-contained
specification you can hand to an AI coding agent (Claude Code, Cursor, Copilot Workspace, Codex,
etc.) or to a human team, to build a full-featured, Windows-only IPTV player from scratch.

**How to use it:** copy everything below the horizontal rule into your agent's prompt (or point the
agent at this file and say "implement `README.md`"). It is written to be executed in phases; tell
the agent to start at Phase 0 and to stop for review at each milestone gate.

**No application code lives in this repo yet.** This is the blueprint, not the build.

---

# SUPER PROMPT — "Aurora TV": A Windows-Native, Full-Feature IPTV Player

## 0. Role, Mission, and Working Agreement

You are a **senior Windows desktop application engineer, media-playback specialist, and product
designer** rolled into one. You are building a production-grade IPTV client called **Aurora TV**
(rename freely) from an empty repository.

**Mission:** ship a Windows desktop app that makes a user's existing IPTV subscription feel like a
premium streaming service. Movies and series must feel like Netflix. Live TV must feel like a real
cable set-top box. Everything plays **inside the app** — the user must never see VLC, MPC-HC, or any
external window.

**Working agreement — follow this exactly:**

1. **Plan before you build.** Produce `docs/ARCHITECTURE.md` and `docs/ROADMAP.md` first. Do not
   write feature code until the plan exists.
2. **Work in the phases in §21.** At the end of each phase, the app must build, run, and be
   demonstrably usable. No phase ends with a broken main branch.
3. **Stop at every milestone gate** (marked 🚩) and summarize: what shipped, what's stubbed, what
   you'd change, what you need decided.
4. **Never leave a TODO in place of a feature listed as required.** If you cannot build something,
   say so explicitly in the phase summary with a reason and a proposed alternative — do not silently
   drop it.
5. **Write tests as you go** (§20), not at the end.
6. **Every feature in §7–§15 is required** unless explicitly marked *Optional* or *Stretch*.
7. **Ask before assuming** only on the items in §22 ("Decisions I need from you"). Everything else:
   pick the sane default, document it in `docs/DECISIONS.md`, and keep moving.
8. Commit in small, reviewable units with Conventional Commit messages
   (`feat(guide): virtualize EPG grid rows`).

---

## 1. Non-Negotiable Constraints

These are the hard walls. A build that violates any of them is a failed build.

| # | Constraint | Why |
|---|---|---|
| C1 | **Windows 10 (21H2+) and Windows 11 only.** x64 and ARM64. No cross-platform compromise — use Win32/WinRT APIs freely. | Target platform is fixed. Cross-platform abstraction layers cost features and speed. |
| C2 | **All playback is in-process.** No spawning `vlc.exe`, `mpc-hc.exe`, `ffplay`, or a separate visible player window. The video surface is composited inside the app's own window. | The single most important product requirement. |
| C3 | **VOD, series, and live TV all use the same embedded player.** One playback pipeline, one OSD, one set of shortcuts. | Consistency and maintainability. |
| C4 | **Hardware-accelerated decoding by default** (D3D11VA, falling back to DXVA2, falling back to software). 4K HEVC must play at <15% CPU on a mid-range GPU. | IPTV users watch for hours; fan noise and battery matter. |
| C5 | **No bundled content, playlists, credentials, or provider lists.** The app ships empty. The user supplies their own M3U URL / Xtream credentials. | Legal and ethical. See §24. |
| C6 | **No DRM circumvention.** No Widevine/PlayReady bypass, no key extraction, no decryption of protected streams. Encrypted-stream errors are reported honestly to the user. | Legal. Non-negotiable. |
| C7 | **Netflix-*inspired*, not Netflix-*copied*.** No Netflix logo, wordmark, Bebas/Netflix Sans fonts, artwork, or literal pixel-for-pixel UI theft. Build your own visual identity that borrows the *interaction patterns*. | Trademark. Also: you can do better. |
| C8 | **UI thread is never blocked.** All I/O, parsing, and decoding is off the UI thread. Any operation over 100ms shows progress and is cancellable. | A frozen TV app is a broken TV app. |
| C9 | **The app must be usable entirely from the keyboard, and entirely from a remote control.** Mouse is a convenience, not a requirement. | People watch TV from a couch. |
| C10 | **Credentials are never written to disk in plaintext** and never appear in logs, screenshots, or diagnostic bundles. | §18. |

---

## 2. Technology Stack

### 2.1 Recommended stack (use this unless you have a strong, documented reason not to)

| Layer | Choice | Notes |
|---|---|---|
| Shell / native host | **Rust + Tauri 2** (or **C# / .NET 9 + WinUI 3**) | Owns the top-level HWND, window chrome, tray, hotkeys, file system, and the media layer. |
| UI | **React 18 + TypeScript 5 + Vite**, rendered in **WebView2** | The Netflix-grade UI work (rails, hover previews, transitions, EPG grid) is dramatically faster to build and polish in a web layer. |
| Media engine | **libmpv** (bundled `mpv-2.dll`), embedded | The single best choice: handles every container/codec IPTV throws at it, has a clean C API, hardware decode, and battle-tested stream resilience. `libVLC`/LibVLCSharp is the sanctioned alternative. |
| Compositing | **libmpv renders into a child HWND positioned *behind* a WebView2 with a transparent background.** The entire UI (including all player controls) lives in the web layer and floats over live video. | This is the key architectural trick that satisfies C2 + C3 while keeping a modern UI. Validate it in Phase 0 with a spike before anything else is built. |
| State | Zustand or Redux Toolkit + TanStack Query | Deliberately boring. |
| Styling | Tailwind CSS + Framer Motion (or equivalent) | Motion is a first-class requirement, not decoration. |
| Local database | **SQLite** (WAL mode, FTS5 for search) | Must handle 200,000+ VOD rows and 5M+ EPG programmes without breaking a sweat. |
| Transcode/remux utility | Bundled **FFmpeg** binary, used only for recording remux, thumbnail generation, and stream probing — **never** for playback. | Playback is libmpv's job. |
| Packaging | MSIX **and** an NSIS/Inno installer, plus a portable ZIP | §19. |

### 2.2 Explicitly rejected approaches — and why

- ❌ **HTML5 `<video>` + hls.js/mpegts.js as the only engine.** Chromium cannot decode AC-3, E-AC-3,
  DTS, MPEG-2 video, or MP2 audio — all extremely common in IPTV feeds, especially European ones.
  You'd ship an app that silently fails on a third of channels. You may use MSE as an *opportunistic
  fast path* for clean HLS/H.264/AAC, but libmpv is the required backstop.
- ❌ **Shelling out to a windowed player.** Violates C2.
- ❌ **Electron with a separate always-on-top overlay window tracking the player window.** It works
  until the user drags, snaps, or alt-tabs. Rejected for jitter.
- ❌ **Server-side transcoding.** Out of scope; this is a client.

### 2.3 Playback capability matrix (the engine must handle all of this)

| Category | Must support |
|---|---|
| Transports | HTTP, HTTPS, HLS (`.m3u8`, incl. fMP4/CMAF and LL-HLS), MPEG-TS over HTTP, MPEG-DASH (`.mpd`), RTSP, RTMP, UDP/RTP multicast (incl. `udp://@`), local files |
| Containers | MPEG-TS, MP4, MKV, WebM, AVI, FLV, MOV, OGM |
| Video codecs | H.264/AVC, H.265/HEVC (8/10-bit), MPEG-2, MPEG-4 Part 2, VP9, AV1, VC-1 |
| Audio codecs | AAC-LC, HE-AAC v1/v2, MP3, MP2, AC-3, E-AC-3, DTS, DTS-HD, TrueHD, Opus, FLAC, Vorbis, PCM |
| Audio output | WASAPI shared **and** exclusive mode; bitstream passthrough for AC-3/E-AC-3/DTS to a receiver; per-device selection; automatic downmix to stereo |
| Subtitles | SRT, ASS/SSA (full styling, fonts, positioning), WebVTT, VobSub (`.idx/.sub`), PGS, **DVB subtitles**, **teletext**, CEA-608/708 closed captions |
| HDR | HDR10/HLG detection; tone-mapping to SDR when the display isn't HDR; pass-through when it is |
| Interlacing | Automatic deinterlace detection with `yadif`/`bwdif`; manual override per channel (SD cable feeds are frequently interlaced) |

---

## 3. Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│ Native Host (Rust/Tauri or C#/.NET)                             │
│                                                                 │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │ Top-level HWND                                            │  │
│  │  ┌──────────────────┐   ┌──────────────────────────────┐  │  │
│  │  │ mpv child HWND   │ < │ WebView2 (transparent bg)    │  │  │
│  │  │ (video surface)  │   │  React UI / OSD / rails /    │  │  │
│  │  │                  │   │  EPG grid / modals           │  │  │
│  │  └──────────────────┘   └──────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────┘  │
│                                                                 │
│  Core services (all off the UI thread):                         │
│   • PlaybackService  — libmpv lifecycle, events, properties     │
│   • SourceService    — Xtream / M3U / Stalker ingestion         │
│   • EpgService       — XMLTV fetch, parse, normalize, index     │
│   • MetadataService  — TMDB/TVDB enrichment + artwork cache     │
│   • LibraryService   — SQLite, FTS5 search, dedupe, rules       │
│   • DvrService       — recording, scheduling, timeshift buffer  │
│   • ProfileService   — profiles, PIN, parental controls         │
│   • ImageService     — fetch, resize, WebP cache, blurhash      │
│   • NetworkService   — pooling, retry/backoff, UA/referer, proxy│
│   • UpdateService    — delta auto-update                        │
└─────────────────────────────────────────────────────────────────┘
             ▲                                  ▲
             │ typed IPC (commands + events)    │ SQLite (WAL)
             ▼                                  ▼
      React UI (TypeScript)              %LOCALAPPDATA%\AuroraTV\
```

**Rules:**
- The IPC boundary is **fully typed** and generated from a single schema (e.g. `ts-rs` or
  `NSwag`) — no stringly-typed channels, no `any`.
- The UI never touches the network or the database directly. Ever.
- Playback state is owned by the native side and **mirrored** to the UI via a subscription; the UI
  sends intents (`play`, `seek`, `setAudioTrack`), never state mutations.
- Every long-running service exposes health and last-error for the diagnostics panel (§17).

### 3.1 Project layout

```
/src-native        Rust or C# host: services, IPC, mpv binding, DB
/src-ui            React app
  /features        live/, movies/, series/, guide/, player/, dvr/, settings/, search/
  /components      design-system primitives
  /hooks
  /styles
/shared            IPC schema + generated types
/assets            icons, fonts, placeholder art (all self-owned/licensed)
/tests             unit, integration, e2e, fixtures
  /fixtures        real-world messy M3U/XMLTV/Xtream samples
/docs              ARCHITECTURE, ROADMAP, DECISIONS, SHORTCUTS, USER_GUIDE, TROUBLESHOOTING
/installer         MSIX + NSIS configs, code-signing scripts
```

---

## 4. Content Sources & Ingestion

The app must ingest all of the following, and must handle **multiple simultaneous providers** with a
single unified library on top.

### 4.1 Xtream Codes API (primary — most IPTV subscriptions use this)

Implement the full API surface: `player_api.php` for authentication and account info (expiry date,
max connections, active connections, status, trial flag), live categories and streams, VOD
categories, streams, and `get_vod_info` (plot, cast, director, genre, release date, rating,
duration, container extension, backdrop, TMDB id), series categories, series list, and
`get_series_info` (seasons, episodes, per-episode metadata, episode images), short EPG per stream,
and full `xmltv.php` EPG. Also support the catch-up/archive endpoints and `timeshift` URLs.

Requirements:
- Detect and surface **account expiry** and **max connections** prominently in Settings; warn the
  user at 7 days and 1 day before expiry.
- Respect the connection limit: track in-flight streams and refuse to open one more than the account
  allows, with a clear "you're already watching on another device" style message instead of a
  cryptic failure.
- Handle the many non-standard Xtream forks gracefully: missing fields, string-vs-number type
  inconsistency, `null` where an array is expected, HTML error pages returned with HTTP 200.

### 4.2 M3U / M3U8 playlists

Parse `#EXTM3U` and `#EXTINF` with all the attributes providers actually use: `tvg-id`, `tvg-name`,
`tvg-logo`, `tvg-chno`/`tvg-channel-number`, `tvg-shift`, `group-title`, `tvg-language`,
`tvg-country`, `catchup`, `catchup-source`, `catchup-days`, `radio`. Support `#EXTGRP`,
`#EXTVLCOPT` (`http-user-agent`, `http-referrer`), `#KODIPROP`, and `#PLAYLIST`.

Requirements:
- **Be ruthlessly forgiving.** Real playlists are broken: BOMs, CRLF/LF mixes, unescaped quotes,
  duplicate entries, missing commas, non-UTF-8 encodings, 400MB files. Parse what you can, log what
  you can't, never crash, never lose the whole list because of one bad line.
- Stream-parse: never load a 400MB playlist fully into memory.
- Support local file, HTTP(S) URL, and gzip-compressed sources.
- **Auto-classify** entries into Live / Movies / Series using group titles, URL shape
  (`/movie/`, `/series/`, `/live/`), and filename patterns (`S01E02`). Let the user override
  classification per group with a rules UI.
- **Series grouping:** detect `Show Name S01E02` patterns across hundreds of flat entries and fold
  them into proper show → season → episode hierarchies.

### 4.3 Stalker Portal / MAC-based portals *(Optional but strongly encouraged)*

Handshake, profile, token refresh, channel list, `create_link` resolution, VOD, and EPG.

### 4.4 EPG (XMLTV)

- Sources: the provider's `xmltv.php`, plus any number of user-added external XMLTV URLs, plus local
  files. Support `.xml`, `.xml.gz`, and `.xz`.
- Stream-parse with a pull parser — a 7-day EPG for 3,000 channels can be 1GB+ uncompressed.
- Parse `programme` fully: title, sub-title, desc, credits (director/actor/writer/presenter), date,
  category, episode-num (xmltv_ns, onscreen, dd_progid), rating, star-rating, icon, country,
  language, audio/video attributes, `new`/`live`/`premiere` flags.
- **Channel matching** is the hard part and must have a dedicated UI: exact `tvg-id` match first,
  then normalized-name fuzzy match (strip "HD", "FHD", "4K", "US:", country prefixes, punctuation,
  case), then user manual mapping. Show a **coverage report**: "2,847 of 3,102 channels have EPG
  data" with a one-click list of the unmatched.
- Handle per-channel **time shifts** (`tvg-shift`), source time zones, and DST correctly. Display in
  the user's local time, always.
- Refresh on a schedule (default every 6 hours, configurable), incrementally, in the background,
  without interrupting playback. Keep 14 days of history + as far forward as the source provides.
- Prune old programmes on a schedule so the DB doesn't grow unbounded.

### 4.5 Metadata enrichment

- **TMDB** integration (user supplies their own API key; the app ships with none) for movies and
  series: posters, backdrops, **title logo art** (critical for the Netflix look), overview, cast with
  headshots, crew, genres, runtime, release date, ratings, certification/age rating, trailers
  (YouTube), collections, "similar" and "recommended" lists.
- Match on IMDb/TMDB id when the provider supplies it, otherwise by title + year parsed from the
  stream name (strip quality tags: `1080p`, `WEB-DL`, `x265`, `MULTI`, `[EN]`…).
- **Manual "fix match"** UI: search TMDB and re-link a mis-identified title.
- Cache everything locally and forever; enrichment runs in a throttled background queue that
  respects TMDB rate limits and degrades gracefully to provider metadata when offline or keyless.
- *Optional:* TVDB, Fanart.tv, OMDb as secondary sources.

### 4.6 Refresh & sync behaviour

- Manual "Refresh now" per provider and globally.
- Scheduled background refresh with configurable cadence and a "only on AC power / only when idle"
  option.
- **Delta detection:** surface "142 new movies, 6 new episodes in shows you watch, 3 channels
  removed" as an in-app **What's New** feed after each refresh.
- Never blow away user data (favorites, watch progress, custom names, mappings) on refresh —
  reconcile by stable ID, and keep orphaned favorites visible with a "no longer in your subscription"
  badge for 30 days.

---

## 5. Data Model & Storage

SQLite in WAL mode at `%LOCALAPPDATA%\AuroraTV\` (or beside the executable in portable mode).

Core tables (design the exact schema yourself, but cover all of this):

| Table | Holds |
|---|---|
| `providers` | credentials ref, type, base URL, connection limit, expiry, last refresh |
| `channels` | provider, stream id, name, number (LCN), logo, group, EPG id, catchup config, custom overrides, hidden flag, sort order |
| `channel_sources` | multiple URLs per logical channel for failover (§7.14) |
| `epg_channels` / `epg_programmes` | indexed on `(channel_id, start, stop)`; this is the hottest table in the app |
| `movies` / `series` / `seasons` / `episodes` | provider ids + enriched metadata, one row per logical title with source links |
| `people` / `credits` | cast and crew for the detail pages |
| `watch_progress` | profile, item, position, duration, completed flag, last watched |
| `favorites`, `my_list`, `ratings`, `hidden_items` | per profile |
| `recordings`, `recording_rules`, `reminders` | DVR |
| `profiles`, `settings` | per-profile and global settings, JSON-typed values |
| `search_index` | FTS5 virtual table over channels, movies, series, episodes, and EPG programme titles |
| `history` | full watch history for "Watch it again" and the stats dashboard |

**Requirements:** schema migrations from day one (versioned, forward-only, tested); automatic
integrity check + repair on startup; `VACUUM` on a schedule; automatic timestamped backup of the DB
before each migration; and a "Reset library but keep settings/favorites" maintenance action.

---

## 6. The Playback Engine

This is the heart of the app. Get it right before you make anything pretty.

### 6.1 Core requirements

- One long-lived libmpv instance for the main surface, plus short-lived instances for PiP, multi-view
  tiles, and thumbnail generation. Pool and reuse them.
- **Fast channel zapping:** target **< 1.5 s** from keypress to first frame on a typical HTTP-TS
  stream. Techniques: keep the mpv instance warm, pre-resolve URLs, tune `cache-secs`,
  `demuxer-lavf-probesize`, `demuxer-lavf-analyzeduration`, and `--vd-lavc-threads`; optionally
  pre-buffer the next/previous channel in the zap order.
- **Resilience:** streams die constantly. On EOF/error, auto-retry with exponential backoff (3
  attempts, 1s/3s/7s) while showing a non-modal reconnecting indicator; then try the channel's backup
  sources; then surface a clear, human error with a "Retry" and "Report stream as broken" action.
- Configurable network buffer (1–60s), with presets: *Low latency* / *Balanced* / *Unstable
  connection*.
- Per-provider User-Agent and Referer headers, honoring `#EXTVLCOPT` values.
- Correct handling of streams that change resolution/codec mid-flight (very common on live feeds).

### 6.2 Player features (all required)

**Transport:** play/pause, stop, seek bar with buffered range, frame step, ±10s/±30s/±5min skips,
chapter navigation, precise seek vs. keyframe seek toggle, speed 0.25×–4× with pitch correction,
loop, A–B repeat.

**Seek preview:** hover the scrub bar on VOD → thumbnail preview. Generate sprites in the background
with FFmpeg on first play and cache them.

**Audio:** track switching (with language names, not "Track 2"), volume 0–200% with soft clipping,
per-title volume memory, **loudness normalization**, **Night Mode** (dynamic range compression),
audio delay/sync offset in 10ms steps, output device picker, exclusive-mode toggle, passthrough
toggle, channel layout/downmix control, and a 10-band equalizer *(Optional)*.

**Subtitles:** track switching, external file load (drag-and-drop), automatic sibling-file detection,
**OpenSubtitles search & download** (user's own key), delay adjustment, and full styling: font,
size, color, outline, shadow, background box opacity, vertical position, and "move subtitles above
the OSD when controls are visible."

**Video:** aspect ratio presets (Auto / 16:9 / 4:3 / 21:9 / Stretch / Zoom / Crop-to-fill), manual
zoom + pan, rotate, deinterlace modes, scaling-quality profiles (*Performance* / *Balanced* /
*Quality* using mpv's high-quality scalers), brightness/contrast/saturation/gamma/hue, sharpening,
HDR→SDR tone-mapping controls, and an optional **screen-color ambient glow** behind the video frame.

**Utility:** screenshot (with and without OSD) to a configurable folder, **record the current stream**
to disk with one key, bookmark timestamps with notes, and a **stats overlay** (press `i` twice)
showing resolution, codecs, FPS, bitrate, dropped/decoded frames, A/V desync, cache/buffer state,
network throughput, and the active hardware decoder.

**Windowing:** fullscreen (borderless), true window mode, **mini-player** (small always-on-top window
that survives navigating the rest of the app), **Picture-in-Picture**, multi-monitor target
selection, remembered per-monitor window geometry, and "keep playing when the window loses focus" vs.
"pause on minimize" as a user choice.

### 6.3 Continue-watching semantics

Save position every 5 seconds and on every pause/stop/close. Mark an item watched at 92% (movies) or
95% (episodes). Offer "Resume / Start over" when reopening. Sync progress across profiles never —
progress is per profile, always.

🚩 **Milestone gate: an in-app player that reliably plays live TS, HLS VOD, and MKV with AC-3 audio,
with working subtitle and audio track switching, before any UI polish begins.**

---

## 7. Live TV — Make It Feel Like Cable

This is the section to over-deliver on. The target feeling is a high-end cable/satellite set-top box:
instant, dense, information-rich, navigable without looking at a mouse.

### 7.1 The full-screen EPG grid (the centerpiece)

- Classic cable layout: **channels down the left, time across the top**, 30-minute columns, programme
  blocks sized proportionally to duration.
- A **"now" line** that moves in real time, with past time dimmed.
- Horizontal scroll through time (30 min per step, smooth-animated), vertical scroll through
  channels, with the channel column pinned and the time header pinned.
- **Jump controls:** Now, +/- 1h, +/- 12h, +/- 24h, "Prime Time" (8pm) for any of the next 7 days,
  and a day picker.
- **Live video preview keeps playing in a panel while you browse the guide** — either as a
  picture-in-picture tile in the corner or a split layout. This is the single most cable-like detail;
  do not skip it.
- Selecting a block shows an **info pane**: title, episode title, S/E, time range, progress, full
  synopsis, genre, year, cast, age rating, and `NEW`/`LIVE`/`HD`/`4K`/`CC`/`Stereo` badges.
- Actions on a block: **Watch** (if now), **Record**, **Record series**, **Remind me**, **Watch from
  start** (if the channel supports catch-up), **Search this title**, **More info**.
- Colour-code by genre (sports, movies, news, kids, etc.) with a legend, toggleable.
- **Virtualized in both axes.** Must scroll at 60 fps with 5,000 channels × 7 days loaded.
- Filters: All / Favorites / a category / HD only / Currently airing / Hide channels without EPG.
- **Search within the guide:** type to find a programme across the next 7 days, grouped by day.

### 7.2 Channel surfing (the set-top-box feel)

- `Ch +` / `Ch −` zaps immediately with a **channel banner** overlay: number, logo, name, current
  programme with a progress bar and time range, and "Next: …".
- **Type a channel number** on the number keys → a digit-entry overlay appears (`2 _ _`), auto-tunes
  after a short timeout or on Enter.
- **Last-channel toggle** (`Backspace`) flips between the two most recent channels.
- `Info` shows the full info panel for what's on now; pressing it again shows what's next.
- **Mini-guide:** a single-row overlay strip at the bottom that you can scroll through channels with
  while the current channel keeps playing — arrow up/down previews the next channel's info without
  tuning; Enter tunes.
- Zap history and a "recently watched channels" rail.

### 7.3 Channel list & organization

Grid and list views; channel logos with graceful letter-avatar fallbacks; sort by number, name,
category, or recently watched; **favorites with multiple named lists** ("Sports", "Kids", "Mine");
drag-to-reorder; hide channels; **rename** channels; **renumber** channels; assign a **custom logo**;
move channels between groups; and bulk operations across a multi-select.

**Rules engine** (a genuine time-saver on 10,000-channel playlists): user-defined regex/keyword rules
that auto-hide, auto-group, auto-rename, or auto-favorite on every refresh. Ship useful presets:
*Hide adult*, *Hide non-[language]*, *Hide "24/7" channels*, *Collapse quality duplicates
(FHD/HD/SD of the same channel into one entry with a quality selector)*.

### 7.4 Multi-view / mosaic

2×2, 3×3, 1+3, and 1+5 layouts. Independent channel per tile, one tile has audio focus (click or
`1–9` to switch), click a tile to promote it to fullscreen, save and name layouts, and an explicit
warning when the layout would exceed the provider's connection limit.

### 7.5 Catch-up / Archive TV

Where the provider supports it (`catchup`, `catchup-source`, Xtream timeshift): show a **"Catch-up"**
marker on channels, let the user browse the past 7 days in the same guide grid, and play any past
programme with full seek. Include "Watch from the start" on a programme that's already airing.

### 7.6 Timeshift / Pause live TV

Configurable on-disk ring buffer (default 1 GB / 30 min, up to 10 GB). Pause live TV, rewind, and
fast-forward back to live. Show a live-edge indicator on the scrub bar and a "Back to live" button.

### 7.7 Recording (DVR)

- Record the current channel instantly (`R`), with a duration prompt defaulting to "until this
  programme ends + 5 min padding."
- Schedule from any guide block; **series rules** ("record all episodes", "new episodes only",
  "this channel only", "weekdays at this time").
- Configurable pre/post padding, conflict detection with resolution UI (respecting connection
  limits), storage quota with an auto-delete policy (oldest / already-watched first), and a
  recordings library with thumbnails, resume, rename, and delete.
- Record via **stream copy / remux to MKV or TS** with FFmpeg — never re-encode.
- Continue recording when the app is minimized to tray; optionally **prevent sleep** while recording;
  show a persistent recording indicator and a toast on start/finish/failure.

### 7.8 Reminders

Set from the guide, fire as a Windows toast notification (configurable lead time), with "Watch now"
and "Snooze" actions, and an optional auto-tune.

### 7.9 Radio channels

Detect `radio="true"` entries and playlist radio groups; give them a dedicated section with an
audio-visualizer or station-art view, and keep playback alive while browsing elsewhere.

### 7.10 Live events / sports

An "On Now" and "Starting Soon" rail built from EPG categories, a sports-specific view grouped by
league/competition where EPG data allows, and team/keyword alerts ("notify me whenever *Liverpool*
appears in the guide").

### 7.11 Stream quality & health

An unobtrusive indicator showing resolution, FPS, and bitrate; a health dot (green/amber/red) driven
by dropped frames and buffer underruns; a per-channel "known issues" memory; and a
**"Test all channels"** diagnostic that probes a category and reports dead streams.

### 7.12 On-screen clock and channel bug

Optional persistent clock and current-channel logo overlay, positioned per user preference.

### 7.13 Channel-change animation

A very short (≤150 ms) fade/black frame on zap instead of a jarring cut — this is what makes zapping
feel like a TV rather than a web page.

### 7.14 Stream failover

Where the same logical channel exists on multiple URLs or providers, store all of them and fail over
automatically on error, with a manual "Try another source" in the OSD.

---

## 8. Movies — Make It Feel Like Netflix

### 8.1 The home screen

- **Hero billboard** at the top: full-bleed backdrop, the title's **logo artwork** (not text) where
  available, a one-line "why you're seeing this" tag, metadata row (year · rating · runtime · genres),
  a two-line truncated synopsis, and `▶ Play` / `+ My List` / `ⓘ More Info` buttons.
- After ~2 seconds, the hero **auto-plays a muted trailer or preview** with a gentle crossfade and a
  mute/unmute toggle; motion-sensitive users can disable this globally.
- Hero rotates through 5–8 picks; manual arrows and pause-on-hover.

### 8.2 Rails (horizontal rows)

Required rails, each lazily loaded and each with its own "see all" page:

`Continue Watching` (with progress bars and an "✕ remove" affordance) · `My List` ·
`Recently Added` · `Trending Now` · `Top 10 in Movies Today` (with the big numeral treatment) ·
`Because You Watched <X>` · `Watch It Again` · per-genre rails · `Critically Acclaimed` (by rating) ·
`4K / HDR` · `New Releases (this year)` · `Hidden Gems` (high rating, low popularity) ·
`From Your Provider's Categories` (mirror the provider's own groupings) · `Short & Sweet (<90 min)`
· `Franchises & Collections`.

Rail behaviour: smooth paginated horizontal scroll with visible arrows on hover, keyboard navigation
that keeps the focused card in view, endless-scroll loading of additional pages, and a subtle
"progress through the rail" indicator.

### 8.3 Cards and hover preview

- Poster cards in a rail, with a **hover state that scales the card up (~1.35×), lifts it above its
  neighbours, pushes siblings aside**, and reveals a quick-action bar: `▶` `+` `👍` `⌄ more`.
- After ~700 ms of hover, the enlarged card **plays a muted preview** (trailer where available,
  otherwise the first N seconds of the film, otherwise a Ken Burns pan on the backdrop).
- The expanded card shows: match score, age rating, runtime, quality badges (4K/HDR/CC), and up to
  three genre tags.
- Focus (keyboard/remote) must produce the identical expansion — this is not a mouse-only feature.

### 8.4 Detail view

A Netflix-style expanded modal (and a deep-linkable full page):

- Backdrop with a gradient scrim, title logo, play/resume CTA with "Resume from 34:12", + My List,
  like/dislike, share (copy deep link), and download *(Optional, §8.6)*.
- Metadata: year, certification, runtime, quality badges, audio/subtitle language pills, TMDB/IMDb
  rating, and a computed **match %** from the user's viewing history.
- Synopsis, cast (horizontally scrollable with headshots, clickable → filmography within the
  library), director, writers, and genre chips (clickable → filtered browse).
- Tabs: **Trailers & More**, **More Like This** (a grid of similar titles present in *this* library),
  and **Details** (full technical info: container, codecs, resolution, bitrate, source URL host).
- Source selector when the same movie exists at multiple qualities or from multiple providers.

### 8.5 Browse, filter, sort

A dedicated browse page with: genre, year range, rating range, certification, language, quality
(SD/HD/FHD/4K), provider, watched/unwatched, and "in My List" filters; sorting by recently added,
title, year, rating, runtime, and popularity; grid-density control; an A–Z jump bar; and a saved-view
feature ("Unwatched 4K sci-fi from the last 5 years").

### 8.6 Downloads *(Optional but recommended)*

A download manager for VOD: queue, concurrent limit, pause/resume, bandwidth cap, "download while I
sleep" scheduling, a downloads library that plays offline, and auto-delete-after-watching.

---

## 9. Series — Netflix, But For Television

- Show page with backdrop, logo, seasons dropdown/tabs, and a full episode list.
- **Episode rows**: thumbnail (with a play overlay and a resume progress bar), number + title,
  runtime, air date, synopsis, and `NEW` badges for episodes added since the user's last visit.
- **Next Episode autoplay:** when an episode ends, a corner card counts down 10 seconds to the next
  episode over the tail of the current one, with "Play now" and "Cancel."
- **Skip Intro / Skip Recap / Skip Credits** buttons where chapters exist or where a
  black-frame/silence heuristic can detect them; remember per-show whether the user always skips.
- **Up Next** rail on home: the exact next unwatched episode of every show in progress, sorted by
  most recently watched.
- Mark season/show watched or unwatched; per-episode watched toggles.
- New-episode notifications for shows in My List, surfaced in the What's New feed and optionally as
  toasts.
- Handle the messy reality: absolute-numbered anime, specials (season 0), multi-part episodes,
  episodes with no metadata, and shows split across multiple provider categories.
- Shuffle play ("play a random unwatched episode") for sitcom-style comfort viewing.

---

## 10. Search & Discovery

- **Instant search** (`Ctrl+F` or `/`) with sub-100ms results from FTS5 as you type.
- Unified results grouped into **Live Channels · On Now (EPG) · Upcoming (EPG) · Movies · Series ·
  Episodes · People · Recordings**, with a count per group and keyboard navigation between groups.
- Fuzzy and typo-tolerant matching; diacritic-insensitive; matches on alternate titles.
- Search by person ("Tom Hanks") returns their filmography as it exists in the library.
- Recent searches, clearable; trending/suggested searches from the library.
- **Voice search** *(Stretch)* using Windows speech APIs.
- **Command palette** (`Ctrl+K`): jump to any channel, movie, show, setting, or action by name.

---

## 11. Profiles, Parental Controls & Personalization

- Up to 6 profiles with avatars (pick from a set, or use an image file) and per-profile everything:
  favorites, My List, watch progress, history, recommendations, settings, and UI theme.
- Optional **profile PIN**; a **Kids profile** that filters by age certification, hides adult and
  unrated categories, disables settings access, and can enforce a daily viewing-time limit.
- **Parental controls**: a master PIN, per-category and per-channel locks, certification threshold,
  "hide locked content entirely" vs. "show but require PIN", and a PIN-protected Settings section.
- **Adult content is hidden by default** on a fresh install and requires explicit opt-in plus the
  master PIN to reveal.
- A personalization engine — simple, transparent, local, and explainable: score titles by watched
  genres, cast, and recency; drive the "Because you watched" and match % features; include a
  "Why am I seeing this?" affordance and a "Not interested" action that actually changes results.

---

## 12. Design System & Visual Language

Build an actual design system before building screens — tokens first, components second, screens
third.

- **Tokens:** an 8px spacing scale, a type scale (12/14/16/20/24/32/48/64), a radius scale, elevation
  levels, motion durations (120/200/320/500 ms) and easings, and a semantic color palette.
- **Default theme:** deep near-black (`#0B0B0F`-ish) background, layered dark surfaces, a single
  vivid accent used sparingly, and high-contrast white/grey text. Additional themes: **OLED True
  Black**, **Light**, **High Contrast**, and a user-selectable accent color.
- **Typography:** one clean geometric/neo-grotesque family with a real weight range. Bundle an
  open-licensed font; never fingerprint a streaming service's proprietary type.
- **Imagery:** every poster/backdrop uses a blurhash or dominant-color placeholder, fades in on load,
  and never causes layout shift. Missing artwork gets a designed fallback, not a broken-image icon.
- **Motion:** purposeful and fast. Rail scrolls, card expansions, modal opens, and view transitions
  are animated; nothing takes longer than 320 ms. A global **Reduce Motion** setting (and respect for
  the OS setting) disables hover-preview autoplay, parallax, and non-essential transitions.
- **Focus:** a highly visible focus ring that works on top of video and artwork (dual-tone
  outline). Focus is never lost, never invisible, and always scrolled into view.
- **Empty, loading, and error states are designed, not default.** Skeleton loaders for rails and the
  guide grid; friendly, actionable error screens; a genuinely helpful empty state on first run.
- **Two UI densities:** **Desktop mode** (mouse-first, denser, window chrome) and **TV mode**
  (10-foot UI: larger type, bigger targets, focus-driven navigation, no hover dependencies).
  Auto-suggest TV mode when the app launches fullscreen on a large display.

---

## 13. Quality-of-Life Features

These are what separate a tech demo from software people actually keep installed. **All required.**

**Onboarding**
- A first-run wizard: add provider (Xtream / M3U URL / M3U file / Stalker) → validate credentials
  live with a clear success/failure → choose content types to import → pick EPG source → optional
  TMDB key → create first profile → done. Under 60 seconds.
- Paste-detection: if the user pastes a full Xtream `get.php` URL, parse out host/username/password
  automatically.
- Import from other apps *(Optional)*: TiviMate, IPTV Smarters, and Kodi PVR favorites/settings.

**Everyday use**
- **Global search and command palette** (§10).
- **Sleep timer:** 15/30/45/60/90 min, "end of programme", "end of episode", with a 60-second warning
  and an optional PC-shutdown action.
- **"Are you still watching?"** after 3 consecutive episodes or 2 hours of idle input on live TV
  (configurable, disable-able).
- **Resume last channel/item on launch** (optional), and full session restore after a crash.
- **Startup options:** launch at login, start minimized to tray, start in fullscreen, start on a
  chosen monitor.
- **System tray** with now-playing info, play/pause, next/previous channel, and quick-quit.
- **Global media-key support** (play/pause/stop/next/prev) even when the app isn't focused, plus
  user-configurable global hotkeys.
- **Windows SMTC integration** so the OS media overlay shows artwork and controls.
- **Mini-player** that stays on top while you use other apps.
- **Recently watched** across everything, with a quick-resume row on launch.
- **Statistics dashboard:** total watch time, by day/week/month, top channels, top genres, most
  watched shows, "you've watched 14 hours this week" — with a full reset/disable option.
- **What's New feed** after each library refresh (§4.6).
- **Bandwidth/quality preference:** when a channel or title exists in multiple qualities, pick
  automatically by preference (Highest / 1080p / 720p / Lowest / Ask), with a per-session override.
- **Duplicate collapsing** across providers so the same movie from three sources is one card with a
  source picker.

**Management**
- **Multiple providers active at once** with a unified library and a per-provider color/badge so the
  user knows where something came from; enable/disable a provider without deleting it.
- **Channel & library editor** (§7.3), including bulk edit and undo.
- **Backup & restore:** one-click encrypted export of settings, profiles, favorites, mappings,
  history, and rules to a single file; import on another machine. Optional scheduled auto-backup to a
  chosen folder.
- **Portable mode:** a flag file next to the executable keeps all data local — no registry, no
  `%APPDATA%`.
- **Cache management UI:** show image/metadata/timeshift/recording sizes with individual clear
  buttons.

**Polish**
- Toast notification system with a history panel (nothing important is missable).
- Contextual right-click menus everywhere, and long-press equivalents in TV mode.
- Drag-and-drop an `.m3u`, `.xml`, `.srt`, or video file onto the window to import or play it.
- Deep links: an `auroratv://` protocol for channels, movies, and episodes.
- "Copy stream URL", "Open in external player" as an explicit, opt-in escape hatch for
  troubleshooting only — **never the default path** (C2).
- An in-app **What's New / changelog** after updates.
- Full **localization scaffolding** (i18n from day one, no hard-coded strings) with English shipped
  and RTL layout support proven by a pseudo-locale.

---

## 14. Input: Keyboard, Remote, and Gamepad

### 14.1 Keyboard map (implement all; make every binding remappable in Settings)

| Context | Key | Action |
|---|---|---|
| Global | `Ctrl+K` / `/` | Command palette / search |
| Global | `F11` | Fullscreen |
| Global | `Esc` | Back / exit fullscreen / close modal |
| Global | `Alt+1..5` | Home, Live TV, Guide, Movies, Series |
| Global | `Ctrl+,` | Settings |
| Player | `Space` / `K` | Play-pause |
| Player | `←/→` | ±10 s · `Shift+←/→` ±30 s · `Ctrl+←/→` ±5 min |
| Player | `J` / `L` | ±10 s (video-site muscle memory) |
| Player | `↑/↓` | Volume · `M` mute |
| Player | `,` / `.` | Frame step back/forward |
| Player | `[` / `]` | Speed down/up · `\` reset speed |
| Player | `S` | Cycle subtitle track · `Shift+S` subtitle settings |
| Player | `A` | Cycle audio track · `Shift+A` audio settings |
| Player | `V` | Cycle aspect ratio · `Z` zoom |
| Player | `I` | Info overlay · `I I` full stats |
| Player | `R` | Record · `T` timeshift pause |
| Player | `P` | Picture-in-Picture · `Shift+P` mini-player |
| Player | `F` | Favorite current item |
| Player | `Ctrl+S` | Screenshot |
| Live TV | `PgUp/PgDn` or `Ch+/Ch-` | Channel up/down |
| Live TV | `0-9` | Direct channel entry |
| Live TV | `Backspace` | Last channel |
| Live TV | `G` | Full guide · `Shift+G` mini-guide |
| Live TV | `C` | Channel list overlay |
| Guide | `Arrows` | Navigate · `Enter` watch · `R` record · `Shift+R` series record |
| Guide | `Home` | Jump to now · `PgUp/PgDn` ±24 h |
| Lists | `Type-ahead` | Jump to first match |

Ship a searchable, printable shortcuts cheat sheet in-app (`?` opens it) and in
`docs/SHORTCUTS.md`.

### 14.2 Remote controls

Support the HID keyboard-emulating remotes people actually use (Flirc, MCE/eHome IR receivers, air
mice, Android-TV-style BT remotes) out of the box by mapping their standard keycodes, plus a
**"Learn remote"** wizard that lets a user press each button and bind it. Handle repeat rates
sensibly (holding Ch+ accelerates), and never require a modifier key for a core action.

### 14.3 Gamepad *(Recommended)*

Xbox controller navigation via XInput: D-pad/stick to move focus, A select, B back, X info,
Y search, bumpers to change channel, triggers to seek, Start for the guide.

### 14.4 Touch *(Optional)*

Swipe to change channel, pinch to zoom video, tap to toggle controls, on-screen number pad.

---

## 15. Settings

Organized, searchable (a filter box at the top of Settings), with every setting having a one-line
explanation and a "reset to default." Sections:

**Providers** (add/edit/remove, credentials, connection limit, account status, per-provider
User-Agent/Referer, refresh schedule, enable/disable) · **EPG** (sources, matching UI, coverage
report, refresh cadence, time offset, retention) · **Playback** (hardware decode, buffer presets,
scaling quality, deinterlace, HDR handling, audio device/exclusive/passthrough, normalization,
default speed, autoplay next, skip intro behaviour, resume behaviour) · **Subtitles** (default
language, auto-enable rules, full styling, OpenSubtitles key) · **Interface** (theme, accent,
density, TV/Desktop mode, animations, hover previews, home rail order and visibility, language,
clock) · **Live TV** (default view, zap behaviour, banner timeout, number-entry timeout, mini-guide,
preview-in-guide, favorites behaviour) · **Library** (classification rules, duplicate handling,
rules engine, hidden items, metadata provider + key, artwork preferences) · **DVR** (recording path,
format, padding, quotas, auto-delete, timeshift buffer size and path) · **Profiles & Parental**
(profiles, PINs, certification limits, kids mode, time limits) · **Network** (proxy, timeouts,
retries, concurrent connections, IPv4/IPv6 preference, TLS strictness, bandwidth cap) ·
**Shortcuts** (full remap UI + remote learning) · **Storage** (cache sizes, clear actions, DB
maintenance, backup/restore, portable mode) · **Updates** (channel: stable/beta, auto-check,
auto-install, changelog) · **Privacy** (telemetry opt-in — off by default, history retention, clear
history) · **Advanced** (custom mpv options, log level, developer tools, diagnostic bundle) ·
**About** (version, licenses, third-party attributions).

---

## 16. Performance Budget

Treat these as tests, not aspirations. Measure them in CI on a synthetic library
(10,000 channels / 60,000 movies / 4,000 series / 7-day EPG).

| Metric | Budget |
|---|---|
| Cold start → interactive UI | ≤ 2.0 s |
| Cold start → library fully rendered | ≤ 4.0 s |
| Channel zap → first frame (HTTP-TS) | ≤ 1.5 s |
| VOD open → first frame | ≤ 2.0 s |
| Guide grid scroll | 60 fps sustained, no dropped frames over a 10 s scroll |
| Rail scroll / hover expansion | 60 fps |
| Search keystroke → results painted | ≤ 100 ms |
| Full M3U import (50 MB, ~40k entries) | ≤ 20 s, UI responsive throughout |
| Full XMLTV import (500 MB uncompressed) | ≤ 60 s, incremental, UI responsive |
| Idle memory (library loaded, not playing) | ≤ 500 MB |
| Playing 1080p, memory | ≤ 900 MB |
| Idle CPU (UI visible, not playing) | ≤ 1% |
| 1080p H.264 playback CPU | ≤ 8% on a modern quad-core |
| 4K HEVC playback CPU | ≤ 15% with hardware decode |
| Installer size | ≤ 150 MB |

**Techniques that are required, not optional:** virtualization for every list and the guide grid;
image lazy-loading with an IntersectionObserver and a decode queue; a disk image cache with WebP
re-encoding and size variants; incremental/streaming parsers; SQLite prepared statements, indices,
and batched transactions; debounced search; background work on worker threads with priority; and
release-build profiling with flame graphs checked into `docs/perf/`.

---

## 17. Reliability, Error Handling & Diagnostics

- **Never show a raw exception.** Every error maps to a human sentence, a likely cause, and at least
  one action. ("This channel didn't respond. It may be temporarily offline, or your provider's
  connection limit may be reached. [Retry] [Try another source] [Report]")
- Distinguish and handle separately: DNS failure, TLS failure, HTTP 401/403 (bad credentials or
  expired account), 404 (dead stream), 429, 5xx, connection-limit rejection, timeout, unsupported
  codec, and encrypted/DRM stream.
- **Offline mode:** when the network drops, the app stays usable — browse the cached library, view
  the EPG, watch recordings and downloads — with a clear offline banner and automatic recovery.
- **Watchdog:** if the player hangs (no frames for N seconds while playing), recover automatically
  and log it.
- **Crash handling:** a crash reporter that writes a local minidump, restores the session on next
  launch, and offers (never auto-sends) a redacted report.
- **Structured logging** with levels, rotation, and a size cap. **Credentials, tokens, and full
  stream URLs are redacted at the logging layer**, not at the call site.
- **Diagnostics panel:** service health, last refresh times, DB size and row counts, cache sizes,
  network throughput, EPG coverage, failed streams, and a **"Create diagnostic bundle"** button that
  zips redacted logs + system info + settings for support.
- Built-in **stream tester** (probe a URL and report codecs, bitrate, and latency) and
  **connection speed test**.

---

## 18. Security & Privacy

- Credentials stored via **Windows DPAPI** / Credential Manager, never plaintext, never in the
  SQLite file, never in exported backups unless the user explicitly opts in **and** supplies a
  passphrase (then AES-256-GCM with a KDF).
- **Certificate validation is always on.** Offer a per-provider "allow self-signed" toggle that
  requires an explicit, scary confirmation and is remembered per host — never a global "ignore TLS."
- Sanitize everything from playlists before it reaches a shell, FFmpeg argv, a file path, or the
  WebView: no command injection, no path traversal, no `file://`/`javascript:` injection into the UI
  layer. Treat all provider-supplied text and images as untrusted.
- Strict CSP in the WebView; no remote code execution; Node/host APIs exposed only through the
  narrow, typed IPC surface.
- **Zero telemetry by default.** If you add analytics at all, it is opt-in, documented in plain
  language, and never includes stream URLs, provider hosts, credentials, or titles watched.
- Auto-update packages must be **signature-verified** before installation.
- Redact credentials from screenshots taken through the app, and from any URL shown in the UI.

---

## 19. Windows Integration & Distribution

**Integration:** Fluent-ish window chrome with Mica/Acrylic where available; snap-layout support;
per-monitor DPI awareness (v2) with correct behaviour when dragged between mismatched displays; dark
mode following the OS by default; jump list ("Resume last channel", "Open Guide", recent items);
taskbar thumbnail toolbar with transport controls; SMTC now-playing; toast notifications with
actions; `SetThreadExecutionState` to block sleep during playback and recording; power-state
awareness (pause and reconnect cleanly across sleep/resume); network-change awareness; a file
association for `.m3u`/`.m3u8`/`.xspf` and the `auroratv://` protocol; and a Settings > Apps entry
with a clean uninstaller that offers to keep or remove user data.

**Distribution:** MSIX (for Store/enterprise) **and** an NSIS or Inno Setup installer (silent-install
flags for IT), **plus** a portable ZIP. Code-sign the executable and installer (document the process
even if a certificate isn't available). Ship x64 and ARM64 builds. Deliver **delta auto-updates** via
a signed update feed with stable/beta channels, background download, install-on-quit, an in-app
changelog, and a "skip this version" option. Bundle the WebView2 evergreen bootstrapper and detect
its absence gracefully. Build reproducibly in CI (GitHub Actions on `windows-latest`), publishing
artifacts and checksums on every tagged release.

---

## 20. Testing & Quality

- **Unit tests** for every parser (M3U, XMLTV, Xtream JSON, Stalker), the series/episode title
  matcher, the EPG channel matcher, the rules engine, and all time-zone/DST math.
- **Golden-file tests** over a fixture set of deliberately horrible real-world playlists and EPGs:
  BOMs, mixed encodings, truncated files, HTML error pages, duplicate ids, 10-year-old programme
  dates, negative durations, and 500 MB inputs.
- **Integration tests** against a local test origin that serves HLS, MPEG-TS, and MP4 fixtures, plus
  simulated failures: 401, 403, 404, 429, slow-loris, mid-stream disconnect, codec change.
- **E2E/UI tests** (Playwright against the WebView, or WinAppDriver) for the critical journeys: first
  run → add provider → watch a channel; browse → open detail → play → resume; guide → schedule a
  recording → verify the file.
- **Performance regression tests** in CI asserting the §16 budgets; fail the build on regression.
- **Accessibility audit:** keyboard-only traversal of every screen, screen-reader labels on every
  control, contrast checks, and focus-order verification.
- **A manual QA checklist** in `docs/QA_CHECKLIST.md` covering the matrix of: Windows 10 / 11,
  x64 / ARM64, 1080p / 4K / multi-monitor / mixed-DPI, 100% / 150% / 200% scaling, integrated /
  discrete GPU, and keyboard / mouse / remote / gamepad.
- **Soak test:** 12 hours of continuous playback with hourly channel changes; assert no memory growth
  beyond 10% and no handle leaks.

---

## 21. Delivery Plan

Each phase ends with a working app and a written summary. 🚩 = stop for review.

| Phase | Deliverable |
|---|---|
| **0 — Spike** | Prove the architecture: a window with libmpv rendering behind a transparent WebView2, playing a hardcoded stream, with HTML controls on top that actually work. Measure zap time. **Do not proceed until this is solid.** 🚩 |
| **1 — Foundation** | Repo scaffolding, typed IPC, SQLite + migrations, logging, settings store, design tokens, app shell and navigation, CI. |
| **2 — Ingestion** | Xtream + M3U parsing, provider management, first-run wizard, library persistence, classification and series grouping. 🚩 |
| **3 — Player** | Full playback service: all §6 features, OSD, shortcuts, resilience, continue-watching. 🚩 |
| **4 — Live TV** | Channel list, zapping, banner, number entry, favorites, categories, mini-guide. |
| **5 — EPG** | XMLTV ingestion, matching UI, the full guide grid with preview-while-browsing, info panes, reminders. 🚩 |
| **6 — Movies** | Metadata enrichment, home rails, hero billboard, hover previews, detail modal, browse/filter. 🚩 |
| **7 — Series** | Show/season/episode model, episode UI, next-episode autoplay, skip intro, Up Next. |
| **8 — DVR** | Recording, scheduling, series rules, timeshift, catch-up, recordings library. 🚩 |
| **9 — Personalization** | Profiles, parental controls, My List, recommendations, search, command palette, stats. |
| **10 — QoL & polish** | Everything in §13, TV mode, multi-view, PiP/mini-player, remote/gamepad, themes, accessibility, i18n. 🚩 |
| **11 — Hardening** | Performance budgets met, soak tests, error-state pass, diagnostics, security review. |
| **12 — Release** | Installers, code signing, auto-update, docs, user guide, 1.0. 🚩 |

---

## 22. Decisions I Need From You

Ask about these — and only these — before or during Phase 0/1. Propose your recommendation with each
question so a one-word answer is enough:

1. Native stack: **Rust/Tauri** (recommended) or **C#/.NET + WinUI 3**?
2. App name and accent color.
3. Is a TMDB API key available, or should the app be fully functional with provider metadata only?
4. Are Stalker portals, downloads, and casting in scope for 1.0, or deferred?
5. Is a code-signing certificate available for release builds?
6. Store distribution (MSIX/Microsoft Store) required, or installer-only?

---

## 23. Definition of Done

The build is complete when a reviewer can verify **every one** of these:

- [ ] Fresh install on a clean Windows 11 VM completes in under 60 seconds and launches to a first-run wizard.
- [ ] A user can add an Xtream account or an M3U URL and be watching a channel in under two minutes.
- [ ] **Not once, anywhere, does an external player window open.** All video is in-app.
- [ ] Live TV zaps in under 1.5 s and survives a mid-stream disconnect without user intervention.
- [ ] The guide shows 7 days across thousands of channels, scrolls at 60 fps, plays a live preview while browsing, and can schedule a recording that produces a playable file.
- [ ] Channel number entry, last-channel toggle, mini-guide, and the channel banner all work exactly as a cable box would.
- [ ] The Movies home screen is genuinely indistinguishable in *feel* from a premium streaming service: hero with trailer autoplay, hover-expanding cards with previews, rails, detail modal.
- [ ] Series support seasons, resume, next-episode autoplay with countdown, and skip-intro.
- [ ] Search returns results across channels, EPG, movies, series, and people in under 100 ms.
- [ ] The entire app is operable with a keyboard only, and with a remote only.
- [ ] Profiles, PINs, and parental controls work; adult content is hidden by default.
- [ ] Timeshift, catch-up, recording, and reminders all function end to end.
- [ ] All §16 performance budgets pass in CI.
- [ ] A 12-hour soak test shows no memory growth or leaks.
- [ ] No credential appears in any log, export, or screenshot.
- [ ] Every error state shows a human explanation and an action.
- [ ] Installer, portable build, and auto-update are all verified working.
- [ ] `docs/` contains ARCHITECTURE, DECISIONS, SHORTCUTS, USER_GUIDE, TROUBLESHOOTING, and QA_CHECKLIST.

---

## 24. Scope, Legal & Ethical Boundaries

Aurora TV is a **media player**, in the same category as VLC, Kodi, or a DLNA client. It ships with
no content, no channel lists, no credentials, and no provider recommendations, and it has no
built-in way to discover or obtain a subscription. Users bring their own lawfully obtained service.

Therefore, and without exception:

- **Do not** bundle, embed, hard-code, link to, or "helpfully suggest" any playlist, portal,
  provider, credential, or stream URL — not in code, not in docs, not in tests, not as a default, not
  as an example. Test fixtures use synthetic data and `example.com` hosts only.
- **Do not** implement DRM circumvention, key extraction, or decryption of protected streams.
- **Do not** implement credential sharing, account reselling, scraping of paid services, or
  connection-limit evasion.
- **Do** show the provider's own account status, expiry, and connection limit honestly, and **respect
  that limit** rather than working around it.
- **Do** include a first-run notice making the "bring your own service" model explicit, and a
  `LICENSE` plus a third-party attributions page covering mpv, FFmpeg, and every bundled dependency —
  including their LGPL/GPL obligations, which you must actually comply with in your packaging.

---

## 25. Final Instruction to the Agent

Start with Phase 0. Prove the video-under-transparent-UI architecture before writing a single rail or
grid — everything in this document depends on it.

Then build outward in the order given, and hold two ideas in tension the whole way: **live TV should
feel like a set-top box from a company with a hardware budget**, and **movies and series should feel
like a streaming service with a design team.** When a decision could go either way, choose the one
that makes the app feel faster, and the one that keeps the user's hands off the mouse.

Ship something you'd actually use every night.
