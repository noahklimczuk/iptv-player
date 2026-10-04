/**
 * Unified search + command palette (README §10): results grouped by kind, keyboard
 * navigable, debounced to keep keystroke-to-paint under the §16 budget.
 */
import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useMemo, useRef, useState } from 'react';
import type { SearchHit, SearchResults } from '@shared/ipc';
import { Icon, type IconName } from '@/components/Icon';
import { invoke } from '@/ipc';
import { report } from '@/lib/errors';

/**
 * How many results of one kind are shown before the rest are folded away.
 *
 * Every group used to render in full into a 52vh scroller, and the guide groups are the
 * big ones: searching a common word returned eight programmes on now and eight more
 * upcoming, which filled the panel and pushed Movies and Series below the fold with
 * nothing on screen to say they existed. On a library of twenty-four thousand films,
 * searching for a film and seeing only live TV reads as "my films are not searchable".
 *
 * Four of each, so every kind that matched is visible at once, with the true count beside
 * the heading and the rest one click away. Narrowing the query is the other way out, and
 * is the one a palette is for.
 */
const PER_GROUP = 4;

/**
 * Order matters more than it looks, because the panel is 52vh and the list is long.
 *
 * The library goes above the guide. Someone who opens search and types a title is
 * usually looking for the title; the guide's matches are every airing of every
 * programme whose name contains the word, which on a real subscription is the bulk of
 * the results and almost never the thing being asked for. With the guide first, a film
 * search ended below the fold and the panel looked like it only knew about live TV.
 */
const GROUPS: { key: keyof SearchResults; label: string; icon: IconName }[] = [
  { key: 'channels', label: 'Live Channels', icon: 'tv' },
  { key: 'movies', label: 'Movies', icon: 'film' },
  { key: 'series', label: 'Series', icon: 'stack' },
  { key: 'onNow', label: 'On Now', icon: 'clock' },
  { key: 'upcoming', label: 'Upcoming', icon: 'bell' },
  { key: 'people', label: 'People', icon: 'heart' },
];

