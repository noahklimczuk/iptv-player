import { useCallback, useMemo, useRef, useState } from 'react';
import type { CatalogItem } from '@shared/ipc';
import { AiRail } from './AiRail';
import { HeroBillboard } from '@/components/HeroBillboard';
import { Rail } from '@/components/Rail';
import { EmptyState, Skeleton } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { invoke } from '@/ipc';
import { report } from '@/lib/errors';
import { useUi } from '@/state/ui';
import { useProfile } from '@/state/profile';

export function HomePage({
  onOpen, onPlay,
}: { onOpen: (i: CatalogItem) => void; onPlay: (i: CatalogItem) => void }) {
  const profileId = useProfile((s) => s.active?.id ?? 1);
  // Re-read when the library has moved. Closing the player navigates nowhere, so
  // nothing here remounts — and without this, watching something left Home showing
  // the rails it built before you did.
  const catalogVersion = useUi((s) => s.catalogVersion);
  const { data: rails, loading, error } = useCommand(
    'library.rails',
    { profileId },
    [profileId, catalogVersion],
  );

  /**
   * A seed that changes every time Home is opened, and not while it is open.
   *
   * The hero used to be `recentlyAdded.slice(0, 6)`: the same six titles, in the same
   * order, every launch until the library gained something. It rotated between them,
   * which reads as a carousel that is working and a library that is not.
   *
   * Picked once per *session*, not per render and not per mount.
   *
   * Per render would reshuffle on every state change — the trailer starting, a card
   * being removed — and the billboard would jump to another film while somebody was
   * reading it.
   *
   * Per mount is subtler and was wrong too: going to Movies and back remounts Home, so
   * the billboard changed every time the viewer glanced away, which is disorienting
   * rather than fresh — and it meant a title opened from the hero could not be found
   * there again a moment later. "Different each time" means each time the app is
   * opened, which is what a session is.
   *
   * `sessionStorage` can throw — a private window, blocked site data — so a failure
   * falls back to a seed for this mount, and the billboard is merely as fresh as it
   * was before.
   */
  const heroSeed = useRef(0);
  if (heroSeed.current === 0) {
    const fresh = Date.now();
    try {
      const stored = window.sessionStorage.getItem('aurora.heroSeed');
      heroSeed.current = stored ? Number(stored) || fresh : fresh;
      window.sessionStorage.setItem('aurora.heroSeed', String(heroSeed.current));
    } catch {
      heroSeed.current = fresh;
    }
  }

  /**
   * What was on My List, half-watched or up next when Home opened.
   *
   * A ref rather than state: it is read while the hero set is being built and must not
   * cause a second build, which is also why `heroSeed` above is one.
   */
  const decidedOnOpen = useRef<Set<string> | null>(null);

  const heroItems = useMemo(() => {
    if (!rails) return [];

    // Everything the home screen is already showing, rather than one shelf of it.
    //
    // `continueWatching` and `upNext` are excluded on purpose: they are things the
    // viewer is part-way through, and a billboard inviting them to start one is
    // offering them something they did not finish. `myList` is excluded for the
    // opposite reason — they have already decided about those.
    const skip = new Set(['continueWatching', 'upNext', 'myList']);

    /**
     * The titles those three rails hold, rather than the rails themselves.
     *
     * `skip` filters by rail *kind*, which is not the same thing: a film on My List is
     * also in `recentlyAdded` and in its genre rail, so it entered the pool through one
     * of those and could headline the billboard. Same for something half-watched. The
     * paragraph above says why neither belongs there -- they have already decided, or
     * they did not finish -- and the code was only doing it for titles that appear
     * nowhere else.
     *
     * Found as a flaky journey rather than by reading: `lists.spec.ts` opens the first
     * billboard item and asserts liking it does not add it to My List, and failed
     * whenever the shuffle happened to put an already-listed title first.
     *
     * Taken once, and then kept, for the same reason the seed is: this memo re-runs
     * whenever `rails` changes, and adding something to My List changes them. Filtering
     * live meant pressing "Add to My List" on the billboard deleted the thing the viewer
     * was looking at -- the next item sliding into its place as a reward for using the
     * button. So the question is "had they decided about this when the screen opened",
     * which is the one the paragraph above is really asking.
     */
    if (decidedOnOpen.current === null) {
      const decided = new Set<string>();
      for (const rail of rails) {
        if (!skip.has(rail.kind)) continue;
        for (const item of rail.items) decided.add(`${item.kind}:${item.id}`);
      }
      decidedOnOpen.current = decided;
    }
    const decided = decidedOnOpen.current;

    const seen = new Set<string>();
    const pool: CatalogItem[] = [];
    for (const rail of rails) {
      if (skip.has(rail.kind)) continue;
      for (const item of rail.items) {
        const key = `${item.kind}:${item.id}`;
        if (seen.has(key) || decided.has(key)) continue;
        seen.add(key);
        pool.push(item);
      }
    }

    // A billboard is mostly its artwork, so one without a backdrop is a grey panel with
    // a title on it. Preferred rather than required: a small library might have none.
    const withArt = pool.filter((i) => i.backdrop);
    const chosen = withArt.length >= 6 ? withArt : pool;

    // A seeded shuffle, so the set is different each time Home is opened and stable
    // while it is open. Mulberry32 — small, and good enough to pick six films.
    let state = heroSeed.current >>> 0;
    const random = () => {
      state = (state + 0x6d2b79f5) >>> 0;
      let t = state;
      t = Math.imul(t ^ (t >>> 15), t | 1);
      t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
    const shuffled = [...chosen];
    for (let i = shuffled.length - 1; i > 0; i--) {
      const j = Math.floor(random() * (i + 1));
      [shuffled[i], shuffled[j]] = [shuffled[j]!, shuffled[i]!];
    }
    return shuffled.slice(0, 6);
  }, [rails]);

  /**
   * Cards the viewer has removed, hidden before the host has been asked.
   *
   * `library.rails` is one query for the whole screen and re-running it to drop a single
   * card would rebuild all nine rails, which on a large library is slow enough to see.
   * So the card goes at once and the rails catch up whenever they are next read. Keyed
   * rather than filtered out of state, because `rails` is the server's answer and
   * editing it in place would make the next read look like a change.
   */
  const [removed, setRemoved] = useState<Set<string>>(new Set());

  const removeFromContinue = useCallback(
    (item: CatalogItem) => {
      const key = `${item.kind}:${item.id}`;
      setRemoved((was) => new Set(was).add(key));
      // A show is removed by its series id; the host clears every episode's position,
      // since taking only one away would promote the next and bring the card back.
      invoke('progress.forget', {
        profileId,
        kind: item.kind === 'series' ? 'series' : 'movie',
        id: item.id,
      }).catch((e: unknown) => {
        // Put it back rather than leave a card that is gone from the screen and not
        // from the library: the next reload would return it anyway, and silently.
        setRemoved((was) => {
          const next = new Set(was);
          next.delete(key);
          return next;
        });
        report(`Could not remove ${item.title} from Continue Watching`)(e);
      });
    },
    [profileId],
  );

  const visibleRails = useMemo(
    () =>
      (rails ?? [])
        .map((rail) =>
          rail.kind === 'continueWatching'
            ? {
                ...rail,
                items: rail.items.filter((i) => !removed.has(`${i.kind}:${i.id}`)),
              }
            : rail,
        )
        // A rail with a heading and no posters reads as a loading failure, which is why
        // the host drops empty ones — so removing the last card has to drop it here too.
        .filter((rail) => rail.items.length > 0)
        // Continue Watching first, whatever order it arrived in.
        //
        // The host already builds it first, so this is belt and braces rather than a
        // correction -- but "first" is a promise this screen makes and the host is free
        // to reorder its own output. Sorted rather than assumed, so the promise survives
        // that.
        .sort((a, b) => Number(b.kind === 'continueWatching')
          - Number(a.kind === 'continueWatching')),
    [rails, removed],
  );

  if (error) {
    return (
      <EmptyState
        icon="info"
        title="Couldn't load your library"
        body={error}
      />
    );
  }

  if (loading || !rails) {
    return (
      <div style={{ padding: 'var(--sp-6)' }}>
        <Skeleton h={380} r={12} />
        <div style={{ marginTop: 'var(--sp-6)', display: 'grid', gap: 'var(--sp-6)' }}>
          {[0, 1, 2].map((i) => (
            <div key={i}>
              <Skeleton w={220} h={22} />
              <div style={{ display: 'flex', gap: 10, marginTop: 12 }}>
                {Array.from({ length: 8 }, (_, j) => (
                  <Skeleton key={j} w={168} h={252} />
                ))}
              </div>
            </div>
          ))}
        </div>
      </div>
    );
  }

  if (rails.length === 0) {
    return (
      <EmptyState
        title="Nothing here yet"
        body="Add a provider in Settings and Aurora will build your library."
      />
    );
  }

  return (
    <div>
      <HeroBillboard items={heroItems} onOpen={onOpen} onPlay={onPlay} />
      {/* Continue Watching, then the suggestion, then the library's own shelves.

          What you were already part-way through beats a suggestion -- which is what the
          comment on `AiRail` claimed before this, while the code rendered the AI rail
          *above* every rail including that one. The comment was the intent and the order
          was the bug. */}
      {visibleRails
        .filter((rail) => rail.kind === 'continueWatching')
        .map((rail) => (
          <Rail
            key={rail.id}
            rail={rail}
            onOpen={onOpen}
            onPlay={onPlay}
            onRemove={removeFromContinue}
          />
        ))}
      {/* Renders nothing until it has something, so on a library with no key it does
          not exist. */}
      <AiRail onOpen={onOpen} onPlay={onPlay} />
      {visibleRails
        .filter((rail) => rail.kind !== 'continueWatching')
        .map((rail) => (
          <Rail key={rail.id} rail={rail} onOpen={onOpen} onPlay={onPlay} />
        ))}
      <div style={{ height: 'var(--sp-8)' }} />
    </div>
  );
}
