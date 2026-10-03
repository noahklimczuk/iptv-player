/**
 * A strip that appears while recordings are using every stream the subscription allows.
 *
 * Panels sell a number of simultaneous connections and this one sells *one*. The DVR
 * already respects that for itself — it will not run more recordings at once than the
 * tightest enabled provider permits — but watching something opens a connection too, and
 * nothing counts that against the same budget. On a single-stream account a recording in
 * flight means the next thing you press cannot play, and until now the only way to find
 * that out was to press it and watch one of the two die.
 *
 * So it says so, where you are about to choose. It is a sentence rather than a block: the
 * viewer may well prefer to lose the recording, and they already know where to stop it.
 *
 * Visible only while it is true, which on a two-stream account with one recording is
 * never.
 */
import { useEffect, useState } from 'react';
import { Icon } from '@/components/Icon';
import { useCommand } from '@/hooks/useCommand';
import { onDvrTick } from '@/ipc';

export function ConnectionBudget() {
  // The scheduler starts and reaps on its own ten-second tick, so the strip follows the
  // same event the recordings page does rather than polling on its own clock.
  const [nonce, setNonce] = useState(0);
  useEffect(() => onDvrTick(() => setNonce((n) => n + 1)), []);

  const { data: storage } = useCommand('dvr.storage', undefined, [nonce]);
  const { data: inFlight } = useCommand('dvr.list', { state: 'recording' }, [nonce]);

  const allowed = storage?.maxConcurrent ?? 0;
  const busy = inFlight?.length ?? 0;
  if (!allowed || busy < allowed) return null;

  const names = (inFlight ?? []).map((r) => r.title);
  const what = names.length === 1 ? names[0] : `${names.length} recordings`;

  return (
    <div
      role="status"
      style={{
        display: 'flex', alignItems: 'center', gap: 'var(--sp-2)',
        margin: '0 var(--sp-6) var(--sp-3)', padding: '8px var(--sp-3)',
        borderRadius: 'var(--r-md)', background: 'var(--bg-elevated)',
        border: '1px solid var(--border-strong)', color: 'var(--text-muted)',
        fontSize: 'var(--fs-sm)',
      }}
    >
      <Icon name="record" size={13} filled />
      <span>
        <strong style={{ color: 'var(--text)', fontWeight: 650 }}>{what}</strong>
        {' '}
        {allowed === 1
          ? 'is using the only stream this subscription allows'
          : `are using all ${allowed} streams this subscription allows`}
        . Starting something now may stop {names.length === 1 ? 'it' : 'one of them'}.
      </span>
    </div>
  );
}
