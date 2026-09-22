# Architecture

Implements `README.md` §3. Read `docs/DECISIONS.md` for why.

## Crate / package map

```
src-native/                      Cargo workspace
  crates/aurora-core/            pure domain logic — NO I/O, NO platform deps
    model.rs                     Channel, Movie, Series, Episode, Programme, …
    m3u.rs                       streaming M3U/M3U8 parser
    xmltv.rs                     streaming XMLTV parser (quick-xml pull parser)
    xtream.rs                    Xtream Codes API response models + normalization
    title.rs                     release-tag stripping, title/year extraction
    series.rs                    SxxEyy detection, flat list → show/season/episode
    epg_match.rs                 tvg-id → EPG channel matching (exact → fuzzy → manual)
    rules.rs                     user rule engine (hide/group/rename/favorite)
    classify.rs                  live / movie / series classification
  crates/aurora-db/              SQLite persistence (rusqlite, bundled)
    schema.rs                    versioned forward-only migrations
    repo/                        typed queries per aggregate
  crates/aurora-player/          playback abstraction
    backend.rs                   PlayerBackend trait + NullBackend (all platforms)
    mpv.rs                       #[cfg(windows)] libmpv + child HWND
  crates/aurora-app/             Tauri 2 host: commands, events, service wiring

src-ui/                          React 18 + TS + Vite + Tailwind v4
  src/ipc/                       typed client; Tauri transport or mock transport
  src/features/                  home, movies, series, live, guide, player, settings
  src/components/                design-system primitives
  src/styles/tokens.css          §12 design tokens

shared/ipc.ts                    hand-kept mirror of the Rust IPC contract
```

## The compositing model (README §2.1)

```
Top-level HWND  (Tauri window)
├── mpv child HWND      ← libmpv renders video here, z-order BELOW
└── WebView2 child HWND ← transparent background, z-order ABOVE, owns ALL input
```

The React UI paints with a transparent page background wherever video should show through.
Because WebView2 sits on top and receives every mouse and key event, there is no hit-testing
split-brain: the UI layer handles all input and forwards playback intents over IPC. Video never
appears in a separate window, satisfying README C2.

On resize the host repositions the mpv child HWND to match the WebView2 client rect in the same
`WM_SIZE` handler, so the two never tear apart.

## Threading

- The Tauri main thread owns windows and the event loop only.
- Ingestion, parsing, EPG import, and metadata enrichment run on a Tokio pool.
- The DB is accessed through a single writer connection plus a read pool; writes are batched into
  transactions (README §16 requires a 50 MB playlist to import in ≤20 s without blocking the UI).
- libmpv runs its own threads; its event loop is pumped on a dedicated thread that forwards
  property changes to the UI as Tauri events.

## IPC contract

Commands are request/response (`library.listChannels`, `player.play`, `epg.gridSlice`, …).
Events are push (`player.state`, `ingest.progress`, `library.refreshed`).
State is owned natively and mirrored to the UI; the UI sends intents, never mutations.

## Data flow: a channel zap

```
keypress → UI intent player.play{channelId}
         → aurora-app resolves URL (provider auth, catchup, failover list)
         → aurora-player::MpvBackend.load(url, headers)
         → mpv property events → player.state event → UI banner + OSD
```
