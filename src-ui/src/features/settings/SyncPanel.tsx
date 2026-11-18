/**
 * Sync across the household's devices (docs/APPLE_PORT_AND_SYNC.md §3).
 *
 * Opt-in: a device that never signs in behaves exactly as Aurora always has. Signed in,
 * favourites, lists, watch progress, profiles and the rest follow the household from
 * device to device, end-to-end encrypted under a key only its devices hold.
 *
 * Two secrets, and the panel keeps them apart on purpose. The **password** signs in to
 * the account. The **passphrase** unlocks the encryption key and never leaves the
 * device; without it, or the recovery code, nobody -- the service included -- can read
 * what is stored.
 */
import { useCallback, useEffect, useState } from 'react';
import type { SyncProblem, SyncStatus } from '@shared/ipc';
import { Button, FIELD } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { invoke } from '@/ipc';

/** How often the panel re-reads the status while it is open and signed in. */
const POLL_MS = 5_000;

const MUTED = { color: 'var(--text-muted)', fontSize: 'var(--fs-sm)', lineHeight: 1.5 } as const;
const CARD = {
  display: 'grid', gap: 'var(--sp-2)', padding: 'var(--sp-3)', borderRadius: 'var(--r-md)',
  background: 'var(--surface)', border: '1px solid var(--border)',
} as const;

function ago(unix: number | null): string {
  if (unix === null) return 'not yet';
  const s = Math.max(0, Math.floor(Date.now() / 1000) - unix);
  if (s < 60) return 'just now';
  if (s < 3_600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86_400) return `${Math.floor(s / 3_600)} h ago`;
  return new Date(unix * 1000).toLocaleDateString();
}

function describe(status: SyncStatus): { text: string; tone: 'ok' | 'warn' | 'bad' } {
  const waiting = status.pending === 1 ? '1 change waiting' : `${status.pending} changes waiting`;
  const problem: SyncProblem | null = status.problem;
  if (problem?.kind === 'offline') return { text: `Offline — ${waiting}`, tone: 'warn' };
  if (problem?.kind === 'quota') {
    return {
      text: `The sync service's free daily allowance is used up. It resets at midnight `
        + `Pacific time — ${waiting}.`,
      tone: 'warn',
    };
  }
  if (problem?.kind === 'signInAgain') {
    return { text: 'Sync has stopped: sign in again to carry on.', tone: 'bad' };
  }
  if (problem?.kind === 'failed') return { text: problem.message, tone: 'bad' };
  if (!status.joined) return { text: 'Finishing the first sync…', tone: 'warn' };
  if (status.pending > 0) return { text: `Syncing — ${waiting}`, tone: 'ok' };
  return { text: `Up to date. Last synced ${ago(status.lastSyncAt)}.`, tone: 'ok' };
}

function Field(props: {
  label: string; value: string; onChange: (v: string) => void;
  type?: string; placeholder?: string; testId?: string; autoComplete?: string;
}) {
  return (
    <label style={{ display: 'grid', gap: 4 }}>
      <span style={{ fontSize: 'var(--fs-sm)', fontWeight: 650 }}>{props.label}</span>
      <input
        className="aurora-field"
        type={props.type ?? 'text'}
        value={props.value}
        onChange={(e) => props.onChange(e.target.value)}
        placeholder={props.placeholder}
        autoComplete={props.autoComplete ?? 'off'}
        spellCheck={false}
        data-testid={props.testId}
        style={FIELD}
      />
    </label>
  );
}

function Message({ ok, children }: { ok: boolean; children: React.ReactNode }) {
  return (
    <div
      role={ok ? 'status' : 'alert'}
      style={{
        padding: 'var(--sp-3)', borderRadius: 'var(--r-md)', fontSize: 'var(--fs-sm)',
        background: ok
          ? 'color-mix(in srgb, var(--success) 12%, transparent)'
          : 'color-mix(in srgb, var(--danger) 12%, transparent)',
        border: `1px solid ${ok ? 'var(--success)' : 'var(--danger)'}`,
      }}
    >
      {children}
    </div>
  );
}

