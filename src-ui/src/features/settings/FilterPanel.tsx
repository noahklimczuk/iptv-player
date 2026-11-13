/**
 * The two library-wide filters (README §7.3).
 *
 * Each says what it would hide before it is turned on, because on a ten-thousand-entry
 * playlist "English only" is either a relief or a catastrophe and the difference is not
 * guessable. The untagged count is shown next to the non-English one for the same
 * reason: it is the number that explains why the filter is not more aggressive.
 */
import { useCallback, useState } from 'react';
import type { LibraryFilters, PlaylistKind } from '@shared/ipc';
import { Badge } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { invoke } from '@/ipc';

const KINDS: { key: PlaylistKind; label: string }[] = [
  { key: 'live', label: 'Channels' },
  { key: 'movies', label: 'Movies' },
  { key: 'series', label: 'Series' },
];

export function FilterPanel() {
  const filters = useCommand('library.filters', undefined, []);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [nonce, setNonce] = useState(0);

  const set = useCallback(
    async (next: LibraryFilters) => {
      setPending(true);
      setError(null);
      try {
        await invoke('library.setFilters', next);
        filters.reload();
        // The counts are of what is left, so they move when a filter does.
        setNonce((n) => n + 1);
      } catch (e: unknown) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setPending(false);
      }
    },
    [filters],
  );

  const current = filters.data ?? {
    englishOnly: false,
    hideDuplicates: false,
    hideUntagged: false,
  };

  return (
    <div>
      <Toggle
        label="Show English content only"
        hint="Hides channels and titles the provider tagged as another language. Anything untagged is kept — most playlists label only some of what they carry."
        checked={current.englishOnly}
        disabled={pending}
        onChange={(englishOnly) => void set({ ...current, englishOnly })}
      />
      <Toggle
        label="Also hide anything untagged"
        hint="Most playlists label only some of what they carry, so the filter above keeps whatever the provider did not tag. Turn this on to hide that too — the counts below say how much it is. On a playlist that tags nothing, it is everything."
        checked={current.englishOnly && current.hideUntagged}
        disabled={pending || !current.englishOnly}
        onChange={(hideUntagged) => void set({ ...current, hideUntagged })}
      />
      <Toggle
        label="Collapse duplicates"
        hint="Shows one entry per title, keeping the highest quality copy. The others stay available as alternate sources."
        checked={current.hideDuplicates}
        disabled={pending}
        onChange={(hideDuplicates) => void set({ ...current, hideDuplicates })}
      />

      <div style={{ display: 'grid', gap: 6, marginTop: 'var(--sp-3)' }}>
        {KINDS.map((k) => (
          <Counts key={k.key} kind={k.key} label={k.label} nonce={nonce} />
        ))}
      </div>

      {error && (
        <div role="alert" style={{ color: 'var(--danger)', fontSize: 'var(--fs-sm)', marginTop: 8 }}>
          {error}
        </div>
      )}
    </div>
  );
}

function Counts({ kind, label, nonce }: { kind: PlaylistKind; label: string; nonce: number }) {
  const { data } = useCommand('library.filterCounts', { kind }, [kind, nonce]);
  if (!data) return null;
  return (
    <div
      style={{
        display: 'flex', alignItems: 'center', gap: 'var(--sp-2)',
        fontSize: 'var(--fs-sm)', color: 'var(--text-muted)',
      }}
    >
      <span style={{ width: 90, color: 'var(--text-faint)' }}>{label}</span>
      <span>{data.total} shown</span>
      <Badge tone="outline">{data.nonEnglish} not English</Badge>
      <Badge tone="outline">{data.untagged} untagged</Badge>
      <Badge tone="outline">{data.duplicates} duplicates</Badge>
    </div>
  );
}

function Toggle({
  label, hint, checked, disabled, onChange,
}: {
  label: string;
  hint: string;
  checked: boolean;
  disabled: boolean;
  onChange: (next: boolean) => void;
}) {
  return (
    <div style={{ display: 'flex', alignItems: 'flex-start', gap: 'var(--sp-4)', padding: 'var(--sp-2) 0' }}>
      <div style={{ flex: 1 }}>
        <div style={{ fontWeight: 800, fontSize: 'var(--fs-lg)' }}>{label}</div>
        <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)', maxWidth: 560 }}>
          {hint}
        </div>
      </div>
      <button
        role="switch"
        aria-checked={checked}
        aria-label={label}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        type="button"
        className="dl-switch"
        title={checked ? 'On' : 'Off'}
        style={{ cursor: disabled ? 'progress' : 'pointer', marginTop: 4 }}
      />
    </div>
  );
}
