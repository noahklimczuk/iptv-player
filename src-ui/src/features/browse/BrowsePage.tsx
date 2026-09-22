/** Browse with filters and sorting (README §8.5). Serves both Movies and Series. */
import { useState } from 'react';
import type { CatalogItem } from '@shared/ipc';
import { CatalogCard } from '@/components/CatalogCard';
import { Button, EmptyState, Skeleton } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';

type Sort = 'recentlyAdded' | 'title' | 'year' | 'rating';
const SORTS: { value: Sort; label: string }[] = [
  { value: 'recentlyAdded', label: 'Recently added' },
  { value: 'title', label: 'A–Z' },
  { value: 'year', label: 'Year' },
  { value: 'rating', label: 'Rating' },
];

export function BrowsePage({
  mode, onOpen, onPlay,
}: {
  mode: 'movies' | 'series';
  onOpen: (i: CatalogItem) => void;
  onPlay: (i: CatalogItem) => void;
}) {
  const [sort, setSort] = useState<Sort>('recentlyAdded');
  const [genre, setGenre] = useState<string | undefined>();

  const { data: genres } = useCommand('library.genres', undefined, []);
  const movies = useCommand(
    'library.movies',
    { sort, limit: 120, offset: 0, genre },
    [sort, genre, mode],
  );
  const series = useCommand('library.series', { limit: 120, offset: 0, genre }, [genre, mode]);

  const loading = mode === 'movies' ? movies.loading : series.loading;
  const items: CatalogItem[] = mode === 'movies'
    ? (movies.data ?? []).map((m) => ({ kind: 'movie' as const, ...m }))
    : (series.data ?? []).map((s) => ({ kind: 'series' as const, ...s }));

  return (
    <div style={{ padding: 'var(--sp-5) var(--sp-6) var(--sp-8)' }}>
      <div
        style={{
          display: 'flex', alignItems: 'center', gap: 'var(--sp-3)',
          marginBottom: 'var(--sp-4)', flexWrap: 'wrap',
        }}
      >
        <h1 style={{ margin: 0, fontSize: 'var(--fs-2xl)', fontWeight: 800 }}>
          {mode === 'movies' ? 'Movies' : 'Series'}
        </h1>
        <span style={{ color: 'var(--text-faint)' }}>{items.length}</span>

        <select
          value={genre ?? ''}
          onChange={(e) => setGenre(e.target.value || undefined)}
          aria-label="Genre"
          style={selectStyle}
        >
          <option value="">All genres</option>
          {(genres ?? []).map((g) => <option key={g} value={g}>{g}</option>)}
        </select>

        {mode === 'movies' && (
          <div style={{ display: 'flex', gap: 4, marginLeft: 'auto' }}>
            {SORTS.map((s) => (
              <Button
                key={s.value} size="sm"
                variant={sort === s.value ? 'primary' : 'ghost'}
                onClick={() => setSort(s.value)}
              >
                {s.label}
              </Button>
            ))}
          </div>
        )}
      </div>

      {loading && (
        <div style={gridStyle}>
          {Array.from({ length: 24 }, (_, i) => <Skeleton key={i} h={252} />)}
        </div>
      )}

      {!loading && items.length === 0 && (
        <EmptyState icon="film" title="Nothing matches those filters" />
      )}

      <div style={gridStyle}>
        {items.map((item, i) => (
          <CatalogCard
            key={`${item.kind}-${item.id}`}
            item={item} index={i} onOpen={onOpen} onPlay={onPlay}
          />
        ))}
      </div>
    </div>
  );
}

const gridStyle = {
  display: 'grid',
  gridTemplateColumns: 'repeat(auto-fill, minmax(158px, 1fr))',
  gap: 'var(--sp-4) var(--sp-3)',
} as const;

const selectStyle = {
  padding: '6px 10px', background: 'var(--surface)', color: 'var(--text)',
  border: '1px solid var(--border-strong)', borderRadius: 'var(--r-md)',
  fontSize: 'var(--fs-sm)',
} as const;
