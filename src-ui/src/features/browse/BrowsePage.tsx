/**
 * Browse (README §8.5). Serves both Movies and Series.
 *
 * Rebuilt against a real subscription — 117,508 films and 28,528 shows — which showed
 * four things a fixture never could.
 *
 * **The genre filter was an empty dropdown.** Genres come from TMDB enrichment, which
 * needs an API key a viewer may never set, so on most libraries there are none at all.
 * Meanwhile the panel had been filing everything under 202 named shelves the whole
 * time and nothing in the app let you see them. Categories are now the primary filter
 * and genres appear only when there are some.
 *
 * **The count was "120+".** That is the page size wearing a library's clothes, and
 * scrolling turned it into a number that climbed. The host now counts the rows the
 * filters actually match, so the heading says 117,508 from the first frame.
 *
 * **There was no way to search within a list.** Ctrl-K searches everything, which is a
 * different question: "find me this" rather than "narrow what I am looking at".
 *
 * **The rows did not line up.** Titles wrap to one or two lines, so every row started
 * at a different height and the grid looked broken. The cards are a fixed height now.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { BrowseSort, CatalogItem } from '@shared/ipc';
import { CatalogCard } from '@/components/CatalogCard';
import { Icon } from '@/components/Icon';
import { Button, EmptyState, Skeleton } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { usePages } from '@/hooks/usePages';
import { invoke } from '@/ipc';

/**
 * How much is fetched at once. This used to be the whole list: one request for 120,
 * no second page, and the number printed beside the heading was the page size.
 */
const PAGE = 120;

/** How long typing settles before the library is asked. */
const SEARCH_DEBOUNCE_MS = 300;

/** How many shelves the bar offers before the rest go behind "All". */
const SHELF_CHIPS = 8;

/**
 * Thousands separators, without depending on the container's locale.
 *
 * `toLocaleString()` produced "117508" under the WebView this runs in, because a
 * process with no locale configured groups by nothing. A library's size is the one
 * number on this page and it has to be readable.
 */
const NUMBER = new Intl.NumberFormat('en-US');

