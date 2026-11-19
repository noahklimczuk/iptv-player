/**
 * Settings → Privacy (README §15: "history retention, clear history").
 *
 * Watch history is what Continue Watching, the ticks on episodes and every
 * recommendation are read from. Until this panel, the only way to forget any of it was
 * one card at a time off Continue Watching, and the rest stayed for good.
 *
 * The host enforces both: the window is applied there (at once, and every half hour
 * after), and both commands sit behind the Settings lock, so a kids profile cannot
 * clear what it watched.
 */
import { useState } from 'react';
import { Badge, Button } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { invoke } from '@/ipc';
import { useProfile } from '@/state/profile';

/** The windows offered; null keeps history for good, which is the default. */
const WINDOWS: ReadonlyArray<{ days: number | null; label: string }> = [
  { days: null, label: 'Forever' },
  { days: 365, label: '1 year' },
  { days: 90, label: '90 days' },
  { days: 30, label: '30 days' },
];

function entries(n: number): string {
  return `${n.toLocaleString()} ${n === 1 ? 'title' : 'titles'}`;
}

export function PrivacyPanel() {
  const profile = useProfile((s) => s.active);
  const profileId = profile?.id ?? 1;
  const [nonce, setNonce] = useState(0);
  const { data: history, error: loadError } = useCommand(
    'privacy.history', { profileId }, [profileId, nonce],
  );
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async (work: () => Promise<string>) => {
    setBusy(true);
    setError(null);
    try {
      setMessage(await work());
    } catch (e) {
      setMessage(null);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      setConfirming(false);
      setNonce((n) => n + 1);
    }
  };

  if (loadError) {
    return <div style={{ color: 'var(--danger)', fontSize: 'var(--fs-sm)' }}>{loadError}</div>;
  }
  if (!history) return <div style={{ color: 'var(--text-faint)' }}>Loading…</div>;

  const who = profile ? `${profile.name}'s` : "This profile's";

  return (
    <div>
      <Row
        label="Keep watch history"
        hint="Anything not watched for longer than this is forgotten, on every device you sync with."
      >
        <div className="dl-seg" role="group" aria-label="Keep watch history">
          {WINDOWS.map((w) => (
            <button
              key={w.label}
              type="button"
              data-active={history.days === w.days}
              data-variant={history.days === w.days ? 'primary' : 'secondary'}
              aria-pressed={history.days === w.days}
              disabled={busy}
              onClick={() => void run(async () => {
                const removed = await invoke('privacy.setHistoryDays', { days: w.days });
                if (w.days == null) return 'Watch history is kept for good.';
                return removed > 0
                  ? `Kept for ${w.label}. Forgot ${entries(removed)} older than that.`
                  : `Kept for ${w.label}.`;
              })}
            >
              {w.label}
            </button>
          ))}
        </div>
      </Row>

      <Row
        label="Clear watch history"
        hint={`${who} Continue Watching, episode ticks and recommendations start again from nothing. Other profiles keep theirs.`}
      >
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <Badge tone="outline">{entries(history.items)}</Badge>
          {/* Two steps, because there is no undo, and the deletion syncs. */}
          {confirming ? (
            <>
              <Button
                size="sm" variant="danger" disabled={busy}
                onClick={() => void run(async () => {
                  const removed = await invoke('privacy.clearHistory', { profileId });
                  return removed > 0 ? `Forgot ${entries(removed)}.` : 'There was nothing to forget.';
                })}
              >
                Yes, clear it
              </Button>
              <Button size="sm" disabled={busy} onClick={() => setConfirming(false)}>Keep</Button>
            </>
          ) : (
            <Button
              size="sm" variant="secondary"
              disabled={busy || history.items === 0}
              onClick={() => setConfirming(true)}
            >
              Clear history
            </Button>
          )}
        </div>
      </Row>

      <Row
        label="Usage data"
        hint="Aurora sends none: there is no analytics or crash reporting to switch on. docs/PRIVACY.md lists everything that does leave this PC."
      >
        <Badge tone="outline">None sent</Badge>
      </Row>

      <div role="status" style={{ fontSize: 'var(--fs-sm)', minHeight: '1.4em' }}>
        {error
          ? <span style={{ color: 'var(--danger)' }}>{error}</span>
          : message && <span style={{ color: 'var(--success)' }}>{message}</span>}
      </div>
    </div>
  );
}

function Row({
  label, hint, children,
}: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="settings-row">
      <div style={{ flex: 1, minWidth: 0 }}>
        <div className="settings-row-label">{label}</div>
        {hint && <div className="settings-row-hint">{hint}</div>}
      </div>
      {children}
    </div>
  );
}
