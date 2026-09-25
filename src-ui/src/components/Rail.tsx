/**
 * Horizontal rail (README §8.2): paginated scroll with hover arrows, keyboard navigation
 * that keeps focus in view, and vertical overflow left visible so expanded cards are not
 * clipped — the detail that makes the Netflix hover work.
 */
import { useCallback, useLayoutEffect, useRef, useState } from 'react';
import type { CatalogItem, Rail as RailData } from '@shared/ipc';
import { CatalogCard } from './CatalogCard';
import { Icon } from './Icon';

const CARD_W = 168;
const GAP = 10;

export function Rail({
  rail, onOpen, onPlay,
}: {
  rail: RailData;
  onOpen: (i: CatalogItem) => void;
  onPlay: (i: CatalogItem) => void;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const [atStart, setAtStart] = useState(true);
  const [atEnd, setAtEnd] = useState(false);

  const sync = useCallback(() => {
    const el = scroller.current;
    if (!el) return;
    setAtStart(el.scrollLeft <= 2);
    setAtEnd(el.scrollLeft + el.clientWidth >= el.scrollWidth - 2);
  }, []);

  useLayoutEffect(() => {
    sync();
    const el = scroller.current;
    if (!el) return;
    const ro = new ResizeObserver(sync);
    ro.observe(el);
    return () => ro.disconnect();
  }, [sync, rail.items.length]);

  const page = (dir: 1 | -1) => {
    const el = scroller.current;
    if (!el) return;
    el.scrollBy({ left: dir * (el.clientWidth - CARD_W), behavior: 'smooth' });
  };

  const showRank = rail.kind === 'top10';

  return (
    <section style={{ marginBottom: 'var(--sp-6)' }} aria-label={rail.title}>
      <div
        style={{
          display: 'flex', alignItems: 'baseline', gap: 'var(--sp-3)',
          padding: '0 var(--sp-6)', marginBottom: 'var(--sp-3)',
        }}
      >
        <h2
          style={{
            margin: 0, fontSize: 'var(--fs-lg)', fontWeight: 700,
            letterSpacing: '-0.01em',
          }}
        >
          {rail.title}
        </h2>
        <span style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-faint)' }}>
          {rail.items.length}
        </span>
      </div>

      <div style={{ position: 'relative' }}>
        {!atStart && (
          <RailArrow dir="left" onClick={() => page(-1)} />
        )}
        {!atEnd && (
          <RailArrow dir="right" onClick={() => page(1)} />
        )}

        <div
          ref={scroller}
          onScroll={sync}
          className="no-scrollbar"
          style={{
            display: 'flex', gap: GAP,
            // Vertical padding so the ~1.32x expanded card is not clipped by overflow-x.
            padding: '28px var(--sp-6)',
            margin: '-28px 0',
            overflowX: 'auto', overflowY: 'visible',
            scrollSnapType: 'x proximity',
          }}
        >
          {rail.items.map((item, i) => (
            <div
              key={`${item.kind}-${item.id}`}
              style={{ flex: `0 0 ${CARD_W}px`, scrollSnapAlign: 'start' }}
            >
              <CatalogCard
                item={item}
                index={i}
                rank={showRank ? i + 1 : undefined}
                progress={rail.progress?.[`${item.kind}:${item.id}`] ?? null}
                onOpen={onOpen}
                onPlay={onPlay}
              />
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

function RailArrow({ dir, onClick }: { dir: 'left' | 'right'; onClick: () => void }) {
  return (
    <button
      aria-label={dir === 'left' ? 'Scroll left' : 'Scroll right'}
      onClick={onClick}
      style={{
        position: 'absolute', top: 28, bottom: 28, [dir]: 0, width: 46, zIndex: 20,
        display: 'grid', placeItems: 'center', border: 'none', cursor: 'pointer',
        color: 'var(--text)',
        background:
          dir === 'left'
            ? 'linear-gradient(to right, var(--bg) 25%, transparent)'
            : 'linear-gradient(to left, var(--bg) 25%, transparent)',
      }}
    >
      <Icon name={dir === 'left' ? 'chevronLeft' : 'chevronRight'} size={28} strokeWidth={2.4} />
    </button>
  );
}
