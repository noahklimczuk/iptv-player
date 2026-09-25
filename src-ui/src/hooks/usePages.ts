import { useCallback, useEffect, useRef, useState } from 'react';
import { notify } from '@/lib/errors';

interface Paged<T> {
  items: T[];
  /** A page is in flight. */
  loading: boolean;
  /** Every page has been read — `items.length` is now the real total. */
  done: boolean;
  error: string | null;
  loadMore: () => void;
}

/**
 * Read a list from the host one page at a time, accumulating as it goes.
 *
 * Movies and Series asked for `limit: 120, offset: 0` and stopped there, then printed
 * `items.length` beside the heading — so a library of twenty thousand films announced
 * itself as "120", and the 121st was unreachable by any means. The commands had taken
 * `limit` and `offset` all along; nothing had ever passed a second page.
 *
 * Accumulating rather than re-requesting a bigger window matters at this size: asking
 * for 240, then 360, then 480 re-sends and re-parses everything already on screen, and
 * a channel list of 22,305 rows is 6.3 MB of JSON per request. Each page here is
 * fetched once.
 *
 * `fetchPage` is held in a ref, so a caller may write it inline without every render
 * restarting the list; `deps` is what decides when the list is genuinely different and
 * should start again from the top.
 */
export function usePages<T>(
  fetchPage: (limit: number, offset: number) => Promise<T[]>,
  pageSize: number,
  deps: unknown[],
  label: string,
): Paged<T> {
  const [items, setItems] = useState<T[]>([]);
  const [loading, setLoading] = useState(true);
  const [done, setDone] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchRef = useRef(fetchPage);
  fetchRef.current = fetchPage;

  // Bumped whenever the list changes identity. A page that was already in flight when
  // the genre changed must not append its rows to the new list.
  const generation = useRef(0);
  // Read inside the callback rather than through state, so two scroll events in the
  // same frame cannot both decide they are the one to fetch the next page.
  const inFlight = useRef(false);
  const offset = useRef(0);

  const load = useCallback(() => {
    if (inFlight.current) return;
    inFlight.current = true;
    const mine = generation.current;
    const at = offset.current;
    setLoading(true);
    fetchRef.current(pageSize, at)
      .then((page) => {
        if (mine !== generation.current) return;
        offset.current = at + page.length;
        setItems((prev) => (at === 0 ? page : [...prev, ...page]));
        // A short page is the end. An exactly-full last page costs one more request
        // that comes back empty, which is the price of not having a total.
        if (page.length < pageSize) setDone(true);
        setError(null);
      })
      .catch((e: unknown) => {
        if (mine !== generation.current) return;
        notify(`Could not load ${label}`, e);
        setError(e instanceof Error ? e.message : String(e));
        // Otherwise the sentinel keeps firing against a host that is refusing, turning
        // one failure into an unbounded retry loop the viewer cannot see.
        setDone(true);
      })
      .finally(() => {
        if (mine !== generation.current) return;
        inFlight.current = false;
        setLoading(false);
      });
  }, [pageSize, label]);

  useEffect(() => {
    generation.current += 1;
    inFlight.current = false;
    offset.current = 0;
    setItems([]);
    setDone(false);
    setError(null);
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  const loadMore = useCallback(() => {
    if (done || inFlight.current) return;
    load();
  }, [done, load]);

  return { items, loading, done, error, loadMore };
}
