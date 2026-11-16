/**
 * "Who's watching?" (README §11) — profile selection, with a PIN keypad for
 * profiles that have one.
 */
import { motion } from 'framer-motion';
import { useEffect, useMemo, useState } from 'react';
import type { PinOutcome, Profile } from '@shared/ipc';
import { Button, TextField, artHue } from '@/components/Primitives';
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

/** The host's `repo::profiles::MAX_PROFILES`. */
const MAX_PROFILES = 6;

export function ProfilePicker() {
  const { profiles, active, activate, load, setPicking } = useProfile();
  const [locked, setLocked] = useState<Profile | null>(null);
  const [managing, setManaging] = useState(false);
  /** The profile being edited, or `'new'` while one is being added. */
  const [editing, setEditing] = useState<Profile | 'new' | null>(null);

  useEffect(() => { void load(); }, [load]);

  if (locked) {
    return <PinPad profile={locked} onCancel={() => setLocked(null)} onUnlocked={activate} />;
  }
  if (editing) {
    return (
      <ProfileEditor
        profile={editing === 'new' ? null : editing}
        canDelete={profiles.length > 1}
        onDone={() => {
          setEditing(null);
          load().catch(report('Could not load profiles'));
        }}
      />
    );
  }

  return (
    <div style={shell}>
      {active && (
        <div style={{ position: 'absolute', top: 'var(--sp-5)', right: 'var(--sp-5)' }}>
          <Button variant="ghost" icon="close" onClick={() => setPicking(false)}>
            Back
          </Button>
        </div>
      )}
      <h1
        className="dl-display"
        style={{ margin: '0 0 56px', fontSize: 'var(--fs-4xl)', color: 'var(--text)' }}
      >
        {managing ? 'Manage profiles' : "Who's watching?"}
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
            whileHover={{ scale: 1.02 }}
            whileFocus={{ scale: 1.02 }}
            transition={{ duration: 0.16 }}
            onClick={() => {
              if (managing) setEditing(profile);
              else if (profile.hasPin) setLocked(profile);
              else activate(profile);
            }}
            aria-label={managing
              ? `Edit ${profile.name}`
              : `${profile.name}${profile.hasPin ? ', PIN required' : ''}`}
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
              {managing && (
                <span
                  aria-hidden
                  style={{
                    position: 'absolute', inset: 0, borderRadius: 44, display: 'grid',
                    placeItems: 'center', background: 'rgba(0,0,0,0.45)',
                  }}
                >
                  <Icon name="settings" size={40} />
                </span>
              )}
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
        {!managing && profiles.length < MAX_PROFILES && (
          <motion.button
            whileHover={{ scale: 1.02 }}
            whileFocus={{ scale: 1.02 }}
            transition={{ duration: 0.16 }}
            onClick={() => setEditing('new')}
            aria-label="Add profile"
            style={{
              background: 'none', border: 'none', cursor: 'pointer', padding: 0,
              display: 'grid', gap: 'var(--sp-3)', justifyItems: 'center', color: 'inherit',
            }}
          >
            <div
              style={{
                width: 150, height: 150, borderRadius: 44, display: 'grid',
                placeItems: 'center', border: '3px dashed var(--border-strong)',
                color: 'var(--text-muted)', fontSize: 72, fontWeight: 300,
              }}
            >
              +
            </div>
            <span style={{ fontSize: 'var(--fs-lg)', fontWeight: 800, color: 'var(--text-muted)' }}>
              Add profile
            </span>
          </motion.button>
        )}
      </div>

      <div style={{ marginTop: 56 }}>
        <Button
          variant={managing ? 'primary' : 'secondary'}
          onClick={() => setManaging((m) => !m)}
        >
          {managing ? 'Done' : 'Manage profiles'}
        </Button>
      </div>
    </div>
  );
}

/**
 * Add a profile, or change one: its name, its PIN, or whether it exists at all.
 *
 * The host has had commands for all of this since profiles were introduced, but no
 * screen ever called them, so "can't create or edit profiles" was simply true.
 */
