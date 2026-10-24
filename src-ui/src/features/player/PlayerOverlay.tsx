/**
 * The single OSD used by live TV, movies, and episodes alike (README C3).
 *
 * On Windows the page behind this is transparent and libmpv renders the video; the OSD
 * floats over it. In the browser the same markup renders over a placeholder surface.
 */
import { AnimatePresence, motion } from 'framer-motion';
import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  Episode, PlayerState, SleepAfter, SleepState, TimeshiftWindow,
} from '@shared/ipc';
import { Badge, IconButton } from '@/components/Primitives';
import { Icon } from '@/components/Icon';
import { hasVideoSurface, invoke, onSleepState } from '@/ipc';
import { notify, report } from '@/lib/errors';
import { duration } from '@/lib/format';
import { useClockTime } from '@/hooks/useClockTime';
import { isTyping } from '@/hooks/useHotkeys';

const HIDE_AFTER_MS = 3200;

/**
 * How much one wheel notch moves the volume.
 *
 * The five the keyboard already uses for ArrowUp and ArrowDown (`useHotkeys`), so a notch
 * and a key press mean the same thing.
 */
const VOLUME_STEP = 5;

/**
 * The ceiling the host applies anyway (`player::set_volume` does `volume.min(200)`), and
 * the floor `VolumeArgs::volume` being a `u32` requires.
 *
 * Clamped here rather than left to the host because the floor is not merely tidiness:
 * sending a negative volume is refused by serde with "invalid value: integer `-5`,
 * expected u32", and the `.catch` turns that into a notification — so scrolling down at
 * zero would raise an error toast for an action that had already finished (R-85).
 */
const MIN_VOLUME = 0;
const MAX_VOLUME = 200;

/**
 * How much accumulated wheel travel spends one notch.
 *
 * A mouse wheel sends one event of about 100px per detent; a trackpad sends a stream of
 * small ones, and a precision mouse sends many smaller still. Acting on each event would
 * make one two-finger drag slam the volume from silence to 200, so travel is accumulated
 * and spent a notch at a time. Fifty is half a detent, which keeps a real wheel feeling
 * immediate — one click, one step — while a trackpad has to travel a sensible distance.
 */
const WHEEL_NOTCH_PX = 50;

/**
 * One wheel event's travel in pixels, whatever units it arrived in.
 *
 * `deltaMode` is lines or pages on some browsers and devices rather than pixels, and the
 * raw number means something completely different in each — three lines and three pixels
 * are not comparable. Firefox on Windows reports lines for an ordinary mouse wheel.
 */
function wheelPixels(e: WheelEvent): number {
  if (e.deltaMode === 1) return e.deltaY * 16;  // lines
  if (e.deltaMode === 2) return e.deltaY * 400; // pages
  return e.deltaY;
}

/**
 * Whether this wheel event was aimed at something other than the picture.
 *
 * Two cases, and the first was the one that mattered. The overlay covers the whole
 * window, so the wheel reaches the transport bar, the channel banner and the audio,
 * subtitle and playback panels as readily as it reaches the video — and over a *control*
 * it must do nothing. Scrolling the playback panel and watching the volume drop is not
 * a wheel shortcut, it is a wheel that goes off wherever you point it.
 *
 * The second is a panel with more content than height, which has its own scrolling to do.
 * Asked of the element rather than of a list of selectors, so a panel that overflows
 * later is covered without anybody remembering it.
 */
function awayFromThePicture(target: EventTarget | null, stopAt: HTMLElement): boolean {
  let node = target instanceof Element ? target : null;
  while (node && node !== stopAt) {
    if (node instanceof HTMLElement && node.dataset.osdChrome !== undefined) return true;
    if (node.scrollHeight > node.clientHeight) {
      const overflow = getComputedStyle(node).overflowY;
      if (overflow === 'auto' || overflow === 'scroll') return true;
    }
    node = node.parentElement;
  }
  return false;
}

/**
 * How long to record when the channel has no guide.
 *
 * A provider with no EPG is common, and "there is nothing in the guide so you cannot
 * record" is a worse answer than recording the hour somebody is actually watching.
 */
const UNGUIDED_RECORDING_SECS = 60 * 60;

/**
 * How much of a programme has to have gone before the notice says so.
 *
 * A minute, because pressing Record as something begins is the ordinary case and
 * "Recording the last 60 minutes of Match of the Day" is a stranger sentence than
 * "Recording Match of the Day" for the same thing.
 */
const PART_GONE_SECS = 60;

/** Speeds the transport offers. Beyond this, pitch correction stops being convincing. */
const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 2] as const;

/** README §13's sleep-timer durations, in minutes. */
const SLEEP_MINUTES = [15, 30, 45, 60, 90] as const;

/** How long "Keep watching" adds when the warning is up. */
const SLEEP_EXTEND_MINUTES = 30;

/** README §6.2's aspect choices, in the order a person tries them. */
const ASPECTS: { value: PlayerState['aspect']; label: string; detail?: string }[] = [
  { value: 'auto', label: 'Auto', detail: 'What the stream says' },
  { value: '16:9', label: '16:9', detail: 'Widescreen' },
  { value: '4:3', label: '4:3', detail: 'Older broadcasts' },
  { value: '21:9', label: '21:9', detail: 'Cinematic' },
  { value: 'zoom', label: 'Zoom', detail: 'Fill the screen, crop the edges' },
  { value: 'stretch', label: 'Stretch', detail: 'Fill the screen, distort' },
];

