/**
 * The programmes for the guide rows that are actually on screen.
 *
 * `GuidePage` asked for `{ from, to, channelIds: [] }`, and an empty `channelIds` is
 * read by the host as *every channel*. Measured against the live panel — the real 165 MB
 * XMLTV and the real channel list, projected into the exact `GuideSlice` shape the host
 * serialises:
 *
 * ```text
 * channels with an epg id: 1,549 distinct (of 6,699 channels)
 * programmes overlapping a 24h window on a channel we have: 35,576
 *   programmes half of the payload: 13,735,695 bytes = 13.10 MB
 *   channels half:                                      1.72 MB
 * TOTAL one epg.gridSlice reply:                        14.82 MB
 * ```
 *
 * 14.8 MB serialised by serde, pushed through Tauri's IPC, parsed by `JSON.parse` and
 * held in a React state object — and re-fetched on every change to `from`, which is what
 * all eight navigation controls do. Walking forward an hour at a time across an evening
 * was a 14.8 MB round trip per press. The virtualiser mounts about 14 of those 6,699
 * rows, so over 99% of it was parsed and never read (R-130).
 *
 * So: the mounted rows plus overscan, which is the argument that was already there and
 * never used. Same shape as `useNowNext`, and for the same reason — it remembers what it
 * learned, so scrolling back over rows already seen costs nothing and a fast scroll
 * coalesces into one request per settle.
 *
 * The difference from `useNowNext`, and it is the half that one gets wrong (R-91): what
 * is cached is only true of one time window. Changing `from` discards everything,
 * because a programme list for 8pm tells you nothing about 9pm and showing it anyway
 * would be worse than the payload this replaces.
 */
import { useEffect, useRef, useState } from 'react';
import type { Programme } from '@shared/ipc';
import { invoke } from '@/ipc';
import { report } from '@/lib/errors';

/** How long a scroll has to stop moving before the visible window is fetched. */
const SETTLE_MS = 120;

export interface GuideProgrammes {
  /** Keyed by EPG channel id, exactly as the host keys its reply. */
  byEpgId: Record<string, Programme[]>;
  /** True while the first rows of this time window are still coming. */
  loading: boolean;
}

export function useGuideSlice(
  from: number,
  to: number,
  visibleIds: number[],
): GuideProgrammes {
  const [byEpgId, setByEpgId] = useState<Record<string, Programme[]>>({});
  const [pending, setPending] = useState(0);
  // Channel ids already asked about for this window, including the ones that came back
  // with nothing: on a real panel most channels have no guide at all, and a channel
  // with none must not be re-requested on every scroll past it.
  const asked = useRef<Set<number>>(new Set());
  const window_ = `${from}:${to}`;
  const lastWindow = useRef(window_);
  const key = visibleIds.join(',');

  useEffect(() => {
    if (lastWindow.current !== window_) {
      lastWindow.current = window_;
      asked.current = new Set();
      setByEpgId({});
    }
    const wanted = visibleIds.filter((id) => !asked.current.has(id));
    if (wanted.length === 0) return;

    let live = true;
    // Counted once and released once, whether the fetch finishes or the window moves
    // out from under it. Decrementing in both places unconditionally is how a spinner
    // comes to run backwards.
    let counted = true;
    const released = () => {
      if (!counted) return;
      counted = false;
      setPending((n) => n - 1);
    };
    setPending((n) => n + 1);
    const timer = globalThis.setTimeout(() => {
      for (const id of wanted) asked.current.add(id);
      invoke('epg.gridSlice', { from, to, channelIds: wanted })
        .then((slice) => {
          if (!live) return;
          setByEpgId((prev) => ({ ...prev, ...slice.programmes }));
        })
        .catch((e: unknown) => {
          // Ask again next time it scrolls past: a guide that failed once is not a
          // guide that does not exist.
          for (const id of wanted) asked.current.delete(id);
          report('Could not load the guide for these channels')(e);
        })
        .finally(released);
    }, SETTLE_MS);

    return () => {
      live = false;
      released();
      globalThis.clearTimeout(timer);
    };
    // `key` is the identity of the window; the array itself is new every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, window_]);

  return { byEpgId, loading: pending > 0 };
}
