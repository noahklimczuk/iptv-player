/**
 * Player commands that show their result at once.
 *
 * Reported as "the player controls are very laggy". The commands themselves were not
 * slow -- each one returns the player's new state within a few milliseconds -- but the
 * screen threw that answer away and waited for the host's next state tick, up to a
 * quarter of a second later (`PLAYER_TICK`), before the pause button changed or the
 * mute icon flipped. The volume slider was worse: it is drawn from the player state, so
 * between ticks every drag was put back to the old value and the knob stuttered after
 * the pointer.
 *
 * So every press now does three things:
 *   1. shows its result immediately (`useUi.holdPlayer`), held against ticks that were
 *      read before the command landed, until the host agrees or a moment passes;
 *   2. sends the command;
 *   3. adopts the state the command returns, rather than waiting for the next tick.
 * If the command fails the hold is dropped and the real state read back, so a refused
 * pause never leaves a pause icon on a playing stream.
 */
import type { PlayerState } from '@shared/ipc';
import { invoke } from '@/ipc';
import { report } from '@/lib/errors';
import { useUi } from '@/state/ui';

const current = () => useUi.getState().player;

function adopt(sent: Promise<PlayerState>, failure: string) {
  sent
    .then((s) => useUi.getState().setPlayer(s))
    .catch((e: unknown) => {
      useUi.getState().releasePlayer();
      invoke('player.state')
        .then((s) => useUi.getState().setPlayer(s))
        .catch(() => undefined);
      report(failure)(e);
    });
}

export function pause() {
  useUi.getState().holdPlayer({ status: 'paused' });
  adopt(invoke('player.pause'), 'The player did not respond');
}

export function resume() {
  useUi.getState().holdPlayer({ status: 'playing' });
  adopt(invoke('player.resume'), 'The player did not respond');
}

export function togglePause() {
  if (current()?.status === 'playing') pause();
  else resume();
}

export function setMuted(muted: boolean) {
  useUi.getState().holdPlayer({ muted });
  adopt(invoke('player.setMuted', { muted }), 'Could not change the volume');
}

export function toggleMute() {
  setMuted(!(current()?.muted ?? false));
}

/**
 * Volume, shown at once and sent at most once a frame.
 *
 * A drag of the slider fires an input event for every pixel; each was a round trip, and
 * they queued up behind one another so the sound lagged the knob. Now the latest value
 * wins: whatever the slider says when the frame comes round is what is sent.
 */
let pendingVolume: number | null = null;
let volumeFrame = 0;
export function setVolume(volume: number) {
  const v = Math.min(200, Math.max(0, Math.round(volume)));
  useUi.getState().holdPlayer({ volume: v });
  pendingVolume = v;
  if (volumeFrame) return;
  const send = () => {
    volumeFrame = 0;
    const next = pendingVolume;
    pendingVolume = null;
    if (next == null) return;
    adopt(invoke('player.setVolume', { volume: next }), 'Could not change the volume');
  };
  volumeFrame =
    typeof requestAnimationFrame === 'function'
      ? requestAnimationFrame(send)
      : (setTimeout(send, 16) as unknown as number);
}

export function changeVolume(delta: number) {
  // From the value on screen, which may be ahead of the host's, so held keys and wheel
  // notches add up instead of all starting from the same stale number.
  setVolume((current()?.volume ?? 70) + delta);
}

/** Relative seek: the playhead moves on screen now, the host catches up. */
export function seekBy(secs: number) {
  const p = current();
  if (p && !p.isLive && p.durationSecs) {
    useUi.getState().holdPlayer({
      positionSecs: Math.min(p.durationSecs, Math.max(0, p.positionSecs + secs)),
    });
  }
  adopt(invoke('player.seek', { positionSecs: secs, relative: true }), 'Could not seek');
}

export function seekTo(positionSecs: number, failure = 'Could not seek') {
  const p = current();
  if (p && !p.isLive) useUi.getState().holdPlayer({ positionSecs });
  adopt(invoke('player.seek', { positionSecs }), failure);
}
