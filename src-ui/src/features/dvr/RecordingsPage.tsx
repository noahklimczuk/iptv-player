/**
 * The recordings library (README §7.7).
 *
 * Three tabs rather than one list, because the three groups answer different
 * questions: "what can I watch", "what is Aurora about to do", and "what went wrong".
 * Mixing them means the failures are the easiest thing to miss.
 */
import { useCallback, useEffect, useMemo, useState } from 'react';
import type { Channel, Recording, RecordingConflict, RecordingRule } from '@shared/ipc';
import { Icon } from '@/components/Icon';
import { Badge, Button, EmptyState, IconButton, ProgressBar, Skeleton } from '@/components/Primitives';
import { useCommand } from '@/hooks/useCommand';
import { invoke, onDvrTick } from '@/ipc';
import { bytes, clockTime, dayLabel, duration, untilLabel } from '@/lib/format';

type Tab = 'recorded' | 'scheduled' | 'rules';

const TABS: { id: Tab; label: string }[] = [
  { id: 'recorded', label: 'Recorded' },
  { id: 'scheduled', label: 'Scheduled' },
  { id: 'rules', label: 'Series rules' },
];

const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

export function RecordingsPage({ onPlay }: { onPlay?: (r: Recording) => void }) {
  const [tab, setTab] = useState<Tab>('recorded');
  const [nonce, setNonce] = useState(0);
  const refresh = useCallback(() => setNonce((n) => n + 1), []);

  const { data: all, loading } = useCommand('dvr.list', {}, [nonce]);
  const { data: rules } = useCommand('dvr.rules', undefined, [nonce]);
  const { data: conflicts } = useCommand('dvr.conflicts', undefined, [nonce]);
  const { data: storage } = useCommand('dvr.storage', undefined, [nonce]);
  const { data: channels } = useCommand('channels.list', {}, []);

  // A recording starting or finishing changes this page, and the host already tells
  // us when that happens — polling would only make it slower to notice.
  useEffect(() => onDvrTick(() => refresh()), [refresh]);

  const byName = useMemo(() => {
    const m = new Map<number, Channel>();
    for (const c of channels ?? []) m.set(c.id, c);
    return m;
  }, [channels]);

  const recordings = all ?? [];
  const recorded = recordings.filter((r) => r.state === 'completed');
  const upcoming = recordings.filter((r) => r.state === 'scheduled' || r.state === 'recording');
  const problems = recordings.filter((r) => r.state === 'failed' || r.state === 'skipped');

  const counts: Record<Tab, number> = {
    recorded: recorded.length,
    scheduled: upcoming.length,
    rules: (rules ?? []).length,
  };

  return (
    <div style={{ padding: 'var(--sp-5) var(--sp-6) var(--sp-8)' }}>
      <div style={headerRow}>
        <h1 style={{ margin: 0, fontSize: 'var(--fs-2xl)', fontWeight: 800 }}>Recordings</h1>
        {storage && (
          <div style={{ marginLeft: 'auto', textAlign: 'right' }}>
            <StorageMeter
              usedBytes={storage.usedBytes}
              quotaBytes={storage.quotaBytes}
              folder={storage.folder}
            />
          </div>
        )}
      </div>

      <div role="tablist" aria-label="Recordings" style={{ display: 'flex', gap: 4, marginBottom: 'var(--sp-4)' }}>
        {TABS.map((t) => (
          <Button
            key={t.id}
            role="tab"
            aria-selected={tab === t.id}
            size="sm"
            variant={tab === t.id ? 'primary' : 'ghost'}
            onClick={() => setTab(t.id)}
          >
            {t.label}
            <span style={{ opacity: 0.65, marginLeft: 6 }}>{counts[t.id]}</span>
          </Button>
        ))}
      </div>

      {(conflicts ?? []).length > 0 && tab !== 'recorded' && (
        <ConflictNotice conflicts={conflicts ?? []} recordings={recordings} />
      )}

      {loading && (
        <div style={{ display: 'grid', gap: 'var(--sp-2)' }}>
          {Array.from({ length: 6 }, (_, i) => <Skeleton key={i} h={72} />)}
        </div>
      )}

      {!loading && tab === 'recorded' && (
        recorded.length === 0 ? (
          <EmptyState
            icon="record"
            title="Nothing recorded yet"
            body="Press Record on anything in the guide, or set up a series rule and Aurora will keep up with it for you."
          />
        ) : (
          <div style={listStyle}>
            {recorded.map((r) => (
              <RecordedRow
                key={r.id}
                rec={r}
                channel={byName.get(r.channelId)?.name ?? null}
                onChanged={refresh}
                onPlay={onPlay}
              />
            ))}
          </div>
        )
      )}

      {!loading && tab === 'scheduled' && (
        <>
          {upcoming.length === 0 ? (
            <EmptyState
              icon="clock"
              title="Nothing scheduled"
              body="Recordings you set from the guide show up here, with a countdown and anything that clashes."
            />
          ) : (
            <div style={listStyle}>
              {upcoming.map((r) => (
                <ScheduledRow
                  key={r.id}
                  rec={r}
                  channel={byName.get(r.channelId)?.name ?? null}
                  onChanged={refresh}
                />
              ))}
            </div>
          )}

          {problems.length > 0 && (
            <>
              <h2 style={sectionHeading}>Didn&rsquo;t record</h2>
              <div style={listStyle}>
                {problems.map((r) => (
                  <ProblemRow
                    key={r.id}
                    rec={r}
                    channel={byName.get(r.channelId)?.name ?? null}
                    onChanged={refresh}
                  />
                ))}
              </div>
            </>
          )}
        </>
      )}

      {!loading && tab === 'rules' && (
        <RulesTab rules={rules ?? []} channels={byName} onChanged={refresh} />
      )}
    </div>
  );
}

