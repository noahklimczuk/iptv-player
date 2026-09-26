/**
 * The things a support message needs, and the one thing a viewer has to be told
 * (README §18).
 *
 * Two gaps this closes. There was no way to get at the log: it sits inside
 * `%LOCALAPPDATA%`, which is not somewhere anyone goes, and a release build has no
 * console. And when the library could not be read at startup, the app used to fail to
 * launch entirely; now it starts a new one, which means a viewer can open Aurora to
 * find their favourites and watch history gone with nothing said about why.
 */
import { useState } from 'react';
import { Button } from '@/components/Primitives';
import { Icon } from '@/components/Icon';
import { useCommand } from '@/hooks/useCommand';
import { invoke } from '@/ipc';
import { report } from '@/lib/errors';

export function DiagnosticsPanel() {
  const { data } = useCommand('app.diagnostics', undefined, []);
  const [exported, setExported] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const exportLogs = () => {
    setBusy(true);
    invoke('logs.export')
      .then((r) => setExported(r.folder))
      .catch(report('Could not export the logs'))
      .finally(() => setBusy(false));
  };

  return (
    <div style={{ display: 'grid', gap: 'var(--sp-3)' }}>
      {data?.libraryWasReplaced && (
        <div
          role="alert"
          style={{
            display: 'grid', gridTemplateColumns: '20px 1fr', gap: 'var(--sp-3)',
            padding: 'var(--sp-3)', borderRadius: 'var(--r-md)',
            background: 'var(--surface)', border: '1px solid var(--border)',
          }}
        >
          <Icon name="alert" size={18} style={{ color: 'var(--danger, #ff6b6b)' }} />
          <div>
            <div style={{ fontWeight: 650 }}>Your library was started again</div>
            <div style={{ color: 'var(--text-muted)', fontSize: 'var(--fs-sm)', marginTop: 4 }}>
              The database could not be read at launch, so Aurora began a new one.
              Favourites, watch history and recording metadata went with it — refresh
              your provider to rebuild the library. The old file was kept at{' '}
              <code style={{ wordBreak: 'break-all' }}>{data.libraryWasReplaced}</code>.
            </div>
          </div>
        </div>
      )}

      {data && !data.videoEngine.rendersVideo && (
        <div
          role="alert"
          data-testid="no-video-engine"
          style={{ color: 'var(--text-muted)', fontSize: 'var(--fs-sm)' }}
        >
          No video engine loaded, so the player will show no picture — sound still
          plays. On Windows that means <code>libmpv-2.dll</code> could not be loaded;
          the log says which. Export it below.
        </div>
      )}

      {data && !data.credentialsPersist && (
        <div style={{ color: 'var(--text-muted)', fontSize: 'var(--fs-sm)' }}>
          This platform has no credential store, so provider passwords are kept in
          memory only and are lost when Aurora closes.
        </div>
      )}

      <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--sp-3)', flexWrap: 'wrap' }}>
        <Button size="sm" icon="stack" onClick={exportLogs} disabled={busy}>
          {busy ? 'Exporting…' : 'Export logs'}
        </Button>
        <span style={{ color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>
          Copies this run and the one before it, for a bug report.
        </span>
      </div>

      {exported && (
        <div data-testid="log-export-result" style={{ fontSize: 'var(--fs-sm)' }}>
          Written to <code style={{ wordBreak: 'break-all' }}>{exported}</code>
        </div>
      )}

      {data && (
        <div style={{ color: 'var(--text-faint)', fontSize: 'var(--fs-xs)' }}>
          Data folder: <code style={{ wordBreak: 'break-all' }}>{data.dataDir}</code>
          <br />
          {/* The first thing to establish about "there is no picture", and until now
              only findable by reading the log. */}
          Video engine:{' '}
          <code data-testid="video-engine">
            {data.videoEngine.version ?? data.videoEngine.name}
          </code>
        </div>
      )}
    </div>
  );
}