/**
 * Record what is on this channel now.
 *
 * The button had no handler at all, on a screen where the DVR is the obvious thing to
 * want: `dvr.schedule` has been implemented, registered, contract-typed and covered by a
 * scheduler with its own tick since the beginning.
 *
 * The guide decides what gets recorded. With a programme on, it is that programme, by
 * name and to its own end — which is what makes it appear in Recordings as something
 * recognisable rather than as a block of time. Without one, the next hour of the channel,
 * said plainly in the notice so nobody is surprised by where it stops.
 *
 * It records from **now**, and says how much that is. This used to pass the programme's
 * own `start` as `airStart`, with a comment about the timeshift buffer holding the part
 * already shown. The buffer is never consulted: `RecordRequest` carries a url, a
 * destination, a window and two headers, `dvr.rs` fills the window with
 * `(stop - now).max(1)` — a fresh HTTP stream from the moment recording starts — and the
 * recorder has no reference to the buffer at all. So pressing Record fifty minutes into
 * an hour-long programme produced a ten-minute file stored against a one-hour window,
 * which Recordings then drew as `20:00–21:00` beside an honest ten-minute duration, with
 * nothing to explain the gap. Prepending the buffer is a feature; it was not a comment
 * (R-109).
 */
async function recordNow(player: PlayerState): Promise<void> {
  const channelId = player.channelId;
  if (channelId == null) return;
  const now = Math.floor(Date.now() / 1000);

  try {
    // A channel with no guide answers with nulls rather than failing, so this is one
    // call either way.
    const guide = await invoke('epg.nowNext', { channelId }).catch(() => null);
    const programme = guide?.now ?? null;
    // The window that will actually be recorded: from now to the programme's end, or
    // the next hour on a channel with no guide.
    const airStop = programme?.stop ?? now + UNGUIDED_RECORDING_SECS;
    const id = await invoke('dvr.schedule', {
      channelId,
      title: programme?.title ?? player.title ?? 'Recording',
      subTitle: programme?.subTitle ?? null,
      airStart: now,
      airStop,
    });
    if (id == null) {
      notify('That is already being recorded');
      return;
    }
    // How much of the programme is actually going to disk. Only said once some of it
    // has gone: pressing Record as something starts is the ordinary case and does not
    // need arithmetic in the notice.
    const missed = programme ? now - programme.start : 0;
    const minutes = Math.max(1, Math.round((airStop - now) / 60));
    notify(
      programme
        ? missed >= PART_GONE_SECS
          ? `Recording the last ${minutes} minutes of ${programme.title}`
          : `Recording ${programme.title}`
        : `Recording the next hour — this channel has no guide data`,
    );
  } catch (e) {
    report('Could not start recording')(e);
  }
}

/**
 * How close to the live edge still counts as live.
 *
 * Mirrors `aurora_core::timeshift::LIVE_EDGE_SECS`: a live stream is always a few
 * seconds behind its own edge, so "Live" has to mean near enough rather than exactly.
 */
const LIVE_EDGE_SECS = 5;

/** Seconds behind the live edge, or 0 when there is no buffer to be behind in. */
export function behindLive(player: PlayerState): number {
  if (!player.timeshift) return 0;
  const delay = player.timeshift.liveSecs - player.timeshift.positionSecs;
  return delay > LIVE_EDGE_SECS ? delay : 0;
}

