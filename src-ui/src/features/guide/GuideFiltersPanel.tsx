/**
 * The guide's Filters button and the panel it opens.
 *
 * The chips beside it are the quick way -- tap a group in or out. This is the thorough
 * way: every group in one searchable list, shown only or hidden, and the filters that
 * are about the channel rather than its group. See `guideFilters.ts` for what each does.
 */
import { useEffect, useMemo, useRef, useState } from 'react';
import { Icon } from '@/components/Icon';
import { useLiquidGlass } from '@/components/liquidGlass';
import {
  NO_FILTERS, QUALITIES, activeCount, toggleGroup, type GuideFilters,
} from './guideFilters';

export function GuideFiltersButton({
  groups, filters, setFilters, shown,
}: {
  groups: { name: string; count: number }[];
  filters: GuideFilters;
  setFilters: (f: GuideFilters) => void;
  shown: number;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const wrap = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  useLiquidGlass({ ref: panel, shown: open, bezel: 16 });
  const count = activeCount(filters);

  // Closed by a click anywhere else, or Escape -- and the focus goes back to the button.
  useEffect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      if (!wrap.current?.contains(e.target as Node)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        setOpen(false);
        wrap.current?.querySelector<HTMLButtonElement>('button')?.focus();
      }
    };
    document.addEventListener('pointerdown', away);
    document.addEventListener('keydown', key, true);
    return () => {
      document.removeEventListener('pointerdown', away);
      document.removeEventListener('keydown', key, true);
    };
  }, [open]);

  const matching = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? groups.filter((g) => g.name.toLowerCase().includes(q)) : groups;
  }, [groups, query]);

  const chosen = new Set(filters.groups);
  const set = (patch: Partial<GuideFilters>) => setFilters({ ...filters, ...patch });

  return (
    <div ref={wrap} style={{ position: 'relative', flexShrink: 0 }}>
      <button
        type="button"
        className="dl-filter"
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-pressed={count > 0}
        data-testid="guide-filters"
        onClick={() => setOpen((o) => !o)}
        style={{ display: 'inline-flex', alignItems: 'center', gap: 8 }}
      >
        <Icon name="filter" size={15} />
        Filters
        {count > 0 && <span className="guide-filter-badge">{count}</span>}
      </button>

      {open && (
        <div
          ref={panel}
          role="dialog"
          aria-label="Guide filters"
          className="glass-strong guide-filter-panel"
          data-testid="guide-filter-panel"
        >
          <div className="guide-filter-head">
            <span style={{ fontWeight: 800 }}>Groups</span>
            <div className="dl-seg" role="radiogroup" aria-label="Chosen groups are">
              <button
                type="button"
                role="radio"
                aria-checked={filters.groupMode === 'only'}
                data-active={filters.groupMode === 'only'}
                onClick={() => set({ groupMode: 'only' })}
              >
                Show only
              </button>
              <button
                type="button"
                role="radio"
                aria-checked={filters.groupMode === 'hide'}
                data-active={filters.groupMode === 'hide'}
                onClick={() => set({ groupMode: 'hide' })}
              >
                Hide
              </button>
            </div>
          </div>

          <input
            className="aurora-field"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={`Search ${groups.length} groups…`}
            aria-label="Search groups"
            autoFocus
            style={{ width: '100%' }}
          />

          <div className="guide-filter-actions">
            <button
              type="button"
              className="guide-filter-link"
              onClick={() =>
                set({ groups: [...new Set([...filters.groups, ...matching.map((g) => g.name)])] })}
            >
              {query ? `Choose all ${matching.length} found` : 'Choose all'}
            </button>
            <button
              type="button"
              className="guide-filter-link"
              disabled={filters.groups.length === 0}
              onClick={() => set({ groups: [] })}
            >
              Clear {filters.groups.length > 0 ? `(${filters.groups.length})` : ''}
            </button>
          </div>

          <div className="guide-filter-list" role="group" aria-label="Groups">
            {matching.length === 0 && (
              <div style={{ padding: 12, color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>
                No group matches “{query}”.
              </div>
            )}
            {matching.map((g) => (
              <label key={g.name} className="guide-filter-row" data-mode={filters.groupMode}>
                <input
                  type="checkbox"
                  checked={chosen.has(g.name)}
                  onChange={() => setFilters(toggleGroup(filters, g.name))}
                />
                <span className="guide-filter-name">{g.name}</span>
                <span className="guide-chip-count">{g.count}</span>
              </label>
            ))}
          </div>

          <div style={{ fontWeight: 800, marginTop: 4 }}>Channels</div>
          <div className="guide-filter-toggles">
            {(
              [
                ['withListings', 'With listings only'],
                ['favoritesOnly', 'Favourites only'],
                ['catchupOnly', 'Catch-up only'],
                ['hideRadio', 'Hide radio'],
              ] as const
            ).map(([key, label]) => (
              <label key={key} className="guide-filter-toggle">
                <button
                  type="button"
                  role="switch"
                  className="dl-switch"
                  aria-checked={filters[key]}
                  aria-label={label}
                  onClick={() => set({ [key]: !filters[key] })}
                />
                <span aria-hidden>{label}</span>
              </label>
            ))}
          </div>

          <div style={{ fontWeight: 800, marginTop: 4 }}>Quality</div>
          <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }} role="group" aria-label="Quality">
            {QUALITIES.map((q) => (
              <button
                key={q}
                type="button"
                className="dl-filter"
                aria-pressed={filters.qualities.includes(q)}
                onClick={() =>
                  set({
                    qualities: filters.qualities.includes(q)
                      ? filters.qualities.filter((x) => x !== q)
                      : [...filters.qualities, q],
                  })}
              >
                {q}
              </button>
            ))}
          </div>

          <div className="guide-filter-foot">
            <span data-testid="guide-filter-shown">
              {shown.toLocaleString()} channel{shown === 1 ? '' : 's'} shown
            </span>
            <button
              type="button"
              className="guide-filter-link"
              disabled={count === 0}
              onClick={() => setFilters(NO_FILTERS)}
            >
              Reset all
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
