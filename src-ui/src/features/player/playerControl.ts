/**
 * Every command the controls send, in one place.
 *
 * Each one shows its effect at once (`holdPlayer`), sends the command, and adopts the
 * state the host answers with. The hold stops a heartbeat that was read before the
 * command landed from undoing what the viewer just did. If the host refuses, the hold
 * is dropped, the host's own state is read back, and the viewer is told why.
 */
import type { PlayerState } from '@shared/ipc';
import { invoke } from '@/ipc';
import { report } from '@/lib/errors';
import { useUi } from '@/state/ui';
import { clampVolume } from './model';

const now = () => useUi.getState().player;

function settle(sent: Promise<PlayerState>, failure: string): Promise<void> {
  return sent
    .then((s) => useUi.getState().setPlayer(s))
    .catch((e: unknown) => {
      useUi.getState().releasePlayer();
      invoke('player.state')
        .then((s) => useUi.getState().setPlayer(s))
        .catch(() => undefined);
      report(failure)(e);
    });
}

/* ── Transport ── */

export function pause() {
  useUi.getState().holdPlayer({ status: 'paused' });
  void settle(invoke('player.pause'), 'The player did not respond');
}

export function resume() {
  useUi.getState().holdPlayer({ status: 'playing' });
  void settle(invoke('player.resume'), 'The player did not respond');
}

/**
 * C6. Buffering and loading count as playing: the viewer pressing pause wants it
 * stopped. Loading used to count as paused, so Space while a film opened sent `resume`
 * to something already on its way and did nothing, press after press, until it played.
 * The host opens it paused when asked, and says Paused, so the next press resumes.
 */
export function togglePause() {
  const p = now();
  if (!p || p.status === 'idle' || p.status === 'error') return;
  if (p.status === 'playing' || p.status === 'buffering' || p.status === 'loading') pause();
  else resume();
}

/** C7: relative, so a press is never computed from a stale position. */
export function seekBy(secs: number) {
  const p = now();
  if (!p || p.status === 'idle') return;
  if (!p.isLive && p.durationSecs > 0) {
    useUi.getState().holdPlayer({
      positionSecs: Math.min(p.durationSecs, Math.max(0, p.positionSecs + secs)),
    });
  }
  void settle(invoke('player.seek', { positionSecs: secs, relative: true }), 'Could not seek');
}

/** C8: one absolute seek, sent when a drag is let go. */
export function seekTo(positionSecs: number, failure = 'Could not seek') {
  const p = now();
  if (!p || p.status === 'idle' || !Number.isFinite(positionSecs)) return;
  const to = Math.max(0, positionSecs);
  if (!p.isLive) useUi.getState().holdPlayer({ positionSecs: to });
  void settle(invoke('player.seek', { positionSecs: to }), failure);
}

/** C10. */
export function backToLive() {
  void settle(invoke('player.backToLive'), 'Could not return to live');
}

/* ── Volume (C12) ── */

export function setMuted(muted: boolean) {
  useUi.getState().holdPlayer({ muted });
  void settle(invoke('player.setMuted', { muted }), 'Could not change the volume');
}

export function toggleMute() {
  setMuted(!(now()?.muted ?? false));
}

/**
 * The volume, shown at once and sent at most once a frame.
 *
 * A slider drag produces a value per pixel. Each is shown immediately; only the latest
 * is sent, on the next frame. Turning the volume up while muted unmutes, which is what
 * anybody turning it up wants.
 */
let pendingVolume: number | null = null;
let volumeFrame = 0;
export function setVolume(volume: number) {
  const v = clampVolume(volume);
  const p = now();
  const unmute = !!p?.muted && v > p.volume;
  useUi.getState().holdPlayer(unmute ? { volume: v, muted: false } : { volume: v });
  if (unmute) void settle(invoke('player.setMuted', { muted: false }), 'Could not change the volume');
  pendingVolume = v;
  if (volumeFrame) return;
  const send = () => {
    volumeFrame = 0;
    const next = pendingVolume;
    pendingVolume = null;
    if (next != null) {
      void settle(invoke('player.setVolume', { volume: next }), 'Could not change the volume');
    }
  };
  volumeFrame = typeof requestAnimationFrame === 'function'
    ? requestAnimationFrame(send)
    : (setTimeout(send, 16) as unknown as number);
}

export function changeVolume(delta: number) {
  setVolume((now()?.volume ?? 70) + delta);
}

/* ── Menus (C13) ── */

export function setSpeed(speed: number) {
  useUi.getState().holdPlayer({ speed });
  void settle(invoke('player.setSpeed', { speed }), 'Could not change the speed');
}

export function setAspect(aspect: PlayerState['aspect']) {
  useUi.getState().holdPlayer({ aspect });
  void settle(invoke('player.setAspect', { aspect }), 'Could not change the aspect ratio');
}

export function setAudioTrack(trackId: number) {
  useUi.getState().holdPlayer({ activeAudioTrack: trackId });
  void settle(invoke('player.setAudioTrack', { trackId }), 'Could not switch the audio track');
}

/** `null` is Off. */
export function setSubtitleTrack(trackId: number | null) {
  useUi.getState().holdPlayer({ activeSubtitleTrack: trackId });
  void settle(invoke('player.setSubtitleTrack', { trackId }), 'Could not switch the subtitles');
}

/* ── Errors (C25) ── */

/**
 * Ask for what failed again, from where it failed.
 *
 * Retry and Try another source both come here. For live, the host re-reads the ranked
 * sources on a deliberate tune, and the failures it just recorded have moved the bad one
 * down, so this does land on another source when there is one. The error is cleared at
 * once, so the panel goes when the button is pressed rather than when the stream opens.
 */
export function playAgain(): Promise<void> {
  const p = now();
  if (!p || p.itemKind == null || p.itemId == null) return Promise.resolve();
  useUi.getState().holdPlayer({ error: null, status: 'loading' });
  return invoke('player.play', {
    kind: p.itemKind,
    id: p.itemId,
    positionSecs: p.isLive ? undefined : Math.max(0, Math.floor(p.positionSecs)),
  })
    .then((s) => {
      useUi.getState().releasePlayer();
      useUi.getState().setPlayer(s);
    })
    .catch((e: unknown) => {
      useUi.getState().releasePlayer();
      invoke('player.state').then((s) => useUi.getState().setPlayer(s)).catch(() => undefined);
      report('Could not start it again')(e);
    });
}
