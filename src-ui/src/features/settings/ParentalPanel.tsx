/**
 * Settings → Parental controls (README §11): the parental PIN, what is hidden from
 * everybody, each profile's age limit, and the channels and categories that are locked.
 *
 * Every one of these was stored by the host and unreachable from any screen, and none of
 * them was applied to anything (`aurora_db::repo::parental` now applies them). Changes
 * go straight to the host, which asks for the parental PIN when one is set -- the
 * dialog for that comes from `invoke`, so nothing here has to check it first.
 */
import { useCallback, useEffect, useState } from 'react';
import type { LockKind, ParentalSettings, PlaylistEntry, Profile } from '@shared/ipc';
import { Button, TextField } from '@/components/Primitives';
import { Icon } from '@/components/Icon';
import { useCommand } from '@/hooks/useCommand';
import { invoke } from '@/ipc';
import { messageOf } from '@/lib/errors';
import { useParental } from '@/state/parental';
import { useProfile } from '@/state/profile';

/** The limits a profile can have, as a minimum age (`aurora_core::parental`). */
const AGE_LIMITS: { value: number | null; label: string }[] = [
  { value: null, label: 'No limit' },
  { value: 0, label: 'U — suitable for all' },
  { value: 6, label: 'Up to PG' },
  { value: 12, label: 'Up to 12' },
  { value: 15, label: 'Up to 15' },
  { value: 18, label: 'Up to 18' },
];

export function ParentalPanel() {
  const { profiles, parental, load: loadProfiles } = useProfile();
  const { locks, state, load: loadParental, setLock } = useParental();
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    loadParental().catch((e: unknown) => setError(messageOf(e)));
  }, [loadParental]);

  /** Run a change, then read everything back: the host is the one that knows. */
  const run = useCallback(async (work: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await work();
    } catch (e: unknown) {
      setError(messageOf(e));
    } finally {
      setBusy(false);
      await Promise.all([loadProfiles(), loadParental()]).catch(() => {});
    }
  }, [loadProfiles, loadParental]);

  if (!parental) return null;

  const setFlags = (patch: Partial<Pick<ParentalSettings, 'hideAdult' | 'lockSettings'>>) =>
    run(() => invoke('profiles.setParental', {
      hideAdult: patch.hideAdult ?? parental.hideAdult,
      lockSettings: patch.lockSettings ?? parental.lockSettings,
    }));

  return (
    <div data-testid="parental-panel">
      <MasterPin parental={parental} busy={busy} run={run} />

      <Toggle
        label="Hide adult categories"
        hint="For every profile. Categories named adult, XXX or 18+ are left out of every list and refused if played. A profile with an age limit always hides them."
        checked={parental.hideAdult}
        disabled={busy}
        onChange={(hideAdult) => void setFlags({ hideAdult })}
      />
      <Toggle
        label="Ask for the PIN to open Settings"
        hint={parental.hasMasterPin
          ? 'Settings stays closed until the parental PIN is entered.'
          : 'Set a parental PIN first.'}
        checked={parental.lockSettings}
        disabled={busy || !parental.hasMasterPin}
        onChange={(lockSettings) => void setFlags({ lockSettings })}
      />
      <Toggle
        label="Hide locked channels and categories"
        hint="Off shows them with a padlock and asks for the PIN when they are played. On leaves them out of every list until the PIN is entered."
        checked={state?.hideLocked ?? false}
        disabled={busy || !parental.hasMasterPin}
        onChange={(hide) => void run(() => invoke('parental.setHideLocked', { hide }))}
      />

      <h3 className="settings-subhead">Age limits</h3>
      {state && state.titles > 0 && state.ratedTitles < state.titles && (
        <p className="settings-row-hint" data-testid="rated-share">
          {state.ratedTitles === 0
            ? 'None of your films and series carry a rating yet'
            : `${state.ratedTitles.toLocaleString()} of ${state.titles.toLocaleString()} films and series carry a rating`}
          . Providers rarely send one, so an age limit can only judge what Artwork and
          metadata has fetched, and a profile that blocks unrated titles sees only those.
        </p>
      )}
      {profiles.map((p) => <AgeLimit key={p.id} profile={p} busy={busy} run={run} />)}

      <h3 className="settings-subhead">Locked</h3>
      {!parental.hasMasterPin ? (
        <p className="settings-row-hint">
          Set a parental PIN to lock channels and categories: the PIN is what opens them.
        </p>
      ) : (
        <>
          {locks.length === 0 && (
            <p className="settings-row-hint">Nothing is locked.</p>
          )}
          <ul aria-label="Locked channels and categories" style={{ listStyle: 'none', margin: 0, padding: 0 }}>
            {locks.map((l) => (
              <li key={`${l.kind}:${l.value}`} className="settings-row">
                <Icon name="lock" size={15} />
                <span style={{ flex: 1, minWidth: 0 }}>
                  {l.label}
                  <span className="dl-chip" style={{ marginLeft: 8 }}>
                    {l.kind === 'channel' ? 'Channel' : 'Category'}
                  </span>
                </span>
                <Button
                  size="sm"
                  variant="outline"
                  disabled={busy}
                  aria-label={`Unlock ${l.label}`}
                  onClick={() => void run(() => setLock(l.kind, l.value, false))}
                >
                  Unlock
                </Button>
              </li>
            ))}
          </ul>
          <LockPicker busy={busy} onLock={(kind, value) => run(() => setLock(kind, value, true))} />
        </>
      )}

      {error && (
        <div role="alert" className="settings-row-hint" style={{ color: 'var(--danger)' }}>
          {error}
        </div>
      )}
    </div>
  );
}

