/**
 * The playlist editor (README §7.3): rename, renumber, regroup, hide, and put back.
 *
 * One screen for all three lists, because the operations are identical whether a row is
 * a channel, a film or a show. It is also the only screen that shows hidden rows —
 * hiding something has to be reversible, and a viewer who cannot find what they hid has
 * lost it.
 *
 * Both axes of a real playlist are hostile: ten thousand rows, and names that repeat.
 * So the list is virtualized, the search and the bulk actions work on the whole match
 * rather than the visible page, and every row says how many other copies of it exist.
 */
import { useVirtualizer } from '@tanstack/react-virtual';
import { useCallback, useMemo, useRef, useState } from 'react';
import type { PlaylistEntry, PlaylistKind, PlaylistShow } from '@shared/ipc';
import { Badge, Button, EmptyState, FIELD, Select, Skeleton } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { invoke } from '@/ipc';
import { notify } from '@/lib/errors';
import { numberPatch } from './number';

const ROW_H = 46;
const PAGE = 300;

const KINDS: { key: PlaylistKind; label: string }[] = [
  { key: 'live', label: 'Live TV' },
  { key: 'movies', label: 'Movies' },
  { key: 'series', label: 'Series' },
];

/** "fr" → "French", without a translation table of our own to keep in step. */
function languageName(code: string | null): string | null {
  if (!code) return null;
  try {
    return new Intl.DisplayNames([navigator.language || 'en'], { type: 'language' }).of(code)
      ?? code.toUpperCase();
  } catch {
    return code.toUpperCase();
  }
}

