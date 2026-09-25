/**
 * Now and next for the rows actually on screen.
 *
 * Every `ChannelRow` used to mount its own `useCommand('epg.nowNext', …)`. With no
 * virtualiser under it, that was one IPC round trip per channel in the library —
 * 22,121 of them on the subscription in docs/ROADMAP.md, each taking the single
 * writer mutex, and (before the commands moved off it) each on the window's own
 * thread. Painting Live TV was not slow so much as impossible.
 *
 * This asks about a window of ids at a time and remembers what it learned, so
 * scrolling back over rows already seen costs nothing and a fast scroll coalesces
 * into one request per settle rather than one per row passed.
 */
import { useEffect, useRef, useState } from 'react';
import type { Programme } from '@shared/ipc';
import { invoke } from '@/ipc';
import { report } from '@/lib/errors';

export interface NowNext {
  now: Programme | null;
  next: Programme | null;
}

/** How long a scroll has to stop moving before the visible window is fetched. */
const SETTLE_MS = 120;

export function useNowNext(visibleIds: number[]): Map<number, NowNext> {
  const [known, setKnown] = useState<Map<number, NowNext>>(new Map());
  // Ids already asked about, including the ones that came back with nothing — a
  // channel with no guide must not be re-requested on every scroll past it. On a real
  // panel that is most of them: 9,476 carry a guide id and 2,949 have programmes.
  const asked = useRef<Set<number>>(new Set());
  const key = visibleIds.join(',');

  useEffect(() => {
    const wanted = visibleIds.filter((id) => !asked.current.has(id));
    if (wanted.length === 0) return;

    let live = true;
    const timer = window.setTimeout(() => {
      for (const id of wanted) asked.current.add(id);
      invoke('epg.nowNextMany', { channelIds: wanted })
        .then((pairs) => {
          if (!live) return;
          setKnown((prev) => {
            const next = new Map(prev);
            for (const [id, pair] of Object.entries(pairs)) next.set(Number(id), pair);
            return next;
          });
        })
        .catch((e: unknown) => {
          // Ask again next time it scrolls past: a guide that failed once is not a
          // guide that does not exist.
          for (const id of wanted) asked.current.delete(id);
          report('Could not load the guide for these channels')(e);
        });
    }, SETTLE_MS);

    return () => {
      live = false;
      window.clearTimeout(timer);
    };
    // `key` is the identity of the window; the array itself is new every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  return known;
}
