/**
 * Rail card with the Netflix expansion behaviour (README §8.3): hover or focus scales the
 * card ~1.32x, lifts it above its neighbours, and after a dwell plays the muted trailer.
 * Focus produces the identical expansion, so the whole thing works from a remote.
 *
 * It pushes its neighbours aside rather than covering them. A `transform` does not affect
 * layout, so a scaled card cannot move its siblings by growing — the rail has to know
 * which card is expanded and shift the rest, which is why `shift` arrives as a prop and
 * `onExpand` reports upward. Each card still owns whether *it* is expanded; the rail owns
 * only which one that is.
 */
import { AnimatePresence, motion } from 'framer-motion';
import { memo, useEffect, useRef, useState } from 'react';
import type { CatalogItem } from '@shared/ipc';
import { progressPct, remaining, runtime } from '@/lib/format';
import { useIsLiked, useMarks, useOnMyList } from '@/state/marks';
import { useUi } from '@/state/ui';
import { Badge, IconButton, ProgressBar, Poster } from './Primitives';
import { TrailerFrame } from './TrailerFrame';

const PREVIEW_DELAY_MS = 700;

/**
 * How far a neighbour moves aside.
 *
 * A card is 168px and grows by a third, so it needs about 27px on each side to stop
 * covering the one next to it. Rounded up: the shadow wants a little air too, and a card
 * that is almost clear reads worse than one that plainly is.
 */
const NEIGHBOUR_SHIFT = 30;

export interface CardProgress {
  positionSecs: number;
  durationSecs: number;
}

