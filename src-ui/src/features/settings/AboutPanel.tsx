/**
 * What this build is, and the licences it is obliged to carry (README §18).
 *
 * The notices are not decoration. Aurora ships libmpv, which is LGPL (or GPL,
 * depending how the DLL was built) and carries FFmpeg inside it, and both require
 * their terms to travel with the binary and be findable by the person holding it. A
 * file in a repository nobody has cloned does not satisfy that. This screen does.
 *
 * The text is read from the file beside the executable rather than bundled into the
 * web build, so what is on screen is what was actually shipped.
 */
import { useState } from 'react';
import { Button } from '@/components/Primitives';
import { Icon } from '@/components/Icon';
import { useCommand } from '@/hooks/useCommand';
import { invoke } from '@/ipc';
import { report } from '@/lib/errors';

export function AboutPanel() {
  const { data } = useCommand('app.about', undefined, []);
  const [showNotices, setShowNotices] = useState(false);

  if (!data) return null;

  return (
    <div style={{ display: 'grid', gap: 'var(--sp-3)' }}>
      <dl
        style={{
          margin: 0, display: 'grid', gridTemplateColumns: '160px 1fr',
          gap: '6px 12px', alignItems: 'baseline',
        }}
      >
        <dt style={{ color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>Version</dt>
        <dd style={{ margin: 0, fontVariantNumeric: 'tabular-nums' }}>{data.version}</dd>

        <dt style={{ color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>Licence</dt>
        <dd style={{ margin: 0 }}>
          {data.license}
          <span style={{ color: 'var(--text-faint)' }}>
            {' — Aurora is free software. You may study, change and share it.'}
          </span>
        </dd>

        <dt style={{ color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>Playback</dt>
        <dd style={{ margin: 0, color: 'var(--text-muted)', fontSize: 'var(--fs-sm)' }}>
          libmpv, shipped as a separate file beside Aurora and replaceable with your
          own build. It contains FFmpeg.
        </dd>

        <dt style={{ color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>Metadata</dt>
        <dd style={{ margin: 0, color: 'var(--text-muted)', fontSize: 'var(--fs-sm)' }}>
          This product uses the TMDB API but is not endorsed or certified by TMDB.
        </dd>
      </dl>

      <div style={{ display: 'flex', gap: 'var(--sp-2)', flexWrap: 'wrap' }}>
        <Button
          size="sm"
          icon="info"
          onClick={() => setShowNotices((v) => !v)}
          disabled={!data.notices}
        >
          {showNotices ? 'Hide notices' : 'Third-party notices'}
        </Button>
        <Button
          size="sm"
          variant="secondary"
          icon="stack"
          onClick={() => {
            invoke('updates.openReleases').catch(report('Could not open the releases page'));
          }}
        >
          Source and releases
        </Button>
      </div>

      {!data.notices && (
        <div
          role="alert"
          style={{
            display: 'grid', gridTemplateColumns: '20px 1fr', gap: 'var(--sp-3)',
            padding: 'var(--sp-3)', borderRadius: 'var(--r-md)',
            background: 'var(--surface)', border: '1px solid var(--border)',
          }}
        >
          <Icon name="alert" size={18} style={{ color: 'var(--danger, #ff6b6b)' }} />
          <div style={{ fontSize: 'var(--fs-sm)' }}>
            <div style={{ fontWeight: 650 }}>The licence notices are missing</div>
            <div style={{ color: 'var(--text-muted)', marginTop: 4 }}>
              This build should carry <code>THIRD-PARTY-NOTICES.md</code> beside the
              application, and it is not at{' '}
              <code style={{ wordBreak: 'break-all' }}>{data.noticesPath}</code>. That
              is a packaging fault — please report it.
            </div>
          </div>
        </div>
      )}

      {showNotices && data.notices && (
        <pre
          data-testid="third-party-notices"
          style={{
            margin: 0, padding: 'var(--sp-3)', maxHeight: 360, overflow: 'auto',
            background: 'var(--surface)', borderRadius: 'var(--r-md)',
            border: '1px solid var(--border)', fontSize: 'var(--fs-xs)',
            whiteSpace: 'pre-wrap', wordBreak: 'break-word', userSelect: 'text',
          }}
        >
          {data.notices}
        </pre>
      )}
    </div>
  );
}
