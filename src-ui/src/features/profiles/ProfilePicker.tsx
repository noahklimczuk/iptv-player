/**
 * "Who's watching?" (README §11) — profile selection, with a PIN keypad for
 * profiles that have one.
 */
import { motion } from 'framer-motion';
import { useEffect, useMemo, useState } from 'react';
import type { PinOutcome, Profile } from '@shared/ipc';
import { Button, artHue } from '@/components/Primitives';
import { Icon } from '@/components/Icon';
import { invoke } from '@/ipc';
import { report } from '@/lib/errors';
import { useProfile } from '@/state/profile';

/**
 * Deterministic avatar colour, so a profile looks the same every launch -- and the same
 * Daylight hue as the profile tile in the top bar, which picks it the same way.
 */
function avatarColor(profile: Profile): string {
  return artHue(profile.name);
}

export function ProfilePicker() {
  const { profiles, activate, load } = useProfile();
  const [locked, setLocked] = useState<Profile | null>(null);

  useEffect(() => { void load(); }, [load]);

  if (locked) {
    return <PinPad profile={locked} onCancel={() => setLocked(null)} onUnlocked={activate} />;
  }

  return (
    <div style={shell}>
      <h1
        className="dl-display"
        style={{ margin: '0 0 56px', fontSize: 'var(--fs-4xl)', color: 'var(--text)' }}
      >
        Who's watching?
      </h1>

      <div
        style={{
          display: 'flex', gap: 40, flexWrap: 'wrap', justifyContent: 'center',
          alignItems: 'flex-start',
        }}
      >
        {profiles.map((profile) => (
          <motion.button
            key={profile.id}
            whileHover={{ scale: 1.04 }}
            whileFocus={{ scale: 1.04 }}
            transition={{ duration: 0.16 }}
            onClick={() => (profile.hasPin ? setLocked(profile) : activate(profile))}
            aria-label={`${profile.name}${profile.hasPin ? ', PIN required' : ''}`}
            style={{
              background: 'none', border: 'none', cursor: 'pointer', padding: 0,
              display: 'grid', gap: 'var(--sp-3)', justifyItems: 'center', color: 'inherit',
            }}
          >
            <div
              style={{
                // A squircle: Daylight's 150px profile tile.
                width: 150, height: 150, borderRadius: 44, display: 'grid',
                placeItems: 'center', position: 'relative',
                background: avatarColor(profile), color: 'var(--on-art)',
                fontFamily: 'var(--font-display)', fontSize: 64, fontWeight: 800,
              }}
            >
              {profile.name.slice(0, 1).toUpperCase()}
            </div>
            <span style={{ fontSize: 'var(--fs-lg)', fontWeight: 800 }}>
              {profile.name}
            </span>
            {(profile.hasPin || profile.isKids) && (
              <span style={{ display: 'flex', gap: 6, marginTop: -4 }}>
                {profile.hasPin && (
                  <span className="dl-chip">
                    <Icon name="settings" size={11} />
                    PIN
                  </span>
                )}
                {profile.isKids && <span className="dl-chip" data-tone="success">KIDS</span>}
              </span>
            )}
          </motion.button>
        ))}
      </div>
    </div>
  );
}

function PinPad({
  profile, onCancel, onUnlocked,
}: {
  profile: Profile;
  onCancel: () => void;
  onUnlocked: (p: Profile) => void;
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
        outcome = await invoke('profiles.verifyPin', {
          profileId: profile.id,
          pin,
        });
      } catch (e: unknown) {
        setDigits('');
        setMessage('Could not check that PIN. Try again.');
        report(`Could not check ${profile.name}'s PIN`)(e);
        return;
      }
      if (outcome === 'ok' || outcome === 'notRequired') {
        onUnlocked(profile);
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
    [profile, onUnlocked],
  );

  const push = (d: string) => {
    if (isLocked) return;
    const next = (digits + d).slice(0, 8);
    setDigits(next);
    setMessage(null);
    if (next.length === 4) void submit(next);
  };

  return (
    <div style={shell}>
      <div
        aria-hidden
        style={{
          width: 96, height: 96, borderRadius: 30, display: 'grid', placeItems: 'center',
          background: avatarColor(profile), color: 'var(--on-art)', marginBottom: 20,
          fontFamily: 'var(--font-display)', fontSize: 40, fontWeight: 800,
        }}
      >
        {profile.name.slice(0, 1).toUpperCase()}
      </div>
      <h1
        className="dl-display"
        style={{ margin: '0 0 var(--sp-3)', fontSize: 'var(--fs-2xl)', color: 'var(--text)' }}
      >
        Enter {profile.name}'s PIN
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

const shell = {
  position: 'fixed', inset: 0, zIndex: 350, background: 'var(--bg)',
  display: 'flex', flexDirection: 'column', alignItems: 'center',
  justifyContent: 'center', padding: 'var(--sp-5)',
} as const;
