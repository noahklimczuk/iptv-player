/**
 * Where a failure is actually shown.
 *
 * Deliberately not a modal: most of these arrive while something is playing, and a
 * dialog over live TV to say a poster would not load is worse than the problem. A
 * toast that names what was being attempted, says what the host said, and goes away.
 */
import { useEffect, useState } from 'react';
import { Icon } from '@/components/Icon';
import { type AppNotice, dismissNotice, subscribeToNotices } from '@/lib/errors';

/** Long enough to read two lines, short enough not to sit over the picture. */
const DISMISS_AFTER_MS = 8000;

export function NoticeStack() {
  const [notices, setNotices] = useState<AppNotice[]>([]);

  useEffect(() => subscribeToNotices(setNotices), []);

  useEffect(() => {
    if (notices.length === 0) return;
    const timers = notices.map((n) =>
      window.setTimeout(() => dismissNotice(n.id), DISMISS_AFTER_MS),
    );
    return () => timers.forEach((t) => window.clearTimeout(t));
  }, [notices]);

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