export function PlaylistPage() {
  const [kind, setKind] = useState<PlaylistKind>('live');
  const [text, setText] = useState('');
  const [group, setGroup] = useState('');
  const [show, setShow] = useState<PlaylistShow>('all');
  const [duplicatesOnly, setDuplicatesOnly] = useState(false);
  const [limit, setLimit] = useState(PAGE);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  /** Whether "Hide all N matching" is waiting for a second press (R-125). */
  const [confirmingBulk, setConfirmingBulk] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const query = useMemo(
    () => ({
      kind,
      text: text.trim() || undefined,
      group: group || undefined,
      show,
      duplicatesOnly,
    }),
    [kind, text, group, show, duplicatesOnly],
  );

  const page = useCommand(
    'playlist.list',
    { ...query, limit, offset: 0 },
    [kind, text, group, show, duplicatesOnly, limit],
  );
  const { data: groups } = useCommand('playlist.groups', { kind }, [kind]);

  const rows = page.data?.rows ?? [];
  const total = page.data?.total ?? 0;

  /** Run one edit, then re-read: the host decides what the row looks like afterwards. */
  const act = useCallback(
    async (fn: () => Promise<unknown>) => {
      setBusy(true);
      setError(null);
      try {
        await fn();
        page.reload();
      } catch (e: unknown) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setBusy(false);
      }
    },
    [page],
  );

  const switchKind = (next: PlaylistKind) => {
    setKind(next);
    setSelected(new Set());
    setGroup('');
    setLimit(PAGE);
  };

  const toggleSelected = (id: number) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  const allOnPageSelected = rows.length > 0 && rows.every((r) => selected.has(r.id));
  const ids = [...selected];

  const scrollRef = useRef<HTMLDivElement>(null);
  const virt = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_H,
    overscan: 12,
  });

  return (
    <div
      style={{
        display: 'flex', flexDirection: 'column',
        // The top bar shares the scroller, so the table gets the rest of the window less
        // the bar -- and its own scroller ends above the nav pill.
        height: 'calc(100% - var(--topbar-h))',
        paddingBottom: 16,
      }}
    >
      <div style={{ padding: '4px 36px 12px' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 20, flexWrap: 'wrap' }}>
          <h1 className="dl-page-title">Playlist</h1>

          <div role="tablist" aria-label="Which list" className="dl-seg">
            {KINDS.map((k) => (
              <button
                key={k.key}
                type="button"
                role="tab"
                aria-selected={kind === k.key}
                data-active={kind === k.key}
                onClick={() => switchKind(k.key)}
              >
                {k.label}
              </button>
            ))}
          </div>
        </div>

        <div
          style={{
            display: 'flex', alignItems: 'center', gap: 'var(--sp-2)', flexWrap: 'wrap',
            marginTop: 'var(--sp-3)',
          }}
        >
          <input className="aurora-field"
            value={text}
            onChange={(e) => { setText(e.target.value); setLimit(PAGE); }}
            placeholder="Search this list"
            aria-label="Search this list"
            style={inputStyle}
          />
          <Select
            label="Group"
            placeholder="All groups"
            options={(groups ?? []).map((g) => ({
              value: g.name, label: g.name, hint: String(g.count),
            }))}
            value={group || undefined}
            onChange={(v) => { setGroup(v ?? ''); setLimit(PAGE); }}
          />
          <Select
            label="Show"
            options={[
              { value: 'all', label: 'Shown and hidden' },
              { value: 'visible', label: 'Shown only' },
              { value: 'hidden', label: 'Hidden only' },
            ]}
            value={show}
            onChange={(v) => { setShow((v ?? 'all') as PlaylistShow); setLimit(PAGE); }}
          />
          <Button
            variant={duplicatesOnly ? 'primary' : 'secondary'}
            aria-pressed={duplicatesOnly}
            onClick={() => { setDuplicatesOnly((d) => !d); setLimit(PAGE); }}
          >
            Duplicates only
          </Button>

          <span style={{ marginLeft: 'auto', color: 'var(--text-muted)', fontSize: 'var(--fs-sm)', fontWeight: 600 }}>
            {total} {total === 1 ? 'entry' : 'entries'}
            {rows.length < total ? ` · showing ${rows.length}` : ''}
          </span>
        </div>

        {/* Bulk actions work on the selection, or on everything the search matched. */}
        <div
          style={{
            display: 'flex', alignItems: 'center', gap: 'var(--sp-2)', flexWrap: 'wrap',
            marginTop: 'var(--sp-3)', minHeight: 32,
          }}
        >
          <label style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 'var(--fs-sm)', fontWeight: 600 }}>
            <input className="aurora-field"
              type="checkbox"
              checked={allOnPageSelected}
              aria-label="Select everything listed"
              onChange={() =>
                setSelected(allOnPageSelected ? new Set() : new Set(rows.map((r) => r.id)))}
            />
            Select all listed
          </label>
          {ids.length > 0 && (
            <>
              <Badge tone="accent">{ids.length} selected</Badge>
              <Button
                size="sm" icon="close" disabled={busy}
                onClick={() => void act(() =>
                  invoke('playlist.setHidden', { kind, ids, hidden: true }))}
              >
                Hide
              </Button>
              <Button
                size="sm" icon="check" disabled={busy}
                onClick={() => void act(() =>
                  invoke('playlist.setHidden', { kind, ids, hidden: false }))}
              >
                Show
              </Button>
              <Button
                size="sm" variant="ghost" disabled={busy}
                onClick={() => void act(() => invoke('playlist.reset', { kind, ids }))}
              >
                Reset
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setSelected(new Set())}>
                Clear selection
              </Button>
            </>
          )}
          {/*
            * Asks first, because its condition is the state the screen is in on arrival.
            *
            * "Nothing selected, and more matches than are on screen" is true the moment
            * the Movies tab opens, so this was a one-click offer to hide all 27,494
            * films before touching anything. It is recoverable -- `show: 'hidden'` lists
            * them and `playlist.reset` takes ids -- but the recovery is per selection in
            * pages of 300, which is ninety-two pages of select-all-and-reset (R-125).
            */}
          {ids.length === 0 && total > rows.length && (
            confirmingBulk ? (
              <>
                <span style={{ fontSize: 'var(--fs-sm)', color: 'var(--danger)' }}>
                  Hide all {total}? Undoing this is {Math.ceil(total / 300)} pages of
                  {' '}select-and-reset.
                </span>
                <Button
                  size="sm"
                  variant="danger"
                  disabled={busy}
                  data-testid="playlist-hide-all-confirm"
                  onClick={() => {
                    setConfirmingBulk(false);
                    void act(() => invoke('playlist.hideMatching', { ...query, hidden: true }));
                  }}
                >
                  Hide them
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setConfirmingBulk(false)}>
                  Cancel
                </Button>
              </>
            ) : (
            <Button
              size="sm"
              variant="secondary"
              disabled={busy}
              onClick={() => setConfirmingBulk(true)}
            >
              Hide all {total} matching
            </Button>
            )
          )}
        </div>

        {error && (
          <div role="alert" style={{ color: 'var(--danger)', fontSize: 'var(--fs-sm)', marginTop: 8 }}>
            {error}
          </div>
        )}
      </div>

      {/* Daylight: the table is one white panel. */}
      <div
        className="dl-panel"
        style={{
          flex: 1, minHeight: 0, margin: '0 36px', display: 'flex', flexDirection: 'column',
          overflow: 'hidden', padding: '6px 0',
        }}
      >
      <Header kind={kind} />

      <div ref={scrollRef} style={{ flex: 1, overflowY: 'auto', padding: '0 16px' }}>
        {page.loading && !page.data ? (
          <div style={{ display: 'grid', gap: 6, paddingTop: 8 }}>
            {Array.from({ length: 12 }, (_, i) => <Skeleton key={i} h={ROW_H - 8} r={12} />)}
          </div>
        ) : rows.length === 0 ? (
          <EmptyState
            title="Nothing here"
            body={
              text || group || duplicatesOnly
                ? 'No entry in this list matches those filters.'
                : 'This list is empty until a provider has been imported.'
            }
          />
        ) : (
          <div style={{ height: virt.getTotalSize(), position: 'relative' }}>
            {virt.getVirtualItems().map((v) => {
              const row = rows[v.index]!;
              return (
                <div
                  key={row.id}
                  style={{
                    position: 'absolute', top: v.start, left: 0, right: 0, height: ROW_H,
                  }}
                >
                  <Row
                    row={row}
                    kind={kind}
                    selected={selected.has(row.id)}
                    onSelect={() => toggleSelected(row.id)}
                    onPatch={(patch) => void act(() =>
                      invoke('playlist.update', { kind, id: row.id, patch }))}
                    onReset={() => void act(() =>
                      invoke('playlist.reset', { kind, ids: [row.id] }))}
                  />
                </div>
              );
            })}
          </div>
        )}

        {rows.length < total && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: 'var(--sp-4)' }}>
            <Button size="sm" onClick={() => setLimit((l) => l + PAGE)}>
              Show more
            </Button>
          </div>
        )}
      </div>
      </div>
    </div>
  );
}

