import { useEffect, useRef } from 'react';
import type { PlayerState } from '@shared/ipc';
import { invoke } from '@/ipc';

/** How often a position is written while something is playing. */
const SAVE_EVERY_MS = 10_000;

/**
 * Write where this item has got to, if it is the kind of thing worth resuming.
 *
 * Exported because closing the player has to flush before it stops: `player.stop`
 * takes the position back to zero, so a save that runs afterwards has nothing left to
 * record and the viewer loses their place at exactly the moment they meant to keep it.
 *
 * Returns whether anything was written, which the timer uses to avoid rewriting a
 * paused player every ten seconds.
 */
export function saveProgress(p: PlayerState | null, profileId: number): boolean {
  if (!p) return false;
  // Live TV has no position worth resuming and no duration to be a fraction of.
  if (p.isLive || p.itemId == null) return false;
  if (p.itemKind !== 'movie' && p.itemKind !== 'episode') return false;
  if (p.durationSecs <= 0 || p.positionSecs <= 0) return false;

  void invoke('progress.save', {
    profileId,
    kind: p.itemKind,
    id: p.itemId,
    positionSecs: Math.round(p.positionSecs),
    durationSecs: Math.round(p.durationSecs),
    // Losing a position is not worth a notification: it is a thing the viewer did not
    // ask for, and the next tick tries again anyway.
  }).catch(() => {});
  return true;
}

/**
 * Remember where the viewer got to, so Continue Watching has something to continue.
 *
 * `progress.save` was implemented on the host, registered, and given a table with a
 * completed-ratio rule — and called from exactly one place in this repository: the
 * mock transport. So every browser journey showed a full Continue Watching rail while
 * a real machine saved nothing, ever, and the rail could only be empty. The same shape
 * as F-09 and F-12: the mock did the work the host was supposed to.
 *
 * Saved on a timer rather than on every state event, because the player emits one
 * roughly four times a second and each write is a transaction on the one connection
 * the DVR scheduler also uses. Ten seconds is close enough — nobody minds resuming ten
 * seconds early, and it is the interval at which losing the answer stops mattering.
 *
 * Also saved the moment playback stops or the item changes, which is the position that
 * actually matters: the timer will not have fired at the interesting moment.
 */
export function useWatchProgress(player: PlayerState | null, profileId: number) {
  // Read inside the effect without making it a dependency: the state object changes
  // several times a second, and re-running the timer on each one would mean it never
  // fires at all.
  const latest = useRef(player);
  latest.current = player;

  // What was last written, so a paused player is not rewritten every ten seconds and
  // the flush on stop can be skipped when nothing moved.
  const written = useRef<string | null>(null);

  useEffect(() => {
    const save = () => {
      const p = latest.current;
      if (!p) return;
      const stamp = `${p.itemKind}:${p.itemId}:${Math.round(p.positionSecs)}`;
      if (stamp === written.current) return;
      if (saveProgress(p, profileId)) written.current = stamp;
    };

    const timer = window.setInterval(save, SAVE_EVERY_MS);
    return () => {
      window.clearInterval(timer);
      // The last position is the one somebody will come back to.
      save();
    };
  }, [profileId]);

  // Stopping or pausing is the moment worth recording, and the one a ten-second timer
  // is least likely to have caught. Keyed on the status alone: at the instant it
  // changes, `latest` still describes the thing that was playing.
  const status = player?.status ?? null;
  useEffect(() => {
    if (status === 'playing') return;
    saveProgress(latest.current, profileId);
  }, [status, profileId]);
}