export function PlayerOverlay({
  player, onClose, onGuide, onSettings, nextEpisode, onPlayNext, fullscreen, onFullscreen,
}: {
  player: PlayerState;
  onClose: () => void;
  onGuide: () => void;
  /** For the error panel's "Open settings", which is where a bad provider is fixed. */
  onSettings: () => void;
  /** The episode after this one, when a series is playing (README §9). */
  nextEpisode?: Episode | null;
  onPlayNext?: () => void;
  /** Passed in rather than held here: the `f` key is the same switch. */
  fullscreen: boolean;
  onFullscreen: () => void;
}) {
  const [visible, setVisible] = useState(true);
  const [showStats, setShowStats] = useState(false);
  const [panel, setPanel] = useState<'audio' | 'subtitles' | 'playback' | null>(null);
  const hideTimer = useRef<number | undefined>(undefined);

  const bump = useCallback(() => {
    setVisible(true);
    window.clearTimeout(hideTimer.current);
    hideTimer.current = window.setTimeout(() => {
      setVisible(false);
      setPanel(null);
    }, HIDE_AFTER_MS);
  }, []);

  useEffect(() => {
    bump();
    return () => window.clearTimeout(hideTimer.current);
  }, [bump]);

  /**
   * Any key wakes it, which is how a remote reaches this at all.
   *
   * `bump` was wired to `onMouseMove` and `onClick` and nothing else — grepped, it
   * appeared five times in this file and nowhere else in `src-ui` — while keyboard and
   * remote input arrives at a global listener in `useHotkeys`. So 3.2 seconds after the
   * last mouse movement: volume-up changed the volume with no indicator, seeking moved
   * the playhead with no scrub bar, pause paused with no sign of it, and channel-up
   * zapped with no banner. The action happened; the feedback did not.
   *
   * With a mouse that is a small annoyance — one twitch brings it all back. On a
   * television driven by a remote, which README §1 names as the target, there is no
   * pointer to twitch and the OSD is simply unreachable (R-74).
   *
   * `keydown` on the window rather than on this div: the div never has focus, and the
   * keys in question are handled globally. The same `isTyping` guard the hotkeys use,
   * so a search field over the player does not keep waking it.
   */
  useEffect(() => {
    const wake = (e: KeyboardEvent) => {
      if (isTyping(e.target)) return;
      bump();
    };
    window.addEventListener('keydown', wake);
    return () => window.removeEventListener('keydown', wake);
  }, [bump]);

  /**
   * Volume on the wheel, over the picture.
   *
   * A native listener rather than React's `onWheel`, for two reasons. React attaches
   * wheel at the root as a *passive* listener, so `preventDefault` there does nothing but
   * log a warning — and the page behind this overlay must not scroll while somebody is
   * turning the volume up. And a native listener is where `{ passive: false }` can be
   * said at all.
   *
   * The volume is read from the player state through a ref rather than captured, so the
   * listener is registered once for the life of the overlay instead of on every one of
   * the twenty state updates a second playback produces.
   */
  const surface = useRef<HTMLDivElement | null>(null);
  const volume = useRef(player.volume);
  volume.current = player.volume;
  const travel = useRef(0);
  useEffect(() => {
    const el = surface.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (awayFromThePicture(e.target, el)) return;
      e.preventDefault();
      travel.current += wheelPixels(e);
      const notches = Math.trunc(travel.current / WHEEL_NOTCH_PX);
      if (notches === 0) return;
      travel.current -= notches * WHEEL_NOTCH_PX;
      // Up is louder: `deltaY` is negative when the wheel goes away from the viewer.
      const next = Math.min(
        MAX_VOLUME,
        Math.max(MIN_VOLUME, volume.current - notches * VOLUME_STEP),
      );
      // Woken for the same reason every other action wakes it: an action with no
      // feedback is the defect R-74 was about, and the volume readout is in the chrome
      // this shows. Done even when the value did not move -- at 0 or 200 the viewer
      // still needs to see why nothing is happening.
      bump();
      if (next === volume.current) return;
      // Optimistic, so a flick of the wheel does not wait for a round trip before the
      // next notch is computed from the old number.
      volume.current = next;
      invoke('player.setVolume', { volume: next })
        .catch(report('Could not change the volume'));
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [bump]);

  const sleep = useSleepTimer();

  const playing = player.status === 'playing';
  const behind = behindLive(player);
  // Whether there is a real video surface behind this overlay.
  const hosted = hasVideoSurface();
  // ...and whether it currently has anything on it. Not the same question: see
  // `showingPicture`.
  const picture = showingPicture(player);

  return (
    // The keyboard half of this is the `keydown` listener above, on the window.
    //
    // Both rules want a key handler here beside the click, and this div is the wrong
    // place for one: it never takes focus, and the keys that matter are handled
    // globally -- which is what R-74 fixed and what the effect above says in as many
    // words. Named here rather than answered with a `tabIndex` on a full-screen div,
    // which would put a focus stop in front of every control it contains.
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions
    <div
      ref={surface}
      onMouseMove={bump}
      onClick={bump}
      style={{
        position: 'fixed', inset: 0, zIndex: 150,
        // The one place in the app that must let the window through — but only while
        // there is something to let through. Under a native host mpv renders into a
        // child window *behind* the WebView2, and anything painted here covers it,
        // which is the difference between an OSD floating over live video and a
        // gradient with buttons on it.
        //
        // Keyed on `showingPicture` and not on `hosted`, because the two come apart
        // exactly when it matters. A tune takes a second or two to open a stream, and
        // for that second mpv has no frame — so a transparent overlay was a hole
        // through to the desktop, with the OSD floating over the viewer's wallpaper.
        // The same held for a stream that failed, and for the moment after stopping.
        background: picture
          ? 'transparent'
          : 'radial-gradient(ellipse at center, #10101a 0%, #05050a 100%)',
        cursor: visible ? 'default' : 'none',
      }}
    >
      {/* Stand-in for the mpv surface, drawn only when there is no mpv. */}
      {!hosted && (
        <div
          aria-hidden
          style={{
            position: 'absolute', inset: 0, display: 'grid', placeItems: 'center',
            color: 'var(--text-faint)',
          }}
        >
          <div style={{ textAlign: 'center' }}>
            <Icon name="tv" size={64} strokeWidth={1} />
            <div style={{ marginTop: 12, fontSize: 'var(--fs-sm)' }}>
              Video surface (libmpv renders here on Windows)
            </div>
          </div>
        </div>
      )}

      {player.status === 'buffering' && (
        <div
          style={{
            position: 'absolute', top: '50%', left: '50%',
            transform: 'translate(-50%, -50%)', color: 'var(--text)',
          }}
        >
          Buffering…
        </div>
      )}

      {player.error && <ErrorPanel error={player.error} player={player} onSettings={onSettings} />}

      {/* The warning, outside the `visible` gate on purpose.

          Everything else here hides after 3.2 seconds of no input, and the sleep timer's
          whole premise is that there has been no input -- so a warning inside that gate
          would be invisible at exactly the moment it matters. README §13 asks for sixty
          seconds' notice, and notice nobody can see is not notice. */}
      <AnimatePresence>
        {sleep.state.warning && (
          <motion.div
            data-osd-chrome=""
            data-testid="sleep-warning"
            role="alert"
            initial={{ opacity: 0, y: -12 }} animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -12 }} transition={{ duration: 0.18 }}
            style={{
              position: 'absolute', top: 'var(--sp-5)', left: '50%', translate: '-50% 0',
              display: 'flex', alignItems: 'center', gap: 'var(--sp-3)',
              background: 'var(--bg-elevated)', border: '1px solid var(--border-strong)',
              borderRadius: 'var(--r-lg)', padding: 'var(--sp-3) var(--sp-4)',
              boxShadow: 'var(--shadow-3)', zIndex: 2,
            }}
          >
            <span style={{ fontWeight: 600 }}>
              {sleepEnding(sleep.state)} in {sleep.state.remainingSecs ?? 0}s
            </span>
            <IconButton
              label={`Keep watching for another ${SLEEP_EXTEND_MINUTES} minutes`}
              icon="play"
              onClick={() => sleep.extend(SLEEP_EXTEND_MINUTES)}
            />
            <IconButton label="Cancel the sleep timer" icon="close" onClick={sleep.cancel} />
          </motion.div>
        )}
      </AnimatePresence>

      <AnimatePresence>
        {visible && (
          <>
            <motion.div
              data-osd-chrome=""
              initial={{ opacity: 0, y: -16 }} animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -16 }} transition={{ duration: 0.18 }}
              style={{
                position: 'absolute', top: 0, left: 0, right: 0,
                padding: 'var(--sp-4) var(--sp-5)',
                display: 'flex', alignItems: 'center', gap: 'var(--sp-3)',
                background: 'linear-gradient(to bottom, rgb(0 0 0 / 0.75), transparent)',
              }}
            >
              <IconButton icon="chevronLeft" label="Back" onClick={onClose} />
              <div style={{ minWidth: 0 }}>
                <div
                  style={{
                    fontSize: 'var(--fs-lg)', fontWeight: 700, whiteSpace: 'nowrap',
                    overflow: 'hidden', textOverflow: 'ellipsis',
                  }}
                >
                  {player.title ?? 'Nothing playing'}
                </div>
                {player.subtitle && (
                  <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)' }}>
                    {player.subtitle}
                  </div>
                )}
              </div>
              {player.isLive && (
                behind > 0
                  ? <Badge tone="accent">↺ {duration(behind)} behind live</Badge>
                  : <Badge tone="live">● Live</Badge>
              )}
              <div style={{ marginLeft: 'auto', display: 'flex', gap: 6 }}>
                <IconButton
                  icon="info" label="Playback stats"
                  active={showStats}
                  onClick={() => setShowStats((s) => !s)}
                />
                {/* Live only: there is nothing to schedule for a film, which is already
                    a file on the provider's disk rather than something going past. */}
                {player.isLive && player.channelId != null && (
                  <IconButton
                    icon="record"
                    label="Record what's on"
                    onClick={() => recordNow(player)}
                  />
                )}
              </div>
            </motion.div>

            {showStats && player.stats && <StatsOverlay stats={player.stats} />}

            <motion.div
              data-osd-chrome=""
              initial={{ opacity: 0, y: 24 }} animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 24 }} transition={{ duration: 0.18 }}
              style={{
                position: 'absolute', left: 0, right: 0, bottom: 0,
                padding: 'var(--sp-5)',
                background: 'linear-gradient(to top, rgb(0 0 0 / 0.9), transparent)',
              }}
            >
              <Scrubber player={player} />

              <div
                style={{
                  display: 'flex', alignItems: 'center', gap: 'var(--sp-2)',
                  marginTop: 'var(--sp-3)',
                }}
              >
                <IconButton
                  icon={playing ? 'pause' : 'play'} filled active size={46}
                  label={playing ? 'Pause' : 'Play'}
                  onClick={() => invoke(playing ? 'player.pause' : 'player.resume')
                    .catch(report('The player did not respond'))}
                />
                {(!player.isLive || player.timeshift) && (
                  <>
                    <IconButton
                      icon="back10" label="Back 10 seconds"
                      onClick={() => invoke('player.seek', { positionSecs: -10, relative: true })
                        .catch(report('Could not seek'))}
                    />
                    <IconButton
                      icon="forward10" label="Forward 10 seconds"
                      onClick={() => invoke('player.seek', { positionSecs: 10, relative: true })
                        .catch(report('Could not seek'))}
                    />
                  </>
                )}
                {nextEpisode && onPlayNext && (
                  <IconButton
                    icon="skip"
                    label={`Next episode: S${nextEpisode.season} E${nextEpisode.episode}`}
                    onClick={onPlayNext}
                  />
                )}
                <IconButton
                  icon={player.muted ? 'volumeOff' : 'volume'}
                  label={player.muted ? 'Unmute' : 'Mute'}
                  onClick={() => invoke('player.setMuted', { muted: !player.muted })
                    .catch(report('Could not change the volume'))}
                />
                <input
                  type="range" min={0} max={200} value={player.volume}
                  aria-label="Volume"
                  onChange={(e) => invoke('player.setVolume', { volume: Number(e.target.value) })
                    .catch(report('Could not change the volume'))}
                  style={{ width: 110, accentColor: 'var(--accent)' }}
                />
                <span
                  style={{
                    fontSize: 'var(--fs-xs)', color: 'var(--text-faint)', width: 38,
                    fontVariantNumeric: 'tabular-nums',
                  }}
                >
                  {player.volume}%
                </span>

                <div style={{ marginLeft: 'auto', display: 'flex', gap: 6 }}>
                  {player.isLive && (
                    <IconButton icon="grid" label="TV Guide" onClick={onGuide} />
                  )}
                  <IconButton
                    icon="audio" label="Audio track"
                    active={panel === 'audio'}
                    onClick={() => setPanel((p) => (p === 'audio' ? null : 'audio'))}
                  />
                  <IconButton
                    icon="subtitles" label="Subtitles"
                    active={panel === 'subtitles'}
                    onClick={() => setPanel((p) => (p === 'subtitles' ? null : 'subtitles'))}
                  />
                  <IconButton
                    icon="settings" label="Playback settings"
                    active={panel === 'playback'}
                    onClick={() => setPanel((p) => (p === 'playback' ? null : 'playback'))}
                  />
                  <IconButton
                    icon="fullscreen"
                    label={fullscreen ? 'Leave fullscreen' : 'Fullscreen'}
                    active={fullscreen}
                    onClick={onFullscreen}
                  />
                </div>
              </div>

              <AnimatePresence>
                {panel === 'playback' && <PlaybackPanel player={player} sleep={sleep} />}
              </AnimatePresence>

              <AnimatePresence>
                {panel && panel !== 'playback' && (
                  <TrackPanel
                    kind={panel}
                    player={player}
                    onPick={(id) => {
                      invoke(
                        panel === 'audio' ? 'player.setAudioTrack' : 'player.setSubtitleTrack',
                        { trackId: id as number },
                      ).catch(report('Could not switch track'));
                    }}
                  />
                )}
              </AnimatePresence>
            </motion.div>
          </>
        )}
      </AnimatePresence>
    </div>
  );
}

