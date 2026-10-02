/**
 * What is on screen between the window appearing and the app knowing what to show.
 *
 * There are two gaps at launch and this covers the second. The first is before any
 * script runs, which `index.html` paints inline because the stylesheet defining these
 * tokens is itself a request that has not finished; the window stays hidden until that
 * is up (`commands::window_ready`), so the transparency that lets mpv show through is
 * never a hole into the desktop.
 *
 * The second gap is this one: React has mounted, and the two questions that decide the
 * first screen — is there a provider, and which profile — are still in flight. Rendering
 * the answer-shaped screen early meant an empty "Who's watching?" appearing for a moment
 * and then being replaced, which reads as a glitch rather than as loading.
 *
 * Deliberately the same marks, colours and positions as the inline splash, so the
 * handover is invisible: the mark does not move, and only the line underneath changes.
 */
import { useEffect, useState } from 'react';

/**
 * How long to wait before admitting this is slow.
 *
 * A fast launch should never show it — a reassurance that flashes past is noise. A cold
 * disk with a large library genuinely takes a few seconds, and saying nothing for that
 * long is how an app comes to look hung.
 */
const PATIENCE_MS = 2500;

export function BootScreen({ label = 'Starting Aurora…' }: { label?: string }) {
  const [slow, setSlow] = useState(false);

  useEffect(() => {
    const t = window.setTimeout(() => setSlow(true), PATIENCE_MS);
    return () => window.clearTimeout(t);
  }, []);

  return (
    <div
      data-testid="boot-screen"
      role="status"
      aria-live="polite"
      style={{
        position: 'fixed',
        inset: 0,
        // Opaque, and stated here rather than inherited. `body` is transparent by design
        // so video can composite behind it, which makes every full-bleed screen
        // responsible for its own background.
        background:
          'radial-gradient(120% 90% at 50% 0%, var(--bg-elevated) 0%, var(--bg) 60%)',
        display: 'grid',
        placeItems: 'center',
        gap: 'var(--sp-5)',
        gridAutoFlow: 'row',
        zIndex: 100,
      }}
    >
      <div
        aria-hidden
        style={{
          width: 56,
          height: 56,
          borderRadius: 13,
          display: 'grid',
          placeItems: 'center',
          fontWeight: 800,
          fontSize: 26,
          color: '#fff',
          background: 'linear-gradient(135deg, var(--accent), var(--accent-2))',
          boxShadow: 'var(--shadow-3)',
        }}
      >
        A
      </div>

      <div style={{ display: 'grid', gap: 'var(--sp-3)', justifyItems: 'center' }}>
        {/* An indeterminate bar rather than a spinner: there is no percentage to be
            honest about, and a bar reads as "working" at a glance without spinning in
            the middle of an otherwise still screen. */}
        <div
          aria-hidden
          style={{
            width: 160,
            height: 3,
            borderRadius: 'var(--r-full)',
            background: 'var(--surface)',
            overflow: 'hidden',
          }}
        >
          <div className="boot-sweep" />
        </div>
        <div style={{ color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>{label}</div>
        {/* Only once it has gone on long enough to be worth explaining. */}
        {slow && (
          <div
            style={{
              color: 'var(--text-faint)',
              fontSize: 'var(--fs-xs)',
              opacity: 0.8,
              maxWidth: 320,
              textAlign: 'center',
            }}
          >
            A large library takes a moment to open.
          </div>
        )}
      </div>
    </div>
  );
}