export const CatalogCard = memo(function CatalogCard({
  item, index, progress, onOpen, onPlay, onRemove, rank, showTitle, reason, shift = 0,
  onExpand,
}: {
  item: CatalogItem;
  index: number;
  /**
   * Which way to get out of the way, and how far: -1 for the cards left of the expanded
   * one, +1 for those right of it, 0 when nothing is expanded or this is the one that is.
   * In card widths' worth of pixels — see `NEIGHBOUR_SHIFT`.
   */
  shift?: number;
  /** Tell the rail this card has expanded, or that nothing is. */
  onExpand?: (index: number | null) => void;
  progress?: CardProgress | null;
  onOpen: (item: CatalogItem) => void;
  onPlay: (item: CatalogItem) => void;
  /**
   * Take this off the rail it is on. Only Continue Watching passes one.
   *
   * A rail built from what you happened to start is the one rail that accumulates
   * things you do not want: a film sampled for ten minutes, an episode left running
   * while you fell asleep. Without a way to remove them they sit at the front of the
   * home screen indefinitely, and the rail stops being about what you are watching.
   */
  onRemove?: (item: CatalogItem) => void;
  /**
   * Print the title under the poster.
   *
   * On for the browse grids, off for the home rails. A rail is a short row you read by
   * its artwork; a grid is hundreds of posters you are looking *through*, and a
   * library of films whose artwork is in a script you cannot read is unusable without
   * the names — which is what it was.
   */
  showTitle?: boolean;
  /** Set for the Top 10 rail's numeral treatment (README §8.2). */
  rank?: number;
  /**
   * Why this is being shown — "Because you watched Blade Runner".
   *
   * Only recommendations have one, and it is the whole difference between a rail
   * somebody trusts and a rail of posters that appeared for no stated reason. Shown
   * under the poster rather than on hover: a reason you have to go looking for is one
   * nobody reads.
   */
  reason?: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const [preview, setPreview] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  const animations = useUi((s) => s.animations);
  const hoverPreviews = useUi((s) => s.hoverPreviews);
  const onMyList = useOnMyList(item);
  const liked = useIsLiked(item);
  const toggleMyList = useMarks((s) => s.toggleMyList);
  const toggleLiked = useMarks((s) => s.toggleLiked);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  function enter() {
    if (!animations) return;
    setExpanded(true);
    onExpand?.(index);
    // Nothing to dwell towards without a trailer. `preview` used to be set for every
    // card and drove a label reading "Preview playing" over a still poster.
    if (!hoverPreviews || !item.trailerKey) return;
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setPreview(true), PREVIEW_DELAY_MS);
  }
  function leave() {
    window.clearTimeout(timer.current);
    setExpanded(false);
    setPreview(false);
    onExpand?.(null);
  }

  const pct = progress ? progressPct(progress.positionSecs, progress.durationSecs) : 0;
  const year = item.year ?? undefined;
  const isSeries = item.kind === 'series';

  return (
    <div
      data-testid="catalog-card"
      style={{
        position: 'relative',
        // Expanded cards must paint above their neighbours in both directions.
        zIndex: expanded ? 30 : 1,
      }}
      onMouseEnter={enter}
      onMouseLeave={leave}
    >
      <motion.div
        role="button"
        tabIndex={0}
        aria-label={`${item.title}${year ? `, ${year}` : ''}`}
        onFocus={enter}
        onBlur={leave}
        onClick={() => onOpen(item)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') { e.preventDefault(); onOpen(item); }
          if (e.key === ' ') { e.preventDefault(); onPlay(item); }
        }}
        animate={{
          scale: expanded ? 1.32 : 1,
          y: expanded ? -12 : 0,
          // Out of the expanded card's way. The card that is expanded never shifts: it
          // grows from where it already is, which is what makes the row look like it
          // opened rather than slid.
          x: expanded ? 0 : shift * NEIGHBOUR_SHIFT,
        }}
        transition={{ duration: animations ? 0.22 : 0, ease: [0.16, 1, 0.3, 1] }}
        style={{
          // So the expanded panel below anchors to the card it belongs to rather than
          // to whatever happens to be positioned above it.
          position: 'relative',
          transformOrigin: index === 0 ? 'left center' : 'center center',
          borderRadius: 'var(--r-md)',
          cursor: 'pointer',
          boxShadow: expanded ? 'var(--shadow-4)' : 'none',
          background: 'var(--bg-elevated)',
          outline: 'none',
        }}
      >
        <div style={{ position: 'relative' }}>
          <Poster src={item.poster} alt={item.title} />

          {/* Over the poster, which stays underneath: the trailer is 16:9 and a poster
              is 2:3, so the frame covers the middle band and the artwork fills the rest.
              Mounted only on the one expanded card — a rail of twenty iframes would be
              twenty video players. */}
          {preview && item.trailerKey && (
            <div
              style={{
                position: 'absolute', left: 0, right: 0, top: '50%',
                transform: 'translateY(-50%)', aspectRatio: '16 / 9',
                overflow: 'hidden', background: '#000',
              }}
            >
              <TrailerFrame
                trailerKey={item.trailerKey}
                muted
                title={`Trailer for ${item.title}`}
              />
            </div>
          )}

          {rank !== undefined && (
            <div
              aria-hidden
              style={{
                position: 'absolute', left: -14, bottom: -10,
                fontSize: 'clamp(54px, 7vw, 92px)', fontWeight: 900, lineHeight: 0.8,
                color: 'var(--bg)', WebkitTextStroke: '3px var(--text-faint)',
                pointerEvents: 'none', fontVariantNumeric: 'tabular-nums',
              }}
            >
              {rank}
            </div>
          )}

          <div
            style={{
              position: 'absolute', top: 6, right: 6, display: 'flex', gap: 4,
            }}
          >
            {item.quality === '4K' && <Badge tone="accent">4K</Badge>}
            {isSeries && <Badge tone="outline">Series</Badge>}
          </div>

          {pct > 0 && (
            <div
              data-testid="card-progress"
              style={{ position: 'absolute', left: 8, right: 8, bottom: 8 }}
            >
              <ProgressBar percent={pct} />
            </div>
          )}
        {/*
          * Expanded panel: quick actions + metadata, Netflix-style.
          *
          * **Absolutely positioned, and that is the fix for two visible bugs.** It used
          * to animate `height: 0 -> auto`, which is a change in *layout*: the card grew
          * taller, so the rail's scroll box grew with it — measured at 308px to 412px —
          * and every row below the rail was shoved down and then pulled back as the
          * pointer moved along. That is the jumpiness.
          *
          * It also made the clipping worse. The card is scaled 1.32x on top of its
          * layout height, so a taller card overflows the scroller's padding by more, and
          * `overflow-x: auto` forces `overflow-y` to compute as `auto` whatever the
          * stylesheet asks for — so the surplus is cut rather than allowed to hang out.
          *
          * Out of flow, the card's layout height never changes, the rail never reflows,
          * and the only thing that has to fit is the transform. `RAIL_EXPANSION_PAD` in
          * Rail.tsx is derived from that and nothing else.
          */}
        <AnimatePresence>
          {expanded && (
            <motion.div
              initial={{ opacity: 0, y: -6 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -6 }}
              transition={{ duration: animations ? 0.18 : 0 }}
              style={{
                position: 'absolute',
                // Inside the card, over the foot of the poster — not below it.
                //
                // Hanging it below meant the panel left the card's box, and an
                // absolutely positioned child still counts towards a scroll
                // container's `scrollHeight`: 105px of overflow, measured, which
                // `overflow-x: auto` then clipped. Reserving that much padding would
                // only have swapped the clipping for a 105px band over the next rail
                // that swallowed its hovers.
                left: 0,
                right: 0,
                bottom: 0,
                overflow: 'hidden',
                borderRadius: '0 0 var(--r-md) var(--r-md)',
                // Fades into the artwork rather than sitting on a hard edge, since it
                // is now over the poster instead of under it.
                background:
                  'linear-gradient(to top, var(--bg-elevated) 72%, transparent)',
              }}
            >
              <div style={{ padding: '8px 9px 10px' }}>
                <div style={{ display: 'flex', gap: 5, marginBottom: 7 }}>
                  <IconButton
                    icon="play" label={`Play ${item.title}`} size={26} filled active
                    onClick={(e) => { e.stopPropagation(); onPlay(item); }}
                  />
                  <IconButton
                    icon={onMyList ? 'check' : 'plus'}
                    label={
                      onMyList
                        ? `Remove ${item.title} from My List`
                        : `Add ${item.title} to My List`
                    }
                    aria-pressed={onMyList}
                    active={onMyList}
                    size={26}
                    onClick={(e) => { e.stopPropagation(); toggleMyList(item); }}
                  />
                  <IconButton
                    icon="thumbUp"
                    filled={liked}
                    label={liked ? `Undo liking ${item.title}` : `I like ${item.title}`}
                    aria-pressed={liked}
                    active={liked}
                    size={26}
                    onClick={(e) => { e.stopPropagation(); toggleLiked(item); }}
                  />
                  {onRemove && (
                    <IconButton
                      icon="close"
                      label={`Remove ${item.title} from Continue Watching`}
                      size={26}
                      onClick={(e) => { e.stopPropagation(); onRemove(item); }}
                    />
                  )}
                  <IconButton
                    icon="chevronDown" label="More info" size={26}
                    style={{ marginLeft: 'auto' }}
                    onClick={(e) => { e.stopPropagation(); onOpen(item); }}
                  />
                </div>

                <div
                  style={{
                    display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap',
                    fontSize: 9, color: 'var(--text-muted)',
                  }}
                >
                  {item.match !== undefined && (
                    <span style={{ color: 'var(--success)', fontWeight: 800 }}>
                      {item.match}% match
                    </span>
                  )}
                  {item.certification && (
                    <span
                      style={{
                        border: '1px solid var(--border-strong)', padding: '0 3px',
                        borderRadius: 2,
                      }}
                    >
                      {item.certification}
                    </span>
                  )}
                  {/* Nothing at all for a show whose listing has not been fetched
                      yet. `seasons` is counted from the episodes table, and an import
                      writes the show without its episodes — one request per show is
                      28,741 of them before the library is usable — so the count is
                      unknown until somebody opens it or the background sweep reaches it.
                      This said "0 seasons", which is a confident wrong answer: every
                      show on a freshly imported library claimed to have none. */}
                  {isSeries && item.seasons.length > 0 && (
                    <span>
                      {item.seasons.length} season{item.seasons.length === 1 ? '' : 's'}
                    </span>
                  )}
                  {!isSeries && <span>{runtime(item.runtimeMins)}</span>}
                </div>

                <div
                  style={{
                    marginTop: 5, fontSize: 9, color: 'var(--text-faint)',
                    display: '-webkit-box', WebkitLineClamp: 1, WebkitBoxOrient: 'vertical',
                    overflow: 'hidden',
                  }}
                >
                  {item.genres.slice(0, 3).join(' · ')}
                </div>

                {progress && (
                  <div style={{ marginTop: 5, fontSize: 9, color: 'var(--text-faint)' }}>
                    {remaining(progress.positionSecs, progress.durationSecs)}
                  </div>
                )}

                {preview && item.trailerKey && (
                  <div
                    style={{
                      marginTop: 6, fontSize: 9, color: 'var(--accent-2)',
                      display: 'flex', alignItems: 'center', gap: 4,
                    }}
                  >
                    <span
                      style={{
                        width: 5, height: 5, borderRadius: '50%',
                        background: 'var(--accent-2)',
                      }}
                    />
                    Preview playing
                  </div>
                )}
              </div>
            </motion.div>
          )}
        </AnimatePresence>
        </div>

        {showTitle && (
          <div
            data-testid="card-title"
            style={{
              padding: '6px 2px 0',
              fontSize: 'var(--fs-sm)',
              fontWeight: 600,
              lineHeight: 1.25,
              // Two lines, then ellipsis: a long title must not push its neighbours
              // out of the grid's rows.
              display: '-webkit-box',
              WebkitLineClamp: 2,
              WebkitBoxOrient: 'vertical',
              overflow: 'hidden',
            }}
          >
            {item.title}
          </div>
        )}
        {showTitle && year && (
          <div style={{ padding: '1px 2px 0', fontSize: 'var(--fs-xs)', color: 'var(--text-faint)' }}>
            {year}
          </div>
        )}

        {reason && (
          <div
            data-testid="card-reason"
            title={reason}
            style={{
              padding: '4px 2px 0', fontSize: 'var(--fs-xs)',
              color: 'var(--accent)', fontWeight: 600,
              // One line: a reason that wraps to three pushes the card below it out of
              // line, and the rail stops being a row.
              whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
            }}
          >
            {reason}
          </div>
        )}

      </motion.div>
    </div>
  );
});