/**
 * Statuses in which mpv has a frame on the surface behind the page.
 *
 * `buffering` counts: mpv holds the last frame while it refills, so the picture is
 * still there. `loading` does not — the stream is being opened and there is nothing
 * behind the page but the desktop.
 */
const HAS_A_FRAME: ReadonlySet<PlayerState['status']> = new Set([
  'playing',
  'paused',
  'buffering',
]);

/**
 * Whether anything is actually being drawn behind the page right now.
 *
 * `hasVideoSurface()` answers "is there a surface at all", which is a property of the
 * build and the platform. This answers "is there a picture on it", which changes
 * several times a minute — and it is the second question that decides whether the app
 * may make itself transparent.
 */
export function showingPicture(player: PlayerState | null): boolean {
  return !!player && hasVideoSurface() && HAS_A_FRAME.has(player.status);
}

/**
 * A range input that seeks when you let go of it, rather than on every value it passes
 * through on the way.
 *
 * Both scrub bars used to be controlled inputs bound straight to the host's reported
 * position, seeking from `onChange`. Two things went wrong with that, and together they
 * are what "scrubbing is broken" means:
 *
 * 1. **Every intermediate value was a seek.** Dragging from one end to the other sent
 *    dozens of them, each a real seek in mpv, each throwing away the demuxer's work.
 * 2. **The handle fought back.** The value came from `player.positionSecs`, which only
 *    moves when the host's heartbeat reports it. Between ticks React re-rendered with
 *    the *old* position, so the handle snapped back under the pointer until a seek
 *    landed — and then jumped again.
 *
 * So: while a drag is in progress the handle shows what the viewer is doing, one seek
 * is sent when they let go, and the host's own position is ignored until it agrees with
 * where it was sent — or until it is clear it never will.
 */
