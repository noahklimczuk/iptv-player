# The player — what it must do

This is the contract for the player rebuild: the video engine, the host layer that
decides what plays, and the controls. It was written before any of the new code, from
four sources:

- what the current player does (its code, and the 70+ tests that pin it down);
- README §6 (the engine), §7.2–7.6 (surfing, catch-up, timeshift) and §14 (input);
- the bugs that have been reported against it;
- one new requirement: **leaving the player leaves fullscreen.**

Each line has an ID. Every test written for the new player names the IDs it proves, and
the rebuild is not done until every ID has one.

The IPC contract in `shared/ipc.ts` stays the boundary. Its command and event *names*
are kept, so that nothing outside the player has to change. Everything behind them is new.

---

## 1. Engine (`aurora-video`, replaces `aurora-player`)

### Ownership and ordering
- **E1** One thread owns the mpv handle. Every command reaches mpv through a queue, in
  the order sent; nothing else touches the handle.
- **E2** Every load gets a generation number. An event, property change or error from
  an older generation is dropped, never applied to what is now playing.
- **E3** A command never blocks the caller for longer than it takes to queue it, except
  `load`, which returns once mpv has accepted the file (not once it plays).
- **E4** Shutting the engine down stops mpv and joins its thread. A second shutdown is
  harmless. Nothing panics on a dead handle; it returns an error.

### Loading
- **E5** Every per-file option is set on *every* load, empty where there is nothing to
  say. Headers, referrer and auth from one provider never leak to the next file.
- **E6** A User-Agent is always sent: the playlist's, or `AuroraTV/<version>`. An empty
  one is never sent (this provider answers it with 403).
- **E7** Live and VOD use different probe profiles: shallow probing on live TS for fast
  zapping, deeper on VOD for correct track detection.
- **E8** `cache-secs` is never zero; it comes from the buffer preset (1–60 s).
- **E9** A VOD start position is honoured; a live one is ignored. A VOD with no resume
  point starts at 0, not where the previous file was.
- **E10** Empty URLs are refused with an error.
- **E11** Chapters are reported for the current file, and cleared on the next load.

### State
- **E12** State is pushed by mpv property observation, not polled: status, position,
  duration, pause, volume, mute, speed, tracks, active tracks, aspect, picture.
- **E13** Status is one of `idle / loading / buffering / playing / paused / error`.
  `buffering` means the playhead is waiting for data, not merely that a cache exists.
- **E14** `hasPicture` turns true only when mpv has drawn a frame of the current file,
  and false again on every load and on stop.
- **E15** Live streams report no duration and never divide by zero; progress is clamped.
- **E16** Tracks carry language names and titles, not "Track 2".
- **E17** Aspect maps Auto / 16:9 / 4:3 / 21:9 / Stretch / Zoom to mpv values, and
  serialises with the names the UI uses.
- **E18** Stats: resolution, codecs, fps, bitrate, dropped frames, buffer seconds, cache
  bytes, cache file bytes, hardware decoder.

### Seeking
- **E19** VOD seeks clamp to [0, duration].
- **E20** A live stream without a buffer cannot be seeked; with one, seeks clamp to the
  buffered window [oldest, live edge].

### Timeshift (README §7.6)
- **E21** A buffered live stream keeps its history on disk in the timeshift folder, with
  the stored budget. A film is never given a buffer.
- **E22** Turning the buffer off is applied on the next tune; it is not left in force.
- **E23** The window never offers more rewind than the channel has been on.
- **E24** Pausing a buffered live stream puts the viewer behind live. Stopping forgets
  the buffer.

### Stalls (the watchdog)
- **E25** A load that never starts is a failure, after a timeout.
- **E26** A playhead that stops moving while "playing" is a failure.
- **E27** Not failures: a long rewind, rebuffering that ends, resting while paused,
  timestamp jitter. A seek or a phase change restarts the clock.
- **E28** A verdict is given once, not on every tick. A new load forgets the old stream.

### Surface
- **E29** mpv draws into a native child window behind the transparent web view. The
  window can be placed in a rectangle (inlay, PiP corner), shown or hidden, and resized.
- **E30** A build with no video engine says so (`rendersVideo: false`) rather than
  showing an empty window.
