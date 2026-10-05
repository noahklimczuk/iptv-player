/**
 * Multi-view — choosing what to open (README §7.4).
 *
 * The arrangement, a channel per tile, saved layouts, and the one thing this screen
 * exists to say before anything is opened: whether the provider's line can carry it.
 *
 * **The warning is not a footnote.** A 3×3 is nine simultaneous streams and the
 * subscription this project was measured against allows one — so on that line no layout
 * here will ever work, and a picker that let you choose one and then failed four times
 * would be telling you the same thing four times more slowly. The limit is shown per
 * layout, the ones that cannot open say so on the button, and where no provider declares
 * a limit at all — every M3U playlist, so the common case — it says that instead of
 * pretending to know.
 */
import { useCallback, useEffect, useMemo, useState } from 'react';
import type { Channel, MosaicCheck, MosaicLayout, SavedMosaicLayout } from '@shared/ipc';
import { MOSAIC_TILES } from '@shared/ipc';
import { Button, EmptyState, Select, TextField } from '@/components/Primitives';
import { Icon } from '@/components/Icon';
import { useCommand } from '@/hooks/useCommand';
import { invoke } from '@/ipc';
import { notify } from '@/lib/errors';

const LAYOUTS: { id: MosaicLayout; label: string; hint: string }[] = [
  { id: 'grid2x2', label: '2 × 2', hint: 'Four equal tiles' },
  { id: 'onePlusThree', label: '1 + 3', hint: 'One large, three beside it' },
  { id: 'onePlusFive', label: '1 + 5', hint: 'One large, five around it' },
  { id: 'grid3x3', label: '3 × 3', hint: 'Nine equal tiles' },
];

/** A miniature of the arrangement, built from the same cell logic as the host. */
function LayoutThumb({ layout }: { layout: MosaicLayout }) {
  const cells: { gridArea: string }[] =
    layout === 'grid2x2'
      ? [1, 2, 3, 4].map((_, i) => ({
          gridArea: `${Math.floor(i / 2) + 1} / ${(i % 2) + 1} / span 1 / span 1`,
        }))
      : layout === 'grid3x3'
        ? Array.from({ length: 9 }, (_, i) => ({
            gridArea: `${Math.floor(i / 3) + 1} / ${(i % 3) + 1} / span 1 / span 1`,
          }))
        : layout === 'onePlusThree'
          ? [
              { gridArea: '1 / 1 / span 3 / span 3' },
              { gridArea: '1 / 4 / span 1 / span 1' },
              { gridArea: '2 / 4 / span 1 / span 1' },
              { gridArea: '3 / 4 / span 1 / span 1' },
            ]
          : [
              { gridArea: '1 / 1 / span 2 / span 2' },
              { gridArea: '1 / 3 / span 1 / span 1' },
              { gridArea: '2 / 3 / span 1 / span 1' },
              { gridArea: '3 / 3 / span 1 / span 1' },
              { gridArea: '3 / 1 / span 1 / span 1' },
              { gridArea: '3 / 2 / span 1 / span 1' },
            ];

  const cols = layout === 'onePlusThree' ? 4 : layout === 'grid2x2' ? 2 : 3;
  const rows = layout === 'grid2x2' ? 2 : 3;

  return (
    <div
      aria-hidden
      style={{
        display: 'grid',
        gridTemplateColumns: `repeat(${cols}, 1fr)`,
        gridTemplateRows: `repeat(${rows}, 1fr)`,
        gap: 2,
        width: 64,
        height: 44,
      }}
    >
      {cells.map((cell, i) => (
        <div
          key={i}
          style={{
            ...cell,
            background: 'color-mix(in srgb, var(--text) 24%, transparent)',
            borderRadius: 2,
          }}
        />
      ))}
    </div>
  );
}

/** What the host said about this layout, in a sentence. */
function budgetLine(check: MosaicCheck | null): { text: string; blocked: boolean } {
  if (!check) return { text: '', blocked: false };
  if (check.verdict === 'exceeds') {
    const recordings =
      check.recordings > 0
        ? ` (${check.recordings} recording${check.recordings === 1 ? '' : 's'} using the line)`
        : '';
    return {
      text: `Needs ${check.needed}, your provider allows ${check.limit}${recordings}`,
      blocked: true,
    };
  }
  if (check.verdict === 'unknown') {
    return {
      text: `Needs ${check.needed} streams — your provider has not said how many it allows`,
      blocked: false,
    };
  }
  return { text: `${check.needed} of ${check.limit} connections`, blocked: false };
}