function SeekBar({
  min, max, value, label, accent, onCommit,
}: {
  min: number;
  max: number;
  value: number;
  label: string;
  accent: string;
  onCommit: (seconds: number) => void;
}) {
  const [dragged, setDragged] = useState<number | null>(null);
  const committedAt = useRef(0);

  /*
   * Let go of the handle once the host has caught up -- or once it is clear it will not.
   *
   * The timeout used to be checked only when this effect re-ran, and it re-ran only when
   * `value` or `dragged` changed. `committedAt` is a ref, so `commit()` causes no render
   * of its own. So the four-second escape hatch depended on the host continuing to
   * report a *changing* position, and `playback.rs` deliberately suppresses an emit when
   * the state is identical to the last one:
   *
   *     let mut last = self.last.lock();
   *     if last.as_ref() == Some(&state) { return None; }
   *
   * A seek that was refused on a paused or stalled stream therefore stopped the position
   * moving, nothing re-ran this, and the handle stayed where it had been dropped --
   * showing a position the player was never at, with no way back but dragging it again
   * (R-111).
   *
   * A timer owns the deadline now, so it fires whether or not anything else changes.
   */
  useEffect(() => {
    if (dragged === null || committedAt.current === 0) return;
    if (Math.abs(value - dragged) <= SEEK_SETTLED_SECS) {
      setDragged(null);
      committedAt.current = 0;
      return;
    }
    const left = SEEK_SETTLE_TIMEOUT_MS - (Date.now() - committedAt.current);
    const giveUp = window.setTimeout(() => {
      setDragged(null);
      committedAt.current = 0;
    }, Math.max(0, left));
    return () => window.clearTimeout(giveUp);
  }, [value, dragged]);

  const commit = () => {
    if (dragged === null || committedAt.current !== 0) return;
    committedAt.current = Date.now();
    onCommit(dragged);
  };

  return (
    <input
      type="range"
      min={min}
      max={max}
      value={dragged ?? value}
      aria-label={label}
      onChange={(e) => setDragged(Number(e.target.value))}
      onPointerUp={commit}
      onLostPointerCapture={commit}
      onKeyUp={commit}
      onBlur={commit}
      style={{ flex: 1, accentColor: accent }}
    />
  );
}

/** How close the host has to get before its position is believed again. */
const SEEK_SETTLED_SECS = 2;
/** And how long to wait before believing it anyway. A seek can be refused. */
const SEEK_SETTLE_TIMEOUT_MS = 4000;

function Scrubber({ player }: { player: PlayerState }) {
  // Bound to the 12/24-hour preference, shadowing the pure import (R-136).
  const clockTime = useClockTime();
  // A buffered live stream has a real span to scrub through; an unbuffered one has
  // nothing behind the live edge, and a bar that cannot move is not offered.
  if (player.isLive && player.timeshift) {
    return <TimeshiftBar window={player.timeshift} />;
  }
  if (player.isLive) {
    return (
      <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--sp-3)' }}>
        <div
          style={{
            flex: 1, height: 4, borderRadius: 'var(--r-full)',
            background: 'linear-gradient(to right, var(--live), var(--live))',
          }}
        />
        <span style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>
          Live · {clockTime(Math.floor(Date.now() / 1000))}
        </span>
      </div>
    );
  }
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--sp-3)' }}>
      <span
        style={{
          fontSize: 'var(--fs-sm)', color: 'var(--text-muted)', width: 58,
          fontVariantNumeric: 'tabular-nums',
        }}
      >
        {duration(player.positionSecs)}
      </span>
      <SeekBar
        min={0}
        max={Math.max(1, player.durationSecs)}
        value={player.positionSecs}
        label="Seek"
        accent="var(--accent)"
        onCommit={(positionSecs) =>
          void invoke('player.seek', { positionSecs }).catch(report('Could not seek'))}
      />
      <span
        style={{
          fontSize: 'var(--fs-sm)', color: 'var(--text-muted)', width: 58,
          textAlign: 'right', fontVariantNumeric: 'tabular-nums',
        }}
      >
        {duration(player.durationSecs)}
      </span>
    </div>
  );
}

/**
 * The live-edge scrub bar (README §7.6).
 *
 * The track is the buffer: its left end is the oldest moment still held and its right
 * end is live, so the handle's distance from the right *is* the delay. Both ends move —
 * the right because time passes, the left because the buffer forgets — which is why the
 * readouts come from the host's window rather than from anything kept here.
 */