/** The code to write down. Shown once at setup, and again only behind the passphrase. */
function RecoveryCode({ code, onDone }: { code: string; onDone: () => void }) {
  return (
    <div style={{ ...CARD, borderColor: 'var(--accent)' }} data-testid="sync-recovery">
      <div style={{ fontWeight: 650 }}>Your recovery code</div>
      <div style={MUTED}>
        Write this down and keep it somewhere safe. If the sync passphrase is ever
        forgotten, this is the only way back in: nobody, Aurora included, can recover it
        for you.
      </div>
      <code
        style={{
          fontSize: 'var(--fs-md)', letterSpacing: '0.04em', wordBreak: 'break-all',
          userSelect: 'all', padding: 'var(--sp-2)', borderRadius: 'var(--r-sm)',
          background: 'var(--bg)',
        }}
      >
        {code}
      </code>
      <div>
        <Button size="sm" variant="primary" onClick={onDone}>I have written it down</Button>
      </div>
    </div>
  );
}

type Mode = 'create' | 'join';

function SignInForm({ status, onJoined }: {
  status: SyncStatus;
  onJoined: (recoveryCode: string | null, problem: SyncProblem | null) => void;
}) {
  const again = status.signedIn && status.problem?.kind === 'signInAgain';
  const [mode, setMode] = useState<Mode>(again ? 'join' : 'create');
  const [email, setEmail] = useState(status.email ?? '');
  const [password, setPassword] = useState('');
  const [passphrase, setPassphrase] = useState('');
  const [confirm, setConfirm] = useState('');
  const [useCode, setUseCode] = useState(false);
  const [code, setCode] = useState('');
  const [deviceName, setDeviceName] = useState(status.deviceName ?? '');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (deviceName) return;
    invoke('sync.deviceName', undefined).then(setDeviceName).catch(() => {});
  }, [deviceName]);

  const mismatch = mode === 'create' && confirm.length > 0 && confirm !== passphrase;
  const ready = email.trim() && password
    && (mode === 'create'
      ? passphrase.length >= 8 && passphrase === confirm
      : (useCode ? code.trim() : passphrase));

  const submit = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const joined = mode === 'create'
        ? await invoke('sync.createAccount', { email, password, passphrase, deviceName })
        : await invoke('sync.signIn', {
          email, password, deviceName,
          ...(useCode ? { recoveryCode: code } : { passphrase }),
        });
      setPassword('');
      setPassphrase('');
      setConfirm('');
      setCode('');
      onJoined(joined.recoveryCode, joined.problem);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }, [mode, email, password, passphrase, deviceName, useCode, code, onJoined]);

  return (
    <form
      style={{ display: 'grid', gap: 'var(--sp-3)', maxWidth: 520 }}
      onSubmit={(e) => { e.preventDefault(); if (ready && !busy) void submit(); }}
    >
      {!again && (
        <div role="tablist" style={{ display: 'flex', gap: 'var(--sp-2)' }}>
          <Button
            type="button" size="sm" role="tab" aria-selected={mode === 'create'}
            variant={mode === 'create' ? 'primary' : 'secondary'}
            onClick={() => { setMode('create'); setError(null); }}
          >
            Set up sync
          </Button>
          <Button
            type="button" size="sm" role="tab" aria-selected={mode === 'join'}
            variant={mode === 'join' ? 'primary' : 'secondary'}
            onClick={() => { setMode('join'); setError(null); }}
          >
            Join my household
          </Button>
        </div>
      )}

      <div style={MUTED}>
        {again
          ? 'The sync account needs its password again. Nothing on this device has been lost.'
          : mode === 'create'
            ? 'For the first device in the household. Every other device then joins with '
              + 'the same email, password and passphrase.'
            : 'For a device joining a household that already set sync up on another one.'}
      </div>

      <Field label="Email" value={email} onChange={setEmail} type="email"
        autoComplete="username" testId="sync-email" />
      <Field label="Account password" value={password} onChange={setPassword}
        type="password" autoComplete={mode === 'create' ? 'new-password' : 'current-password'}
        testId="sync-password" />

      {mode === 'join' && useCode ? (
        <Field label="Recovery code" value={code} onChange={setCode}
          placeholder="XXXXX-XXXXX-…" testId="sync-code" />
      ) : (
        <Field label="Sync passphrase" value={passphrase} onChange={setPassphrase}
          type="password" testId="sync-passphrase"
          placeholder={mode === 'create' ? 'At least 8 characters' : undefined} />
      )}
      {mode === 'create' && (
        <>
          <Field label="Passphrase again" value={confirm} onChange={setConfirm}
            type="password" testId="sync-confirm" />
          <div style={MUTED}>
            Different from the password, and it never leaves your devices: it is what
            encrypts everything before it is uploaded. Each device asks for it once.
          </div>
        </>
      )}
      {mismatch && <div style={{ ...MUTED, color: 'var(--danger)' }}>The passphrases differ.</div>}
      {mode === 'join' && !again && (
        <div>
          <Button type="button" size="sm" variant="ghost" onClick={() => setUseCode(!useCode)}>
            {useCode ? 'Use the passphrase instead' : 'Forgot the passphrase? Use the recovery code'}
          </Button>
        </div>
      )}

      <Field label="Call this device" value={deviceName} onChange={setDeviceName}
        testId="sync-device-name" />

      <div>
        <Button type="submit" variant="primary" disabled={!ready || busy}>
          {busy
            ? (mode === 'create' ? 'Setting up…' : 'Signing in…')
            : again ? 'Sign in again' : mode === 'create' ? 'Set up sync' : 'Join'}
        </Button>
      </div>
      {error && <Message ok={false}>{error}</Message>}
    </form>
  );
}

