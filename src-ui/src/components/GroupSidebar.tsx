/**
 * The list of groups a browse screen is divided into, down the left.
 *
 * Every browse screen used a horizontal strip of pills: Live TV scrolled one sideways,
 * Movies and Series showed the first eight and hid the rest behind a dropdown. That works
 * for the handful of shelves a fixture has. A real subscription has 202 VOD categories
 * and several hundred channel groups — "UK | ENTERTAINMENT", "US| SPORTS HD", "DE Kinder"
 * — and a sideways strip of those is a list you cannot see, cannot scan, and cannot get
 * back to the start of. The group you were in also scrolled out of sight as you went.
 *
 * So: a vertical list that is always visible, with counts, a filter box above it, and the
 * pinned entries (All, and Favourites where there are any) held at the top where they do
 * not move.
 *
 * Virtualised, because this is the screen where "a few hundred" is the optimistic case.
 */
import { useDeferredValue, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';

import { Icon, type IconName } from './Icon';
import { TextField } from './Primitives';

export interface Group {
  name: string;
  count: number;
}

/** An entry that is not a group: All, Favourites. Always visible, never filtered out. */
export interface PinnedGroup {
  label: string;
  /** What `onSelect` is called with. `undefined` means "no group filter". */
  value: string | undefined;
  count?: number;
  icon?: IconName;
  /** Marked active by its own rule rather than by matching `selected`. */
  active?: boolean;
  onSelect?: () => void;
}

const ROW_H = 34;

/**
 * Thousands separators without depending on the container's locale — a WebView with no
 * locale configured groups by nothing, and these counts run to five figures.
 */
const NUMBER = new Intl.NumberFormat('en-US');

export function GroupSidebar({
  groups,
  selected,
  onSelect,
  pinned = [],
  label,
  loading,
}: {
  groups: Group[];
  /** The selected group's name, or undefined for "all". */
  selected: string | undefined;
  onSelect: (group: string | undefined) => void;
  pinned?: PinnedGroup[];
  /** Names the list for anything that cannot see the layout. */
  label: string;
  loading?: boolean;
}) {
  const [filter, setFilter] = useState('');
  // Typing filters a list of hundreds and each keystroke re-renders it. Deferring keeps
  // the box itself responsive, which is the one thing that must never stutter.
  const lazyFilter = useDeferredValue(filter);

  const shown = useMemo(() => {
    const needle = lazyFilter.trim().toLowerCase();
    if (!needle) return groups;
    return groups.filter((g) => g.name.toLowerCase().includes(needle));
  }, [groups, lazyFilter]);

  const scroller = useRef<HTMLDivElement>(null);
  const virt = useVirtualizer({
    count: shown.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW_H,
    overscan: 12,
  });

  return (
    <nav
      aria-label={label}
      style={{
        width: 248,
        flexShrink: 0,
        display: 'flex',
        flexDirection: 'column',
        gap: 'var(--sp-3)',
        // Its own scroll region, so the content beside it scrolls independently and the
        // group you are in stays on screen however far down the films you are.
        position: 'sticky',
        top: 0,
        alignSelf: 'flex-start',
        maxHeight: 'calc(100vh - var(--topbar-h))',
        borderRight: '1px solid var(--border)',
        paddingRight: 'var(--sp-4)',
      }}
    >
      {/* Only worth showing when there is enough to search. Below that it is a control
          that takes up more room than the list it would narrow. */}
      {groups.length > 12 && (
        <TextField
          icon="search"
          clearable
          onClear={() => setFilter('')}
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="Filter groups…"
          aria-label={`Filter ${label.toLowerCase()}`}
          data-testid="group-filter"
        />
      )}

      {pinned.length > 0 && (
        <div style={{ display: 'grid', gap: 2 }}>
          {pinned.map((p) => (
            <Row
              key={p.label}
              label={p.label}
              count={p.count}
              icon={p.icon}
              active={p.active ?? (selected === p.value && p.value === undefined)}
              onClick={() => (p.onSelect ? p.onSelect() : onSelect(p.value))}
            />
          ))}
        </div>
      )}

      {pinned.length > 0 && groups.length > 0 && (
        <div style={{ height: 1, background: 'var(--border)', flexShrink: 0 }} />
      )}

      <div
        ref={scroller}
        className="no-scrollbar"
        data-testid="group-list"
        style={{ overflowY: 'auto', flex: 1, minHeight: 0 }}
      >
        {loading && groups.length === 0 && (
          <div style={{ color: 'var(--text-faint)', fontSize: 'var(--fs-sm)', padding: 6 }}>
            Loading…
          </div>
        )}
        {!loading && groups.length > 0 && shown.length === 0 && (
          <div style={{ color: 'var(--text-faint)', fontSize: 'var(--fs-sm)', padding: 6 }}>
            No group matches “{filter.trim()}”.
          </div>
        )}
        <div style={{ height: virt.getTotalSize(), position: 'relative' }}>
          {virt.getVirtualItems().map((row) => {
            const group = shown[row.index]!;
            return (
              <div
                key={group.name}
                style={{
                  position: 'absolute',
                  top: 0,
                  left: 0,
                  width: '100%',
                  height: row.size,
                  transform: `translateY(${row.start}px)`,
                }}
              >
                <Row
                  label={group.name}
                  count={group.count}
                  active={selected === group.name}
                  onClick={() => onSelect(selected === group.name ? undefined : group.name)}
                />
              </div>
            );
          })}
        </div>
      </div>
    </nav>
  );
}

function Row({
  label, count, icon, active, onClick,
}: {
  label: string;
  count?: number;
  icon?: IconName;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      // The pressed state, not just a colour: this is a list of filters and which one is
      // on is the single most important thing about it.
      aria-pressed={active}
      onClick={onClick}
      title={label}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        width: '100%',
        height: ROW_H - 2,
        padding: '0 10px',
        border: 'none',
        borderRadius: 'var(--r-md)',
        cursor: 'pointer',
        textAlign: 'left',
        fontSize: 'var(--fs-sm)',
        fontWeight: active ? 700 : 500,
        background: active ? 'var(--surface-hover)' : 'transparent',
        color: active ? 'var(--text)' : 'var(--text-muted)',
        transition: 'background var(--t-fast) var(--ease), color var(--t-fast) var(--ease)',
      }}
    >
      {icon && <Icon name={icon} size={15} />}
      {/* One line, ellipsised: provider group names are long and a wrapping one would
          make every row a different height, which the virtualiser cannot allow. */}
      <span
        style={{
          flex: 1,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
        {label}
      </span>
      {count !== undefined && count > 0 && (
        <span
          style={{
            fontSize: 'var(--fs-xs)',
            color: 'var(--text-faint)',
            fontVariantNumeric: 'tabular-nums',
            flexShrink: 0,
          }}
        >
          {NUMBER.format(count)}
        </span>
      )}
    </button>
  );
}
