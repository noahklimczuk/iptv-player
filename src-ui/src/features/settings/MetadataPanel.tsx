/**
 * Metadata enrichment settings (README §4.5).
 *
 * The key is write-only from here: it goes to the OS credential store and is never read
 * back, so the field shows whether one is set, not what it is. Offering to reveal it
 * would mean keeping it somewhere this screen could reach.
 */
import { useEffect, useState } from 'react';
import type { EnrichmentCoverage } from '@shared/ipc';
import { Badge, Button, FIELD, ProgressBar } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { invoke, onArtworkProgress, onMetadataProgress } from '@/ipc';
import { bytes } from '@/lib/format';

export function MetadataPanel() {
  const [nonce, setNonce] = useState(0);
  const { data: status } = useCommand('metadata.status', undefined, [nonce]);

  const [key, setKey] = useState('');
  const [saving, setSaving] = useState(false);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => onMetadataProgress(setProgress), []);

  const saveKey = async (value: string | null) => {
    setSaving(true);
    setError(null);
    try {
      await invoke('metadata.setKey', { key: value });
      setKey('');
      setMessage(value ? 'Key saved.' : 'Key removed.');
      setNonce((n) => n + 1);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const run = async () => {
    setRunning(true);
    setError(null);
    setMessage(null);
    setProgress(null);
    try {
      const report = await invoke('metadata.run', {});
      const parts = [`${report.matched} matched`];
      if (report.noMatch) parts.push(`${report.noMatch} not found`);
      if (report.failed) parts.push(`${report.failed} failed, will retry`);
      setMessage(report.matched + report.noMatch + report.failed === 0
        ? 'Everything is already up to date.'
        : parts.join(' · '));
      setNonce((n) => n + 1);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
      setProgress(null);
    }
  };

  const remaining = status
    ? pendingOf(status.movies) + pendingOf(status.series)
    : 0;

  return (
    <div style={{ display: 'grid', gap: 'var(--sp-4)' }}>
      <p style={{ margin: 0, fontSize: 'var(--fs-sm)', color: 'var(--text-muted)', lineHeight: 1.6 }}>
        Posters, backdrops, descriptions and cast come from TMDB. Aurora only asks about
        a title once — if nothing matches confidently, it leaves the title alone rather
        than guessing, so a wrong poster never ends up on the wrong film.
      </p>

      <div style={{ display: 'flex', gap: 'var(--sp-2)', alignItems: 'center', flexWrap: 'wrap' }}>
        <label htmlFor="tmdb-key" style={{ fontSize: 'var(--fs-sm)', minWidth: 90 }}>
          API key
        </label>
        <input className="aurora-field"
          id="tmdb-key"
          type="password"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          placeholder={
            status?.keyIsBuiltIn
              ? 'Built in — nothing to enter'
              : status?.hasKey
                ? '•••••••• (a key is saved)'
                : 'Paste your TMDB API key'
          }
          autoComplete="off"
          spellCheck={false}
          style={inputStyle}
        />
        <Button
          size="sm"
          variant="primary"
          disabled={saving || !key.trim()}
          onClick={() => void saveKey(key)}
        >
          Save
        </Button>
        {status?.hasKey && !status.keyIsBuiltIn && (
          <Button size="sm" variant="ghost" disabled={saving} onClick={() => void saveKey(null)}>
            Remove
          </Button>
        )}
      </div>

      {status?.keyIsBuiltIn && (
        <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)' }}>
          This build ships with a metadata key, so artwork and cast work without any
          setup. Entering your own replaces it — worth doing only if you would rather the
          requests counted against your own rate limit than one shared with every copy of
          this build.
        </div>
      )}

      {status && !status.keyIsPersistent && status.hasKey && !status.keyIsBuiltIn && (
        <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--warning)' }}>
          This key is only kept for as long as Aurora is running — there is no OS
          credential store on this platform, so it will need re-entering after a restart.
        </div>
      )}

      {status && (
        <div style={{ display: 'grid', gap: 'var(--sp-3)' }}>
          <CoverageRow label="Movies" coverage={status.movies} />
          <CoverageRow label="Series" coverage={status.series} />
        </div>
      )}

      <div style={{ display: 'flex', gap: 'var(--sp-2)', alignItems: 'center', flexWrap: 'wrap' }}>
        <Button
          size="sm"
          icon="sparkle"
          disabled={running || !status?.hasKey || remaining === 0}
          onClick={() => void run()}
        >
          {running ? 'Fetching…' : 'Fetch metadata now'}
        </Button>
        {!status?.hasKey && (
          <span style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)' }}>
            Add a key to enable this.
          </span>
        )}
        {status?.hasKey && remaining === 0 && (
          <span style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)' }}>
            Nothing left to look up.
          </span>
        )}
        {status?.hasKey && remaining > 0 && !running && (
          <span style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)' }}>
            {remaining} {remaining === 1 ? 'title' : 'titles'} to go.
          </span>
        )}
      </div>

      {progress && progress.total > 0 && (
        <div>
          <ProgressBar percent={(progress.done / progress.total) * 100} height={4} />
          <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-faint)', marginTop: 4 }}>
            {progress.done} of {progress.total}
          </div>
        </div>
      )}

      {message && (
        <div role="status" style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)' }}>
          {message}
        </div>
      )}
      {error && (
        <div role="alert" style={{ fontSize: 'var(--fs-sm)', color: 'var(--danger)' }}>
          {error}
        </div>
      )}

      <ArtworkCachePanel />
    </div>
  );
}