function Header({ kind }: { kind: PlaylistKind }) {
  return (
    <div
      style={{
        display: 'flex', alignItems: 'center', gap: 'var(--sp-3)',
        padding: '10px 16px', fontSize: 'var(--fs-sm)',
        color: 'var(--text-muted)', fontWeight: 700,
      }}
    >
      <span style={{ width: 20 }} />
      {kind === 'live' && <span style={{ width: 64 }}>No.</span>}
      <span style={{ flex: 1 }}>Name</span>
      <span style={{ width: 160 }}>Group</span>
      {kind === 'live' && <span style={{ width: 200 }}>Logo</span>}
      <span style={{ width: 120 }}>Quality</span>
      <span style={{ width: 110 }}>Language</span>
      <span style={{ width: 150, textAlign: 'right' }}>Shown</span>
    </div>
  );
}

function Row({
  row, kind, selected, onSelect, onPatch, onReset,
}: {
  row: PlaylistEntry;
  kind: PlaylistKind;
  selected: boolean;
  onSelect: () => void;
  onPatch: (
    patch: { name?: string; number?: number; group?: string; logo?: string; hidden?: boolean },
  ) => void;
  onReset: () => void;
}) {
  const language = languageName(row.lang);
  return (
    <div
      style={{
        display: 'flex', alignItems: 'center', gap: 'var(--sp-3)', height: ROW_H,
        borderBottom: '1px solid var(--border)', opacity: row.hidden ? 0.5 : 1,
      }}
    >
      <input className="aurora-field"
        type="checkbox"
        checked={selected}
        onChange={onSelect}
        aria-label={`Select ${row.name}`}
        style={{ width: 20 }}
      />

      {kind === 'live' && (
        <EditableCell
          value={row.number == null ? '' : String(row.number)}
          label={`Number for ${row.name}`}
          width={64}
          weight={400}
          onCommit={(next) => {
            const patch = numberPatch(next);
            if (patch === undefined) {
              notify(
                `“${next.trim()}” is not a channel number`,
                'Enter a whole number above zero, or clear the field to use the '
                + "provider's own number.",
              );
              return;
            }
            onPatch(patch);
          }}
        />
      )}

      <div style={{ flex: 1, minWidth: 0 }}>
        <EditableCell
          value={row.name}
          label={`Name for ${row.name}`}
          onCommit={(next) => onPatch({ name: next })}
        />
        {row.edited && row.providerName !== row.name && (
          <div
            style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-faint)' }}
            title="What the provider calls it"
          >
            {row.providerName}
          </div>
        )}
      </div>

      <div style={{ width: 160, minWidth: 0 }}>
        {kind === 'live' ? (
          <EditableCell
            value={row.group ?? ''}
            label={`Group for ${row.name}`}
            weight={400}
            onCommit={(next) => onPatch({ group: next })}
          />
        ) : (
          <span style={{ color: 'var(--text-muted)', fontSize: 'var(--fs-sm)' }}>
            {row.group ?? '—'}
          </span>
        )}
      </div>

      {/* The logo, which the provider often gets wrong and nothing could fix.
          `custom_logo` has been read by the channel grid and cleared by Reset since the
          column existed; it simply had no writer, so a broken logo was permanent. */}
      {kind === 'live' && (
        <div style={{ width: 200, minWidth: 0, display: 'flex', gap: 6, alignItems: 'center' }}>
          {row.logo ? (
            <img
              src={row.logo}
              alt=""
              width={28}
              height={28}
              style={{ objectFit: 'contain', borderRadius: 4, flex: '0 0 auto' }}
              // A broken URL is the ordinary case here -- it is why somebody is
              // overriding it -- so the slot collapses rather than showing a torn page.
              onError={(e) => { e.currentTarget.style.visibility = 'hidden'; }}
            />
          ) : (
            <span style={{ width: 28, flex: '0 0 auto' }} />
          )}
          <EditableCell
            value={row.logo ?? ''}
            label={`Logo for ${row.name}`}
            weight={400}
            onCommit={(next) => onPatch({ logo: next })}
          />
        </div>
      )}

      <div style={{ width: 120, display: 'flex', gap: 6, alignItems: 'center' }}>
        {row.quality && <Badge tone="neutral">{row.quality}</Badge>}
        {row.duplicates > 0 && (
          <span title={`${row.duplicates} other copies of this entry`}>
            <Badge tone="outline">+{row.duplicates}</Badge>
          </span>
        )}
      </div>

      <div style={{ width: 110, color: 'var(--text-muted)', fontSize: 'var(--fs-sm)' }}>
        {language ?? <span style={{ color: 'var(--text-faint)' }}>Untagged</span>}
      </div>

      <div style={{ width: 150, display: 'flex', gap: 6, justifyContent: 'flex-end' }}>
        {row.edited && (
          <Button size="sm" variant="ghost" onClick={onReset} title="Back to the provider's values">
            Reset
          </Button>
        )}
        {/*
          * Quiet when shown, solid when hidden — the opposite way round from how this
          * started. Almost every row in a playlist is shown, so filling that state made
          * the column a wall of accent and left the handful of hidden rows, which are the
          * only ones anyone is scanning for, as the faintest things on the screen. The
          * row already dims when it is hidden; this stops the switch arguing with it.
          */}
        {/* Daylight's switch, blue when the entry is shown. The row dims when it is
            hidden, which is what makes the handful of hidden rows easy to scan for. */}
        <button
          type="button"
          className="dl-switch"
          role="switch"
          aria-checked={!row.hidden}
          aria-label={`Show ${row.name}`}
          title={row.hidden ? 'Hidden' : 'Shown'}
          onClick={() => onPatch({ hidden: !row.hidden })}
        />
      </div>
    </div>
  );
}

