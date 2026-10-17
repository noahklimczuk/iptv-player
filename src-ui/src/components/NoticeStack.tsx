/**
 * Where a failure is actually shown.
 *
 * Deliberately not a modal: most of these arrive while something is playing, and a
 * dialog over live TV to say a poster would not load is worse than the problem. A
 * toast that names what was being attempted, says what the host said, and goes away.
 */
import { useEffect, useRef, useState } from 'react';
import { Icon } from '@/components/Icon';
import { type AppNotice, dismissNotice, subscribeToNotices } from '@/lib/errors';

/** Long enough to read two lines, short enough not to sit over the picture. */
const DISMISS_AFTER_MS = 8000;

export function NoticeStack() {
  const [notices, setNotices] = useState<AppNotice[]>([]);

  useEffect(() => subscribeToNotices(setNotices), []);

  /**
   * One timer per notice, started once and left alone.
   *
   * This effect depended on the whole `notices` array, so every change tore down *all*
   * the timers and started them again from zero: a toast that had been up for seven of
   * its eight seconds went back to eight the moment another arrived. `errors.ts` caps
   * the stack at three and dedupes by title, so this could not grow without bound --
   * but it did not bound the *time*, and a sequence of distinct failures arriving more
   * often than every eight seconds (which is what a provider refusing artwork during a
   * browse produces) kept the oldest toast on screen for as long as the sequence lasted.
   *
   * `n.id` is already stable, so the timers are keyed on it: a notice that already has
   * one keeps it, and the eight seconds mean eight seconds (R-97).
   */
  const timers = useRef(new Map<number, number>());
  useEffect(() => {
    const live = new Set(notices.map((n) => n.id));
    for (const [id, timer] of timers.current) {
      if (!live.has(id)) {
        window.clearTimeout(timer);
        timers.current.delete(id);
      }
    }
    for (const n of notices) {
      if (timers.current.has(n.id)) continue;
      timers.current.set(
        n.id,
        window.setTimeout(() => {
          timers.current.delete(n.id);
          dismissNotice(n.id);
        }, DISMISS_AFTER_MS),
      );
    }
  }, [notices]);

  // On unmount only. Clearing on every change is what the bug was.
  useEffect(
    () => () => {
      for (const timer of timers.current.values()) window.clearTimeout(timer);
      timers.current.clear();
    },
    [],
  );

  if (notices.length === 0) return null;

  return (
    <div
      // `status`, not `alert`: these are reported after the fact and should not
      // interrupt a screen reader mid-sentence.
      role="status"
      aria-live="polite"
      data-testid="notices"
      style={{
        position: 'fixed', right: 'var(--sp-5)', bottom: 'var(--sp-5)', zIndex: 400,
        display: 'grid', gap: 'var(--sp-2)', maxWidth: 420,
      }}
    >
      {notices.map((n) => (
        <div
          key={n.id}
          style={{
            display: 'grid', gridTemplateColumns: '20px 1fr 24px', gap: 'var(--sp-3)',
            alignItems: 'start', padding: 'var(--sp-3) var(--sp-4)',
            background: 'var(--bg-elevated)', color: 'var(--text)',
            border: '1px solid var(--border)', borderRadius: 'var(--r-md)',
            boxShadow: '0 10px 30px rgb(0 0 0 / 0.35)',
          }}
        >
          <Icon name="alert" size={18} style={{ color: 'var(--danger, #ff6b6b)' }} />
          <div style={{ minWidth: 0 }}>
            <div style={{ fontWeight: 650, fontSize: 'var(--fs-sm)' }}>{n.title}</div>
            {n.detail && (
              <div
                style={{
                  marginTop: 2, fontSize: 'var(--fs-xs)', color: 'var(--text-muted)',
                  wordBreak: 'break-word',
                }}
              >
                {n.detail}
              </div>
            )}
          </div>
          <button
            type="button"
            aria-label="Dismiss"
            onClick={() => dismissNotice(n.id)}
            style={{
              background: 'transparent', border: 'none', cursor: 'pointer',
              color: 'var(--text-faint)', display: 'grid', placeItems: 'center',
            }}
          >
            <Icon name="close" size={15} />
          </button>
        </div>
      ))}
    </div>
  );
}