/* ── Storage ──────────────────────────────────────────────────────────────── */

function StorageMeter({
  usedBytes, quotaBytes, folder,
}: {
  usedBytes: number;
  quotaBytes: number;
  folder: string;
}) {
  const pct = quotaBytes > 0 ? (usedBytes / quotaBytes) * 100 : 0;
  return (
    <div style={{ minWidth: 220 }}>
      <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)', marginBottom: 4 }}>
        {bytes(usedBytes)}
        {quotaBytes > 0 && <> of {bytes(quotaBytes)}</>}
      </div>
      {quotaBytes > 0 && <ProgressBar percent={pct} height={4} />}
      <div
        style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-faint)', marginTop: 4 }}
        title={folder}
      >
        {folder}
      </div>
    </div>
  );
}

/* ── Conflicts ────────────────────────────────────────────────────────────── */

function ConflictNotice({
  conflicts, recordings,
}: {
  conflicts: RecordingConflict[];
  recordings: Recording[];
}) {
  const titleOf = (id: number) => recordings.find((r) => r.id === id)?.title ?? `#${id}`;
  return (
    <div role="status" style={noticeStyle}>
      <Icon name="info" size={18} />
      <div>
        <strong>
          {conflicts.length === 1 ? 'A recording clashes' : `${conflicts.length} clashes`}
        </strong>
        {conflicts.slice(0, 2).map((c) => (
          <div key={`${c.start}-${c.stop}`} style={{ color: 'var(--text-muted)', marginTop: 2 }}>
            {dayLabel(c.start)} {clockTime(c.start)}–{clockTime(c.stop)}: {c.slotIds.length} at
            once, {c.overBy} more than your subscription allows.{' '}
            {/* The tail of the priority-ordered list is what loses, so name it. */}
            <em>{titleOf(c.slotIds[c.slotIds.length - 1] ?? 0)}</em> will be dropped.
          </div>
        ))}
      </div>
    </div>
  );
}

/* ── Rows ─────────────────────────────────────────────────────────────────── */

