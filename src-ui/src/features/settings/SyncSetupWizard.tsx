/**
 * Setting up sync, one decision at a time (docs/APPLE_PORT_AND_SYNC.md §3).
 *
 * The panel used to put everything on one form: two tabs, an email, the password twice,
 * a passphrase twice, a device name and a privacy checkbox, then the recovery code and
 * the email confirmation as cards that appeared afterwards. Every field was needed, but
 * the two secrets sat next to each other with one paragraph to tell them apart, and the
 * one thing that cannot be recovered -- the recovery code -- arrived after the form had
 * already said "done".
 *
 * So a wizard, in the order someone thinks about it:
 *
 *   1. Is this the first device, or is the household already using sync?
 *   2. The account: email and password (the service checks these).
 *   3. The passphrase (it never leaves the device), with what it is for in the step's
 *      own words -- or, joining, the recovery code instead.
 *   4. This device: what to call it, whether to share sign-ins, and the privacy policy.
 *   5. Creating: the recovery code, which has to be saved before going on.
 *   6. Creating: confirm the email address, or leave it for later.
 *   7. Done, with what the first sync did and how the next device joins.
 *
 * Nothing is sent until step 4. A refusal from the host goes back to the step whose
 * answer it is about, so a wrong passphrase reopens the passphrase, not the whole form.
 * The dialog cannot be dismissed while the account is being made or while the recovery
 * code is on screen: closing there would lose the only copy of the code.
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import type { SyncJoined, SyncStatus } from '@shared/ipc';
import { Button } from '@/components/Primitives';
import { useFocusTrap } from '@/hooks/useFocusTrap';
import { invoke } from '@/ipc';
import { CARD, Field, MIN_PASSWORD, MUTED, Message } from '@/features/settings/SyncPanel';

type Path = 'create' | 'join';
type Step = 'choose' | 'account' | 'passphrase' | 'device' | 'working' | 'recovery' | 'verify' | 'done';

/** The steps shown in the progress line, per path. `working` and `done` are not steps. */
const TRACK: Record<Path, { step: Step; label: string }[]> = {
  create: [
    { step: 'account', label: 'Account' },
    { step: 'passphrase', label: 'Passphrase' },
    { step: 'device', label: 'This device' },
    { step: 'recovery', label: 'Recovery code' },
    { step: 'verify', label: 'Email' },
  ],
  join: [
    { step: 'account', label: 'Account' },
    { step: 'passphrase', label: 'Passphrase' },
    { step: 'device', label: 'This device' },
  ],
};

const MIN_PASSPHRASE = 8;

/**
 * A rough guide, not a gate beyond the eight characters the host enforces. Length is
 * what matters for a passphrase, and several words is the easy way to get it.
 */
export function passphraseStrength(
  p: string,
): { label: string; tone: 'bad' | 'warn' | 'ok'; level: 1 | 2 | 3 | 4 } | null {
  if (!p) return null;
  const chars = [...p].length;
  const words = p.trim().split(/\s+/).filter(Boolean).length;
  if (chars < MIN_PASSPHRASE) {
    return { label: `Too short: at least ${MIN_PASSPHRASE} characters`, tone: 'bad', level: 1 };
  }
  if (chars >= 20 || (words >= 4 && chars >= 16)) return { label: 'Strong', tone: 'ok', level: 4 };
  if (chars >= 12 || words >= 3) return { label: 'Good', tone: 'ok', level: 3 };
  return { label: 'Fair: longer is safer. Three or four words is easy to remember', tone: 'warn', level: 2 };
}

/** Which step a refusal from the host is about, so the viewer lands where they can fix it. */
export function stepForError(message: string): Step {
  const m = message.toLowerCase();
  if (m.includes('passphrase') || m.includes('recovery code')) return 'passphrase';
  if (m.includes('email') || m.includes('password') || m.includes('account')) return 'account';
  return 'device';
}

const TONE = { ok: 'var(--success-text)', warn: 'var(--warning)', bad: 'var(--danger)' } as const;

function Problem({ children }: { children: React.ReactNode }) {
  return <div style={{ ...MUTED, color: 'var(--danger)' }}>{children}</div>;
}

