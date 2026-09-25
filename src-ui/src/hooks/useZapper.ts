/**
 * Set-top-box behaviours that must work from any screen (README §7.2):
 * channel up/down, direct number entry with a timeout, and the last-channel toggle.
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import type { Channel } from '@shared/ipc';
import { invoke } from '@/ipc';
import { report } from '@/lib/errors';
import { useUi } from '@/state/ui';

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

export function useZapper(channels: Channel[], options: ZapperOptions = {}) {
  const onTuneRef = useRef(options.onTune);
  onTuneRef.current = options.onTune;

  const [current, setCurrent] = useState<Channel | null>(null);
  const previous = useRef<Channel | null>(null);
  const digitTimer = useRef<number | undefined>(undefined);
  const { digits, pushDigit, clearDigits, showBanner } = useUi();

  const tune = useCallback(
    (ch: Channel) => {
      if (current && current.id !== ch.id) previous.current = current;
      setCurrent(ch);
      showBanner(ch.id);
      // A channel whose every source is dead used to fail into an unhandled
      // rejection: the banner appeared, the player opened, and nothing ever said
      // why the picture was black.
      invoke('player.play', { kind: 'live', id: ch.id })
        .catch(report(`Could not tune ${ch.name}`));
      onTuneRef.current?.(ch);
    },
    [current, showBanner],
  );

  const step = useCallback(
    (delta: 1 | -1) => {
      if (channels.length === 0) return;
      const i = current ? channels.findIndex((c) => c.id === current.id) : -1;
      const next = channels[(i + delta + channels.length) % channels.length];
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

  return { current, tune, step, lastChannel, pushDigit, commitDigits, digits };
}
