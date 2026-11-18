/**
 * "Who's watching?" (README §11) — profile selection, with a PIN keypad for
 * profiles that have one.
 */
import { motion } from 'framer-motion';
import { useEffect, useState } from 'react';
import type { Profile } from '@shared/ipc';
import { Button, TextField, artHue } from '@/components/Primitives';
import { Icon } from '@/components/Icon';
import { invoke } from '@/ipc';
import { report } from '@/lib/errors';
import { useProfile } from '@/state/profile';
import { PinPad, shell } from './PinPad';

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
    return (
      <PinPad
        heading={`Enter ${locked.name}'s PIN`}
        badge={locked.name.slice(0, 1).toUpperCase()}
        badgeColor={avatarColor(locked)}
        // Checked by the host as it chooses the profile, not by this screen: what a
        // profile may watch is decided there, so who is watching must be too.
        verify={(pin) => invoke('profiles.select', { profileId: locked.id, pin })}
        failure={`Could not check ${locked.name}'s PIN`}
        onCancel={() => setLocked(null)}
        onUnlocked={() => activate(locked)}
      />
    );
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
              else {
                invoke('profiles.select', { profileId: profile.id })
                  .then(() => activate(profile))
                  .catch(report(`Could not switch to ${profile.name}`));
              }
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