function MasterPin({
  parental, busy, run,
}: {
  parental: ParentalSettings;
  busy: boolean;
  run: (work: () => Promise<unknown>) => Promise<void>;
}) {
  const [pin, setPin] = useState('');
  const [again, setAgain] = useState('');
  const [open, setOpen] = useState(false);
  const shapeOk = /^\d{4}$/.test(pin);
  const ready = shapeOk && pin === again;

  const save = (masterPin: string | null) => run(async () => {
    await invoke('profiles.setParental', {
      hideAdult: parental.hideAdult,
      lockSettings: masterPin === null ? false : parental.lockSettings,
      masterPin,
    });
    setPin('');
    setAgain('');
    setOpen(false);
  });

  return (
    <div className="settings-row" style={{ alignItems: 'flex-start', flexWrap: 'wrap' }}>
      <div style={{ flex: 1, minWidth: 240 }}>
        <div className="settings-row-label">Parental PIN</div>
        <div className="settings-row-hint">
          {parental.hasMasterPin
            ? 'Set. It opens locked channels, and guards these settings and adding or removing profiles.'
            : 'Not set, so anybody can change these settings and add an unrestricted profile. Set one before relying on an age limit.'}
        </div>
        {open && (
          <form
            onSubmit={(e) => { e.preventDefault(); if (ready) void save(pin); }}
            style={{ display: 'flex', gap: 8, marginTop: 10, flexWrap: 'wrap' }}
          >
            <TextField
              aria-label="New parental PIN"
              placeholder="4 digits"
              type="password"
              inputMode="numeric"
              maxLength={4}
              value={pin}
              onChange={(e) => setPin(e.target.value.replace(/\D/g, ''))}
            />
            <TextField
              aria-label="New parental PIN again"
              placeholder="Again"
              type="password"
              inputMode="numeric"
              maxLength={4}
              value={again}
              onChange={(e) => setAgain(e.target.value.replace(/\D/g, ''))}
            />
            <Button type="submit" variant="primary" disabled={busy || !ready}>
              Save PIN
            </Button>
            {again.length === 4 && pin !== again && (
              <span className="settings-row-hint" style={{ color: 'var(--danger)' }}>
                The two PINs differ.
              </span>
            )}
          </form>
        )}
      </div>
      <div style={{ display: 'flex', gap: 8 }}>
        {!open && (
          <Button variant="secondary" disabled={busy} onClick={() => setOpen(true)}>
            {parental.hasMasterPin ? 'Change PIN' : 'Set a PIN'}
          </Button>
        )}
        {parental.hasMasterPin && !open && (
          <Button variant="outline" disabled={busy} onClick={() => void save(null)}>
            Remove PIN
          </Button>
        )}
      </div>
    </div>
  );
}