function Choice({ title, body, onPick, testId }: {
  title: string; body: string; onPick: () => void; testId: string;
}) {
  return (
    <button
      type="button"
      onClick={onPick}
      data-testid={testId}
      className="sync-choice"
      style={{
        ...CARD, textAlign: 'left', cursor: 'pointer', font: 'inherit', color: 'inherit',
        gap: 4, padding: 'var(--sp-4)',
      }}
    >
      <span style={{ fontWeight: 700, fontSize: 'var(--fs-md)' }}>{title}</span>
      <span style={MUTED}>{body}</span>
    </button>
  );
}

function Progress({ path, step }: { path: Path; step: Step }) {
  const track = TRACK[path];
  const at = track.findIndex((t) => t.step === step);
  if (at < 0) return null;
  return (
    <ol
      aria-label={`Step ${at + 1} of ${track.length}`}
      style={{ display: 'flex', gap: 'var(--sp-2)', listStyle: 'none', margin: 0, padding: 0, flexWrap: 'wrap' }}
    >
      {track.map((t, i) => (
        <li
          key={t.step}
          aria-current={i === at ? 'step' : undefined}
          style={{
            fontSize: 'var(--fs-xs)', fontWeight: 650, padding: '3px 10px',
            borderRadius: 'var(--r-pill)',
            background: i === at ? 'var(--accent)' : i < at ? 'var(--accent-soft)' : 'var(--surface)',
            color: i === at ? 'var(--accent-text)' : i < at ? 'var(--text)' : 'var(--text-faint)',
            border: '1px solid var(--border)',
          }}
        >
          {i < at ? '✓ ' : ''}{t.label}
        </li>
      ))}
    </ol>
  );
}

function PolicyLink() {
  return (
    <button
      type="button"
      onClick={() => void invoke('sync.openPrivacyPolicy', undefined).catch(() => {})}
      style={{
        background: 'none', border: 0, padding: 0, color: 'var(--accent)',
        textDecoration: 'underline', cursor: 'pointer', font: 'inherit',
      }}
    >
      privacy policy
    </button>
  );
}

