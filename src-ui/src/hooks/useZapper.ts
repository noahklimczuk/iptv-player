/**
 * Set-top-box behaviours that must work from any screen (README §7.2):
 * channel up/down, direct number entry with a timeout, and the last-channel toggle.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Channel } from '@shared/ipc';
import { invoke } from '@/ipc';
import { report } from '@/lib/errors';
import { useUi } from '@/state/ui';
import { stepIndex } from './zap';

const DIGIT_TIMEOUT_MS = 1800;

export interface ZapperOptions {
  /**
   * Called after any tune, however it was triggered — clicking a channel, Ch+/Ch-,
   * last-channel, or a committed digit entry. The caller uses this to surface the
   * player; routing it through one place is what stops a path (digit entry) from
   * silently tuning with no visible player.
   */
  onTune?: (channel: Channel) => void;
}

/** How a tune should be presented once the stream is opening. */
export interface TuneOptions {
  /**
   * Whether to surface the player.
   *
   * `false` tunes without opening it, which is what the guide's preview panel needs:
   * the picture goes into a box on a page the viewer is still reading, so throwing the
   * full-screen player over it would defeat the point. Everything else about the tune is
   * identical, which is why this is a flag here rather than a second code path — a
   * separate "preview tune" would be a second place for `player.play` to be called
   * wrongly, and that is exactly how digit entry once tuned with no visible player.
   */
  reveal?: boolean;
}

export function useZapper(channels: Channel[], options: ZapperOptions = {}) {
  const onTuneRef = useRef(options.onTune);
  onTuneRef.current = options.onTune;

  const [current, setCurrent] = useState<Channel | null>(null);
  const previous = useRef<Channel | null>(null);
  const digitTimer = useRef<number | undefined>(undefined);
  const { digits, pushDigit, clearDigits, showBanner } = useUi();

  const tune = useCallback(
    (ch: Channel, { reveal = true }: TuneOptions = {}) => {
      if (current && current.id !== ch.id) previous.current = current;
      setCurrent(ch);
      showBanner(ch.id);
      // A channel whose every source is dead used to fail into an unhandled
      // rejection: the banner appeared, the player opened, and nothing ever said
      // why the picture was black.
      invoke('player.play', { kind: 'live', id: ch.id })
        .catch(report(`Could not tune ${ch.name}`));
      if (reveal) onTuneRef.current?.(ch);
    },
    [current, showBanner],
  );

  const step = useCallback(
    (delta: 1 | -1) => {
      const i = current ? channels.findIndex((c) => c.id === current.id) : -1;
      const at = stepIndex(channels.length, i, delta);
      const next = at == null ? undefined : channels[at];
      if (next) tune(next);
    },
    [channels, current, tune],
  );

  /** Backspace flips between the two most recent channels. */
  const lastChannel = useCallback(() => {
    const prev = previous.current;
    if (prev) tune(prev);
  }, [tune]);

  /** Commit typed digits to a channel number. */
  const commitDigits = useCallback(async () => {
    const n = Number(digits);
    clearDigits();
    if (!Number.isFinite(n) || n <= 0) return;
    const hit = await invoke('channels.byNumber', { number: n }).catch((e: unknown) => {
      report(`Could not find channel ${n}`)(e);
      return null;
    });
    if (hit) tune(hit);
  }, [digits, clearDigits, tune]);

  useEffect(() => {
    if (!digits) return;
    window.clearTimeout(digitTimer.current);
    digitTimer.current = window.setTimeout(() => void commitDigits(), DIGIT_TIMEOUT_MS);
    return () => window.clearTimeout(digitTimer.current);
  }, [digits, commitDigits]);

  /**
   * One object per meaningful change, not one per render.
   *
   * Everything in here is already stable — four `useCallback`s and a store action — but
   * the literal was not, so every consumer that lists the zapper as a dependency rebuilt
   * on every render of `App`. `App` re-renders on each player heartbeat, which is four
   * times a second while anything is playing, and `handlers` → `useHotkeys` turned that
   * into the global `keydown` listener being removed and re-added at the same rate
   * (R-134).
   */
  return useMemo(
    () => ({ current, tune, step, lastChannel, pushDigit, commitDigits, digits }),
    [current, tune, step, lastChannel, pushDigit, commitDigits, digits],
  );
}
