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
import { Icon } from '@/components/Icon';
import { useCommand } from '@/hooks/useCommand';
import { invoke } from '@/ipc';

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
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      <div style={{ padding: 'var(--sp-5) var(--sp-6) var(--sp-3)' }}>
        <h1 style={{ margin: '0 0 var(--sp-4)', fontSize: 'var(--fs-2xl)', fontWeight: 800 }}>
          Playlist
        </h1>

        <div role="tablist" aria-label="Which list" style={{ display: 'flex', gap: 6 }}>
          {KINDS.map((k) => (
            <Button
              key={k.key}
              role="tab"
              aria-selected={kind === k.key}
              size="sm"
              variant={kind === k.key ? 'primary' : 'secondary'}
              onClick={() => switchKind(k.key)}
            >
              {k.label}
            </Button>
          ))}
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
            size="sm"
            variant={duplicatesOnly ? 'primary' : 'secondary'}
            aria-pressed={duplicatesOnly}
            onClick={() => { setDuplicatesOnly((d) => !d); setLimit(PAGE); }}
          >
            Duplicates only
          </Button>

          <span style={{ marginLeft: 'auto', color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>
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
          <label style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 'var(--fs-sm)' }}>
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
          {ids.length === 0 && total > rows.length && (
            <Button
              size="sm"
              variant="secondary"
              disabled={busy}
              onClick={() => void act(() =>
                invoke('playlist.hideMatching', { ...query, hidden: true }))}
            >
              Hide all {total} matching
            </Button>
          )}
        </div>

        {error && (
          <div role="alert" style={{ color: 'var(--danger)', fontSize: 'var(--fs-sm)', marginTop: 8 }}>
            {error}
          </div>
        )}
      </div>

      <Header kind={kind} />

      <div ref={scrollRef} style={{ flex: 1, overflowY: 'auto', padding: '0 var(--sp-6)' }}>
        {page.loading && !page.data ? (
          <div style={{ display: 'grid', gap: 6, paddingTop: 8 }}>
            {Array.from({ length: 12 }, (_, i) => <Skeleton key={i} h={ROW_H - 8} />)}
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
  );
}

function Header({ kind }: { kind: PlaylistKind }) {
  return (
    <div
      style={{
        display: 'flex', alignItems: 'center', gap: 'var(--sp-3)',
        padding: '6px var(--sp-6)', borderBottom: '1px solid var(--border)',
        background: 'var(--bg-elevated)', fontSize: 'var(--fs-xs)',
        letterSpacing: '0.06em', textTransform: 'uppercase', color: 'var(--text-faint)',
        fontWeight: 700,
      }}
    >
      <span style={{ width: 20 }} />
      {kind === 'live' && <span style={{ width: 64 }}>No.</span>}
      <span style={{ flex: 1 }}>Name</span>
      <span style={{ width: 160 }}>Group</span>
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
  onPatch: (patch: { name?: string; number?: number; group?: string; hidden?: boolean }) => void;
  onReset: () => void;
}) {
  const language = languageName(row.lang);
  return (
    <div
      style={{
        display: 'flex', alignItems: 'center', gap: 'var(--sp-3)', height: ROW_H,
        borderBottom: '1px solid var(--border)', opacity: row.hidden ? 0.45 : 1,
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
          onCommit={(next) => onPatch({ number: Number(next) || 0 })}
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
        <button
          role="switch"
          aria-checked={!row.hidden}
          aria-label={`Show ${row.name}`}
          onClick={() => onPatch({ hidden: !row.hidden })}
          style={{
            display: 'inline-flex', alignItems: 'center', gap: 6, padding: '4px 10px',
            borderRadius: 'var(--r-full)', cursor: 'pointer', fontSize: 'var(--fs-sm)',
            border: `1px solid ${row.hidden ? 'var(--border-strong)' : 'transparent'}`,
            background: row.hidden ? 'transparent' : 'var(--accent)',
            color: row.hidden ? 'var(--text-muted)' : 'var(--accent-text)',
            fontWeight: 600,
          }}
        >
          <Icon name={row.hidden ? 'close' : 'check'} size={13} />
          {row.hidden ? 'Hidden' : 'Shown'}
        </button>
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
const inputStyle: React.CSSProperties = { ...FIELD, minWidth: 220 };
