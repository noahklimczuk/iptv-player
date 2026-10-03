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
  rail, onOpen, onPlay, onRemove,
}: {
  rail: RailData;
  onOpen: (i: CatalogItem) => void;
  onPlay: (i: CatalogItem) => void;
  /**
   * Take an item off this rail, where that means anything. Passed down to the cards and
   * only ever supplied for Continue Watching — every other rail is derived from the
   * library or from ratings, so there is nothing on it a person could remove.
   */
  onRemove?: (i: CatalogItem) => void;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const [atStart, setAtStart] = useState(true);
  const [atEnd, setAtEnd] = useState(false);
  /**
   * Which card is expanded, so the others can get out of its way.
   *
   * It has to live here. A card grows with a `transform`, which does not affect layout,
   * so it cannot move its siblings by growing — the only thing that can is something
   * that knows about all of them.
   */
  const [expanded, setExpanded] = useState<number | null>(null);

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
    <section
      aria-label={rail.title}
      style={{
        marginBottom: 'var(--sp-6)',
        // Lifted above the rails below it while a card is open.
        //
        // An expanded card grows downward, past the bottom of this section and over the
        // next one. Sections are static and painted in document order, so the rail *below*
        // was drawn on top of that overhang — and the quick actions live at the bottom of
        // it. Play, My List and Remove were all sitting under the next rail and swallowing
        // their own clicks, which is how it was found: a journey timed out with
        // "<div> from <section aria-label='Up Next'> intercepts pointer events".
        //
        // Only while something is open, so a page of rails has no standing stack order to
        // reason about.
        position: expanded === null ? undefined : 'relative',
        zIndex: expanded === null ? undefined : 40,
      }}
    >
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
                reason={rail.reasons?.[`${item.kind}:${item.id}`]}
                onOpen={onOpen}
                onPlay={onPlay}
                onRemove={onRemove}
                onExpand={setExpanded}
                // Everything before the expanded card leans left and everything after it
                // leans right. Shifting only the immediate neighbours would leave a gap
                // opening in the middle of a row that is otherwise still, which looks
                // like a hole rather than like room being made.
                shift={expanded === null || expanded === i ? 0 : i < expanded ? -1 : 1}
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
