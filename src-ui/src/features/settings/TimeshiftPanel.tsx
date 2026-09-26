/**
 * The timeshift buffer (README §7.6, §15).
 *
 * Two caps, because both are true: bytes are what the disk spends and minutes are what
 * a person means, and whichever runs out first is what the viewer can actually rewind.
 * The panel says so rather than pretending one number describes the buffer.
 */
import { useState } from 'react';
import { Badge, Button, TextField } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { invoke } from '@/ipc';
import { bytes } from '@/lib/format';

const GIB = 1024 * 1024 * 1024;
/** README §7.6: 1 GB by default, up to 10. */
const SIZES = [1, 2, 5, 10];
const MINUTES = [15, 30, 60, 180];

/** "30 min", "3 hr" — `duration` would render half an hour as `30:00`, which reads as
 *  thirty seconds as easily as thirty minutes. */
function limitLabel(secs: number): string {
  const mins = Math.round(secs / 60);
  return mins % 60 === 0 && mins >= 60 ? `${mins / 60} hr` : `${mins} min`;
}

export function TimeshiftPanel() {
  const [nonce, setNonce] = useState(0);
  const { data: settings, error } = useCommand('timeshift.settings', undefined, [nonce]);
  const [folder, setFolder] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const apply = async (patch: Parameters<typeof invoke<'timeshift.setSettings'>>[1]) => {
    await invoke('timeshift.setSettings', patch);
    setMessage('Saved. Applies to the next channel you tune.');
    setNonce((n) => n + 1);
  };

  if (error) {
    return <div style={{ color: 'var(--danger)', fontSize: 'var(--fs-sm)' }}>{error}</div>;
  }
  if (!settings) return <div style={{ color: 'var(--text-faint)' }}>Loading…</div>;

  return (
    <div style={{ display: 'grid', gap: 'var(--sp-4)' }}>
      <Row
        label="Pause live TV"
        hint="Keeps the channel you are watching on disk so it can be paused and rewound."
      >
        <Button
          size="sm"
          variant={settings.enabled ? 'primary' : 'secondary'}
          aria-label={settings.enabled ? 'Turn pause live TV off' : 'Turn pause live TV on'}
          onClick={() => void apply({ enabled: !settings.enabled })}
        >
          {settings.enabled ? 'On' : 'Off'}
        </Button>
      </Row>

      <Row label="Buffer size" hint="The disk it may use.">
        <div style={{ display: 'flex', gap: 6 }}>
          {SIZES.map((gb) => (
            <Button
              key={gb} size="sm"
              variant={settings.bytes === gb * GIB ? 'primary' : 'secondary'}
              disabled={!settings.enabled}
              onClick={() => void apply({ bytes: gb * GIB })}
            >
              {gb} GB
            </Button>
          ))}
        </div>
      </Row>

      <Row
        label="Rewind limit"
        hint="How far back to offer. A gigabyte is eighteen minutes of HD and seven of 4K, so the shorter of the two is what you get."
      >
        <div style={{ display: 'flex', gap: 6 }}>
          {MINUTES.map((m) => (
            <Button
              key={m} size="sm"
              variant={settings.secs === m * 60 ? 'primary' : 'secondary'}
              disabled={!settings.enabled}
              onClick={() => void apply({ secs: m * 60 })}
            >
              {limitLabel(m * 60)}
            </Button>
          ))}
        </div>
      </Row>

      <Row label="Folder" hint="Somewhere fast, and not the disk you are recording to.">
        <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
          <TextField
            value={folder ?? settings.folder}
            onChange={(e) => setFolder(e.target.value)}
            aria-label="Timeshift folder"
            spellCheck={false}
            style={{ width: 300 }}
          />
          <Button
            size="sm"
            disabled={folder === null || folder === settings.folder}
            onClick={() => {
              void apply({ folder: folder ?? '' });
              setFolder(null);
            }}
          >
            Apply
          </Button>
          <Button
            size="sm" variant="secondary"
            onClick={() => {
              setFolder(null);
              void apply({ folder: '' });
            }}
          >
            Default
          </Button>
        </div>
      </Row>

      <Row label="On disk now" hint="Emptying it loses the rewind, not the channel.">
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <Badge tone="outline">{bytes(settings.bytesOnDisk)}</Badge>
          <Button
            size="sm"
            disabled={settings.bytesOnDisk === 0}
            onClick={async () => {
              const freed = await invoke('timeshift.clear');
              setMessage(freed > 0 ? `Freed ${bytes(freed)}.` : 'Nothing to empty.');
              setNonce((n) => n + 1);
            }}
          >
            Empty buffer
          </Button>
        </div>
      </Row>

      <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)' }}>
        {settings.enabled
          ? `Up to ${limitLabel(settings.secs)} of rewind, within ${bytes(settings.bytes)}.`
          : 'Pausing live TV will freeze the picture and rejoin at the live edge.'}
        {message && <span style={{ color: 'var(--success)' }}> {message}</span>}
      </div>
    </div>
  );
}

function Row({
  label, hint, children,
}: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--sp-4)' }}>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontWeight: 600 }}>{label}</div>
        {hint && (
          <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)' }}>{hint}</div>
        )}
      </div>
      {children}
    </div>
  );
}
