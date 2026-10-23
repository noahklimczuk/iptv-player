/**
 * A remote image URL, swapped for the copy on disk when the host has one.
 *
 * `aurora-ingest::artwork` has downloaded, evicted and reported for a long time, and
 * nothing read from it: every poster in a hundred-thousand-row library was fetched from
 * the network on each paint (checklist item 7). This is the half that was missing.
 *
 * Two rules it does not break, both from that item:
 *
 * 1. **It always starts from something that works.** The remote URL is returned
 *    immediately and only replaced once the host confirms a local copy, so a cold
 *    cache, a scope that was not granted, or a host that refuses the question all
 *    degrade to exactly the behaviour that shipped before the cache was wired up.
 * 2. **It asks about a whole page at once.** A browse grid paints a hundred cards; a
 *    hundred separate round trips would cost more than the downloads being avoided. So
 *    the questions queue and go in one call on the next microtask.
 */
import { useEffect, useState } from 'react';

import { invoke, isNativeHost, onArtworkProgress } from '@/ipc';

/** What the host has already answered for a URL: a local URL, or null for "not here". */
const answered = new Map<string, string | null>();

/**
 * How many answers either table keeps.
 *
 * Both of these were unbounded and module-level, and `watchWarms` deletes only the
 * *absences* -- so every URL that resolved to a local copy was held until the window
 * closed, as was every refusal. One entry per distinct artwork URL, up to two per title
 * with posters and backdrops, so a long session browsing a six-figure library
 * accumulated tens of thousands of string pairs. A few megabytes rather than a leak that
 * matters today, which is why this is a ceiling rather than a rewrite: what it removes is
 * a `Map` with no limit, which is fine until the library is ten times bigger (R-92).
 *
 * Evicted oldest-first, which is insertion order rather than least-recently-used: a
 * `Map` gives that for nothing and an LRU would mean touching the table on every read.
 * The cost of evicting something still on screen is one extra command to re-ask for it,
 * and the answer is the same, so the cache heals itself. 4,096 is about two thousand
 * titles' worth of artwork -- far more than any screenful, and small enough to bound.
 */
const CEILING = 4096;

/** Remember an answer, forgetting the oldest once the table is full. */
function remember(url: string, local: string | null) {
  answered.set(url, local);
  while (answered.size > CEILING) {
    const oldest = answered.keys().next().value;
    if (oldest === undefined) break;
    answered.delete(oldest);
  }
}

/**
 * Images whose local copy would not decode, and which must never be offered again.
 *
 * Without this, `fallBackToRemote` and the warm below argue: the fallback forgets the
 * answer so the picture can come from the network, the next warm re-asks, the host says
 * it is cached, the same unreadable file is set again, and it fails again.
 */
const refused = new Set<string>();

/** Questions waiting to go out together, and who is waiting for each answer. */
const waiting = new Map<string, Set<(local: string | null) => void>>();
let flushQueued = false;

function flush() {
  flushQueued = false;
  const urls = [...waiting.keys()];
  if (urls.length === 0) return;
  const askers = new Map(waiting);
  waiting.clear();

  const settle = (url: string, local: string | null) => {
    remember(url, local);
    askers.get(url)?.forEach((notify) => notify(local));
  };

  invoke('artwork.local', { urls })
    .then((locals) => urls.forEach((url, i) => settle(url, locals[i] ?? null)))
    // A cache that cannot answer is not an error worth showing anyone: every image is
    // already displaying its remote URL and will carry on doing so.
    .catch(() => urls.forEach((url) => settle(url, null)));
}

/**
 * Components to tell when a warm finishes.
 *
 * The host fetches what it did not have and reports through `artwork.progress`, so a
 * screen painted from the network on its first visit swaps to the local copies as they
 * land — and is instant the next time. Without this the answer "not cached" would stand
 * until the component remounted.
 */
const recheckers = new Set<() => void>();
let watchingWarms = false;
let recheckTimer: number | undefined;

function watchWarms() {
  if (watchingWarms) return;
  watchingWarms = true;
  onArtworkProgress(() => {
    // One event per image; re-asking for each would undo the batching above.
    window.clearTimeout(recheckTimer);
    recheckTimer = window.setTimeout(() => {
      // Only the absences are stale. A URL already resolved to a local copy stays.
      for (const [url, local] of [...answered]) {
        if (!local && !refused.has(url)) answered.delete(url);
      }
      recheckers.forEach((recheck) => recheck());
    }, 400);
  });
}

function ask(url: string, notify: (local: string | null) => void): () => void {
  let set = waiting.get(url);
  if (!set) {
    set = new Set();
    waiting.set(url, set);
  }
  set.add(notify);
  if (!flushQueued) {
    flushQueued = true;
    queueMicrotask(flush);
  }
  return () => {
    set?.delete(notify);
  };
}

export function useAssetSrc(remote: string | null | undefined): string | null {
  const [src, setSrc] = useState<string | null>(remote ?? null);

  useEffect(() => {
    if (!remote) {
      setSrc(null);
      return;
    }
    // Whatever happens next, this renders.
    setSrc(remote);
    // In a browser there is no cache and no host to hold one, so asking is meaningless
    // work on every card of every grid — and measurably so: it was enough to change the
    // settling time of the end-to-end journeys and make two of them flaky.
    if (!isNativeHost()) return;
    watchWarms();

    let live = true;
    let stop: (() => void) | undefined;

    const resolve = () => {
      if (!live) return;
      const known = answered.get(remote);
      if (known) {
        setSrc(known);
        return;
      }
      if (refused.has(remote)) return; // the local copy is unreadable; stay remote
      if (answered.has(remote)) return; // known absent; a warm will clear this
      stop?.();
      stop = ask(remote, (local) => {
        if (live && local) setSrc(local);
      });
    };

    resolve();
    recheckers.add(resolve);
    return () => {
      live = false;
      recheckers.delete(resolve);
      stop?.();
    };
  }, [remote]);

  return src;
}

/**
 * What to do when a local copy will not load after all.
 *
 * The file can go between the host answering and the WebView fetching it — eviction
 * runs on a budget, and a viewer may clear the cache from Settings while a page is
 * open. Falling back to the remote URL costs one request and keeps the picture.
 */
export function fallBackToRemote(
  event: { currentTarget: HTMLImageElement },
  remote: string | null | undefined,
) {
  const img = event.currentTarget;
  if (remote && img.src !== remote) {
    refused.add(remote);
    while (refused.size > CEILING) {
      const oldest = refused.values().next().value;
      if (oldest === undefined) break;
      refused.delete(oldest);
    }
    remember(remote, null);
    img.src = remote;
  }
}

/** Test seam: forget what the host has said, so a cleared cache is asked about again. */
export function forgetCachedAnswers() {
  answered.clear();
  refused.clear();
}
