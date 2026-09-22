/**
 * Unified search + command palette (README §10): results grouped by kind, keyboard
 * navigable, debounced to keep keystroke-to-paint under the §16 budget.
 */
import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useMemo, useRef, useState } from 'react';
import type { SearchHit, SearchResults } from '@shared/ipc';
import { Icon, type IconName } from '@/components/Icon';
import { invoke } from '@/ipc';

const GROUPS: { key: keyof SearchResults; label: string; icon: IconName }[] = [
  { key: 'channels', label: 'Live Channels', icon: 'tv' },
  { key: 'onNow', label: 'On Now', icon: 'clock' },
  { key: 'upcoming', label: 'Upcoming', icon: 'bell' },
  { key: 'movies', label: 'Movies', icon: 'film' },
  { key: 'series', label: 'Series', icon: 'stack' },
  { key: 'people', label: 'People', icon: 'heart' },
];

export function CommandPalette({
  open, onClose, onPick,
}: {
  open: boolean;
  onClose: () => void;
  onPick: (hit: SearchHit) => void;
}) {
  const [text, setText] = useState('');
  const [results, setResults] = useState<SearchResults | null>(null);
  const [cursor, setCursor] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setText('');
      setResults(null);
      setCursor(0);
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const t = window.setTimeout(() => {
      void invoke('search.query', { text }).then(setResults);
    }, 90);
    return () => window.clearTimeout(t);
  }, [text, open]);

  const flat = useMemo(() => {
    if (!results) return [] as SearchHit[];
    return GROUPS.flatMap((g) => results[g.key]);
  }, [results]);

  useEffect(() => setCursor(0), [flat.length]);

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
              const hits = results[g.key];
              if (hits.length === 0) return null;
              const offset = GROUPS.slice(0, GROUPS.indexOf(g))
                .reduce((n, gg) => n + results[gg.key].length, 0);
              return (
                <div key={g.key} style={{ marginBottom: 'var(--sp-2)' }}>
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
                    <span style={{ opacity: 0.6 }}>{hits.length}</span>
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
                </div>
              );
            })}
          </div>
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
}