function RowShell({
  children, accent,
}: {
  children: React.ReactNode;
  accent?: string;
}) {
  return (
    <div
      style={{
        display: 'flex', alignItems: 'center', gap: 'var(--sp-3)',
        padding: 'var(--sp-3)', background: 'var(--surface)',
        borderRadius: 'var(--r-md)', border: '1px solid var(--border)',
        borderLeft: accent ? `3px solid ${accent}` : '1px solid var(--border)',
      }}
    >
      {children}
    </div>
  );
}

function TitleBlock({ rec, channel }: { rec: Recording; channel: string | null }) {
  const ep = rec.season != null && rec.episode != null
    ? `S${String(rec.season).padStart(2, '0')}E${String(rec.episode).padStart(2, '0')}`
    : null;
  return (
    <div style={{ minWidth: 0, flex: 1 }}>
      <div style={{ fontWeight: 700, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
        {rec.title}
        {ep && <span style={{ color: 'var(--text-faint)', fontWeight: 500 }}> · {ep}</span>}
      </div>
      <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)' }}>
        {channel && <>{channel} · </>}
        {dayLabel(rec.airStart)} {clockTime(rec.airStart)}–{clockTime(rec.airStop)}
      </div>
    </div>
  );
}

function RecordedRow({
  rec, channel, onChanged, onPlay,
}: {
  rec: Recording;
  channel: string | null;
  onChanged: () => void;
  onPlay?: (r: Recording) => void;
}) {
  const act = async (fn: () => Promise<unknown>) => {
    await fn();
    onChanged();
  };
  return (
    <RowShell>
      <TitleBlock rec={rec} channel={channel} />

      <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--sp-2)' }}>
        {rec.reason && (
          // A completed-but-short recording still plays. Saying so beats letting the
          // viewer discover it 40 minutes in.
          <span title={rec.reason}>
            <Badge tone="outline">Cut short</Badge>
          </span>
        )}
        {!rec.watched && <Badge tone="new">New</Badge>}
        {rec.keep && <Badge tone="outline">Kept</Badge>}
        <span style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)', whiteSpace: 'nowrap' }}>
          {duration(rec.durationSecs)} · {bytes(rec.bytes)}
        </span>

        <Button size="sm" variant="primary" icon="play" onClick={() => onPlay?.(rec)}>
          Play
        </Button>
        <IconButton
          size={32}
          icon="heart"
          filled={rec.keep}
          active={rec.keep}
          label={rec.keep ? 'Allow deleting to free space' : 'Keep — never delete for space'}
          onClick={() => act(() => invoke('dvr.setKeep', { id: rec.id, value: !rec.keep }))}
        />
        <IconButton
          size={32}
          icon="close"
          label={`Delete ${rec.title}`}
          onClick={() => act(() => invoke('dvr.delete', { id: rec.id }))}
        />
      </div>
    </RowShell>
  );
}

function ScheduledRow({
  rec, channel, onChanged,
}: {
  rec: Recording;
  channel: string | null;
  onChanged: () => void;
}) {
  const live = rec.state === 'recording';
  const elapsed = live ? Math.max(0, Math.floor(Date.now() / 1000) - rec.start) : 0;
  const pct = live ? (elapsed / Math.max(1, rec.stop - rec.start)) * 100 : 0;

  return (
    <RowShell accent={live ? 'var(--live)' : undefined}>
      <TitleBlock rec={rec} channel={channel} />

      <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--sp-2)' }}>
        {live ? (
          <>
            <Badge tone="live">
              <Icon name="record" size={11} filled /> Recording
            </Badge>
            <div style={{ width: 120 }}>
              <ProgressBar percent={pct} />
            </div>
            <span style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)' }}>
              {bytes(rec.liveBytes ?? rec.bytes)}
            </span>
          </>
        ) : (
          <span style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)', whiteSpace: 'nowrap' }}>
            {untilLabel(rec.start)}
          </span>
        )}
        <Button
          size="sm"
          variant="ghost"
          onClick={async () => {
            await invoke('dvr.cancel', { id: rec.id });
            onChanged();
          }}
        >
          {live ? 'Stop' : 'Cancel'}
        </Button>
      </div>
    </RowShell>
  );
}

