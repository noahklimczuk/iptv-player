/**
 * The player's rules, as plain functions (docs/PLAYER.md §3).
 *
 * Everything here is a decision about what the controls should show or do, kept apart
 * from React so each one can be tested on its own: what kind of timeline a stream has,
 * whether the controls may hide yet, how far a wheel has turned the volume. The
 * components only draw what these say.
 */
import type { PlayerState } from '@shared/ipc';
import { hasVideoSurface } from '@/ipc';

/* ── Timing ─────────────────────────────────────────────────────────────── */

/** C1: the controls hide after this long without input... */
export const HIDE_AFTER_MS = 3200;
/** ...or this long when the pointer is resting on the controls themselves. */
export const HIDE_OVER_CONTROLS_MS = 5000;

/** What keeps the controls on screen, beyond the time since the last input. */
export interface Holds {
  /** Milliseconds since the last pointer move, click or key. */
  idleMs: number;
  /** The pointer is on the controls (the dock, the top bar, a menu). */
  overControls: boolean;
  /** A button or slider is held down: a drag in progress (C2). */
  pressed: boolean;
  /** A menu is open and the pointer is on it (C2). */
  readingMenu: boolean;
  /** Paused content keeps the controls up (C5). */
  paused: boolean;
}

/** C1, C2, C5: whether the controls may hide now. */
export function mayHide(h: Holds): boolean {
  if (h.pressed || h.readingMenu || h.paused) return false;
  return h.idleMs >= (h.overControls ? HIDE_OVER_CONTROLS_MS : HIDE_AFTER_MS);
}

/** How long to wait before asking `mayHide` again. */
export function nextCheckMs(h: Holds): number {
  if (h.pressed || h.readingMenu || h.paused) return 250;
  const limit = h.overControls ? HIDE_OVER_CONTROLS_MS : HIDE_AFTER_MS;
  return Math.max(50, limit - h.idleMs);
}

/** C6, C14: a click this long without a second one is a click, not half a double. */
export const DOUBLE_CLICK_MS = 260;

/* ── Volume ─────────────────────────────────────────────────────────────── */

/**
 * C12: what the host accepts. Past 100 is mpv's software gain (`volume-max` 200), for
 * a stream mastered quietly -- the old player went that far, and the rebuild's 100 was
 * missed ("add back the volume more than 100% in the player").
 */
export const MIN_VOLUME = 0;
export const MAX_VOLUME = 200;
/** C12: one wheel notch, one arrow press. */
export const VOLUME_STEP = 5;

export function clampVolume(v: number): number {
  if (!Number.isFinite(v)) return MIN_VOLUME;
  return Math.min(MAX_VOLUME, Math.max(MIN_VOLUME, Math.round(v)));
}

/** A mouse wheel's notch, in pixels of travel. */
export const WHEEL_NOTCH_PX = 50;

/** A wheel event's travel in pixels, whatever unit the device reported it in. */
export function wheelPixels(deltaY: number, deltaMode: number): number {
  if (deltaMode === 1) return deltaY * 16; // lines
  if (deltaMode === 2) return deltaY * 400; // pages
  return deltaY;
}

/**
 * Turn wheel travel into volume steps, carrying the remainder.
 *
 * A trackpad sends a stream of small deltas; acting on each one would slam the volume to
 * an end. Travel accumulates until it is worth a notch. Up (negative `deltaY`) is louder.
 */
export function wheelVolume(
  carried: number,
  pixels: number,
  volume: number,
): { carried: number; volume: number } {
  const travel = carried + pixels;
  const notches = Math.trunc(travel / WHEEL_NOTCH_PX);
  return {
    carried: travel - notches * WHEEL_NOTCH_PX,
    volume: clampVolume(volume - notches * VOLUME_STEP),
  };
}

/* ── Timelines ──────────────────────────────────────────────────────────── */

/** Within this many seconds of the live edge counts as live. */
export const LIVE_EDGE_SECS = 5;

/** How far behind live the viewer is, in seconds; 0 at the edge or off live. */
export function behindLive(player: PlayerState): number {
  const w = player.timeshift;
  if (!player.isLive || !w) return 0;
  const behind = w.liveSecs - w.positionSecs;
  return behind > LIVE_EDGE_SECS ? behind : 0;
}

