/**
 * Settings → Account and sync → Backup: everything sync would carry, saved to a file
 * instead of a server, for the people who do not use sync (`aurora-app/src/backup.rs`).
 *
 * Saving goes to `Documents\Aurora TV\Backups`; the host picks the folder and the name.
 * Restoring reads a file the viewer picks here, shows what it holds, and only writes once
 * they say so. Neither ever carries a password: a restored provider asks for its own
 * again in Providers.
 */

import { useRef, useState } from 'react';
import type { BackupRestored, BackupSaved, BackupSummary } from '@shared/ipc';
import { Button } from '@/components/Primitives';
import { invoke } from '@/ipc';
import { CARD, MUTED, Message } from '@/features/settings/SyncPanel';

/** What each kind is called on screen, in the order a viewer thinks of them. */
const KIND_NAMES: [kind: string, one: string, many: string][] = [
  ['provider', 'provider', 'providers'],
  ['profile', 'profile', 'profiles'],
  ['favorite', 'favourite', 'favourites'],
  ['my_list', 'title on My List', 'titles on My List'],
  ['progress', 'thing in progress', 'things in progress'],
  ['override', 'channel edit', 'channel edits'],
  ['recording_rule', 'recording rule', 'recording rules'],
  ['reminder', 'reminder', 'reminders'],
  ['setting', 'setting', 'settings'],
];

/** "2 providers, 3 profiles, 41 favourites and 12 settings". */
export function describeCounts(counts: Record<string, number>): string {
  const parts: string[] = [];
  let other = 0;
  const named = new Set(KIND_NAMES.map(([k]) => k));
  for (const [kind, one, many] of KIND_NAMES) {
    const n = counts[kind] ?? 0;
    if (n > 0) parts.push(`${n} ${n === 1 ? one : many}`);
  }
  for (const [kind, n] of Object.entries(counts)) if (!named.has(kind)) other += n;
  if (other > 0) parts.push(`${other} other ${other === 1 ? 'item' : 'items'}`);
  if (parts.length === 0) return 'nothing';
  if (parts.length === 1) return parts[0] ?? 'nothing';
  return `${parts.slice(0, -1).join(', ')} and ${parts[parts.length - 1] ?? ''}`;
}

function made(summary: BackupSummary): string {
  const when = summary.createdAt > 0
    ? new Date(summary.createdAt * 1000).toLocaleString(undefined, {
      dateStyle: 'medium', timeStyle: 'short',
    })
    : 'an unknown date';
  return summary.appVersion ? `Made ${when} by Aurora ${summary.appVersion}.` : `Made ${when}.`;
}

function restoredText(r: BackupRestored): string {
  const lines = [`Restored ${r.applied} ${r.applied === 1 ? 'item' : 'items'}.`];
  if (r.waiting > 0) {
    lines.push(
      `${r.waiting} more ${r.waiting === 1 ? 'names' : 'name'} channels or titles this library `
        + 'has not loaded yet; they go in by themselves after the next refresh.',
    );
  }
  if (r.skipped > 0) {
    lines.push(`${r.skipped} came from a newer Aurora and were left out.`);
  }
  if (r.providers > 0) {
    lines.push(
      'Passwords are never saved in a backup: enter each provider’s again in '
        + 'Settings → Providers.',
    );
  }
  return lines.join(' ');
}

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function BackupPanel() {
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState<BackupSaved | null>(null);
  const [picked, setPicked] = useState<{ name: string; text: string; summary: BackupSummary } | null>(null);
  const [restored, setRestored] = useState<BackupRestored | null>(null);
  const [error, setError] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  const reset = () => {
    setSaved(null);
    setRestored(null);
    setError(null);
  };

  const save = async () => {
    reset();
    setPicked(null);
    setBusy(true);
    try {
      setSaved(await invoke('backup.save', undefined));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const choose = async (file: File | undefined) => {
    reset();
    setPicked(null);
    if (!file) return;
    setBusy(true);
    try {
      const text = await file.text();
      const summary = await invoke('backup.inspect', { text });
      setPicked({ name: file.name, text, summary });
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
      // So picking the same file again is still a change.
      if (fileInput.current) fileInput.current.value = '';
    }
  };

  const restore = async () => {
    if (!picked) return;
    reset();
    setBusy(true);
    try {
      setRestored(await invoke('backup.restore', { text: picked.text }));
      setPicked(null);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={CARD} aria-label="Backup" role="group">
      <strong>Backup</strong>
      <p style={{ ...MUTED, margin: 0 }}>
        Save your profiles, favourites and lists, what you are watching, channel edits,
        parental controls and settings to a file, to set up another PC or a reinstall the
        same way. Passwords are never saved in it.
      </p>
      <div style={{ display: 'flex', gap: 'var(--sp-2)', flexWrap: 'wrap' }}>
        <Button onClick={() => void save()} disabled={busy}>
          Save a backup
        </Button>
        <Button
          variant="ghost"
          onClick={() => fileInput.current?.click()}
          disabled={busy}
        >
          Restore from a file…
        </Button>
        <input
          ref={fileInput}
          type="file"
          accept=".json,application/json"
          aria-label="Backup file to restore"
          style={{ display: 'none' }}
          onChange={(e) => void choose(e.target.files?.[0])}
        />
      </div>

      {saved && (
        <Message ok>
          <div>
            Saved {describeCounts(saved.summary.counts)} to{' '}
            <span style={{ wordBreak: 'break-all' }}>{saved.path}</span>.
          </div>
          <Button
            size="sm"
            variant="ghost"
            style={{ marginTop: 'var(--sp-2)' }}
            onClick={() => void invoke('backup.openFolder', undefined).catch((e) => setError(errorText(e)))}
          >
            Show in folder
          </Button>
        </Message>
      )}

      {picked && (
        <div style={{ ...CARD, background: 'transparent' }} role="group" aria-label="Backup to restore">
          <div>
            <strong style={{ wordBreak: 'break-all' }}>{picked.name}</strong> holds{' '}
            {describeCounts(picked.summary.counts)}.
          </div>
          <div style={MUTED}>{made(picked.summary)}</div>
          <div style={MUTED}>
            Restoring writes these over what this PC has for the same things, and leaves
            everything else alone. Nothing is deleted.
          </div>
          <div style={{ display: 'flex', gap: 'var(--sp-2)', flexWrap: 'wrap' }}>
            <Button variant="primary" onClick={() => void restore()} disabled={busy}>
              Restore
            </Button>
            <Button variant="ghost" onClick={() => setPicked(null)} disabled={busy}>
              Cancel
            </Button>
          </div>
        </div>
      )}

      {restored && <Message ok>{restoredText(restored)}</Message>}
      {error && <Message ok={false}>{error}</Message>}
    </div>
  );
}
