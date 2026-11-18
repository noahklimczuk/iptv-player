/**
 * The PIN keypad, shared by "Who's watching?" and the parental PIN (README §11).
 */
import { useEffect, useMemo, useState } from 'react';
import type { PinOutcome } from '@shared/ipc';
import { Button } from '@/components/Primitives';
import { report } from '@/lib/errors';

/**
 * A four-digit keypad that asks the host whether the PIN is right.
 *
 * One pad for every PIN in the app -- a profile's in "Who's watching?", the parental PIN
 * for locked content and the parental settings -- so the throttle, the countdown and the
 * "Could not check" path behave the same wherever a PIN is asked for. The host decides;
 * `verify` is the command that asks it.
 */
export function PinPad({
  heading, badge, badgeColor, verify, failure, onCancel, onUnlocked, layout = 'screen',
}: {
  heading: string;
  /** One letter, or a glyph, in the tile above the heading. */
  badge: string;
  badgeColor: string;
  verify: (pin: string) => Promise<PinOutcome>;
  /** The title of the notice when the host could not be asked at all. */
  failure: string;
  onCancel: () => void;
  onUnlocked: () => void;
  /** `screen` fills the window, as "Who's watching?" does; `dialog` sits in a modal. */
  layout?: 'screen' | 'dialog';
}) {
  const [digits, setDigits] = useState('');
  const [message, setMessage] = useState<string | null>(null);
  const [lockedUntil, setLockedUntil] = useState(0);
  /**
   * A clock, because the lockout needs one and there was none.
   *
   * `isLocked` and `secondsLeft` were computed from `Date.now()` during render, and
   * nothing scheduled another render. Every key is disabled while locked -- 0-9 and
   * backspace -- so there was nothing left to press that could cause one, and the only
   * enabled control was **Back**. So the pad stayed dead and the countdown stayed
   * frozen at the number it was refused with: "(60s)" for as long as the pad was open.
   *
   * The recovery path was pressing Back and re-entering the profile, which resets
   * `lockedUntil` to 0 locally, re-enables the keys, and has the host refuse again
   * until the real minute is up -- undiscoverable, and confusing once found.
   *
   * `PIN_LOCKOUT_SECS` is 60 and `MAX_PIN_FAILURES` is 5, so the honest behaviour is a
   * minute's wait with a ticking number (R-152).
   */
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    if (lockedUntil <= now) return;
    // A second at a time while locked, and nothing at all when it is not. The deadline
    // itself also gets a timer, so the last partial second does not leave the keys
    // disabled for up to a second after the lockout has expired.
    const tick = window.setInterval(() => setNow(Date.now() / 1000), 500);
    const done = window.setTimeout(
      () => setNow(Date.now() / 1000),
      Math.max(0, (lockedUntil - Date.now() / 1000) * 1000) + 50,
    );
    return () => {
      window.clearInterval(tick);
      window.clearTimeout(done);
    };
  }, [lockedUntil, now]);

  const isLocked = lockedUntil > now;
  const secondsLeft = Math.max(0, Math.ceil(lockedUntil - now));

  const submit = useMemo(
    () => async (pin: string) => {
      // `invoke` can reject -- the database locked, the profile deleted in another
      // window -- and without this the rejection skipped both the success path and
      // `setDigits('')`, leaving the pad showing four filled dots under a generic
      // "Something went wrong" toast with nothing saying the PIN was never checked.
      let outcome: PinOutcome;
      try {
        outcome = await verify(pin);
      } catch (e: unknown) {
        setDigits('');
        setMessage('Could not check that PIN. Try again.');
        report(failure)(e);
        return;
      }
      if (outcome === 'ok' || outcome === 'notRequired') {
        onUnlocked();
        return;
      }
      if (typeof outcome === 'object' && 'lockedOut' in outcome) {
        setLockedUntil(outcome.lockedOut.until);
        setNow(Date.now() / 1000);
        setMessage('Too many attempts. Try again in');
      } else if (typeof outcome === 'object' && 'wrong' in outcome) {
        setMessage(`Incorrect PIN — ${outcome.wrong.remaining} attempts left`);
      }
      setDigits('');
    },
    [verify, failure, onUnlocked],
  );

  const push = (d: string) => {
    if (isLocked) return;
    const next = (digits + d).slice(0, 8);
    setDigits(next);
    setMessage(null);
    if (next.length === 4) void submit(next);
  };

  return (
    <div style={layout === 'screen' ? shell : dialog}>
      <div
        aria-hidden
        style={{
          width: 96, height: 96, borderRadius: 30, display: 'grid', placeItems: 'center',
          background: badgeColor, color: 'var(--on-art)', marginBottom: 20,
          fontFamily: 'var(--font-display)', fontSize: 40, fontWeight: 800,
        }}
      >
        {badge}
      </div>
      <h1
        className="dl-display"
        style={{ margin: '0 0 var(--sp-3)', fontSize: 'var(--fs-2xl)', color: 'var(--text)' }}
      >
        {heading}
      </h1>

      <div
        role="status"
        style={{
          display: 'flex', gap: 'var(--sp-3)', margin: 'var(--sp-4) 0 var(--sp-5)',
        }}
      >
        {[0, 1, 2, 3].map((i) => (
          <div
            key={i}
            style={{
              width: 18, height: 18, borderRadius: '50%',
              border: `2px solid ${i < digits.length ? 'var(--accent)' : 'var(--border-strong)'}`,
              background: i < digits.length ? 'var(--accent)' : 'transparent',
            }}
          />
        ))}
      </div>

      {message && (
        <div
          role="alert"
          style={{
            color: 'var(--danger)', marginBottom: 'var(--sp-4)', fontWeight: 700,
            padding: '10px 16px', borderRadius: 'var(--r-md)',
            background: 'var(--danger-soft)', border: '1px solid var(--danger-border)',
          }}
        >
          {message}
          {isLocked && (
            <span data-testid="pin-countdown"> {secondsLeft}s</span>
          )}
        </div>
      )}

      <div
        style={{
          display: 'grid', gridTemplateColumns: 'repeat(3, 76px)', gap: 'var(--sp-3)',
        }}
      >
        {['1', '2', '3', '4', '5', '6', '7', '8', '9'].map((d) => (
          <PinKey key={d} label={d} disabled={isLocked} onClick={() => push(d)} />
        ))}
        <div />
        <PinKey label="0" disabled={isLocked} onClick={() => push('0')} />
        <PinKey
          label="⌫"
          disabled={isLocked}
          onClick={() => setDigits((s) => s.slice(0, -1))}
        />
      </div>

      <Button variant="outline" onClick={onCancel} style={{ marginTop: 'var(--sp-6)' }}>
        Back
      </Button>
    </div>
  );
}

function PinKey({
  label, onClick, disabled,
}: { label: string; onClick: () => void; disabled: boolean }) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      aria-label={label === '⌫' ? 'Delete' : label}
      style={{
        height: 76, borderRadius: 24, fontSize: 'var(--fs-2xl)', fontWeight: 800,
        border: 'none', background: 'var(--surface)', color: 'var(--text)',
        boxShadow: 'var(--shadow-1)',
        cursor: disabled ? 'not-allowed' : 'pointer', opacity: disabled ? 0.45 : 1,
      }}
    >
      {label}
    </button>
  );
}

export const shell = {
  position: 'fixed', inset: 0, zIndex: 350, background: 'var(--bg)',
  display: 'flex', flexDirection: 'column', alignItems: 'center',
  justifyContent: 'center', padding: 'var(--sp-5)',
} as const;

const dialog = {
  display: 'flex', flexDirection: 'column', alignItems: 'center',
  justifyContent: 'center', padding: 'var(--sp-6) var(--sp-5)',
} as const;