export function CommandPalette({
  open, onClose, onPick, initialQuery = '',
}: {
  open: boolean;
  onClose: () => void;
  onPick: (hit: SearchHit) => void;
  /** Opened from somewhere with a subject in mind — a programme title, say. */
  initialQuery?: string;
}) {
  const [text, setText] = useState('');
  const [results, setResults] = useState<SearchResults | null>(null);
  const [cursor, setCursor] = useState(0);
  const [expanded, setExpanded] = useState<string[]>([]);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setText(initialQuery);
      setResults(null);
      setCursor(0);
      setExpanded([]);
      requestAnimationFrame(() => {
        inputRef.current?.focus();
        // Selected rather than appended to: the seed is a starting point, and typing
        // over it is the most likely next thing.
        inputRef.current?.select();
      });
    }
  }, [open, initialQuery]);

  useEffect(() => {
    if (!open) return;
    const t = window.setTimeout(() => {
      invoke('search.query', { text })
        .then(setResults)
        .catch(report('Search failed'));
    }, 90);
    return () => window.clearTimeout(t);
  }, [text, open]);

  /** What each group is showing right now — the first few, or all of them. */
  const shown = useMemo(() => {
    const out = new Map<string, SearchHit[]>();
    for (const g of GROUPS) {
      const hits = results?.[g.key] ?? [];
      out.set(g.key, expanded.includes(g.key) ? hits : hits.slice(0, PER_GROUP));
    }
    return out;
  }, [results, expanded]);

  /**
   * The keyboard's view of the list, which has to be what is on screen.
   *
   * Built from the visible hits rather than from every result: arrow keys walking rows
   * that are folded away would move the highlight to nothing and pick something the
   * viewer never saw.
   */
  const flat = useMemo(
    () => GROUPS.flatMap((g) => shown.get(g.key) ?? []),
    [shown],
  );

  useEffect(() => setCursor(0), [flat.length]);
  // A new query is a new set of groups; whatever was unfolded no longer applies.
  useEffect(() => setExpanded([]), [text]);

  if (!open) return null;

  return (
    <AnimatePresence>
      <motion.div
        initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}
        transition={{ duration: 0.12 }}
        onClick={onClose}
        style={{
          position: 'fixed', inset: 0, zIndex: 300, background: 'rgb(0 0 0 / 0.6)',
          backdropFilter: 'blur(3px)', paddingTop: '12vh',
        }}
      >
        <motion.div
          initial={{ y: -14, scale: 0.98 }} animate={{ y: 0, scale: 1 }}
          transition={{ duration: 0.16, ease: [0.16, 1, 0.3, 1] }}
          onClick={(e) => e.stopPropagation()}
          role="dialog" aria-modal="true" aria-label="Search"
          style={{
            width: 'min(680px, 92vw)', margin: '0 auto', background: 'var(--bg-elevated)',
            border: '1px solid var(--border-strong)', borderRadius: 'var(--r-lg)',
            boxShadow: 'var(--shadow-4)', overflow: 'hidden',
          }}
        >
          <div
            style={{
              display: 'flex', alignItems: 'center', gap: 'var(--sp-3)',
              padding: 'var(--sp-4)', borderBottom: '1px solid var(--border)',
            }}
          >
            <Icon name="search" size={20} style={{ color: 'var(--text-faint)' }} />
            <input
              ref={inputRef}
              value={text}
              placeholder="Search channels, guide, movies, series, people…"
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Escape') onClose();
                if (e.key === 'ArrowDown') {
                  e.preventDefault();
                  setCursor((c) => Math.min(c + 1, flat.length - 1));
                }
                if (e.key === 'ArrowUp') {
                  e.preventDefault();
                  setCursor((c) => Math.max(0, c - 1));
                }
                if (e.key === 'Enter' && flat[cursor]) {
                  onPick(flat[cursor]!);
                  onClose();
                }
              }}
              style={{
                flex: 1, background: 'transparent', border: 'none', outline: 'none',
                color: 'var(--text)', fontSize: 'var(--fs-lg)',
              }}
            />
            <kbd
              style={{
                fontSize: 'var(--fs-xs)', color: 'var(--text-faint)',
                border: '1px solid var(--border-strong)', borderRadius: 4, padding: '2px 6px',
              }}
            >
              Esc
            </kbd>
          </div>

          <div style={{ maxHeight: '52vh', overflowY: 'auto', padding: 'var(--sp-2)' }}>
            {!text && (
              <div
                style={{
                  padding: 'var(--sp-5)', color: 'var(--text-faint)',
                  fontSize: 'var(--fs-sm)', textAlign: 'center',
                }}
              >
                Type to search across live channels, the next 7 days of guide data,
                your movies, series, and cast.
              </div>
            )}
            {text && flat.length === 0 && (
              <div
                style={{
                  padding: 'var(--sp-5)', color: 'var(--text-faint)',
                  fontSize: 'var(--fs-sm)', textAlign: 'center',
                }}
              >
                Nothing matches “{text}”.
              </div>
            )}

            {results && GROUPS.map((g) => {
              const all = results[g.key];
              if (all.length === 0) return null;
              const hits = shown.get(g.key) ?? [];
              // Counted over what is drawn, not over what matched, so the keyboard
              // index and the rows on screen stay the same list.
              const offset = GROUPS.slice(0, GROUPS.indexOf(g))
                .reduce((n, gg) => n + (shown.get(gg.key)?.length ?? 0), 0);
              const folded = all.length - hits.length;
              return (
                <div
                  key={g.key}
                  // So a journey can click a result of a known kind: picking the first
                  // button in the dialog picks whatever group happens to be first, which
                  // is usually a channel.
                  data-testid={`palette-group-${g.key}`}
                  style={{ marginBottom: 'var(--sp-2)' }}
                >
                  <div
                    style={{
                      display: 'flex', alignItems: 'center', gap: 6,
                      padding: 'var(--sp-2) var(--sp-3)', fontSize: 'var(--fs-xs)',
                      color: 'var(--text-faint)', fontWeight: 700,
                      letterSpacing: '0.06em', textTransform: 'uppercase',
                    }}
                  >
                    <Icon name={g.icon} size={13} />
                    {g.label}
                    <span style={{ opacity: 0.6 }}>{all.length}</span>
                  </div>
                  {hits.map((hit, i) => {
                    const idx = offset + i;
                    return (
                      <button
                        key={`${hit.kind}-${hit.refId}-${i}`}
                        onMouseEnter={() => setCursor(idx)}
                        onClick={() => { onPick(hit); onClose(); }}
                        style={{
                          display: 'flex', alignItems: 'center', gap: 'var(--sp-3)',
                          width: '100%', padding: 'var(--sp-2) var(--sp-3)',
                          borderRadius: 'var(--r-md)', border: 'none', cursor: 'pointer',
                          textAlign: 'left', color: 'inherit',
                          background: cursor === idx ? 'var(--surface-hover)' : 'transparent',
                        }}
                      >
                        <span style={{ flex: 1, fontSize: 'var(--fs-md)' }}>{hit.title}</span>
                        {hit.subtitle && (
                          <span style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-faint)' }}>
                            {hit.subtitle}
                          </span>
                        )}
                      </button>
                    );
                  })}
                  {folded > 0 && (
                    <button
                      onClick={() => setExpanded((e) => [...e, g.key])}
                      style={{
                        width: '100%', padding: 'var(--sp-2) var(--sp-3)',
                        background: 'transparent', border: 'none', cursor: 'pointer',
                        textAlign: 'left', color: 'var(--text-faint)',
                        fontSize: 'var(--fs-sm)', borderRadius: 'var(--r-md)',
                      }}
                    >
                      Show {folded} more {g.label.toLowerCase()}
                    </button>
                  )}
                </div>
              );
            })}
          </div>
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
}