/**
 * The local artwork cache.
 *
 * Enrichment stores the remote URL, so this is purely an accelerator: emptying it costs
 * nothing but the next download, which is why Clear needs no confirmation.
 */
function ArtworkCachePanel() {
  const [nonce, setNonce] = useState(0);
  const { data: cache } = useCommand('artwork.status', undefined, [nonce]);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => onArtworkProgress(setProgress), []);

  const act = async (fn: () => Promise<string>) => {
    setBusy(true);
    setError(null);
    setMessage(null);
    setProgress(null);
    try {
      setMessage(await fn());
      setNonce((n) => n + 1);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      setProgress(null);
    }
  };

  return (
    <div style={{ borderTop: '1px solid var(--border)', paddingTop: 'var(--sp-4)', display: 'grid', gap: 'var(--sp-3)' }}>
      <div style={{ fontWeight: 650 }}>Artwork cache</div>
      <p style={{ margin: 0, fontSize: 'var(--fs-sm)', color: 'var(--text-muted)', lineHeight: 1.6 }}>
        Posters and backdrops are downloaded once and kept on disk. The library stores the
        original addresses, so clearing this only costs the next download.
      </p>

      {cache && (
        <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)' }}>
          {cache.files} {cache.files === 1 ? 'image' : 'images'} · {bytes(cache.usedBytes)}
          {cache.maxBytes > 0 && <> of {bytes(cache.maxBytes)}</>}
          <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-faint)', marginTop: 2 }}>
            {cache.folder}
          </div>
        </div>
      )}

      <div style={{ display: 'flex', gap: 'var(--sp-2)', flexWrap: 'wrap' }}>
        <Button
          size="sm"
          icon="stack"
          disabled={busy}
          onClick={() => void act(async () => {
            const r = await invoke('artwork.prefetch', {});
            if (r.downloaded === 0 && r.failed === 0) return 'Everything is already downloaded.';
            const parts = [`${r.downloaded} downloaded`];
            if (r.failed) parts.push(`${r.failed} unavailable`);
            if (r.evicted) parts.push(`${r.evicted} evicted to stay under the limit`);
            return parts.join(' · ');
          })}
        >
          {busy ? 'Downloading…' : 'Download artwork'}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          disabled={busy || !cache?.files}
          onClick={() => void act(async () => {
            const removed = await invoke('artwork.clear', undefined);
            return `${removed} ${removed === 1 ? 'image' : 'images'} removed.`;
          })}
        >
          Clear cache
        </Button>
      </div>

      {progress && progress.total > 0 && (
        <div>
          <ProgressBar percent={(progress.done / progress.total) * 100} height={4} />
          <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-faint)', marginTop: 4 }}>
            {progress.done} of {progress.total}
          </div>
        </div>
      )}

      {message && (
        <div role="status" style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)' }}>
          {message}
        </div>
      )}
      {error && (
        <div role="alert" style={{ fontSize: 'var(--fs-sm)', color: 'var(--danger)' }}>
          {error}
        </div>
      )}
    </div>
  );
}

const pendingOf = (c: EnrichmentCoverage) =>
  Math.max(0, c.total - c.matched - c.noMatch - c.failed);

function CoverageRow({ label, coverage }: { label: string; coverage: EnrichmentCoverage }) {
  const { total, matched, noMatch, failed } = coverage;
  const pct = total > 0 ? (matched / total) * 100 : 0;
  return (
    <div>
      <div
        style={{
          display: 'flex', alignItems: 'center', gap: 'var(--sp-2)',
          fontSize: 'var(--fs-sm)', marginBottom: 4,
        }}
      >
        <span style={{ minWidth: 90 }}>{label}</span>
        <span style={{ color: 'var(--text-muted)' }}>
          {matched} of {total}
        </span>
        {/* Not-found is a settled answer, not an error, and is labelled as one. */}
        {noMatch > 0 && <Badge tone="outline">{noMatch} not found</Badge>}
        {failed > 0 && <Badge tone="outline">{failed} to retry</Badge>}
      </div>
      <ProgressBar percent={pct} height={4} />
    </div>
  );
}

/** One field, defined once. See `FIELD` in Primitives. */
const inputStyle: React.CSSProperties = { ...FIELD, flex: 1, minWidth: 220 };