function ProblemRow({
  rec, channel, onChanged,
}: {
  rec: Recording;
  channel: string | null;
  onChanged: () => void;
}) {
  return (
    <RowShell accent="var(--danger)">
      <div style={{ minWidth: 0, flex: 1 }}>
        <TitleBlock rec={rec} channel={channel} />
        {rec.reason && (
          <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--danger)', marginTop: 2 }}>
            {rec.reason}
          </div>
        )}
      </div>
      <Badge tone="outline">{rec.state === 'skipped' ? 'Skipped' : 'Failed'}</Badge>
      <IconButton
        size={32}
        icon="close"
        label={`Dismiss ${rec.title}`}
        onClick={async () => {
          await invoke('dvr.delete', { id: rec.id });
          onChanged();
        }}
      />
    </RowShell>
  );
}

/* ── Series rules ─────────────────────────────────────────────────────────── */

function RulesTab({
  rules, channels, onChanged,
}: {
  rules: RecordingRule[];
  channels: Map<number, Channel>;
  onChanged: () => void;
}) {
  if (rules.length === 0) {
    return (
      <EmptyState
        icon="stack"
        title="No series rules"
        body="A series rule records every episode of a show as it airs. Set one from the Record button in the guide."
      />
    );
  }
  return (
    <div style={listStyle}>
      {rules.map((rule) => (
        <RowShell key={rule.id}>
          <div style={{ minWidth: 0, flex: 1, opacity: rule.enabled ? 1 : 0.55 }}>
            <div style={{ fontWeight: 700 }}>{rule.title}</div>
            <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-muted)' }}>
              {rule.channelId ? (channels.get(rule.channelId)?.name ?? 'One channel') : 'Any channel'}
              {rule.newOnly && ' · New episodes only'}
              {rule.weekdays && ` · ${rule.weekdays.map((d) => WEEKDAYS[d] ?? '?').join(', ')}`}
              {rule.aroundLocalMinute != null && ` · around ${
                String(Math.floor(rule.aroundLocalMinute / 60)).padStart(2, '0')
              }:${String(rule.aroundLocalMinute % 60).padStart(2, '0')}`}
              {rule.keepEpisodes != null && ` · keep ${rule.keepEpisodes}`}
            </div>
          </div>

          <span style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-faint)' }}>
            {rule.scheduled} recorded
          </span>
          <Button
            size="sm"
            variant="ghost"
            onClick={async () => {
              await invoke('dvr.setRuleEnabled', { id: rule.id, value: !rule.enabled });
              onChanged();
            }}
          >
            {rule.enabled ? 'Pause' : 'Resume'}
          </Button>
          <IconButton
            size={32}
            icon="close"
            label={`Delete the rule for ${rule.title}`}
            onClick={async () => {
              await invoke('dvr.deleteRule', { id: rule.id });
              onChanged();
            }}
          />
        </RowShell>
      ))}
    </div>
  );
}

/* ── Styles ───────────────────────────────────────────────────────────────── */

const headerRow: React.CSSProperties = {
  display: 'flex', alignItems: 'flex-end', gap: 'var(--sp-3)',
  marginBottom: 'var(--sp-4)', flexWrap: 'wrap',
};

const listStyle: React.CSSProperties = { display: 'grid', gap: 'var(--sp-2)' };

const sectionHeading: React.CSSProperties = {
  fontSize: 'var(--fs-lg)', fontWeight: 700,
  margin: 'var(--sp-6) 0 var(--sp-3)',
};

const noticeStyle: React.CSSProperties = {
  display: 'flex', gap: 'var(--sp-3)', alignItems: 'flex-start',
  padding: 'var(--sp-3)', marginBottom: 'var(--sp-4)',
  borderRadius: 'var(--r-md)',
  background: 'color-mix(in srgb, var(--warning) 14%, transparent)',
  border: '1px solid color-mix(in srgb, var(--warning) 40%, transparent)',
  fontSize: 'var(--fs-sm)',
};
