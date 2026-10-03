/**
 * Editing, revealing and removing a provider (README §15).
 *
 * The password is shown only on request and starts hidden. It is the viewer's own
 * subscription password on their own machine, and an account they cannot re-read is one
 * they cannot correct after a typo or a provider rotation — but a settings page that
 * renders it by default is one nobody can screen-share (README C10).
 */
import { useCallback, useEffect, useState } from 'react';
import type { Provider, ProviderCredentials, ValidationResult } from '@shared/ipc';
import { Button, FIELD } from '@/components/Primitives';
import { invoke } from '@/ipc';

export function ProviderEditor({
  provider, onClose, onChanged,
}: {
  provider: Provider;
  onClose: () => void;
  onChanged: () => void;
}) {
  const [loaded, setLoaded] = useState<ProviderCredentials | null>(null);
  const [name, setName] = useState(provider.name);
  const [url, setUrl] = useState('');
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  /**
   * The result of checking what is in the form, if it has been checked.
   *
   * The wizard has had this since it was written and the editor never did — so the one
   * screen where you go to *fix* a provider that has stopped working was the one with no
   * way to find out whether your fix worked. You saved, and learned at the next refresh.
   */
  const [checked, setChecked] = useState<ValidationResult | null>(null);
  const [checking, setChecking] = useState(false);

  /**
   * Ask the host about what is in the form, without saving it.
   *
   * Takes the address as an argument for the same reason the wizard's does: the "use that
   * address" button sets it and re-checks in one go, and state has not landed yet.
   */
  const check = useCallback(async (overrideUrl?: string) => {
    setChecking(true);
    try {
      setChecked(await invoke('providers.validate', {
        draft: {
          // `Provider.kind` knows about Stalker and `providers.validate` does not; the
          // button offering this is hidden for one, so the narrowing is never a guess.
          name,
          kind: provider.kind === 'xtream' ? 'xtream' : 'm3u',
          url: overrideUrl ?? url,
          username: username || null,
          password: password || null,
        },
      }));
    } catch (e) {
      setChecked({
        ok: false,
        message: 'Could not check that',
        detail: e instanceof Error ? e.message : String(e),
        expiresAt: null, daysUntilExpiry: null, maxConnections: null,
        activeConnections: null, isTrial: false, credentialsDetected: false,
        suggestedUrl: null, suggestedKind: null,
      });
    } finally {
      setChecking(false);
    }
  }, [name, provider.kind, url, username, password]);
  const [revealed, setRevealed] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    invoke('providers.credentials', { providerId: provider.id })
      .then((c) => {
        if (!live) return;
        setLoaded(c);
        setName(c.name);
        setUrl(c.url);
        setUsername(c.username);
        setPassword(c.password ?? '');
      })
      .catch((e: unknown) => live && setError(e instanceof Error ? e.message : String(e)));
    return () => { live = false; };
  }, [provider.id]);

  const save = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke('providers.update', {
        providerId: provider.id,
        // Stalker portals are listed in the contract but the wizard never creates one
        // (docs/DECISIONS.md D13, deferred), so an edit cannot produce a draft for a
        // kind the draft type has no value for. Anything unexpected stays an M3U URL,
        // which is the shape that needs no credentials.
        draft: {
          name,
          kind: provider.kind === 'xtream' ? 'xtream' : 'm3u',
          url,
          username,
          password,
        },
      });
      onChanged();
      onClose();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }, [provider.id, provider.kind, name, url, username, password, onChanged, onClose]);

  const remove = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke('providers.delete', { providerId: provider.id });
      onChanged();
      onClose();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  }, [provider.id, onChanged, onClose]);

  return (
    <div
      style={{
        marginTop: 'var(--sp-3)', padding: 'var(--sp-4)',
        border: '1px solid var(--border-strong)', borderRadius: 'var(--r-md)',
        background: 'var(--bg-base)', display: 'grid', gap: 'var(--sp-3)',
      }}
    >
      <Field label="Name">
        <input className="aurora-field" value={name} onChange={(e) => setName(e.target.value)} aria-label="Provider name" style={input} />
      </Field>
      <Field label="Address">
        <input className="aurora-field" value={url} onChange={(e) => setUrl(e.target.value)} aria-label="Provider address" style={input} />
      </Field>

      {provider.kind === 'xtream' && (
        <>
          <Field label="Username">
            <input className="aurora-field" value={username} onChange={(e) => setUsername(e.target.value)} aria-label="Username" style={input} />
          </Field>
          <Field label="Password">
            <div style={{ display: 'flex', gap: 6, flex: 1 }}>
              <input className="aurora-field"
                type={revealed ? 'text' : 'password'}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                aria-label="Password"
                autoComplete="off"
                spellCheck={false}
                style={{ ...input, flex: 1 }}
              />
              <Button
                size="sm"
                aria-pressed={revealed}
                onClick={() => setRevealed((r) => !r)}
              >
                {revealed ? 'Hide' : 'Show'}
              </Button>
            </div>
          </Field>
        </>
      )}

      {checked && (
        <div
          role="status"
          style={{
            padding: 'var(--sp-3)', borderRadius: 'var(--r-md)',
            fontSize: 'var(--fs-sm)',
            background: checked.ok
              ? 'color-mix(in srgb, var(--success) 12%, transparent)'
              : 'color-mix(in srgb, var(--danger) 12%, transparent)',
            border: `1px solid ${checked.ok ? 'var(--success)' : 'var(--danger)'}`,
          }}
        >
          <strong>{checked.message}</strong>
          {checked.detail && (
            <div style={{ color: 'var(--text-muted)', marginTop: 3 }}>{checked.detail}</div>
          )}
          {/* The same offer the wizard makes: the host found this panel answering under
              the other scheme. Which is the case this editor most needs, because a
              provider that was saved with an address that no longer works is exactly what
              somebody opens this screen to repair. */}
          {!checked.ok && checked.suggestedUrl && (
            <div style={{ marginTop: 'var(--sp-3)' }}>
              <div style={{ marginBottom: 6 }}>
                {checked.suggestedKind === 'm3u'
                  ? 'Its playlist does work, even though its API does not. Most other'
                    + ' players use the playlist and never touch the API, which is why'
                    + ' this subscription works elsewhere.'
                  : <>It does answer at <strong>{checked.suggestedUrl}</strong>.</>}
                {checked.suggestedKind !== 'm3u' && checked.suggestedUrl.startsWith('http://')
                  && ' That is an unencrypted address, so your sign-in would be sent in'
                    + ' clear text — which is how most panels work.'}
              </div>
              <Button
                size="sm"
                data-testid="editor-use-suggested-url"
                onClick={() => { setUrl(checked.suggestedUrl!); void check(checked.suggestedUrl!); }}
              >
                {checked.suggestedKind === 'm3u' ? 'Use its playlist instead' : 'Use that address'}
              </Button>
            </div>
          )}
        </div>
      )}

      {loaded && !loaded.passwordIsPersistent && (
        <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--warning)' }}>
          There is no OS credential store on this platform, so a password saved here
          lasts only until Aurora closes.
        </div>
      )}

      <div style={{ display: 'flex', gap: 'var(--sp-2)', alignItems: 'center' }}>
        <Button size="sm" variant="primary" disabled={busy || !name.trim()} onClick={() => void save()}>
          Save changes
        </Button>
        {/* Stalker portals authenticate differently and `providers.validate` has no path
            for them, so there is nothing honest to offer here. */}
        {provider.kind !== 'stalker' && (
          <Button
            size="sm"
            disabled={busy || checking || !url.trim()}
            onClick={() => void check()}
          >
            {checking ? 'Checking…' : 'Check connection'}
          </Button>
        )}
        <Button size="sm" disabled={busy} onClick={onClose}>Cancel</Button>

        <div style={{ flex: 1 }} />

        {/* Two steps, because this also deletes everything the provider imported and
            there is no undo for it. */}
        {confirming ? (
          <>
            <span style={{ fontSize: 'var(--fs-sm)', color: 'var(--danger)' }}>
              Remove {provider.name} and its {provider.channelCount + provider.movieCount
                + provider.seriesCount} imported items?
            </span>
            <Button size="sm" variant="danger" disabled={busy} onClick={() => void remove()}>
              Yes, remove
            </Button>
            <Button size="sm" disabled={busy} onClick={() => setConfirming(false)}>Keep</Button>
          </>
        ) : (
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => setConfirming(true)}>
            Remove provider
          </Button>
        )}
      </div>

      {error && (
        <div role="alert" style={{ color: 'var(--danger)', fontSize: 'var(--fs-sm)' }}>{error}</div>
      )}
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--sp-3)' }}>
      <label style={{ fontSize: 'var(--fs-sm)', minWidth: 90, color: 'var(--text-muted)' }}>
        {label}
      </label>
      {children}
    </div>
  );
}

/** One field, defined once. See `FIELD` in Primitives. */
const input: React.CSSProperties = { ...FIELD, flex: 1, minWidth: 0 };
