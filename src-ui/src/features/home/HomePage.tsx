import { useMemo } from 'react';
import type { CatalogItem } from '@shared/ipc';
import { HeroBillboard } from '@/components/HeroBillboard';
import { Rail } from '@/components/Rail';
import { EmptyState, Skeleton } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
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
      {rails.map((rail) => (
        <Rail key={rail.id} rail={rail} onOpen={onOpen} onPlay={onPlay} />
      ))}
      <div style={{ height: 'var(--sp-8)' }} />
    </div>
  );
}
