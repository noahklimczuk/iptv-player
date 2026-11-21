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
 *
 * Public since 2.0 (docs/RELEASE_2.0.md §2), so the account is a real one: creating it
 * asks for the password twice and agreement to the privacy policy, sends a verification
 * email, a forgotten password can be reset from the sign-in form, and the account can be
 * deleted -- server copy and all -- from here (App Store guideline 5.1.1(v)).
 */
import { useCallback, useEffect, useState } from 'react';
import type { SyncProblem, SyncStatus } from '@shared/ipc';
import { Button, FIELD } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { invoke } from '@/ipc';

/** How often the panel re-reads the status while it is open and signed in. */
const POLL_MS = 5_000;

export const MUTED = { color: 'var(--text-muted)', fontSize: 'var(--fs-sm)', lineHeight: 1.5 } as const;
export const CARD = {
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

export function Message({ ok, children }: { ok: boolean; children: React.ReactNode }) {
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

/** Our rule, stricter than Firebase's six: an account guards a household's sync. */
const MIN_PASSWORD = 8;

function SignInForm({ status, onJoined }: {
  status: SyncStatus;
  onJoined: (recoveryCode: string | null, problem: SyncProblem | null) => void;
}) {
  const again = status.signedIn && status.problem?.kind === 'signInAgain';
  const [mode, setMode] = useState<Mode>(again ? 'join' : 'create');
  const [email, setEmail] = useState(status.email ?? '');
  const [password, setPassword] = useState('');
  const [password2, setPassword2] = useState('');
  const [agreed, setAgreed] = useState(false);
  const [resetSent, setResetSent] = useState<string | null>(null);
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

  const creating = mode === 'create';
  const mismatch = creating && confirm.length > 0 && confirm !== passphrase;
  const passwordShort = creating && password.length > 0 && password.length < MIN_PASSWORD;
  const passwordMismatch = creating && password2.length > 0 && password2 !== password;
  const samePassphrase = creating && passphrase.length > 0 && passphrase === password;
  const ready = email.trim() && password
    && (creating
      ? password.length >= MIN_PASSWORD && password === password2 && agreed
        && passphrase.length >= 8 && passphrase === confirm && !samePassphrase
      : (useCode ? code.trim() : passphrase));

  const forgot = useCallback(async () => {
    setError(null);
    setResetSent(null);
    try {
      await invoke('sync.sendPasswordReset', { email });
      setResetSent(email.trim());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [email]);

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
      setPassword2('');
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
            onClick={() => { setMode('create'); setError(null); setResetSent(null); }}
          >
            Create account
          </Button>
          <Button
            type="button" size="sm" role="tab" aria-selected={mode === 'join'}
            variant={mode === 'join' ? 'primary' : 'secondary'}
            onClick={() => { setMode('join'); setError(null); }}
          >
            Sign in
          </Button>
        </div>
      )}

      <div style={MUTED}>
        {again
          ? 'The sync account needs its password again. Nothing on this device has been lost.'
          : creating
            ? 'A free Aurora account, for the first device in your household. Every other '
              + 'device then signs in with the same email, password and passphrase.'
            : 'For a device joining a household that already has an Aurora account.'}
      </div>

      <Field label="Email" value={email} onChange={setEmail} type="email"
        autoComplete="username" testId="sync-email" />
      <Field label="Account password" value={password} onChange={setPassword}
        type="password" autoComplete={creating ? 'new-password' : 'current-password'}
        placeholder={creating ? `At least ${MIN_PASSWORD} characters` : undefined}
        testId="sync-password" />
      {creating && (
        <Field label="Password again" value={password2} onChange={setPassword2}
          type="password" autoComplete="new-password" testId="sync-password-again" />
      )}
      {passwordShort && (
        <div style={{ ...MUTED, color: 'var(--danger)' }}>
          The password needs at least {MIN_PASSWORD} characters.
        </div>
      )}
      {passwordMismatch && (
        <div style={{ ...MUTED, color: 'var(--danger)' }}>The passwords differ.</div>
      )}
      {mode === 'join' && !again && (
        <div>
          <Button type="button" size="sm" variant="ghost" disabled={!email.includes('@')}
            onClick={() => void forgot()} data-testid="sync-forgot-password">
            Forgot the password?
          </Button>
        </div>
      )}
      {resetSent && (
        <Message ok>
          If there is an Aurora account for {resetSent}, an email with a link to choose a
          new password is on its way. Check the spam folder if it does not arrive.
        </Message>
      )}

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
      {samePassphrase && (
        <div style={{ ...MUTED, color: 'var(--danger)' }}>
          Use a passphrase that is not the account password: the password is checked by
          the sync service, the passphrase must never reach it.
        </div>
      )}
      {mode === 'join' && !again && (
        <div>
          <Button type="button" size="sm" variant="ghost" onClick={() => setUseCode(!useCode)}>
            {useCode ? 'Use the passphrase instead' : 'Forgot the passphrase? Use the recovery code'}
          </Button>
        </div>
      )}

      <Field label="Call this device" value={deviceName} onChange={setDeviceName}
        testId="sync-device-name" />

      {creating && (
        <label style={{ display: 'flex', gap: 'var(--sp-2)', alignItems: 'start', ...MUTED }}>
          <input
            type="checkbox" checked={agreed} onChange={(e) => setAgreed(e.target.checked)}
            data-testid="sync-agree" style={{ marginTop: 3 }}
          />
          <span>
            I agree to the{' '}
            <button
              type="button" className="link-button"
              onClick={() => void invoke('sync.openPrivacyPolicy', undefined).catch(() => {})}
              style={{
                background: 'none', border: 0, padding: 0, color: 'var(--accent)',
                textDecoration: 'underline', cursor: 'pointer', font: 'inherit',
              }}
            >
              privacy policy
            </button>
            . Aurora stores your email address and your encrypted library with Google
            Firebase; it cannot read the library, and you can delete the account and
            everything in it at any time from this page.
          </span>
        </label>
      )}

      <div>
        <Button type="submit" variant="primary" disabled={!ready || busy}>
          {busy
            ? (creating ? 'Creating account…' : 'Signing in…')
            : again ? 'Sign in again' : creating ? 'Create account' : 'Sign in'}
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
  const [confirming, setConfirming] = useState<'signOut' | 'reset' | 'delete' | null>(null);
  const [deletePassword, setDeletePassword] = useState('');
  // Asked once per sign-in rather than on every poll: it is an account call, not a
  // Firestore read, but there is no reason to make one every five seconds.
  const [verified, setVerified] = useState<boolean | null>(null);
  const [verifyNote, setVerifyNote] = useState<string | null>(null);
  const [showCode, setShowCode] = useState(false);
  const [codePassphrase, setCodePassphrase] = useState('');

  // Keep "3 changes waiting" and "last synced" honest while the panel is open.
  const signedIn = status?.signedIn ?? false;
  useEffect(() => {
    if (!signedIn) return undefined;
    const id = window.setInterval(reload, POLL_MS);
    return () => window.clearInterval(id);
  }, [signedIn, reload]);

  const account = signedIn ? status?.email ?? null : null;
  useEffect(() => {
    setVerified(null);
    setVerifyNote(null);
    if (!account) return;
    invoke('sync.emailVerified', undefined).then(setVerified).catch(() => setVerified(null));
  }, [account]);

  const recheck = useCallback(async () => {
    setVerifyNote(null);
    try {
      const ok = await invoke('sync.emailVerified', undefined);
      setVerified(ok);
      if (!ok) setVerifyNote('Not confirmed yet. Open the link in the email, then try again.');
    } catch (e) {
      setVerifyNote(e instanceof Error ? e.message : String(e));
    }
  }, []);

  const resend = useCallback(async () => {
    setVerifyNote(null);
    try {
      await invoke('sync.sendVerification', undefined);
      setVerifyNote('Sent. Check the spam folder if it does not arrive.');
    } catch (e) {
      setVerifyNote(e instanceof Error ? e.message : String(e));
    }
  }, []);

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
      passwords and API keys stay on each device unless you choose to share them too.
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
      {verified === false && (
        <div style={{ ...CARD, borderColor: 'var(--warning)' }} data-testid="sync-verify">
          <div style={{ fontWeight: 650 }}>Confirm your email address</div>
          <div style={MUTED}>
            We sent a link to {status.email}. Confirming it is what lets you reset the
            password if you ever forget it. Sync works in the meantime.
          </div>
          <div style={{ display: 'flex', gap: 'var(--sp-2)', flexWrap: 'wrap' }}>
            <Button size="sm" variant="primary" onClick={() => void recheck()}>
              I have confirmed it
            </Button>
            <Button size="sm" onClick={() => void resend()}>Send the email again</Button>
          </div>
          {verifyNote && <div style={MUTED}>{verifyNote}</div>}
        </div>
      )}
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
        {/* §3.8: skipped, never applied, and asked for again by the update that can
            read them -- which is the one thing the viewer can do about it. */}
        {status.unreadable > 0 && (
          <div style={MUTED} data-testid="sync-unreadable">
            {status.unreadable} {status.unreadable === 1 ? 'change' : 'changes'} from
            your other devices {status.unreadable === 1 ? 'needs' : 'need'} a newer
            version of Aurora. {status.unreadable === 1 ? 'It is' : 'They are'} kept
            on the server and will be read once this device updates.
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

      {/* D34: a password is a different promise from a favourite, so this is a choice
          somebody makes, off until they do -- and the panel says what it means both ways. */}
      {status.canShareSecrets && (
        <div style={{ display: 'flex', gap: 'var(--sp-3)', alignItems: 'start' }}>
          <div style={{ flex: 1 }}>
            <div style={{ fontWeight: 650, fontSize: 'var(--fs-sm)' }}>
              Also share sign-ins and keys
            </div>
            <div style={MUTED}>
              {status.sharesSecrets
                ? 'Playlist passwords and your TMDB and Gemini keys go to the household’s '
                  + 'other devices, encrypted like everything else, so a new device needs '
                  + 'only the sync passphrase. Turning this off stops sending them; what was '
                  + 'sent stays in the household’s encrypted sync until you reset it.'
                : 'Off: each device keeps its own playlist passwords and TMDB and Gemini '
                  + 'keys. On sends them to your other devices, encrypted with your sync '
                  + 'passphrase, so nobody has to type them twice.'}
            </div>
          </div>
          <button
            type="button"
            className="dl-switch"
            data-testid="sync-share-secrets"
            aria-pressed={status.sharesSecrets}
            aria-label={status.sharesSecrets
              ? 'Stop sharing sign-ins and keys' : 'Share sign-ins and keys'}
            disabled={busy}
            onClick={() => void run(async () => {
              const on = !status.sharesSecrets;
              await invoke('sync.setShareSecrets', { enabled: on });
              return on
                ? 'Sign-ins and keys will go to your other devices from the next sync.'
                : 'Sign-ins and keys are no longer sent.';
            }, 'Could not change that')}
          />
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
            <Button size="sm" variant="ghost" disabled={busy}
              onClick={() => { setConfirming('delete'); setDeletePassword(''); setResult(null); }}
              data-testid="sync-delete-account">
              Delete account
            </Button>
          </>
        )}
      </div>

      {/* Three steps, the last behind the password: this is the one action here that
          cannot be undone, and it reaches every device in the household. */}
      {confirming === 'delete' && (
        <form
          style={{ ...CARD, borderColor: 'var(--danger)', maxWidth: 520 }}
          data-testid="sync-delete-form"
          onSubmit={(e) => {
            e.preventDefault();
            const password = deletePassword;
            setDeletePassword('');
            void run(async () => {
              await invoke('sync.deleteAccount', { password });
              return 'Your Aurora account and everything it stored have been deleted. '
                + 'This device keeps its library; other devices stop syncing.';
            }, 'Could not delete the account');
          }}
        >
          <div style={{ fontWeight: 650, color: 'var(--danger)' }}>Delete your Aurora account?</div>
          <div style={MUTED}>
            This permanently deletes {status.email} and everything the household has synced
            to it: favourites, lists, watch progress, profiles and settings on the server.
            The library on each device stays as it is, but no device will sync again until
            someone creates a new account. This cannot be undone.
          </div>
          <Field label="Account password" value={deletePassword} onChange={setDeletePassword}
            type="password" autoComplete="current-password" testId="sync-delete-password" />
          <div style={{ display: 'flex', gap: 'var(--sp-2)' }}>
            <Button type="submit" size="sm" variant="danger" disabled={busy || !deletePassword}>
              {busy ? 'Deleting…' : 'Delete account permanently'}
            </Button>
            <Button type="button" size="sm" disabled={busy} onClick={() => setConfirming(null)}>
              Keep it
            </Button>
          </div>
        </form>
      )}
    </div>
  );
}
