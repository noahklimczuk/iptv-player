/**
 * Rail card with the Netflix expansion behaviour (README §8.3):
 * hover or focus scales the card ~1.35x, lifts it above its neighbours, pushes siblings
 * aside, and after a dwell delay plays a muted preview. Focus produces the identical
 * expansion so the whole thing works from a remote.
 */
import { AnimatePresence, motion } from 'framer-motion';
import { memo, useEffect, useRef, useState } from 'react';
import type { CatalogItem } from '@shared/ipc';
import { progressPct, remaining, runtime } from '@/lib/format';
import { useUi } from '@/state/ui';
import { Badge, IconButton, ProgressBar, Poster } from './Primitives';

const PREVIEW_DELAY_MS = 700;

export interface CardProgress {
  positionSecs: number;
  durationSecs: number;
}

export const CatalogCard = memo(function CatalogCard({
  item, index, progress, onOpen, onPlay, rank, showTitle, reason,
}: {
  item: CatalogItem;
  index: number;
  progress?: CardProgress | null;
  onOpen: (item: CatalogItem) => void;
  onPlay: (item: CatalogItem) => void;
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

  useEffect(() => () => window.clearTimeout(timer.current), []);

  function enter() {
    if (!animations) return;
    setExpanded(true);
    if (!hoverPreviews) return;
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setPreview(true), PREVIEW_DELAY_MS);
  }
  function leave() {
    window.clearTimeout(timer.current);
    setExpanded(false);
    setPreview(false);
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
        }}
        transition={{ duration: animations ? 0.22 : 0, ease: [0.16, 1, 0.3, 1] }}
        style={{
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

        {/* Expanded panel: quick actions + metadata, Netflix-style. */}
        <AnimatePresence>
          {expanded && (
            <motion.div
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: 'auto' }}
              exit={{ opacity: 0, height: 0 }}
              transition={{ duration: animations ? 0.18 : 0 }}
              style={{
                overflow: 'hidden',
                background: 'var(--bg-elevated)',
                borderRadius: '0 0 var(--r-md) var(--r-md)',
              }}
            >
              <div style={{ padding: '8px 9px 10px' }}>
                <div style={{ display: 'flex', gap: 5, marginBottom: 7 }}>
                  <IconButton
                    icon="play" label={`Play ${item.title}`} size={26} filled active
                    onClick={(e) => { e.stopPropagation(); onPlay(item); }}
                  />
                  <IconButton
                    icon="plus" label="Add to My List" size={26}
                    onClick={(e) => e.stopPropagation()}
                  />
                  <IconButton
                    icon="thumbUp" label="I like this" size={26}
                    onClick={(e) => e.stopPropagation()}
                  />
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
                  <span>
                    {isSeries
                      ? `${item.seasons.length} season${item.seasons.length === 1 ? '' : 's'}`
                      : runtime(item.runtimeMins)}
                  </span>
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

                {preview && (
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
      </motion.div>
    </div>
  );
});
