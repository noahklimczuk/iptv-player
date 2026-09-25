/**
 * Re-import one provider's library.
 *
 * `providers.refresh` existed from the beginning and was called from exactly one
 * place: the first-run wizard. So a library could be imported once and never again —
 * a channel your provider added last week was unreachable short of deleting the
 * provider and adding it back, and nothing in the app said so.
 *
 * The progress is the point, not decoration. A refresh on a real subscription took
 * 23.7 seconds in the measurement this project keeps; a button that looks dead for
 * that long is a button people press twice.
 */
import { useEffect, useState } from 'react';
import type { IngestProgress, SyncReport } from '@shared/ipc';
import { Button } from '@/components/Primitives';
import { invoke, onIngestProgress } from '@/ipc';
import { report as reportError } from '@/lib/errors';

const PHASE_LABEL: Record<string, string> = {
  downloading: 'Downloading',
  importingChannels: 'Channels',
  importingMovies: 'Movies',
  importingSeries: 'Series',
  importingEpg: 'Guide',
  indexing: 'Indexing',
  done: 'Done',
};

export function RefreshButton({
  providerId, onDone,
}: {
  providerId: number;
  onDone: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<IngestProgress | null>(null);
  const [done, setDone] = useState<SyncReport | null>(null);

  useEffect(() => onIngestProgress(setProgress), []);

  async function run() {
    setBusy(true);
    setDone(null);
    setProgress(null);
    try {
      const result = await invoke('providers.refresh', { providerId });
      setDone(result);
      onDone();
    } catch (e) {
      reportError('Could not refresh this provider')(e);
    } finally {
      setBusy(false);
      setProgress(null);
    }
  }

  return (
    <>
      <Button
        size="sm"
        icon="refresh"
        disabled={busy}
        onClick={() => void run()}
        data-testid={`refresh-provider-${providerId}`}
      >
        {busy ? 'Refreshing…' : 'Refresh'}
      </Button>

      {busy && progress && (
        <span
          data-testid="refresh-progress"
          style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)' }}
        >
          {PHASE_LABEL[progress.phase] ?? progress.phase}
          {progress.total > 0 && ` ${progress.done}/${progress.total}`}
        </span>
      )}

      {/* What a refresh actually found. A count of zero is the single most useful
          thing this screen can say — "0 channels" on a provider with twenty thousand
          films is the difference between a bug in Aurora and a panel that serves no
          live streams to this account, and nothing else in the app distinguishes
          them. */}
      {!busy && done && (
        <span
          data-testid="refresh-report"
          style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)' }}
        >
          {done.channels} channels · {done.movies} movies · {done.series} series
          {skipped(done) > 0 && (
            <>
              {' · '}
              <span style={{ color: 'var(--text)' }} title={whySkipped(done)}>
                {skipped(done)} skipped
              </span>
            </>
          )}
        </span>
      )}
    </>
  );
}

/** How many entries the provider listed that the import did not keep. */
function skipped(r: SyncReport): number {
  const d = r.dropped;
  if (!d) return 0;
  return d.hiddenByRule + d.kindExcluded + d.noStreamId + d.noEpisodeMarker;
}

/**
 * Why they went, in the order somebody would want to know.
 *
 * Each of these used to be a `continue` with nothing behind it, which is how a
 * refresh could report twenty thousand channels to a screen that showed none.
 */
function whySkipped(r: SyncReport): string {
  const d = r.dropped;
  const parts: string[] = [];
  if (d.hiddenByRule) parts.push(`${d.hiddenByRule} hidden by a playlist rule`);
  if (d.kindExcluded) parts.push(`${d.kindExcluded} of a content type not being imported`);
  if (d.noStreamId) parts.push(`${d.noStreamId} listed by the provider with no stream id`);
  if (d.noEpisodeMarker) parts.push(`${d.noEpisodeMarker} episodes with no season/episode marker`);
  return parts.join(', ');
}