export function MosaicPage({
  onOpened,
  /**
   * Bumped by the app when a layout is saved. The save happens in the overlay, over
   * this screen, so without it the list underneath would still be the one from before
   * — and the obvious reading of that is that saving did not work.
   */
  layoutsVersion = 0,
}: {
  onOpened: () => void;
  layoutsVersion?: number;
}) {
  const [layout, setLayout] = useState<MosaicLayout>('grid2x2');
  const [picked, setPicked] = useState<(number | null)[]>([]);
  const [opening, setOpening] = useState(false);

  const { data: channels, loading } = useCommand('channels.list', {}, []);
  const { data: check } = useCommand('mosaic.check', { layout }, [layout]);
  const [layoutNonce, setLayoutNonce] = useState(0);
  const { data: saved } = useCommand('mosaic.layouts', undefined, [layoutNonce, layoutsVersion]);

  const tiles = MOSAIC_TILES[layout];

  // Choosing a smaller arrangement drops the tiles that no longer exist rather than
  // keeping them invisibly — otherwise going 3×3 → 2×2 → 3×3 silently reopens channels
  // the viewer thought they had removed.
  useEffect(() => {
    setPicked((prev) => prev.slice(0, tiles));
  }, [tiles]);

  const filled = useMemo(() => picked.filter((id) => id != null).length, [picked]);
  const line = budgetLine(check);

  // What *this* selection costs, which is not what the layout costs: an empty tile
  // opens no connection, so a 3×3 with two channels is two streams.
  const selectionBlocked =
    check?.verdict === 'exceeds' && filled + check.recordings > check.limit;

  // Thousands of channels in one list is why `Select` grows a search box past a dozen
  // options; built once per channel list rather than per tile.
  const channelOptions = useMemo(
    () =>
      (channels ?? []).map((c: Channel) => ({
        value: String(c.id),
        label: c.name,
        hint: c.number ? String(c.number) : undefined,
      })),
    [channels],
  );

  const setTile = (index: number, value: string | undefined) => {
    setPicked((prev) => {
      const next = [...prev];
      while (next.length < tiles) next.push(null);
      next[index] = value === undefined ? null : Number(value);
      return next;
    });
  };

  const open = useCallback(async () => {
    setOpening(true);
    try {
      await invoke('mosaic.open', { layout, channelIds: picked });
      onOpened();
    } catch (e) {
      notify('Could not open multi-view', e);
    } finally {
      setOpening(false);
    }
  }, [layout, picked, onOpened]);

  const openSaved = useCallback(
    async (row: SavedMosaicLayout) => {
      try {
        await invoke('mosaic.openSaved', { id: row.id });
        onOpened();
      } catch (e) {
        notify(`Could not open ${row.name}`, e);
      }
    },
    [onOpened],
  );

  const deleteSaved = useCallback(async (row: SavedMosaicLayout) => {
    try {
      await invoke('mosaic.deleteLayout', { id: row.id });
      setLayoutNonce((n) => n + 1);
    } catch (e) {
      notify(`Could not delete ${row.name}`, e);
    }
  }, []);

  if (!loading && (!channels || channels.length === 0)) {
    return (
      <EmptyState
        icon="layers"
        title="No channels to show"
        body="Multi-view needs live channels. Add a provider in Settings and refresh your library."
      />
    );
  }

  return (
    <div style={{ padding: 'var(--sp-6)', display: 'grid', gap: 'var(--sp-6)' }}>
      <header style={{ display: 'grid', gap: 'var(--sp-2)' }}>
        <h1 style={{ margin: 0, fontSize: 24 }}>Multi-view</h1>
        <p style={{ margin: 0, color: 'var(--text-muted)', maxWidth: '60ch' }}>
          Watch several channels at once. One tile has sound — press <kbd>1</kbd>–
          <kbd>9</kbd> or click a tile to move it, and double-click to open that channel
          on its own.
        </p>
      </header>

      <section style={{ display: 'grid', gap: 'var(--sp-3)' }}>
        <h2 style={{ margin: 0, fontSize: 14, color: 'var(--text-muted)' }}>Arrangement</h2>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 'var(--sp-3)' }}>
          {LAYOUTS.map((l) => (
            <button
              key={l.id}
              type="button"
              data-testid={`mosaic-layout-${l.id}`}
              aria-pressed={layout === l.id}
              onClick={() => setLayout(l.id)}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 'var(--sp-3)',
                padding: 'var(--sp-3)',
                borderRadius: 'var(--r-md)',
                background:
                  layout === l.id
                    ? 'color-mix(in srgb, var(--accent) 18%, transparent)'
                    : 'color-mix(in srgb, var(--surface) 78%, transparent)',
                border: `1px solid ${layout === l.id ? 'var(--accent)' : 'var(--border)'}`,
                color: 'var(--text)',
                cursor: 'pointer',
                textAlign: 'left',
              }}
            >
              <LayoutThumb layout={l.id} />
              <span style={{ display: 'grid', gap: 2 }}>
                <span style={{ fontWeight: 600 }}>{l.label}</span>
                <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>{l.hint}</span>
                <span style={{ fontSize: 12, color: 'var(--text-faint)' }}>
                  {MOSAIC_TILES[l.id]} tiles
                </span>
              </span>
            </button>
          ))}
        </div>

        {check && (
          <div
            data-testid="mosaic-budget"
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 'var(--sp-2)',
              fontSize: 13,
              color: line.blocked ? 'var(--danger)' : 'var(--text-muted)',
            }}
          >
            <Icon name={line.blocked ? 'alert' : 'info'} size={16} />
            <span>{line.text}</span>
            {line.blocked && (
              <span>
                {check.largestFitting
                  ? `— the largest that fits is ${
                      LAYOUTS.find((l) => l.id === check.largestFitting)?.label ??
                      check.largestFitting
                    }.`
                  : '— no arrangement fits on this line, so multi-view cannot be used with this provider.'}
              </span>
            )}
          </div>
        )}
      </section>

      <section style={{ display: 'grid', gap: 'var(--sp-3)' }}>
        <h2 style={{ margin: 0, fontSize: 14, color: 'var(--text-muted)' }}>
          Channels — leave a tile empty to open fewer streams
        </h2>
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))',
            gap: 'var(--sp-3)',
          }}
        >
          {Array.from({ length: tiles }, (_, index) => (
            <div key={index} data-testid={`mosaic-pick-${index}`}>
              <Select
                label={`Tile ${index + 1}${index === 0 ? ' — starts with sound' : ''}`}
                placeholder="Empty"
                options={channelOptions}
                value={picked[index] == null ? undefined : String(picked[index])}
                onChange={(v) => setTile(index, v)}
                width={240}
              />
            </div>
          ))}
        </div>
      </section>

      <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--sp-3)' }}>
        <Button
          variant="primary"
          icon="layers"
          data-testid="mosaic-open"
          disabled={filled === 0 || selectionBlocked || opening}
          onClick={open}
        >
          {opening ? 'Opening…' : `Open ${filled} channel${filled === 1 ? '' : 's'}`}
        </Button>
        {filled === 0 && (
          <span style={{ fontSize: 13, color: 'var(--text-muted)' }}>
            Pick at least one channel.
          </span>
        )}
        {selectionBlocked && check?.verdict === 'exceeds' && (
          <span style={{ fontSize: 13, color: 'var(--danger)' }}>
            {filled + check.recordings} streams is more than your provider allows.
          </span>
        )}
      </div>

      {saved && saved.length > 0 && (
        <section style={{ display: 'grid', gap: 'var(--sp-3)' }}>
          <h2 style={{ margin: 0, fontSize: 14, color: 'var(--text-muted)' }}>Saved layouts</h2>
          <ul style={{ listStyle: 'none', margin: 0, padding: 0, display: 'grid', gap: 'var(--sp-2)' }}>
            {saved.map((row) => (
              <li
                key={row.id}
                data-testid={`mosaic-saved-${row.id}`}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 'var(--sp-3)',
                  padding: 'var(--sp-3)',
                  borderRadius: 'var(--r-md)',
                  background: 'color-mix(in srgb, var(--surface) 78%, transparent)',
                  border: '1px solid var(--border)',
                }}
              >
                <LayoutThumb layout={row.layout} />
                <span style={{ flex: 1, display: 'grid', gap: 2 }}>
                  <span style={{ fontWeight: 600 }}>{row.name}</span>
                  <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>
                    {row.channels.filter((c) => c != null).length} channels
                  </span>
                </span>
                <Button icon="play" onClick={() => openSaved(row)}>
                  Open
                </Button>
                <Button variant="ghost" icon="close" onClick={() => deleteSaved(row)} aria-label={`Delete ${row.name}`} />
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

/** The name prompt for "Save layout", kept out of the overlay so it can be tested. */
export function SaveLayoutDialog({
  open,
  onSave,
  onCancel,
}: {
  open: boolean;
  onSave: (name: string) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState('');
  if (!open) return null;
  return (
    <div
      role="dialog"
      aria-label="Save this layout"
      data-testid="mosaic-save-dialog"
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 200,
        display: 'grid',
        placeItems: 'center',
        background: 'color-mix(in srgb, var(--bg) 70%, transparent)',
      }}
    >
      <div
        style={{
          display: 'grid',
          gap: 'var(--sp-4)',
          padding: 'var(--sp-5)',
          minWidth: 320,
          borderRadius: 'var(--r-lg)',
          background: 'var(--bg-elevated)',
          border: '1px solid var(--border)',
        }}
      >
        <h2 style={{ margin: 0, fontSize: 18 }}>Save this layout</h2>
        <label htmlFor="mosaic-layout-name" style={{ fontSize: 12, color: 'var(--text-muted)' }}>
          Name
        </label>
        <TextField
          id="mosaic-layout-name"
          value={name}
          autoFocus
          placeholder="Sunday football"
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && name.trim()) onSave(name.trim());
          }}
        />
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 'var(--sp-2)' }}>
          <Button variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={!name.trim()}
            data-testid="mosaic-save-confirm"
            onClick={() => onSave(name.trim())}
          >
            Save
          </Button>
        </div>
      </div>
    </div>
  );
}