function TimeshiftBar({ window: w }: { window: TimeshiftWindow }) {
  const delay = Math.max(0, w.liveSecs - w.positionSecs);
  const atLive = delay <= LIVE_EDGE_SECS;
  const span = Math.max(0, w.liveSecs - w.startSecs);

  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--sp-3)' }}>
      <span
        aria-label="Position relative to live"
        style={{
          fontSize: 'var(--fs-sm)', width: 74,
          fontVariantNumeric: 'tabular-nums',
          color: atLive ? 'var(--live)' : 'var(--text)',
          fontWeight: atLive ? 700 : 600,
        }}
      >
        {atLive ? 'Live' : `−${duration(delay)}`}
      </span>

      <div style={{ flex: 1, position: 'relative', display: 'flex', alignItems: 'center' }}>
        <SeekBar
          min={Math.floor(w.startSecs)}
          max={Math.ceil(w.liveSecs)}
          value={w.positionSecs}
          label="Timeshift"
          accent={atLive ? 'var(--live)' : 'var(--accent)'}
          onCommit={(positionSecs) =>
            void invoke('player.seek', { positionSecs }).catch(report('Could not seek'))}
        />
        {/* The live edge itself, so the end of the track reads as "now" rather than as
            the end of a recording. */}
        <span
          aria-hidden
          title="Live edge"
          style={{
            position: 'absolute', right: -1, top: '50%', transform: 'translateY(-50%)',
            width: 3, height: 14, borderRadius: 2, background: 'var(--live)',
            pointerEvents: 'none',
          }}
        />
      </div>

      <span
        style={{
          fontSize: 'var(--fs-xs)', color: 'var(--text-faint)',
          fontVariantNumeric: 'tabular-nums', whiteSpace: 'nowrap',
        }}
      >
        {span > 0 ? `${duration(span)} buffered` : 'buffering…'}
      </span>

      {!atLive && (
        <button
          onClick={() => invoke('player.backToLive')
            .catch(report('Could not return to live'))}
          style={{
            display: 'flex', alignItems: 'center', gap: 6, padding: '5px 12px',
            borderRadius: 'var(--r-full)', cursor: 'pointer', whiteSpace: 'nowrap',
            border: '1px solid var(--live)', background: 'transparent',
            color: 'var(--live)', fontWeight: 700, fontSize: 'var(--fs-xs)',
          }}
        >
          <Icon name="skip" size={13} />
          Back to live
        </button>
      )}
    </div>
  );
}

function TrackPanel({
  kind, player, onPick,
}: {
  kind: 'audio' | 'subtitles';
  player: PlayerState;
  onPick: (id: number | null) => void;
}) {
  const tracks = kind === 'audio' ? player.audioTracks : player.subtitleTracks;
  const active = kind === 'audio' ? player.activeAudioTrack : player.activeSubtitleTrack;

  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: 8 }}
      transition={{ duration: 0.15 }}
      style={{
        position: 'absolute', right: 'var(--sp-5)', bottom: 84, width: 300,
        background: 'var(--bg-elevated)', border: '1px solid var(--border)',
        borderRadius: 'var(--r-lg)', padding: 'var(--sp-3)', boxShadow: 'var(--shadow-3)',
      }}
    >
      <div
        style={{
          fontSize: 'var(--fs-xs)', color: 'var(--text-faint)', fontWeight: 700,
          letterSpacing: '0.06em', marginBottom: 'var(--sp-2)', textTransform: 'uppercase',
        }}
      >
        {kind === 'audio' ? 'Audio track' : 'Subtitles'}
      </div>

      {kind === 'subtitles' && (
        <TrackOption
          label="Off" active={active === null} onClick={() => onPick(null)}
        />
      )}
      {tracks.map((t) => (
        <TrackOption
          key={t.id}
          label={t.title ?? t.language ?? `Track ${t.id}`}
          detail={[t.language, t.codec, t.channels].filter(Boolean).join(' · ')}
          active={active === t.id}
          onClick={() => onPick(t.id)}
        />
      ))}
    </motion.div>
  );
}

/**
 * Speed and aspect ratio, which nothing in the interface could reach.
 *
 * `player.setSpeed` and `player.setAspect` are implemented on the host, carried in the
 * state, handled by the mpv backend — `Aspect::mpv_value` and its panscan fallback exist
 * precisely for this — and no screen had ever called either. The button that should have
 * opened them had no handler.
 */
function PlaybackPanel({
  player, sleep,
}: { player: PlayerState; sleep: ReturnType<typeof useSleepTimer> }) {
  return (
    <motion.div
      data-testid="playback-panel"
      initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: 8 }}
      transition={{ duration: 0.15 }}
      style={{
        position: 'absolute', right: 'var(--sp-5)', bottom: 84, width: 300,
        background: 'var(--bg-elevated)', border: '1px solid var(--border)',
        borderRadius: 'var(--r-lg)', padding: 'var(--sp-3)', boxShadow: 'var(--shadow-3)',
        maxHeight: '60vh', overflowY: 'auto',
      }}
    >
      {/* Live TV has no speed worth changing: the stream arrives at the rate it arrives,
          and playing it at 1.5x means running out of buffer and rebuffering for ever. */}
      {!player.isLive && (
        <>
          <PanelHeading>Speed</PanelHeading>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginBottom: 'var(--sp-3)' }}>
            {SPEEDS.map((speed) => (
              <button
                key={speed}
                aria-pressed={player.speed === speed}
                onClick={() => {
                  invoke('player.setSpeed', { speed }).catch(report('Could not change the speed'));
                }}
                style={{
                  padding: '5px 11px', borderRadius: 'var(--r-full)', cursor: 'pointer',
                  fontSize: 'var(--fs-xs)', fontWeight: 700,
                  border: `1px solid ${player.speed === speed ? 'transparent' : 'var(--border-strong)'}`,
                  background: player.speed === speed ? 'var(--accent)' : 'transparent',
                  color: player.speed === speed ? 'var(--accent-text)' : 'var(--text-muted)',
                }}
              >
                {speed === 1 ? 'Normal' : `${speed}×`}
              </button>
            ))}
          </div>
        </>
      )}

      <PanelHeading>Aspect ratio</PanelHeading>
      {ASPECTS.map((a) => (
        <TrackOption
          key={a.value}
          label={a.label}
          detail={a.detail}
          active={player.aspect === a.value}
          onClick={() => {
            invoke('player.setAspect', { aspect: a.value })
              .catch(report('Could not change the aspect ratio'));
          }}
        />
      ))}

      <div style={{ marginTop: 'var(--sp-3)' }}>
        <PanelHeading>Sleep timer</PanelHeading>
        {sleep.state.armed ? (
          <div style={{ display: 'grid', gap: 6 }}>
            <div data-testid="sleep-armed" style={{ fontSize: 'var(--fs-sm)' }}>
              {sleepEnding(sleep.state)} in {sleepLeft(sleep.state)}
            </div>
            <div style={{ display: 'flex', gap: 6 }}>
              <button
                onClick={() => sleep.extend(SLEEP_EXTEND_MINUTES)}
                style={sleepChip(false)}
              >
                +{SLEEP_EXTEND_MINUTES} min
              </button>
              <button onClick={sleep.cancel} style={sleepChip(false)}>Off</button>
            </div>
          </div>
        ) : (
          <>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
              {SLEEP_MINUTES.map((m) => (
                <button
                  key={m}
                  onClick={() => sleep.set({ minutes: m })}
                  style={sleepChip(false)}
                >
                  {m} min
                </button>
              ))}
            </div>
            {/* The two that depend on what is playing. Offered only where they can be
                answered -- the host refuses them otherwise, with a sentence, and a
                button that exists to produce an error message is worse than no button.
                Live TV has a programme; a film or episode has a runtime. */}
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginTop: 4 }}>
              {player.isLive ? (
                <button onClick={() => sleep.set('endOfProgramme')} style={sleepChip(false)}>
                  End of programme
                </button>
              ) : (
                player.durationSecs > 0 && (
                  <button onClick={() => sleep.set('endOfEpisode')} style={sleepChip(false)}>
                    End of {player.itemKind === 'episode' ? 'episode' : 'film'}
                  </button>
                )
              )}
            </div>
          </>
        )}
      </div>
    </motion.div>
  );
}