- **E31** Frames can be grabbed from the current file for seek previews: BGR→RGB, row
  padding skipped, never enlarged, nonsense sizes refused.

### Other instances
- **E32** PiP, each multi-view tile and seek-preview grabbing use their own instances
  through the same API. Multi-view tiles open per tile and close per tile, so a closed
  tile releases its provider connection.

---

## 2. Host — what plays (`aurora-app`, replaces `playback.rs` and the player commands)

### Starting
- **H1** `player.play` resolves a movie, episode, channel, radio channel or recording to
  a URL, headers and a start position. Hidden channels are still playable.
- **H2** A title that is gone, has no address, or is a recording that cannot be played
  says which, in words. An unknown kind or missing item is an error, never a panic.
- **H3** `player.playCatchup` plays a past programme, seekable, named after the channel.
  A channel without catch-up, a programme that hasn't started, and one past the
  provider's window are three different refusals.
- **H4** **A later tune always wins**, however slowly the earlier one finishes. A stale
  result is never published. Stopping during a slow tune leaves the player stopped.

### Recovery (README §6.1, §7.14)
- **H5** A channel takes its first source, and remembers which source worked; the next
  tune starts from that one.
- **H6** A source that won't load rolls straight to the next. A stream that dies
  mid-programme rolls over by itself. Rollovers are bounded, even with many sources.
- **H7** A channel with one source is retried with backoff (1 s / 3 s / 7 s) rather
  than given up on. A channel that comes back gets its retries again.
- **H8** A film is not a failover session: a refusal is retried; one that is refused
  every time is tried three times and no more; a 404 is asked for once. A film whose
  stream dies is reloaded where it died. Healthy playback forgives earlier failures.
- **H9** A stall counts as one failure against the source, not one per heartbeat.
- **H10** No recovery runs while a tune is in flight.
- **H11** When nothing will play, the failure is reported with a code, a human message,
  the cause, and only the actions that can be carried out (`retry`,
  `tryAnotherSource` for live, `openSettings` for a bad provider).
- **H12** While retrying, the UI shows a non-modal "reconnecting" state, not an error.

### Progress (README §6.3)
- **H13** Position is saved every 5 s and on pause, stop, close and switching title.
  Live TV is never saved.
- **H14** Watched at 92 % (movies) or 95 % (episodes). Progress is per profile.

### Markers and series aids
- **H15** Chapters on the loaded file are turned into Skip Intro / Recap / Credits
  markers once per file.
- **H16** An episode with no chapters still gets Skip Credits from its runtime, and
  Up Next, including on a stream that never reports a duration.
- **H17** A drag past the titles teaches Skip Intro for the show; a ten-second nudge
  teaches nothing.
- **H18** Per-show preferences (auto-skip intro, auto-play next) persist, and auto-skip
  keeps working on the next episode.

### Sleep timer (README §13)
- **H19** 15 / 30 / 45 / 60 / 90 minutes, end of programme (live, from the guide) or
  end of episode/film (from its duration). Nothing to go on arms nothing; a time already
  past is not a deadline.
- **H20** It can be extended and cancelled, it warns in the last minute, and it survives
  the controls hiding. Actions: stop playback (default), quit, shut down — shutdown only
  when chosen by name.

### Picture placement
- **H21** PiP starts off, bottom right; it toggles, moves clockwise, and accepts a named
  corner. A resize keeps it in its corner. Closing the tile stops playback.
- **H22** An inlay (the guide preview) puts the surface in the page's rectangle, trimmed
  to the window; one entirely off the window is refused and falls back to full. PiP wins
  over an inlay. Clearing gives the whole window back.
- **H23** PiP and multi-view never fight over the surface: PiP is refused while a
  mosaic is open.

### Seek preview
- **H24** Nothing is grabbed until playback has run steadily with a picture; a seek
  restarts that wait. Frames are spaced by the length of the title, kept near their
  moment, never offered for another title, and the furthest are dropped first.
- **H25** The bubble shows a picture of what has played (never of time ahead), the
  marker or chapter name, and on live TV how far behind, the clock time and the
  programme that was on.

### Recording
- **H26** Record on live TV schedules the programme on now, by name, starting from now.
  A film offers no Record.

---

## 3. Controls (`src-ui/src/features/player`, replaced)

