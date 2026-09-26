/** Browse with filters and sorting (README §8.5). Serves both Movies and Series. */
import { useCallback, useEffect, useRef, useState } from 'react';
import type { CatalogItem } from '@shared/ipc';
import { CatalogCard } from '@/components/CatalogCard';
import { Button, EmptyState, Skeleton } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { usePages } from '@/hooks/usePages';
import { invoke } from '@/ipc';

/**
 * How much is fetched at once. This used to be the whole list: one request for 120,
 * no second page, and the number printed beside the heading was the page size wearing
 * a library's clothes.
 */
const PAGE = 120;

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

  // One paged read for whichever list this page is showing, rather than two unpaged
  // ones for both. Before, Movies fetched the series list too and threw it away.
  const page = useCallback(
    async (limit: number, offset: number): Promise<CatalogItem[]> => (
      mode === 'movies'
        ? (await invoke('library.movies', { sort, limit, offset, genre }))
          .map((m) => ({ kind: 'movie' as const, ...m }))
        : (await invoke('library.series', { limit, offset, genre }))
          .map((s) => ({ kind: 'series' as const, ...s }))
    ),
    [mode, sort, genre],
  );

  const { items, loading, done, loadMore } = usePages<CatalogItem>(
    page,
    PAGE,
    [mode, sort, genre],
    mode === 'movies' ? 'your films' : 'your series',
  );

  // Fetch the next page when the end of the list comes into view. `rootMargin` asks
  // early enough that scrolling does not stop at a spinner.
  const sentinel = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const el = sentinel.current;
    if (!el || done) return undefined;
    const io = new IntersectionObserver(
      (entries) => { if (entries.some((e) => e.isIntersecting)) loadMore(); },
      { rootMargin: '600px' },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [done, loadMore, items.length]);

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
        {/* `120` used to sit here whatever the library held, because it was the page
            size. The `+` is the honest form of "at least this many, still counting";
            once every page is in, the number is the total. */}
        <span style={{ color: 'var(--text-faint)' }} data-testid="browse-count">
          {done ? items.length : `${items.length}+`}
        </span>

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

      {/* Only for the first page. Replacing a screenful of posters with skeletons
          every time another page arrives would make scrolling flicker. */}
      {loading && items.length === 0 && (
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
            showTitle
          />
        ))}
      </div>

      {items.length > 0 && !done && (
        <div
          ref={sentinel}
          data-testid="browse-sentinel"
          style={{
            padding: 'var(--sp-6)', textAlign: 'center', color: 'var(--text-faint)',
            fontSize: 'var(--fs-sm)',
          }}
        >
          Loading more…
        </div>
      )}
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
