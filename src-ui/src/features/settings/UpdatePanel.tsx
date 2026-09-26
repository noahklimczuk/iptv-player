/**
 * Whether there is a newer build than this one, and installing it (README §23).
 *
 * Aurora installs from a GitHub release rather than a store, so nothing else would
 * ever tell someone the bug they hit was fixed a week ago. It fetches and applies the
 * update too, without leaving the app — never without checking the file against the
 * SHA-256 GitHub published beside it (docs/DECISIONS.md D17).
 *
 * Both kinds of copy update themselves, by different means: an installed one runs the
 * installer, a portable one unpacks the new files and swaps them in on the way back
 * up. The release page is still one click away, but it is no longer the only way for
 * a portable copy to get a newer build — which it used to be.
 */
import { useCallback, useEffect, useState } from 'react';
import type { UpdateDownload, UpdateStatus } from '@shared/ipc';
import { Badge, Button, ProgressBar } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { invoke, onUpdateDownload } from '@/ipc';
import { bytes as formatBytes } from '@/lib/format';

export function UpdatePanel() {
  const status = useCommand('updates.check', undefined, []);
  const [busy, setBusy] = useState<'checking' | 'opening' | 'starting' | 'installing' | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Seeded from the check and then driven by the host's events, so the bar moves
  // without this panel polling anything.
  const [live, setLive] = useState<UpdateDownload | null>(null);
  useEffect(() => onUpdateDownload(setLive), []);

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
  const download = live ?? data?.download ?? null;

  const startDownload = useCallback(async () => {
    setBusy('starting');
    setError(null);
    try {
      setLive(await invoke('updates.download'));
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }, []);

  const install = useCallback(async () => {
    setBusy('installing');
    setError(null);
    try {
      // On success the app exits and the installer takes over, so nothing after this
      // line runs in the ordinary case.
      await invoke('updates.install');
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(null);
    }
  }, []);

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
            {data.canInstall ? (
              <InstallControls
                download={download}
                kind={data.kind}
                busy={busy}
                onDownload={() => void startDownload()}
                onInstall={() => void install()}
                onOpen={() => void open()}
              />
            ) : (
              <>
                <Button
                  size="sm" variant="primary"
                  onClick={() => void open()} disabled={busy === 'opening'}
                >
                  Open release page
                </Button>
                <span
                  style={{ marginLeft: 10, fontSize: 'var(--fs-sm)', color: 'var(--text-faint)' }}
                >
                  An installed copy can only update itself on Windows, because what it
                  runs is the Windows installer. A portable copy can, on any platform.
                </span>
              </>
            )}
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

/**
 * Download, then install. Four states and one button, because at any moment there is
 * exactly one thing worth pressing.
 */
function InstallControls({
  download, kind, busy, onDownload, onInstall, onOpen,
}: {
  download: UpdateDownload | null;
  /** What pressing Install will actually do, which is different for the two copies. */
  kind: UpdateStatus['kind'];
  busy: string | null;
  onDownload: () => void;
  onInstall: () => void;
  onOpen: () => void;
}) {
  const status = download?.status ?? 'idle';

  if (status === 'downloading') {
    const total = download?.totalBytes ?? 0;
    const received = download?.receivedBytes ?? 0;
    const pct = total > 0 ? Math.min(100, Math.round((received / total) * 100)) : 0;
    return (
      <div style={{ maxWidth: 420 }}>
        <div
          style={{
            display: 'flex', justifyContent: 'space-between',
            fontSize: 'var(--fs-sm)', color: 'var(--text-muted)', marginBottom: 6,
          }}
        >
          <span>Downloading {download?.version}…</span>
          <span style={{ fontVariantNumeric: 'tabular-nums' }}>
            {formatBytes(received)}{total > 0 && ` of ${formatBytes(total)}`}
          </span>
        </div>
        <ProgressBar percent={pct} height={6} />
      </div>
    );
  }

  if (status === 'ready') {
    const portable = kind === 'portable';
    return (
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <Button size="sm" variant="primary" onClick={onInstall} disabled={busy === 'installing'}>
          {busy === 'installing'
            ? (portable ? 'Restarting…' : 'Starting the installer…')
            : 'Install and restart'}
        </Button>
        <span style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)', maxWidth: 420 }}>
          {portable
            // Unpacked when it was downloaded, so the only thing left is the restart —
            // and no installer means no unsigned-binary prompt to explain.
            ? 'Checked against the checksum GitHub published for it, and already '
              + 'unpacked. Aurora will restart into the new version; nothing else on '
              + 'this machine is touched.'
            : 'Checked against the checksum GitHub published for it. Aurora will close '
              + 'so the installer can replace it; these builds are not signed, so '
              + 'Windows will ask.'}
        </span>
      </div>
    );
  }

  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
      <Button size="sm" variant="primary" onClick={onDownload} disabled={busy === 'starting'}>
        {status === 'failed' ? 'Try again' : 'Download update'}
      </Button>
      <Button size="sm" onClick={onOpen} disabled={busy === 'opening'}>
        Open release page
      </Button>
      {status === 'failed' && download?.message && (
        <span role="alert" style={{ fontSize: 'var(--fs-sm)', color: 'var(--danger)' }}>
          {download.message}
        </span>
      )}
    </div>
  );
}