/** A chip in the sleep section, matching the speed buttons above it. */
function sleepChip(active: boolean): React.CSSProperties {
  return {
    padding: '5px 11px', borderRadius: 'var(--r-full)', cursor: 'pointer',
    fontSize: 'var(--fs-xs)', fontWeight: 700,
    border: `1px solid ${active ? 'transparent' : 'var(--border-strong)'}`,
    background: active ? 'var(--accent)' : 'transparent',
    color: active ? 'var(--accent-text)' : 'var(--text-muted)',
  };
}

/** "Sleeping", "Quitting", "Shutting down" — what is about to happen, as a verb. */
function sleepEnding(state: SleepState): string {
  switch (state.armed?.action) {
    case 'quitApp':
      return 'Closing Aurora';
    case 'shutDownPc':
      return 'Shutting down';
    default:
      return 'Stopping';
  }
}

/** The countdown, in the largest unit that is still honest. */
function sleepLeft(state: SleepState): string {
  const left = state.remainingSecs ?? 0;
  if (left >= 120) return `${Math.round(left / 60)} min`;
  return `${left}s`;
}

/**
 * The sleep timer, read from the host rather than counted here (README §13).
 *
 * Every number comes off `sleep.state`, which the player heartbeat pushes while a timer
 * is armed. Counting down in the renderer would be the obvious thing and would drift:
 * the deadline was set against the *host's* clock, a backgrounded tab's timers are
 * throttled, and the whole premise of this feature is that nobody is interacting with
 * the window. The host is the one thing still keeping time.
 */
function useSleepTimer() {
  const [state, setState] = useState<SleepState>({
    armed: null, remainingSecs: null, warning: false,
  });

  useEffect(() => {
    let live = true;
    invoke('sleep.state')
      .then((s) => { if (live) setState(s); })
      // Not fatal: no sleep timer is the state this starts in anyway.
      .catch(() => {});
    // `sleep.fired` is deliberately not subscribed to here. The host disarms the timer
    // before acting and emits the empty state in the same tick, so this is already told
    // that nothing is armed -- and a second listener for the same fact is a second place
    // for the two to disagree.
    const off = onSleepState(setState);
    return () => { live = false; off(); };
  }, []);

  const set = useCallback((after: SleepAfter) => {
    invoke('sleep.set', { after, action: 'stopPlayback' })
      .then(setState)
      // The host refuses "end of programme" on a channel with no guide and "end of
      // episode" on a stream with no duration, and it says which in a sentence. Shown
      // rather than swallowed: the alternative is a button that silently does nothing.
      .catch(report('Could not set the sleep timer'));
  }, []);

  const cancel = useCallback(() => {
    invoke('sleep.cancel').then(setState).catch(report('Could not cancel the sleep timer'));
  }, []);

  const extend = useCallback((minutes: number) => {
    invoke('sleep.extend', { minutes })
      .then(setState)
      .catch(report('Could not extend the sleep timer'));
  }, []);

  return { state, set, cancel, extend };
}

function PanelHeading({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        fontSize: 'var(--fs-xs)', color: 'var(--text-faint)', fontWeight: 700,
        letterSpacing: '0.06em', marginBottom: 'var(--sp-2)', textTransform: 'uppercase',
      }}
    >
      {children}
    </div>
  );
}

function TrackOption({
  label, detail, active, onClick,
}: { label: string; detail?: string; active: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      style={{
        display: 'flex', alignItems: 'center', gap: 'var(--sp-2)', width: '100%',
        padding: 'var(--sp-2) var(--sp-3)', borderRadius: 'var(--r-md)', border: 'none',
        background: active ? 'var(--surface-hover)' : 'transparent',
        color: 'inherit', cursor: 'pointer', textAlign: 'left',
      }}
    >
      <Icon
        name="check" size={15}
        style={{ opacity: active ? 1 : 0, color: 'var(--accent)' }}
      />
      <span style={{ flex: 1 }}>
        <span style={{ display: 'block', fontSize: 'var(--fs-sm)', fontWeight: 600 }}>
          {label}
        </span>
        {detail && (
          <span style={{ display: 'block', fontSize: 'var(--fs-xs)', color: 'var(--text-faint)' }}>
            {detail}
          </span>
        )}
      </span>
    </button>
  );
}