export function SyncSetupWizard({ status, onClose }: {
  status: SyncStatus;
  /** `joined` is true once this device is signed in, whatever step the dialog closed on. */
  onClose: (joined: boolean) => void;
}) {
  const [path, setPath] = useState<Path>('create');
  const [step, setStep] = useState<Step>('choose');
  const [email, setEmail] = useState(status.email ?? '');
  const [password, setPassword] = useState('');
  const [password2, setPassword2] = useState('');
  const [passphrase, setPassphrase] = useState('');
  const [confirm, setConfirm] = useState('');
  const [reveal, setReveal] = useState(false);
  const [useCode, setUseCode] = useState(false);
  const [code, setCode] = useState('');
  const [deviceName, setDeviceName] = useState(status.deviceName ?? '');
  const [share, setShare] = useState(false);
  const [agreed, setAgreed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [joined, setJoined] = useState<SyncJoined | null>(null);
  const [shareFailed, setShareFailed] = useState<string | null>(null);
  const [savedCode, setSavedCode] = useState(false);
  const [copied, setCopied] = useState(false);
  const [verified, setVerified] = useState(false);

  const dialog = useRef<HTMLDivElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  useFocusTrap(dialog, true);

  useEffect(() => {
    if (deviceName) return;
    invoke('sync.deviceName', undefined).then(setDeviceName).catch(() => {});
  }, [deviceName]);

  // Each step announces itself: focus goes to its heading, not to wherever the last
  // button was.
  useEffect(() => { heading.current?.focus(); }, [step]);

  const creating = path === 'create';
  const signedIn = joined !== null;
  const locked = step === 'working' || step === 'recovery';

  const close = useCallback(() => {
    if (locked) return;
    onClose(signedIn);
  }, [locked, onClose, signedIn]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.stopPropagation();
      close();
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [close]);

  const go = useCallback((next: Step) => { setError(null); setNote(null); setStep(next); }, []);

  // ── What each step needs before Continue ──────────────────────────────────
  const emailOk = /\S+@\S+\.\S+/.test(email.trim());
  const passwordShort = creating && password.length > 0 && password.length < MIN_PASSWORD;
  const passwordMismatch = creating && password2.length > 0 && password2 !== password;
  const accountReady = emailOk && password.length > 0
    && (!creating || (password.length >= MIN_PASSWORD && password === password2));

  const strength = creating ? passphraseStrength(passphrase) : null;
  const samePassphrase = creating && passphrase.length > 0 && passphrase === password;
  const passphraseMismatch = creating && confirm.length > 0 && confirm !== passphrase;
  const passphraseReady = creating
    ? [...passphrase].length >= MIN_PASSPHRASE && passphrase === confirm && !samePassphrase
    : useCode ? code.trim().length > 0 : passphrase.length > 0;

  const deviceReady = deviceName.trim().length > 0 && (!creating || agreed);

  const forgot = useCallback(async () => {
    setError(null);
    setNote(null);
    try {
      await invoke('sync.sendPasswordReset', { email });
      setNote(`If there is an Aurora account for ${email.trim()}, an email with a link to `
        + 'choose a new password is on its way. Check the spam folder if it does not arrive.');
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [email]);

  const submit = useCallback(async () => {
    setError(null);
    setStep('working');
    try {
      const result = creating
        ? await invoke('sync.createAccount', { email, password, passphrase, deviceName })
        : await invoke('sync.signIn', {
          email, password, deviceName,
          ...(useCode ? { recoveryCode: code } : { passphrase }),
        });
      setPassword('');
      setPassword2('');
      setConfirm('');
      setCode('');
      setJoined(result);
      if (creating && share && status.canShareSecrets) {
        // Its own call, after the account exists: a refusal here (the Settings lock) is
        // not a reason to undo an account that worked.
        await invoke('sync.setShareSecrets', { enabled: true })
          .catch((e: unknown) => setShareFailed(e instanceof Error ? e.message : String(e)));
      }
      setPassphrase('');
      setStep(result.recoveryCode ? 'recovery' : creating ? 'verify' : 'done');
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
      setStep(stepForError(message));
    }
  }, [creating, email, password, passphrase, deviceName, useCode, code, share, status.canShareSecrets]);

  const copyCode = useCallback(async () => {
    if (!joined?.recoveryCode) return;
    try {
      await navigator.clipboard.writeText(joined.recoveryCode);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }, [joined]);

  const recheck = useCallback(async () => {
    setNote(null);
    try {
      const ok = await invoke('sync.emailVerified', undefined);
      setVerified(ok);
      if (ok) go('done');
      else setNote('Not confirmed yet. Open the link in the email, then try again.');
    } catch (e) {
      setNote(e instanceof Error ? e.message : String(e));
    }
  }, [go]);

  const resend = useCallback(async () => {
    setNote(null);
    try {
      await invoke('sync.sendVerification', undefined);
      setNote('Sent. Check the spam folder if it does not arrive.');
    } catch (e) {
      setNote(e instanceof Error ? e.message : String(e));
    }
  }, []);

  // ── Steps ────────────────────────────────────────────────────────────────
  const title: Record<Step, string> = {
    choose: 'Sync your devices',
    account: creating ? 'Create your Aurora account' : 'Sign in to your household',
    passphrase: creating ? 'Choose a sync passphrase' : 'Unlock your household’s sync',
    device: 'About this device',
    working: creating ? 'Creating your account…' : 'Joining your household…',
    recovery: 'Save your recovery code',
    verify: 'Confirm your email address',
    done: 'Sync is on',
  };

  const back = (to: Step) => (
    <Button type="button" variant="ghost" onClick={() => go(to)}>Back</Button>
  );

  let body: React.ReactNode;
  switch (step) {
    case 'choose':
      body = (
        <div style={{ display: 'grid', gap: 'var(--sp-3)' }}>
          <ul style={{ ...MUTED, margin: 0, paddingLeft: '1.2em', display: 'grid', gap: 4 }}>
            <li>Favourites, My List, watch progress, profiles, parental settings and
              recording rules follow you from device to device.</li>
            <li>Encrypted on this device before it is uploaded, with a passphrase only your
              devices know. Not even Aurora can read it.</li>
            <li>Free. Playlist passwords and API keys stay on each device unless you choose
              to share them.</li>
          </ul>
          <Choice
            title="This is my first device"
            body="Create a free account for your household. It takes about a minute."
            testId="sync-path-create"
            onPick={() => { setPath('create'); go('account'); }}
          />
          <Choice
            title="I already use sync on another device"
            body="Sign in with the household’s email, password and passphrase."
            testId="sync-path-join"
            onPick={() => { setPath('join'); go('account'); }}
          />
        </div>
      );
      break;

    case 'account':
      body = (
        <form
          style={{ display: 'grid', gap: 'var(--sp-3)' }}
          onSubmit={(e) => { e.preventDefault(); if (accountReady) go('passphrase'); }}
        >
          <div style={MUTED}>
            {creating
              ? 'The email and password sign each of your devices in. Every other device in '
                + 'the household uses the same ones.'
              : 'The email and password your household’s Aurora account was made with.'}
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
          {passwordShort && <Problem>The password needs at least {MIN_PASSWORD} characters.</Problem>}
          {passwordMismatch && <Problem>The passwords differ.</Problem>}
          {!creating && (
            <div>
              <Button type="button" size="sm" variant="ghost" disabled={!emailOk}
                onClick={() => void forgot()} data-testid="sync-forgot-password">
                Forgot the password?
              </Button>
            </div>
          )}
          <Footer back={back('choose')} ready={accountReady} />
        </form>
      );
      break;

    case 'passphrase':
      body = (
        <form
          style={{ display: 'grid', gap: 'var(--sp-3)' }}
          onSubmit={(e) => { e.preventDefault(); if (passphraseReady) go('device'); }}
        >
          {creating ? (
            <div style={{ ...CARD, gap: 6 }}>
              <div style={{ fontWeight: 650, fontSize: 'var(--fs-sm)' }}>
                Why a second secret?
              </div>
              <div style={MUTED}>
                The <strong>password</strong> is checked by the sync service. The
                {' '}<strong>passphrase</strong> never leaves your devices: it encrypts
                everything before it is uploaded, so nobody else can read it. Each new
                device asks for it once.
              </div>
            </div>
          ) : (
            <div style={MUTED}>
              The passphrase that was chosen when the household set sync up. Forgotten it?
              The recovery code that was saved then works instead.
            </div>
          )}

          {!creating && useCode ? (
            <Field label="Recovery code" value={code} onChange={setCode}
              placeholder="XXXXX-XXXXX-…" testId="sync-code" />
          ) : (
            <Field label="Sync passphrase" value={passphrase} onChange={setPassphrase}
              type={reveal ? 'text' : 'password'} testId="sync-passphrase"
              placeholder={creating ? 'For example, four words you will remember' : undefined} />
          )}
          {creating && (
            <Field label="Passphrase again" value={confirm} onChange={setConfirm}
              type={reveal ? 'text' : 'password'} testId="sync-confirm" />
          )}
          {!(useCode && !creating) && (
            <label style={{ display: 'flex', gap: 'var(--sp-2)', alignItems: 'center', ...MUTED }}>
              <input type="checkbox" checked={reveal} onChange={(e) => setReveal(e.target.checked)}
                data-testid="sync-reveal" />
              Show the passphrase
            </label>
          )}
          {strength && (
            <div data-testid="sync-strength" style={{ display: 'grid', gap: 6 }}>
              <div aria-hidden="true" style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 4 }}>
                {[1, 2, 3, 4].map((n) => (
                  <span key={n} style={{
                    height: 4, borderRadius: 'var(--r-pill)',
                    background: n <= strength.level ? TONE[strength.tone] : 'var(--surface-sunk)',
                    transition: 'background var(--t-fast) var(--ease)',
                  }} />
                ))}
              </div>
              <div style={{ ...MUTED, color: TONE[strength.tone], fontWeight: 650 }}>
                Passphrase strength: {strength.label}
              </div>
            </div>
          )}
          {passphraseMismatch && <Problem>The passphrases differ.</Problem>}
          {samePassphrase && (
            <Problem>
              Use a passphrase that is not the account password: the password is checked by
              the sync service, the passphrase must never reach it.
            </Problem>
          )}
          {!creating && (
            <div>
              <Button type="button" size="sm" variant="ghost"
                onClick={() => { setUseCode(!useCode); setError(null); }}>
                {useCode ? 'Use the passphrase instead' : 'Forgot the passphrase? Use the recovery code'}
              </Button>
            </div>
          )}
          <Footer back={back('account')} ready={passphraseReady} />
        </form>
      );
      break;

    case 'device':
      body = (
        <form
          style={{ display: 'grid', gap: 'var(--sp-3)' }}
          onSubmit={(e) => { e.preventDefault(); if (deviceReady) void submit(); }}
        >
          <Field label="Call this device" value={deviceName} onChange={setDeviceName}
            testId="sync-device-name" />
          <div style={{ ...MUTED, marginTop: -6 }}>
            So you can tell your devices apart. You can sign any of them out later.
          </div>
          {creating && status.canShareSecrets && (
            <label style={{ display: 'flex', gap: 'var(--sp-2)', alignItems: 'start', ...MUTED }}>
              <input type="checkbox" checked={share} onChange={(e) => setShare(e.target.checked)}
                data-testid="sync-setup-share" style={{ marginTop: 3 }} />
              <span>
                <strong style={{ color: 'var(--text)' }}>Also share sign-ins and keys.</strong>
                {' '}Playlist passwords and your TMDB and Gemini keys go to your other devices,
                encrypted like everything else, so a new device needs only the passphrase.
                You can change this later.
              </span>
            </label>
          )}
          {creating && (
            <label style={{ display: 'flex', gap: 'var(--sp-2)', alignItems: 'start', ...MUTED }}>
              <input type="checkbox" checked={agreed} onChange={(e) => setAgreed(e.target.checked)}
                data-testid="sync-agree" style={{ marginTop: 3 }} />
              <span>
                I agree to the <PolicyLink />. Aurora stores your email address and your
                encrypted library with Google Firebase; it cannot read the library, and you
                can delete the account and everything in it at any time from Settings.
              </span>
            </label>
          )}
          <Footer
            back={back('passphrase')}
            ready={deviceReady}
            label={creating ? 'Create account' : 'Sign in'}
          />
        </form>
      );
      break;

    case 'working':
      body = (
        <div role="status" style={{ ...MUTED, display: 'grid', gap: 'var(--sp-2)' }}>
          <div className="sync-working" aria-hidden />
          {creating
            ? 'Creating the account, setting up encryption and sending this device’s library.'
            : 'Signing in, unlocking the household’s encryption and bringing in your other '
              + 'devices’ changes.'}
          {' '}This can take a minute on a large library.
        </div>
      );
      break;

    case 'recovery':
      body = (
        <form
          style={{ display: 'grid', gap: 'var(--sp-3)' }}
          data-testid="sync-recovery"
          onSubmit={(e) => { e.preventDefault(); if (savedCode) go('verify'); }}
        >
          <div style={MUTED}>
            If the passphrase is ever forgotten, this code is the only way back in. Nobody,
            Aurora included, can recover it for you. Keep it somewhere safe, away from this
            computer: a password manager, or written down.
          </div>
          <code
            data-testid="sync-recovery-code"
            style={{
              fontSize: 'var(--fs-md)', letterSpacing: '0.04em', wordBreak: 'break-all',
              userSelect: 'all', padding: 'var(--sp-3)', borderRadius: 'var(--r-md)',
              background: 'var(--surface-sunk)', border: '1px solid var(--border)',
            }}
          >
            {joined?.recoveryCode}
          </code>
          <div>
            <Button type="button" size="sm" onClick={() => void copyCode()}>
              {copied ? 'Copied' : 'Copy the code'}
            </Button>
          </div>
          <label style={{ display: 'flex', gap: 'var(--sp-2)', alignItems: 'center', ...MUTED }}>
            <input type="checkbox" checked={savedCode} onChange={(e) => setSavedCode(e.target.checked)}
              data-testid="sync-recovery-saved" />
            I have saved my recovery code somewhere safe
          </label>
          <Footer ready={savedCode} />
        </form>
      );
      break;

    case 'verify':
      body = (
        <div style={{ display: 'grid', gap: 'var(--sp-3)' }} data-testid="sync-setup-verify">
          <div style={MUTED}>
            We sent a link to <strong style={{ color: 'var(--text)' }}>{email.trim()}</strong>.
            Confirming it is what lets you reset the password if you ever forget it. Sync
            already works; you can do this later from Settings.
          </div>
          <div style={{ display: 'flex', gap: 'var(--sp-2)', flexWrap: 'wrap' }}>
            <Button variant="primary" onClick={() => void recheck()}>I have confirmed it</Button>
            <Button onClick={() => void resend()}>Send the email again</Button>
            <Button variant="ghost" onClick={() => go('done')}>Later</Button>
          </div>
          {note && <div style={MUTED}>{note}</div>}
        </div>
      );
      break;

    case 'done': {
      const report = joined?.report ?? null;
      body = (
        <div style={{ display: 'grid', gap: 'var(--sp-3)' }} data-testid="sync-setup-done">
          {joined?.problem ? (
            <Message ok={false}>
              This device is signed in. The first sync has not finished yet and will carry on
              by itself; Settings shows how it is going.
            </Message>
          ) : (
            <Message ok>
              {creating
                ? `${report?.pushed ?? 0} ${report?.pushed === 1 ? 'item' : 'items'} from this device are now in your household’s encrypted sync.`
                : `Brought in ${report?.applied ?? 0} ${report?.applied === 1 ? 'change' : 'changes'} from your other devices.`}
              {verified ? ' Your email address is confirmed.' : ''}
            </Message>
          )}
          {shareFailed && (
            <Message ok={false}>
              Sign-ins and keys are not being shared yet: {shareFailed}. You can turn it on
              in Settings.
            </Message>
          )}
          {creating && (
            <div style={{ ...CARD, gap: 6 }} data-testid="sync-next-device">
              <div style={{ fontWeight: 650, fontSize: 'var(--fs-sm)' }}>Adding another device</div>
              <ol style={{ ...MUTED, margin: 0, paddingLeft: '1.2em', display: 'grid', gap: 2 }}>
                <li>Install Aurora on it and open Settings → Account and sync.</li>
                <li>Choose <strong>Set up sync</strong>, then <strong>I already use sync on
                  another device</strong>.</li>
                <li>Sign in as {email.trim()} with your account password, then enter the
                  sync passphrase.</li>
              </ol>
            </div>
          )}
          <div style={{ display: 'flex', justifyContent: 'end' }}>
            <Button variant="primary" onClick={() => onClose(true)}>Done</Button>
          </div>
        </div>
      );
      break;
    }
  }

  const errorHere = error && step !== 'working';

  // Portalled: a page is its own stacking context (`.aurora-page`), and the wizard has to
  // cover the top bar and the tab bar too, as the parental PIN dialog does.
  return createPortal(
    // A click on the scrim closes, as a mouse convenience; Escape and the close button are
    // the keyboard's way out.
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 400, display: 'grid', placeItems: 'center',
        background: 'var(--scrim-dialog)', backdropFilter: 'blur(4px)', padding: 'var(--sp-4)',
      }}
      onMouseDown={(e) => { if (e.target === e.currentTarget) close(); }}
    >
      <div
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="sync-setup-title"
        data-testid="sync-setup"
        tabIndex={-1}
        style={{
          width: 'min(560px, 100%)', maxHeight: 'calc(100vh - 2 * var(--sp-4))', overflowY: 'auto',
          background: 'var(--bg-elevated)', borderRadius: 'var(--r-xl)', boxShadow: 'var(--shadow-4)',
          border: '1px solid var(--border)', padding: 'var(--sp-5)', display: 'grid', gap: 'var(--sp-4)',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'start', gap: 'var(--sp-3)' }}>
          <div style={{ flex: 1, display: 'grid', gap: 'var(--sp-2)' }}>
            <Progress path={path} step={step} />
            <h2
              id="sync-setup-title"
              ref={heading}
              tabIndex={-1}
              style={{ margin: 0, fontSize: 'var(--fs-xl)', fontFamily: 'var(--font-display)', outline: 'none' }}
            >
              {title[step]}
            </h2>
          </div>
          {!locked && step !== 'done' && (
            <Button type="button" variant="ghost" size="sm" onClick={close} aria-label="Close sync setup">
              ✕
            </Button>
          )}
        </div>
        {body}
        {note && step !== 'verify' && <Message ok>{note}</Message>}
        {errorHere && <Message ok={false}>{error}</Message>}
      </div>
    </div>,
    document.body,
  );
}

function Footer({ back, ready, label = 'Continue' }: {
  back?: React.ReactNode; ready: boolean; label?: string;
}) {
  return (
    <div style={{ display: 'flex', justifyContent: 'space-between', gap: 'var(--sp-2)', marginTop: 'var(--sp-1)' }}>
      <div>{back}</div>
      <Button type="submit" variant="primary" disabled={!ready}>{label}</Button>
    </div>
  );
}