### Showing and hiding
- **C1** The controls show on any pointer move or key press, and hide after 3.2 s of
  no input. Over the controls themselves they stay 5 s.
- **C2** An open menu stays open while the pointer is on it, and doesn't hide while
  a slider is being dragged.
- **C3** The cursor hides with the controls, and comes back on a move.
- **C4** A key brings the controls back after they've hidden, *and* does what the key
  does. The key that wakes them is not swallowed.
- **C5** Paused content keeps the controls up.

### Transport
- **C6** Play/pause (button, click on the picture, Space, K). Space on a focused button
  presses it once, not twice.
- **C7** Back / forward 10 s (buttons, ← → , J L); 30 s with Shift.
- **C8** A scrub bar with the played part and the buffered part. Dragging previews and
  sends **one** seek on release; a handle held still is not lost.
- **C9** Time shown as elapsed / total for VOD; for live, a LIVE badge, or with a
  buffer, how far behind live with a scrubbable window and a live-edge marker.
- **C10** Back to live button whenever behind live. T pauses live into the buffer, and
  again rejoins it.
- **C11** Next episode button on an episode that has one.

### Audio and picture
- **C12** Volume slider 0–100 % (wheel and ↑ ↓ in steps of 5), mute (button, M).
  Volume and mute persist between titles.
- **C13** Audio track, subtitle track (including Off), speed (0.25–4×) and aspect menus,
  each showing the current choice.
- **C14** Fullscreen (button, F, F11, double-click on the picture).
- **C15** **Leaving the player always leaves fullscreen**: Back, Escape, the Back
  button, stopping, an error that ends playback, a route change, and closing PiP. A
  fullscreen preview taken from the guide returns to the guide un-fullscreened.
- **C16** Escape in fullscreen leaves fullscreen first; a second Escape leaves the player.

### Information
- **C17** Title and subtitle (episode name / programme) at the top.
- **C18** Stats panel (I, or the stats button) with the engine's figures.
- **C19** Channel banner on zap: number, logo, name, programme now with progress and
  time range, and next.
- **C20** Number entry (`2 _ _`) tunes after a pause or Enter. A channel clicked during
  a number lookup is not overridden by it.
- **C21** Channel up/down (PageUp/PageDown), last channel (Backspace).

### Series aids
- **C22** Skip Intro / Recap / Credits appears inside its stretch and is gone outside it;
  pressing it seeks past. "Learned from your skips" is said when the marker was taught.
- **C23** Up Next counts down at the credits, can be cancelled, and Play now starts the
  next episode immediately.

### Errors and loading
- **C24** Loading shows a spinner over the title, not a black screen.
- **C25** An error shows its message and only the actions that work. Retry asks again
  and clears the error; Try another source re-tunes; Open settings goes to Settings.
- **C26** A tune the viewer replaced is not reported as a failure.

### The window and the page
- **C27** The window stays opaque until there is a picture to show through it.
- **C28** Leaving the player paints the page before the picture is taken away.
- **C29** With a surface behind it, nothing in the shell paints over the video; with no
  picture yet, the shell is not a hole. In a browser (no surface) the shell still paints.
- **C30** The guide preview: first click previews, second goes full screen; leaving the
  guide stops its preview; a preview taken full screen keeps playing.

### Input
- **C31** Typing in a text field never triggers a player key.
- **C32** After touching a slider, Space pauses, M mutes, and the arrows seek (not move
  the slider). The guide's arrows never change the volume.
- **C33** All controls are reachable by Tab and by arrow keys between buttons, with a
  visible focus ring, and have accessible names.
- **C34** The key handler is registered once, not on every state change.

---

## 4. Order of work

1. **Engine:** the `aurora-video` crate, tested against a fake mpv for E1–E28 and on
   this machine against the real one.
2. **Host:** the new host layer on the new engine, with H1–H26 as Rust tests. PiP,
   multi-view, seek preview and timeshift move onto it. The old crate and modules are
   deleted.
3. **Controls:** the new UI, C1–C34, as Playwright tests against the mock.
4. **Real line:** the full matrix against the provider on an isolated test profile —
   live, VOD (MKV, MP4), an episode with Up Next, catch-up, timeshift, PiP, multi-view.

Each step is its own PR and merges only on green CI.
