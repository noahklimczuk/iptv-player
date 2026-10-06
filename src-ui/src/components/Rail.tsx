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

/**
 * How far an expanded card reaches past its own box, and therefore how much room the
 * scroller has to keep for it.
 *
 * Derived rather than picked, because this is the number that was wrong: a card is a 2:3
 * poster plus its title, it is scaled by `CARD_SCALE` about its centre, and it is lifted
 * by `CARD_LIFT`. Half the growth goes each way, and the lift adds to the top.
 *
 * It has to be derived because a scrolling container cannot let the surplus hang out:
 * `overflow-x: auto` forces `overflow-y` to `auto` too, so anything outside the padding
 * box is clipped. A hand-picked 28px was 16px short, which is exactly the sort of number
 * that is right when it is written and wrong after the card gains a line of text.
 */
const CARD_SCALE = 1.32;
const CARD_LIFT = 12;
/** A 2:3 poster at `CARD_W`, plus two lines of title and a year. */
const CARD_H = Math.round(CARD_W * 1.5) + 46;
export const RAIL_EXPANSION_PAD = Math.ceil((CARD_H * (CARD_SCALE - 1)) / 2) + CARD_LIFT;

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
            // Room for the expanded card, and `overflowY` spelled as what it actually
            // computes to.
            //
            // `overflow-y: visible` was a wish, not a rule: CSS forces the other axis to
            // `auto` when one axis scrolls, so a browser reports `auto` here whatever is
            // asked for, and anything past the padding box is cut. Measured on the home
            // rails before this, an expanded card overflowed by 16px — the bottom of the
            // poster and the top of the shadow.
            //
            // So the padding is the whole mechanism, and it is derived rather than
            // guessed: see `RAIL_EXPANSION_PAD`. The negative margin cancels it, so the
            // rail still occupies the height it looks like it occupies.
            padding: `${RAIL_EXPANSION_PAD}px var(--sp-6)`,
            margin: `-${RAIL_EXPANSION_PAD}px 0`,
            overflowX: 'auto', overflowY: 'auto',
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