/** README §6.2: resolution, codecs, fps, bitrate, dropped frames, buffer, hw decoder. */
function StatsOverlay({ stats }: { stats: NonNullable<PlayerState['stats']> }) {
  const rows: [string, string][] = [
    ['Resolution', stats.resolution ?? '—'],
    ['Video codec', stats.videoCodec ?? '—'],
    ['Audio codec', stats.audioCodec ?? '—'],
    ['Frame rate', stats.fps ? `${stats.fps} fps` : '—'],
    ['Bitrate', stats.bitrateKbps ? `${(stats.bitrateKbps / 1000).toFixed(1)} Mbps` : '—'],
    ['Dropped frames', String(stats.droppedFrames)],
    ['Buffer', `${stats.bufferSecs.toFixed(1)}s`],
    ['Hardware decoder', stats.hwDecoder ?? 'software'],
  ];
  return (
    <div
      style={{
        position: 'absolute', top: 88, left: 'var(--sp-5)',
        background: 'rgb(0 0 0 / 0.78)', border: '1px solid var(--border)',
        borderRadius: 'var(--r-md)', padding: 'var(--sp-3) var(--sp-4)',
        fontFamily: 'ui-monospace, "Cascadia Code", Consolas, monospace',
        fontSize: 'var(--fs-xs)', display: 'grid',
        gridTemplateColumns: 'auto auto', gap: '3px var(--sp-4)',
      }}
    >
      {rows.map(([k, v]) => (
        <div key={k} style={{ display: 'contents' }}>
          <span style={{ color: 'var(--text-faint)' }}>{k}</span>
          <span style={{ color: 'var(--text)' }}>{v}</span>
        </div>
      ))}
    </div>
  );
}

/**
 * README §17: a human sentence, a likely cause, and at least one action.
 *
 * The actions used to be drawn with no `onClick` at all -- not a no-op handler, not a
 * disabled attribute, no handler -- so all four did nothing when pressed. This is the
 * panel that only appears once a stream has already died, so the one screen a viewer
 * reaches while something is wrong offered them four ways out and honoured none (R-110).
 *
 * The host does not pick these actions carelessly: `neterr.rs` chooses them per failure
 * class, with a comment explaining why a 400 gets "Try another source" but not "Report
 * stream as broken" because "the source is not what is wrong". That reasoning was then
 * rendered as four identical dead buttons.
 *
 * The structural reason it was missed is in the signature: `ErrorPanel({ error })` took
 * nothing else, so it had no callback to call and nothing to act on -- the one component
 * in this file that could not act even if it wanted to.
 */
function ErrorPanel({
  error, player, onSettings,
}: {
  error: NonNullable<PlayerState['error']>;
  player: PlayerState;
  onSettings: () => void;
}) {
  const LABELS: Record<string, string> = {
    retry: 'Retry',
    tryAnotherSource: 'Try another source',
    openSettings: 'Open settings',
  };

  /**
   * Ask for the same thing again.
   *
   * Which is what both `retry` and `tryAnotherSource` amount to, and is not a placebo
   * for either. For live, `playback.rs`'s `recover()` has already walked the ranked
   * sources and spent `MAX_ROLLOVERS` by the time this panel appears -- and its own
   * comment says what a deliberate tune then does: "The next deliberate tune re-reads
   * the order, by which time the failures just recorded will have moved the good one to
   * the front." So this really can land on a different source than the one that failed.
   * For a film or an episode there is one address, so the two buttons converge; the host
   * offers both and this does not pretend otherwise.
   */
  const again = useCallback(() => {
    if (player.itemKind == null || player.itemId == null) return;
    void invoke('player.play', {
      kind: player.itemKind,
      id: player.itemId,
      // Where it died, for anything that has a position. Live has none worth keeping:
      // the point of retuning is to get back to the live edge.
      positionSecs: player.isLive ? undefined : Math.max(0, Math.floor(player.positionSecs)),
    }).catch(report('Could not start it again'));
  }, [player.itemKind, player.itemId, player.isLive, player.positionSecs]);

  const run = (action: string) => {
    if (action === 'openSettings') return onSettings();
    return again();
  };

  return (
    <div
      role="alert"
      style={{
        position: 'absolute', top: '50%', left: '50%', transform: 'translate(-50%,-50%)',
        maxWidth: 460, textAlign: 'center', background: 'var(--bg-elevated)',
        border: '1px solid var(--border-strong)', borderRadius: 'var(--r-lg)',
        padding: 'var(--sp-6)', boxShadow: 'var(--shadow-4)',
      }}
    >
      <div style={{ color: 'var(--danger)', marginBottom: 'var(--sp-3)' }}>
        <Icon name="info" size={32} style={{ margin: '0 auto' }} />
      </div>
      <h2 style={{ margin: '0 0 var(--sp-2)', fontSize: 'var(--fs-lg)' }}>{error.message}</h2>
      <p style={{ margin: '0 0 var(--sp-4)', color: 'var(--text-muted)', fontSize: 'var(--fs-sm)' }}>
        {error.cause}
      </p>
      <div style={{ display: 'flex', gap: 'var(--sp-2)', justifyContent: 'center', flexWrap: 'wrap' }}>
        {/* `reportBroken` is deliberately absent: there is no host command behind it, so
            drawing it would be the same dead button in a new form. The host still offers
            the action for `NotFound`, `UnsupportedCodec` and `Dropped`, and the moment
            something implements it this filter is what to delete. */}
        {error.actions.filter((a) => a in LABELS).map((a) => (
          <button
            key={a}
            onClick={() => run(a)}
            style={{
              padding: '8px 16px', borderRadius: 'var(--r-md)', cursor: 'pointer',
              border: '1px solid var(--border-strong)', background: 'var(--surface)',
              color: 'var(--text)', fontWeight: 600,
            }}
          >
            {LABELS[a] ?? a}
          </button>
        ))}
      </div>
    </div>
  );
}