function ProfileEditor({
  profile, canDelete, onDone,
}: {
  /** `null` to add a new profile. */
  profile: Profile | null;
  canDelete: boolean;
  onDone: () => void;
}) {
  const [name, setName] = useState(profile?.name ?? '');
  const [kids, setKids] = useState(profile?.isKids ?? false);
  const [pin, setPin] = useState('');
  const [removePin, setRemovePin] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const trimmed = name.trim();
  const pinOk = pin === '' || /^\d{4}$/.test(pin);
  const canSave = trimmed.length > 0 && pinOk && !busy;

  async function run(work: () => Promise<unknown>) {
    setBusy(true);
    setError(null);
    try {
      await work();
      onDone();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  }

  const save = () => run(async () => {
    if (!profile) {
      const id = await invoke('profiles.create', {
        name: trimmed, isKids: kids, maxAge: kids ? 12 : null,
      });
      if (pin) await invoke('profiles.setPin', { profileId: id, pin });
      return;
    }
    if (trimmed !== profile.name) {
      await invoke('profiles.rename', { profileId: profile.id, name: trimmed });
    }
    if (pin) await invoke('profiles.setPin', { profileId: profile.id, pin });
    else if (removePin) await invoke('profiles.setPin', { profileId: profile.id, pin: null });
  });

  const remove = () => run(async () => {
    if (!profile) return;
    const gone = await invoke('profiles.delete', { profileId: profile.id });
    if (!gone) throw new Error('The last profile cannot be deleted.');
  });

  const field = { display: 'grid', gap: 6, fontWeight: 700 } as const;
  const check = { display: 'flex', gap: 10, alignItems: 'center', fontWeight: 700 } as const;

  return (
    <div style={shell}>
      <form
        aria-label={profile ? `Edit ${profile.name}` : 'Add profile'}
        onSubmit={(e) => {
          e.preventDefault();
          if (canSave) void save();
        }}
        style={{ display: 'grid', gap: 'var(--sp-4)', width: 'min(420px, 100%)' }}
      >
        <h1
          className="dl-display"
          style={{ margin: 0, fontSize: 'var(--fs-3xl)', color: 'var(--text)' }}
        >
          {profile ? `Edit ${profile.name}` : 'Add profile'}
        </h1>

        <div style={field}>
          <span id="profile-name-label">Name</span>
          <TextField
            aria-labelledby="profile-name-label"
            value={name} maxLength={24}
            onChange={(e) => setName(e.target.value)}
          />
        </div>

        {!profile && (
          <label style={check}>
            <input type="checkbox" checked={kids} onChange={(e) => setKids(e.target.checked)} />
            Kids profile (rated 12 and under)
          </label>
        )}

        <div style={field}>
          <span id="profile-pin-label">
            {profile?.hasPin ? 'New PIN (leave empty to keep the current one)' : 'PIN (optional, 4 digits)'}
          </span>
          <TextField
            aria-labelledby="profile-pin-label"
            value={pin}
            type="password"
            inputMode="numeric"
            autoComplete="off"
            maxLength={4}
            aria-invalid={!pinOk}
            onChange={(e) => {
              setPin(e.target.value.replace(/\D/g, ''));
              setRemovePin(false);
            }}
          />
        </div>
        {!pinOk && <span role="alert" style={{ color: 'var(--danger)' }}>A PIN is 4 digits.</span>}

        {profile?.hasPin && !pin && (
          <label style={check}>
            <input
              type="checkbox" checked={removePin}
              onChange={(e) => setRemovePin(e.target.checked)}
            />
            Remove the PIN
          </label>
        )}

        {error && <span role="alert" style={{ color: 'var(--danger)' }}>{error}</span>}

        <div style={{ display: 'flex', gap: 'var(--sp-3)', justifyContent: 'flex-end' }}>
          <Button type="button" variant="ghost" onClick={onDone} disabled={busy}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={!canSave}>
            {profile ? 'Save' : 'Add profile'}
          </Button>
        </div>

        {profile && canDelete && (
          <div style={{ borderTop: '1px solid var(--border)', paddingTop: 'var(--sp-4)' }}>
            {confirmDelete ? (
              <div
                style={{ display: 'flex', gap: 'var(--sp-3)', alignItems: 'center', flexWrap: 'wrap' }}
              >
                <span>Delete {profile.name}, with its progress and My List?</span>
                <Button type="button" variant="danger" onClick={() => void remove()} disabled={busy}>
                  Delete
                </Button>
                <Button
                  type="button" variant="ghost" disabled={busy}
                  onClick={() => setConfirmDelete(false)}
                >
                  Keep
                </Button>
              </div>
            ) : (
              <Button type="button" variant="ghost" onClick={() => setConfirmDelete(true)}>
                Delete profile
              </Button>
            )}
          </div>
        )}
      </form>
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
