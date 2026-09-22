/**
 * "Who's watching?" (README §11) — profile selection, with a PIN keypad for
 * profiles that have one.
 */
import { motion } from 'framer-motion';
import { useEffect, useMemo, useState } from 'react';
import type { PinOutcome, Profile } from '@shared/ipc';
import { Button } from '@/components/Primitives';
import { Icon } from '@/components/Icon';
import { invoke } from '@/ipc';
import { useProfile } from '@/state/profile';

/** Deterministic avatar colour, so a profile looks the same every launch. */
function avatarColor(profile: Profile): string {
  let n = 0;
  for (const c of profile.name) n = (n * 31 + c.charCodeAt(0)) >>> 0;
  return `hsl(${n % 360} 55% 42%)`;
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
      <h1 style={{ margin: '0 0 var(--sp-7)', fontSize: 'var(--fs-2xl)', fontWeight: 800 }}>
        Who's watching?
      </h1>

      <div
        style={{
          display: 'flex', gap: 'var(--sp-5)', flexWrap: 'wrap', justifyContent: 'center',
        }}
      >
        {profiles.map((profile) => (
          <motion.button
            key={profile.id}
            whileHover={{ scale: 1.06 }}
            whileFocus={{ scale: 1.06 }}
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
                width: 118, height: 118, borderRadius: 'var(--r-lg)', display: 'grid',
                placeItems: 'center', position: 'relative',
                background: avatarColor(profile), fontSize: 46, fontWeight: 800, color: '#fff',
              }}
            >
              {profile.name.slice(0, 1).toUpperCase()}
              {profile.hasPin && (
                <span
                  aria-hidden
                  style={{
                    position: 'absolute', right: 8, bottom: 8, width: 26, height: 26,
                    borderRadius: '50%', background: 'rgb(0 0 0 / 0.55)',
                    display: 'grid', placeItems: 'center',
                  }}
                >
                  <Icon name="settings" size={14} />
                </span>
              )}
            </div>
            <span style={{ fontSize: 'var(--fs-md)', fontWeight: 600 }}>
              {profile.name}
              {profile.isKids && (
                <span style={{ marginLeft: 6, fontSize: 'var(--fs-xs)', color: 'var(--accent-2)' }}>
                  KIDS
                </span>
              )}
            </span>
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

  const isLocked = lockedUntil > Date.now() / 1000;
  const secondsLeft = Math.max(0, Math.ceil(lockedUntil - Date.now() / 1000));

  const submit = useMemo(
    () => async (pin: string) => {
      const outcome: PinOutcome = await invoke('profiles.verifyPin', {
        profileId: profile.id,
        pin,
      });
      if (outcome === 'ok' || outcome === 'notRequired') {
        onUnlocked(profile);
        return;
      }
      if (typeof outcome === 'object' && 'lockedOut' in outcome) {
        setLockedUntil(outcome.lockedOut.until);
        setMessage('Too many attempts. Try again shortly.');
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
      <h1 style={{ margin: '0 0 var(--sp-3)', fontSize: 'var(--fs-xl)', fontWeight: 700 }}>
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
              border: '2px solid var(--border-strong)',
              background: i < digits.length ? 'var(--text)' : 'transparent',
            }}
          />
        ))}
      </div>

      {message && (
        <div role="alert" style={{ color: 'var(--danger)', marginBottom: 'var(--sp-4)' }}>
          {message}{isLocked ? ` (${secondsLeft}s)` : ''}
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

      <Button variant="ghost" onClick={onCancel} style={{ marginTop: 'var(--sp-5)' }}>
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
        height: 76, borderRadius: 'var(--r-lg)', fontSize: 'var(--fs-xl)', fontWeight: 600,
        border: '1px solid var(--border)', background: 'var(--surface)', color: 'var(--text)',
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
