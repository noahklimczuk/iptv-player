/**
 * Whether there is a newer build than this one (README §23).
 *
 * Aurora installs from a GitHub release rather than a store, so nothing else would
 * ever tell someone the bug they hit was fixed a week ago. What this does not do is
 * install: the host has no signing key to verify a download against, so the honest
 * offer is the release page and its notes, not a silent replacement of the running
 * binary (docs/DECISIONS.md D17).
 */
import { useCallback, useState } from 'react';
import type { UpdateStatus } from '@shared/ipc';
import { Badge, Button } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { invoke } from '@/ipc';

export function UpdatePanel() {
  const status = useCommand('updates.check', undefined, []);
  const [busy, setBusy] = useState<'checking' | 'opening' | null>(null);
  const [error, setError] = useState<string | null>(null);

  const check = useCallback(async () => {
    setBusy('checking');
    setError(null);
    try {
      // Forced: the whole point of pressing this is to bypass the cached answer.
      await invoke('updates.check', { force: true });
      status.reload();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }, [status]);

  const open = useCallback(async () => {
    setBusy('opening');
    setError(null);
    try {
      await invoke('updates.openReleases');
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }, []);

  const setAutomatic = useCallback(
    async (enabled: boolean) => {
      setError(null);
      try {
        await invoke('updates.setAutomatic', { enabled });
        status.reload();
      } catch (e: unknown) {
        setError(e instanceof Error ? e.message : String(e));
      }
    },
    [status],
  );

  const data: UpdateStatus | null = status.data;

  return (
    <div>
      <div
        style={{
          display: 'flex', alignItems: 'center', gap: 'var(--sp-3)',
          paddingBottom: 'var(--sp-3)', borderBottom: '1px solid var(--border)',
        }}
      >
        <div style={{ flex: 1 }}>
          <div style={{ fontWeight: 600 }}>
            This build
            {data && (
              <span style={{ marginLeft: 8, fontVariantNumeric: 'tabular-nums' }}>
                {data.current}
              </span>
            )}
          </div>
          <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)' }}>
            {status.loading && 'Checking…'}
            {!status.loading && data && !data.available && data.latest &&
              'This is the newest published build.'}
            {!status.loading && data && !data.latest &&
              'Nothing is published yet to compare against.'}
            {!status.loading && data?.available && data.latest && (
              <>Version {data.latest.version} is available.</>
            )}
            {!status.loading && !data && !status.error && 'Not checked yet.'}
          </div>
        </div>
        <Button size="sm" onClick={() => void check()} disabled={busy === 'checking'}>
          {busy === 'checking' ? 'Checking…' : 'Check now'}
        </Button>
      </div>

      {data?.available && data.latest && (
        <div style={{ padding: 'var(--sp-3) 0', borderBottom: '1px solid var(--border)' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--sp-2)' }}>
            <Badge tone="accent">Update available</Badge>
            <strong style={{ fontVariantNumeric: 'tabular-nums' }}>{data.latest.version}</strong>
            {data.latest.installerBytes != null && (
              <span style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)' }}>
                {Math.round(data.latest.installerBytes / 1024 / 1024)} MB
              </span>
            )}
          </div>
          {data.latest.notes && (
            // Deliberately preformatted text and not markup: these notes come off a web
            // page, and nothing here should be rendering what a release body contains.
            <pre
              style={{
                margin: 'var(--sp-2) 0 0', maxHeight: 160, overflowY: 'auto',
                whiteSpace: 'pre-wrap', wordBreak: 'break-word',
                fontFamily: 'inherit', fontSize: 'var(--fs-sm)',
                color: 'var(--text-muted)',
              }}
            >
              {data.latest.notes}
            </pre>
          )}
          <div style={{ marginTop: 'var(--sp-3)' }}>
            <Button size="sm" variant="primary" onClick={() => void open()} disabled={busy === 'opening'}>
              Get the update
            </Button>
            <span style={{ marginLeft: 10, fontSize: 'var(--fs-sm)', color: 'var(--text-faint)' }}>
              Opens the release page. Aurora does not install it for you.
            </span>
          </div>
        </div>
      )}

      <div
        style={{
          display: 'flex', alignItems: 'flex-start', gap: 'var(--sp-4)',
          paddingTop: 'var(--sp-3)',
        }}
      >
        <div style={{ flex: 1 }}>
          <div style={{ fontWeight: 600 }}>Check on launch</div>
          <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)', maxWidth: 560 }}>
            Asks GitHub once every six hours, in the background, and never interrupts
            playback. Turning it off means finding out about fixes by looking.
          </div>
        </div>
        <button
          role="switch"
          aria-checked={data?.automatic ?? true}
          aria-label="Check for updates on launch"
          disabled={!data}
          onClick={() => void setAutomatic(!(data?.automatic ?? true))}
          style={{
            display: 'inline-flex', alignItems: 'center', gap: 7, flexShrink: 0,
            padding: '7px 14px', borderRadius: 'var(--r-full)', cursor: 'pointer',
            fontSize: 'var(--fs-sm)', fontWeight: 600,
            border: `1px solid ${data?.automatic ? 'transparent' : 'var(--border-strong)'}`,
            background: data?.automatic ? 'var(--accent)' : 'transparent',
            color: data?.automatic ? 'var(--accent-text)' : 'var(--text-muted)',
          }}
        >
          {data?.automatic ? 'On' : 'Off'}
        </button>
      </div>

      {(error ?? status.error) && (
        <div role="alert" style={{ color: 'var(--danger)', fontSize: 'var(--fs-sm)', marginTop: 10 }}>
          {error ?? status.error}
        </div>
      )}
    </div>
  );
}
