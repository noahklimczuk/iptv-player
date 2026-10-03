import { useCallback, useMemo, useState } from 'react';
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

  const heroItems = useMemo(() => {
    if (!rails) return [];
    const pool = rails.find((r) => r.kind === 'recentlyAdded')?.items ?? rails[0]?.items ?? [];
    return pool.slice(0, 6);
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
        .filter((rail) => rail.items.length > 0),
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
      {/* Second, under Continue Watching: what you were already part-way through beats a
          suggestion, and everything below is the library's own shelves. Renders nothing
          until it has something, so on a library with no key it does not exist. */}
      <AiRail onOpen={onOpen} onPlay={onPlay} />
      {visibleRails.map((rail) => (
        <Rail
          key={rail.id}
          rail={rail}
          onOpen={onOpen}
          onPlay={onPlay}
          onRemove={rail.kind === 'continueWatching' ? removeFromContinue : undefined}
        />
      ))}
      <div style={{ height: 'var(--sp-8)' }} />
    </div>
  );
}