const SORTS: { value: BrowseSort; label: string }[] = [
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
  const [sort, setSort] = useState<BrowseSort>(mode === 'series' ? 'title' : 'recentlyAdded');
  const [genre, setGenre] = useState<string | undefined>();
  const [category, setCategory] = useState<string | undefined>();
  const [typed, setTyped] = useState('');
  const [query, setQuery] = useState<string | undefined>();

  // Typing filters a library of a hundred thousand rows, so it waits for a pause.
  useEffect(() => {
    const id = window.setTimeout(
      () => setQuery(typed.trim() || undefined),
      SEARCH_DEBOUNCE_MS,
    );
    return () => window.clearTimeout(id);
  }, [typed]);

  const { data: facets } = useCommand(
    'library.browseFacets',
    { kind: mode, genre, category, query },
    [mode, genre, category, query],
  );

  const page = useCallback(
    async (limit: number, offset: number): Promise<CatalogItem[]> => (
      mode === 'movies'
        ? (await invoke('library.movies', { sort, limit, offset, genre, category, query }))
          .map((m) => ({ kind: 'movie' as const, ...m }))
        : (await invoke('library.series', { sort, limit, offset, genre, category, query }))
          .map((s) => ({ kind: 'series' as const, ...s }))
    ),
    [mode, sort, genre, category, query],
  );

  const { items, loading, done, loadMore } = usePages<CatalogItem>(
    page,
    PAGE,
    [mode, sort, genre, category, query],
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

  const shelves = facets?.categories ?? [];
  const chips = useMemo(() => shelves.slice(0, SHELF_CHIPS), [shelves]);
  const rest = useMemo(() => shelves.slice(SHELF_CHIPS), [shelves]);
  const filtered = Boolean(genre || category || query);
  const total = facets?.total;

  return (
    <div style={{ padding: 'var(--sp-5) var(--sp-6) var(--sp-8)' }}>
      <div style={{ display: 'flex', alignItems: 'baseline', gap: 'var(--sp-3)' }}>
        <h1 style={{ margin: 0, fontSize: 'var(--fs-2xl)', fontWeight: 800 }}>
          {mode === 'movies' ? 'Movies' : 'Series'}
        </h1>
        {/* The real total for these filters, counted by the host. This used to be
            `items.length`, which on the first page was the page size and after that
            was a number that climbed while you scrolled. */}
        <span
          data-testid="browse-count"
          style={{ color: 'var(--text-faint)', fontVariantNumeric: 'tabular-nums' }}
        >
          {total === undefined ? '' : NUMBER.format(total)}
        </span>
      </div>

      <div
        style={{
          display: 'flex', alignItems: 'center', gap: 'var(--sp-2)',
          margin: 'var(--sp-4) 0', flexWrap: 'wrap',
        }}
      >
        <SearchBox
          value={typed}
          onChange={setTyped}
          placeholder={mode === 'movies' ? 'Search films…' : 'Search series…'}
        />

        {/* Genres only when there are any. An empty dropdown labelled "All genres" is
            a control that looks broken and is, on every library without a TMDB key. */}
        {(facets?.genres.length ?? 0) > 0 && (
          <Picker
            label="Genre"
            all="All genres"
            options={facets!.genres.map((name) => ({ name }))}
            selected={genre}
            onSelect={setGenre}
          />
        )}

        {rest.length > 0 && (
          <Picker
            label="Category"
            all="All categories"
            options={shelves}
            selected={category}
            onSelect={setCategory}
          />
        )}

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
      </div>

      {chips.length > 0 && (
        <div
          data-testid="browse-categories"
          style={{
            display: 'flex', gap: 'var(--sp-2)', marginBottom: 'var(--sp-4)',
            overflowX: 'auto', paddingBottom: 4,
          }}
        >
          <Chip active={!category} onClick={() => setCategory(undefined)}>All</Chip>
          {chips.map((c) => (
            <Chip
              key={c.name}
              active={category === c.name}
              onClick={() => setCategory(category === c.name ? undefined : c.name)}
            >
              {c.name}
              <span style={{ opacity: 0.55, marginLeft: 6, fontVariantNumeric: 'tabular-nums' }}>
                {NUMBER.format(c.count)}
              </span>
            </Chip>
          ))}
        </div>
      )}

      {/* Only for the first page. Replacing a screenful of posters with skeletons
          every time another page arrives would make scrolling flicker. */}
      {loading && items.length === 0 && (
        <div style={gridStyle}>
          {Array.from({ length: 24 }, (_, i) => <Skeleton key={i} h={CARD_H} />)}
        </div>
      )}

      {!loading && items.length === 0 && (
        <EmptyState
          icon="film"
          title={filtered ? 'Nothing matches those filters' : 'Nothing here yet'}
          action={filtered ? (
            <Button
              onClick={() => { setGenre(undefined); setCategory(undefined); setTyped(''); }}
            >
              Clear filters
            </Button>
          ) : undefined}
        />
      )}

      <div style={gridStyle}>
        {items.map((item, i) => (
          // Fixed height, so a two-line title does not push the row below it out of
          // alignment — which is what made a grid of a hundred posters look broken.
          <div key={`${item.kind}-${item.id}`} style={{ height: CARD_H }}>
            <CatalogCard item={item} index={i} onOpen={onOpen} onPlay={onPlay} showTitle />
          </div>
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

function SearchBox({
  value, onChange, placeholder,
}: {
  value: string; onChange: (v: string) => void; placeholder: string;
}) {
  return (
    <div style={{ position: 'relative', display: 'flex', alignItems: 'center' }}>
      <Icon
        name="search" size={15}
        style={{
          position: 'absolute', left: 10, color: 'var(--text-faint)',
          pointerEvents: 'none',
        }}
      />
      <input
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        aria-label={placeholder}
        data-testid="browse-search"
        style={{
          padding: '7px 10px 7px 30px', width: 220,
          background: 'var(--surface)', color: 'var(--text)',
          border: '1px solid var(--border-strong)', borderRadius: 'var(--r-md)',
          fontSize: 'var(--fs-sm)',
        }}
      />
      {value && (
        <button
          aria-label="Clear search"
          onClick={() => onChange('')}
          style={{
            position: 'absolute', right: 6, display: 'grid', placeItems: 'center',
            background: 'none', border: 'none', cursor: 'pointer',
            color: 'var(--text-faint)', padding: 4,
          }}
        >
          <Icon name="close" size={13} />
        </button>
      )}
    </div>
  );
}

/**
 * Pick one option out of a long list, by name.
 *
 * A native `<select>` was the obvious thing and is wrong twice over. Two hundred and
 * two options is not a list anybody scrolls, and the control renders as a blank box
 * in the WebView this ships inside — which is why the old genre dropdown appeared as
 * an empty rectangle in every screenshot of this page.
 */
function Picker({
  label, all, options, selected, onSelect,
}: {
  /** What this filters, for the accessible name. */
  label: string;
  /** What the "no filter" row says. */
  all: string;
  options: { name: string; count?: number }[];
  selected?: string;
  onSelect: (c: string | undefined) => void;
}) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState('');
  const box = useRef<HTMLDivElement | null>(null);

  // Click anywhere else and it closes, which is the one thing every popover must do
  // and the one thing hand-rolled ones usually forget.
  useEffect(() => {
    if (!open) return undefined;
    const away = (e: MouseEvent) => {
      if (box.current && !box.current.contains(e.target as Node)) setOpen(false);
    };
    const escape = (e: KeyboardEvent) => { if (e.key === 'Escape') setOpen(false); };
    document.addEventListener('mousedown', away);
    document.addEventListener('keydown', escape);
    return () => {
      document.removeEventListener('mousedown', away);
      document.removeEventListener('keydown', escape);
    };
  }, [open]);

  const shown = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    return needle
      ? options.filter((c) => c.name.toLowerCase().includes(needle))
      : options;
  }, [options, filter]);

  return (
    <div ref={box} style={{ position: 'relative' }}>
      <Button
        size="sm"
        variant={selected ? 'primary' : 'secondary'}
        icon="chevronDown"
        onClick={() => { setOpen((o) => !o); setFilter(''); }}
        data-testid={`browse-picker-${label.toLowerCase()}`}
        aria-label={label}
        aria-expanded={open}
      >
        {selected ?? all}
      </Button>

      {open && (
        <div
          role="listbox"
          style={{
            position: 'absolute', top: 'calc(100% + 6px)', left: 0, zIndex: 60,
            width: 300, maxHeight: 340, overflowY: 'auto',
            background: 'var(--bg-elevated)',
            border: '1px solid var(--border-strong)',
            borderRadius: 'var(--r-md)',
            boxShadow: '0 12px 32px rgb(0 0 0 / 0.5)',
            padding: 'var(--sp-2)',
          }}
        >
          <input
            autoFocus
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder={`Filter ${label.toLowerCase()}…`}
            aria-label={`Filter ${label.toLowerCase()}`}
            style={{
              width: '100%', padding: '6px 9px', marginBottom: 'var(--sp-2)',
              background: 'var(--surface)', color: 'var(--text)',
              border: '1px solid var(--border-strong)', borderRadius: 'var(--r-sm)',
              fontSize: 'var(--fs-sm)',
            }}
          />
          <PickerRow
            active={!selected}
            onClick={() => { onSelect(undefined); setOpen(false); }}
          >
            {all}
          </PickerRow>
          {shown.map((c) => (
            <PickerRow
              key={c.name}
              active={selected === c.name}
              onClick={() => { onSelect(c.name); setOpen(false); }}
            >
              <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{c.name}</span>
              {c.count !== undefined && (
                <span
                  style={{
                    marginLeft: 'auto', paddingLeft: 10, opacity: 0.55,
                    fontVariantNumeric: 'tabular-nums',
                  }}
                >
                  {NUMBER.format(c.count)}
                </span>
              )}
            </PickerRow>
          ))}
          {shown.length === 0 && (
            <div
              style={{
                padding: 'var(--sp-3)', color: 'var(--text-faint)',
                fontSize: 'var(--fs-sm)',
              }}
            >
              Nothing matches that.
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function PickerRow({
  active, onClick, children,
}: {
  active: boolean; onClick: () => void; children: React.ReactNode;
}) {
  return (
    <button
      role="option"
      aria-selected={active}
      onClick={onClick}
      data-testid="browse-picker-row"
      style={{
        display: 'flex', alignItems: 'center', width: '100%', textAlign: 'left',
        padding: '7px 9px', borderRadius: 'var(--r-sm)', border: 'none',
        cursor: 'pointer', fontSize: 'var(--fs-sm)', whiteSpace: 'nowrap',
        background: active ? 'var(--surface-hover)' : 'transparent',
        color: active ? 'var(--text)' : 'var(--text-muted)',
        fontWeight: active ? 700 : 500,
      }}
      onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--surface)'; }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = active ? 'var(--surface-hover)' : 'transparent';
      }}
    >
      {children}
    </button>
  );
}

function Chip({
  active, onClick, children,
}: {
  active: boolean; onClick: () => void; children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      aria-pressed={active}
      data-testid="browse-category"
      style={{
        flexShrink: 0, padding: '6px 13px', borderRadius: 'var(--r-full)',
        fontSize: 'var(--fs-sm)', fontWeight: 600, cursor: 'pointer',
        whiteSpace: 'nowrap',
        border: `1px solid ${active ? 'var(--text)' : 'var(--border-strong)'}`,
        background: active ? 'var(--text)' : 'transparent',
        color: active ? 'var(--text-invert)' : 'var(--text-muted)',
        transition: 'all var(--t-fast) var(--ease)',
      }}
    >
      {children}
    </button>
  );
}

/**
 * Poster plus two lines of title plus a year, fixed.
 *
 * The grid used to size each cell to its content, so a row whose titles all fitted on
 * one line was shorter than the row above it and the whole page stepped up and down.
 */
const CARD_H = 300;

const gridStyle = {
  display: 'grid',
  gridTemplateColumns: 'repeat(auto-fill, minmax(158px, 1fr))',
  gap: 'var(--sp-5) var(--sp-3)',
  alignItems: 'start',
} as const;