export function SyncPanel() {
  const [nonce, setNonce] = useState(0);
  const reload = useCallback(() => setNonce((n) => n + 1), []);
  const { data: status, error: statusError } = useCommand('sync.status', undefined, [nonce]);
  const [recovery, setRecovery] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const [confirming, setConfirming] = useState<'signOut' | 'reset' | null>(null);
  const [showCode, setShowCode] = useState(false);
  const [codePassphrase, setCodePassphrase] = useState('');

  // Keep "3 changes waiting" and "last synced" honest while the panel is open.
  const signedIn = status?.signedIn ?? false;
  useEffect(() => {
    if (!signedIn) return undefined;
    const id = window.setInterval(reload, POLL_MS);
    return () => window.clearInterval(id);
  }, [signedIn, reload]);

  const run = useCallback(async (what: () => Promise<string>, failed: string) => {
    setBusy(true);
    setResult(null);
    try {
      setResult({ ok: true, text: await what() });
    } catch (e) {
      setResult({ ok: false, text: `${failed}: ${e instanceof Error ? e.message : String(e)}` });
    } finally {
      setBusy(false);
      setConfirming(null);
      reload();
    }
  }, [reload]);

  if (statusError) return <Message ok={false}>Could not read the sync status: {statusError}</Message>;
  if (!status) return null;

  if (!status.available) {
    return (
      <div style={MUTED} data-testid="sync-unavailable">
        This build of Aurora was made without a sync service, so there is nothing to sign
        in to. Official releases include it.
      </div>
    );
  }

  if (recovery) {
    return <RecoveryCode code={recovery} onDone={() => { setRecovery(null); reload(); }} />;
  }

  const intro = (
    <div style={MUTED}>
      Keeps favourites, My List, watch progress, profiles, parental settings and recording
      rules the same on every device in the household. Everything is encrypted on this
      device before it is uploaded, with a passphrase only your devices know. Playlist
      passwords are never synced: each device keeps its own.
    </div>
  );

  if (!status.signedIn || status.problem?.kind === 'signInAgain') {
    return (
      <div style={{ display: 'grid', gap: 'var(--sp-3)' }}>
        {intro}
        <SignInForm
          status={status}
          onJoined={(code, problem) => {
            if (code) setRecovery(code);
            setResult(problem
              ? { ok: false, text: `Signed in. The first sync did not finish yet and will `
                + `carry on by itself: ${describe({ ...status, problem }).text}` }
              : null);
            reload();
          }}
        />
        {result && <Message ok={result.ok}>{result.text}</Message>}
      </div>
    );
  }

  const state = describe(status);
  return (
    <div style={{ display: 'grid', gap: 'var(--sp-3)' }} data-testid="sync-signed-in">
      <div style={CARD}>
        <div style={{ display: 'flex', gap: 'var(--sp-2)', alignItems: 'baseline', flexWrap: 'wrap' }}>
          <span style={{ fontWeight: 650 }}>{status.email}</span>
          <span style={{ ...MUTED, color: 'var(--text-faint)' }}>on {status.deviceName}</span>
        </div>
        <div
          data-testid="sync-state"
          style={{
            fontSize: 'var(--fs-sm)',
            color: state.tone === 'ok' ? 'var(--text-muted)'
              : state.tone === 'warn' ? 'var(--warning)' : 'var(--danger)',
          }}
        >
          {state.text}
        </div>
        {status.parked > 0 && (
          <div style={MUTED}>
            {status.parked} {status.parked === 1 ? 'change refers' : 'changes refer'} to
            channels or titles this device has not imported. They apply once it has.
          </div>
        )}
        <div style={{ ...MUTED, color: 'var(--text-faint)' }} data-testid="sync-usage">
          Today this device has read {status.usage.reads.toLocaleString()} and
          written {status.usage.writes.toLocaleString()} of the free plan&apos;s 50,000
          reads and 20,000 writes.
        </div>
        {status.thrifty && (
          <div style={{ ...MUTED, color: 'var(--warning)' }} data-testid="sync-thrifty">
            That is past this device&apos;s share, so until the quota resets at midnight
            Pacific it sends watch progress and checks for changes every ten minutes.
            Favourites and other edits still go straight away.
          </div>
        )}
      </div>

      {!status.secretsPersistent && (
        <div style={{ ...MUTED, color: 'var(--warning)' }}>
          There is no credential store on this platform, so this sign-in lasts only until
          Aurora closes.
        </div>
      )}

      <div style={{ display: 'flex', gap: 'var(--sp-2)', flexWrap: 'wrap' }}>
        <Button
          size="sm" variant="primary" disabled={busy}
          onClick={() => void run(async () => {
            const r = await invoke('sync.now', undefined);
            return r.pushed + r.applied === 0
              ? 'Already up to date.'
              : `Sent ${r.pushed} and received ${r.applied} changes.`;
          }, 'Sync failed')}
        >
          {busy ? 'Syncing…' : 'Sync now'}
        </Button>
        <Button size="sm" disabled={busy} onClick={() => { setShowCode(!showCode); setResult(null); }}>
          Show recovery code
        </Button>
      </div>

      {showCode && (
        <form
          style={{ display: 'flex', gap: 'var(--sp-2)', alignItems: 'end', maxWidth: 520 }}
          onSubmit={(e) => {
            e.preventDefault();
            setBusy(true);
            setResult(null);
            invoke('sync.recoveryCode', { passphrase: codePassphrase })
              .then((code) => { setShowCode(false); setCodePassphrase(''); setRecovery(code); })
              .catch((err) => setResult({
                ok: false, text: err instanceof Error ? err.message : String(err),
              }))
              .finally(() => setBusy(false));
          }}
        >
          <div style={{ flex: 1 }}>
            <Field label="Sync passphrase" value={codePassphrase} onChange={setCodePassphrase}
              type="password" testId="sync-code-passphrase" />
          </div>
          <Button type="submit" size="sm" disabled={busy || !codePassphrase}>Show</Button>
        </form>
      )}

      {result && <Message ok={result.ok}>{result.text}</Message>}

      <div style={{ display: 'flex', gap: 'var(--sp-2)', flexWrap: 'wrap', alignItems: 'center' }}>
        {/* Two steps each: one forgets the account here, the other replaces what every
            other device will see. */}
        {confirming === 'signOut' && (
          <>
            <span style={{ ...MUTED, color: 'var(--danger)' }}>
              Stop syncing on this device? Its library stays as it is.
            </span>
            <Button size="sm" variant="danger" disabled={busy}
              onClick={() => void run(async () => {
                await invoke('sync.signOut', undefined);
                return 'Signed out. This device keeps everything it has.';
              }, 'Could not sign out')}>
              Yes, sign out
            </Button>
            <Button size="sm" disabled={busy} onClick={() => setConfirming(null)}>Keep</Button>
          </>
        )}
        {confirming === 'reset' && (
          <>
            <span style={{ ...MUTED, color: 'var(--danger)' }}>
              Replace everything the household has synced with this device's library? Changes
              other devices have not sent yet are lost.
            </span>
            <Button size="sm" variant="danger" disabled={busy}
              onClick={() => void run(async () => {
                const r = await invoke('sync.reset', undefined);
                return `Sync reset from this device: ${r.pushed} records uploaded.`;
              }, 'Could not reset sync')}>
              Yes, reset
            </Button>
            <Button size="sm" disabled={busy} onClick={() => setConfirming(null)}>Keep</Button>
          </>
        )}
        {confirming === null && (
          <>
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => setConfirming('signOut')}>
              Sign out
            </Button>
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => setConfirming('reset')}>
              Reset sync from this device
            </Button>
          </>
        )}
      </div>
    </div>
  );
}