function AgeLimit({
  profile, busy, run,
}: {
  profile: Profile;
  busy: boolean;
  run: (work: () => Promise<unknown>) => Promise<void>;
}) {
  const set = (maxAge: number | null, allowUnrated: boolean) => run(() =>
    invoke('profiles.setLimits', {
      profileId: profile.id,
      maxAge,
      allowUnrated,
      dailyLimitMin: profile.dailyLimitMin,
    }));
  return (
    <div className="settings-row" style={{ flexWrap: 'wrap' }}>
      <div style={{ flex: 1, minWidth: 160 }}>
        <div className="settings-row-label">{profile.name}</div>
        {profile.maxAge != null && (
          <label className="settings-row-hint" style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
            <input
              type="checkbox"
              checked={profile.allowUnrated}
              disabled={busy}
              onChange={(e) => void set(profile.maxAge, e.target.checked)}
            />
            Allow titles with no rating. Provider listings often leave it out, so these are
            usually missing data rather than suitable.
          </label>
        )}
      </div>
      <select
        aria-label={`Age limit for ${profile.name}`}
        value={profile.maxAge == null ? '' : String(profile.maxAge)}
        disabled={busy}
        onChange={(e) => {
          const maxAge = e.target.value === '' ? null : Number(e.target.value);
          void set(maxAge, maxAge == null ? true : profile.allowUnrated);
        }}
        className="dl-select"
      >
        {AGE_LIMITS.map((a) => (
          <option key={a.label} value={a.value == null ? '' : String(a.value)}>
            {a.label}
          </option>
        ))}
      </select>
    </div>
  );
}

/** Lock a category, or find a channel by name and lock it. */
function LockPicker({
  busy, onLock,
}: {
  busy: boolean;
  onLock: (kind: LockKind, value: string) => Promise<void>;
}) {
  const { data: groups } = useCommand('playlist.groups', { kind: 'live' }, []);
  const [group, setGroup] = useState('');
  const [text, setText] = useState('');
  const [found, setFound] = useState<PlaylistEntry[]>([]);

  useEffect(() => {
    const q = text.trim();
    if (q.length < 2) {
      setFound([]);
      return;
    }
    let live = true;
    const t = window.setTimeout(() => {
      invoke('playlist.list', { kind: 'live', text: q, show: 'visible', limit: 8, offset: 0 })
        .then((page) => { if (live) setFound(page.rows); })
        .catch(() => { if (live) setFound([]); });
    }, 200);
    return () => {
      live = false;
      window.clearTimeout(t);
    };
  }, [text]);

  return (
    <div style={{ display: 'grid', gap: 10, marginTop: 10 }}>
      <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
        <select
          aria-label="Category to lock"
          value={group}
          onChange={(e) => setGroup(e.target.value)}
          className="dl-select"
          style={{ minWidth: 220 }}
        >
          <option value="">Choose a category…</option>
          {(groups ?? []).map((g) => (
            <option key={g.name} value={g.name}>{g.name} ({g.count})</option>
          ))}
        </select>
        <Button
          variant="secondary"
          icon="lock"
          disabled={busy || group === ''}
          onClick={() => void onLock('category', group).then(() => setGroup(''))}
        >
          Lock category
        </Button>
      </div>
      <TextField
        icon="search"
        aria-label="Find a channel to lock"
        placeholder="Find a channel to lock"
        value={text}
        clearable
        onClear={() => setText('')}
        onChange={(e) => setText(e.target.value)}
      />
      {found.length > 0 && (
        <ul aria-label="Channels found" style={{ listStyle: 'none', margin: 0, padding: 0 }}>
          {found.map((c) => (
            <li key={c.id} className="settings-row">
              <span style={{ flex: 1, minWidth: 0 }}>
                {c.name}
                {c.group && <span className="settings-row-hint"> · {c.group}</span>}
              </span>
              <Button
                size="sm"
                variant="outline"
                icon="lock"
                disabled={busy}
                aria-label={`Lock ${c.name}`}
                onClick={() => void onLock('channel', String(c.id)).then(() => setText(''))}
              >
                Lock
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Toggle({
  label, hint, checked, disabled, onChange,
}: {
  label: string;
  hint: string;
  checked: boolean;
  disabled: boolean;
  onChange: (next: boolean) => void;
}) {
  return (
    <div className="settings-row" style={{ alignItems: 'flex-start' }}>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div className="settings-row-label">{label}</div>
        <div className="settings-row-hint">{hint}</div>
      </div>
      <button
        role="switch"
        aria-checked={checked}
        aria-label={label}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        type="button"
        className="dl-switch"
        title={checked ? 'On' : 'Off'}
        style={{ cursor: disabled ? 'not-allowed' : 'pointer', marginTop: 4 }}
      />
    </div>
  );
}