/**
 * A cell that becomes an input when you click it. Enter commits, Escape abandons, and
 * clicking away commits — the same bargain every spreadsheet makes.
 */
function EditableCell({
  value, label, width, weight = 600, onCommit,
}: {
  value: string;
  label: string;
  width?: number;
  weight?: number;
  onCommit: (next: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);

  if (!editing) {
    return (
      <button
        onClick={() => { setDraft(value); setEditing(true); }}
        aria-label={label}
        style={{
          width: width ?? '100%', textAlign: 'left', background: 'transparent',
          border: '1px solid transparent', borderRadius: 'var(--r-sm)', cursor: 'text',
          fontSize: 'var(--fs-md)',
          color: 'inherit', padding: '3px 6px', font: 'inherit', fontWeight: weight,
          overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
        }}
      >
        {value || <span style={{ color: 'var(--text-faint)', fontWeight: 400 }}>—</span>}
      </button>
    );
  }

  const commit = () => {
    setEditing(false);
    if (draft !== value) onCommit(draft);
  };

  return (
    <input className="aurora-field"
      autoFocus
      value={draft}
      aria-label={label}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') commit();
        if (e.key === 'Escape') setEditing(false);
      }}
      style={{
        width: width ?? '100%', padding: '3px 6px', font: 'inherit',
        background: 'var(--surface)', color: 'var(--text)',
        border: '1px solid var(--accent)', borderRadius: 'var(--r-sm)',
      }}
    />
  );
}

/** One field, defined once. See `FIELD` in Primitives. */
const inputStyle: React.CSSProperties = { ...FIELD, minWidth: 240, background: 'var(--surface)' };