/**
 * C9: what the scrub area is for this stream.
 *
 * - `vod`: a film, an episode, a recording or catch-up -- elapsed / total, seekable.
 * - `timeshift`: live with a buffer -- a window from the oldest moment held to live.
 * - `live`: live with nothing held -- a LIVE badge and the programme's progress.
 * - `unknown`: on demand but with no duration yet; nothing to scrub through.
 */
export type Timeline =
  | { kind: 'vod'; position: number; duration: number; buffered: number }
  | { kind: 'timeshift'; start: number; live: number; position: number; behind: number }
  | { kind: 'live' }
  | { kind: 'unknown'; position: number };

export function timeline(p: PlayerState): Timeline {
  if (p.isLive) {
    const w = p.timeshift;
    if (!w) return { kind: 'live' };
    return {
      kind: 'timeshift',
      start: w.startSecs,
      live: Math.max(w.startSecs, w.liveSecs),
      position: Math.min(Math.max(w.positionSecs, w.startSecs), w.liveSecs),
      behind: behindLive(p),
    };
  }
  if (!(p.durationSecs > 0)) return { kind: 'unknown', position: Math.max(0, p.positionSecs) };
  const position = Math.min(Math.max(0, p.positionSecs), p.durationSecs);
  const ahead = p.stats?.bufferSecs ?? 0;
  return {
    kind: 'vod',
    position,
    duration: p.durationSecs,
    buffered: Math.min(p.durationSecs, position + Math.max(0, ahead)),
  };
}

/** C7: whether there is anything to skip back and forward through. */
export function canSkip(p: PlayerState): boolean {
  const kind = timeline(p).kind;
  return kind === 'vod' || kind === 'timeshift';
}

/* ── The picture ────────────────────────────────────────────────────────── */

/**
 * C27, C29: whether mpv has a frame behind the page right now.
 *
 * `buffering` counts, because mpv holds the last frame while it refills; `loading` does
 * not. `hasPicture` as well, because `playing` arrives when the file opens, a moment
 * before anything is drawn. Only with a frame may the page let the window show through.
 */
export function showingPicture(player: PlayerState | null): boolean {
  if (!player || !hasVideoSurface() || !player.hasPicture) return false;
  return player.status === 'playing' || player.status === 'paused' || player.status === 'buffering';
}

/** C24: the stream is being opened, or refilling. */
export function isWaiting(p: PlayerState): boolean {
  return p.status === 'loading' || p.status === 'buffering' || p.reconnecting;
}

/* ── Menus ──────────────────────────────────────────────────────────────── */

/** C13: 0.25x to 4x, which is what the engine accepts. */
export const SPEEDS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 1.75, 2, 3, 4] as const;

export function speedLabel(speed: number): string {
  return speed === 1 ? 'Normal' : `${speed}×`;
}

export const ASPECTS: { value: PlayerState['aspect']; label: string; detail: string }[] = [
  { value: 'auto', label: 'Auto', detail: 'What the stream says' },
  { value: '16:9', label: '16:9', detail: 'Widescreen' },
  { value: '4:3', label: '4:3', detail: 'Older broadcasts' },
  { value: '21:9', label: '21:9', detail: 'Cinematic' },
  { value: 'zoom', label: 'Zoom', detail: 'Fill the screen, crop the edges' },
  { value: 'stretch', label: 'Stretch', detail: 'Fill the screen, distort' },
];

type Track = PlayerState['audioTracks'][number];

export function trackName(t: Track): string {
  return t.title ?? t.language ?? `Track ${t.id}`;
}

/** C13: what each menu is set to now, in words. */
export function currentChoices(p: PlayerState): { subtitles: string; audio: string | null } {
  const sub = p.subtitleTracks.find((t) => t.id === p.activeSubtitleTrack);
  const audio = p.audioTracks.find((t) => t.id === p.activeAudioTrack);
  return {
    subtitles: sub ? trackName(sub) : 'Off',
    audio: audio ? trackName(audio) : null,
  };
}

export const SLEEP_MINUTES = [15, 30, 45, 60, 90] as const;
export const SLEEP_EXTEND_MINUTES = 30;
