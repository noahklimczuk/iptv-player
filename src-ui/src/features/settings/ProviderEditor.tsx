/**
 * Editing, revealing and removing a provider (README §15).
 *
 * The password is shown only on request and starts hidden. It is the viewer's own
 * subscription password on their own machine, and an account they cannot re-read is one
 * they cannot correct after a typo or a provider rotation — but a settings page that
 * renders it by default is one nobody can screen-share (README C10).
 */
import { useCallback, useEffect, useState } from 'react';
import type { Provider, ProviderCredentials } from '@shared/ipc';
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
