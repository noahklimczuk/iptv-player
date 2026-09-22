/**
 * Full-screen EPG grid — README §7.1.
 *
 * Channels down the left, time across the top, blocks sized proportionally to duration,
 * a live "now" line, and a live video preview panel that keeps playing while you browse
 * (§7.1: "the single most cable-like detail; do not skip it").
 *
 * Both axes are virtualized: only visible channel rows are mounted, and each row renders
 * only the programmes overlapping the visible time window.
 */
import { useVirtualizer } from '@tanstack/react-virtual';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Channel, Programme } from '@shared/ipc';
import { useDvrMarks } from '@/features/dvr/useDvrMarks';
import { Badge, Button, EmptyState, Skeleton } from '@/components/Primitives';
import { Icon } from '@/components/Icon';
import { useCommand } from '@/hooks/useCommand';
import { clockTime, dayLabel, duration } from '@/lib/format';

const HALF_HOUR = 1800;
const PX_PER_MIN = 6;
const SLOT_W = (HALF_HOUR / 60) * PX_PER_MIN; // 180px per 30 min
const CHANNEL_W = 220;
const ROW_H = 64;
/** How much time the grid holds in memory at once. */
const WINDOW_HOURS = 24;

const floorToSlot = (t: number) => Math.floor(t / HALF_HOUR) * HALF_HOUR;
const xFor = (t: number, from: number) => ((t - from) / 60) * PX_PER_MIN;

export function GuidePage({
  onTune,
  onCatchup,
  onSearch,
}: {
  onTune: (channel: Channel) => void;
  onCatchup: (channelId: number, start: number, stop: number) => Promise<void>;
  /** Open the command palette looking for this title. */
  onSearch: (query: string) => void;
}) {
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  const [from, setFrom] = useState(() => floorToSlot(Math.floor(Date.now() / 1000)));
  const [group, setGroup] = useState<string | undefined>(undefined);
  const [selected, setSelected] = useState<{ ch: Channel; prog: Programme } | null>(null);
  const dvr = useDvrMarks();
  const [previewChannel, setPreviewChannel] = useState<Channel | null>(null);

  const scrollRef = useRef<HTMLDivElement>(null);
  const to = from + WINDOW_HOURS * 3600;

  // The "now" line has to actually move (README §7.1).
  useEffect(() => {
    const t = window.setInterval(() => setNow(Math.floor(Date.now() / 1000)), 30_000);
    return () => window.clearInterval(t);
  }, []);

  const { data: groups } = useCommand('channels.groups', undefined, []);
  const { data: slice, loading } = useCommand(
    'epg.gridSlice',
    { from, to, channelIds: [] },
    [from, to],
  );

  const channels = useMemo(() => {
    const all = slice?.channels ?? [];
    return group ? all.filter((c) => c.group === group) : all;
  }, [slice, group]);

  useEffect(() => {
    if (!previewChannel && channels.length) setPreviewChannel(channels[0]!);
  }, [channels, previewChannel]);

  const rowVirt = useVirtualizer({
    count: channels.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_H,
    overscan: 6,
  });

  const totalWidth = ((to - from) / 60) * PX_PER_MIN;

  const jumpTo = useCallback((target: number) => {
    setFrom(floorToSlot(target));
    requestAnimationFrame(() => {
      if (scrollRef.current) scrollRef.current.scrollLeft = 0;
    });
  }, []);

  const scrollToNow = useCallback(() => {
    jumpTo(Math.floor(Date.now() / 1000) - HALF_HOUR);
  }, [jumpTo]);

  /** Prime time = 20:00 on the day `offsetDays` from today (README §7.1). */
  const primeTime = useCallback((offsetDays: number) => {
    const d = new Date();
    d.setDate(d.getDate() + offsetDays);
    d.setHours(20, 0, 0, 0);
    jumpTo(Math.floor(d.getTime() / 1000) - HALF_HOUR);
  }, [jumpTo]);

  const timeSlots = useMemo(() => {
    const out: number[] = [];
    for (let t = from; t < to; t += HALF_HOUR) out.push(t);
    return out;
  }, [from, to]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', background: 'var(--bg)' }}>
      <GuideToolbar
        from={from}
        groups={groups ?? []}
        group={group}
        setGroup={setGroup}
        onNow={scrollToNow}
        onShift={(hours) => jumpTo(from + hours * 3600)}
        onPrimeTime={primeTime}
      />

      <div style={{ display: 'flex', flex: 1, minHeight: 0 }}>
        {/* Live preview + info pane. */}
        <aside
          style={{
            width: 340, flexShrink: 0, borderRight: '1px solid var(--border)',
            display: 'flex', flexDirection: 'column', background: 'var(--bg-elevated)',
          }}
        >
          <PreviewPane channel={previewChannel} onTune={onTune} />
          <InfoPane
            selected={selected}
            onTune={onTune}
            onCatchup={onCatchup}
            onSearch={onSearch}
            dvr={dvr}
          />
        </aside>

        {/* The grid. */}
        <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column' }}>
          {loading && !slice ? (
            <div style={{ padding: 'var(--sp-5)', display: 'grid', gap: 8 }}>
              {Array.from({ length: 10 }, (_, i) => <Skeleton key={i} h={ROW_H - 6} />)}
            </div>
          ) : channels.length === 0 ? (
            <EmptyState title="No channels in this category" />
          ) : (
            <div
              ref={scrollRef}
              style={{ flex: 1, overflow: 'auto', position: 'relative' }}
            >
              <div style={{ width: CHANNEL_W + totalWidth, position: 'relative' }}>
                <TimeHeader slots={timeSlots} from={from} />

                {/* Moving "now" line. */}
                {now >= from && now <= to && (
                  <div
                    aria-hidden
                    style={{
                      position: 'absolute', top: 0, bottom: 0,
                      left: CHANNEL_W + xFor(now, from),
                      width: 2, background: 'var(--live)', zIndex: 5,
                      pointerEvents: 'none',
                    }}
                  >
                    <div
                      style={{
                        position: 'sticky', top: 34, width: 8, height: 8,
                        borderRadius: '50%', background: 'var(--live)',
                        transform: 'translateX(-3px)',
                      }}
                    />
                  </div>
                )}

                <div
                  style={{ height: rowVirt.getTotalSize(), position: 'relative' }}
                >
                  {rowVirt.getVirtualItems().map((vr) => {
                    const ch = channels[vr.index]!;
                    const progs = slice?.programmes[ch.epgChannelId ?? String(ch.id)] ?? [];
                    return (
                      <div
                        key={ch.id}
                        style={{
                          position: 'absolute', top: vr.start, left: 0,
                          height: ROW_H, width: '100%', display: 'flex',
                        }}
                      >
                        <ChannelCell
                          channel={ch}
                          active={previewChannel?.id === ch.id}
                          onClick={() => { setPreviewChannel(ch); onTune(ch); }}
                        />
                        <ProgrammeRow
                          channel={ch}
                          programmes={progs}
                          from={from}
                          to={to}
                          now={now}
                          selectedId={selected?.prog.id ?? null}
                          recordingKeys={dvr.recordingKeys}
                          onSelect={(prog) => {
                            setSelected({ ch, prog });
                            setPreviewChannel(ch);
                          }}
                          onTune={() => onTune(ch)}
                        />
                      </div>
                    );
                  })}
                </div>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function GuideToolbar({
  from, groups, group, setGroup, onNow, onShift, onPrimeTime,
}: {
  from: number;
  groups: { name: string; count: number }[];
  group: string | undefined;
  setGroup: (g: string | undefined) => void;
  onNow: () => void;
  onShift: (hours: number) => void;
  onPrimeTime: (offsetDays: number) => void;
}) {
  return (
    <div
      style={{
        display: 'flex', alignItems: 'center', gap: 'var(--sp-3)', flexWrap: 'wrap',
        padding: 'var(--sp-3) var(--sp-5)', borderBottom: '1px solid var(--border)',
        background: 'var(--bg-elevated)',
      }}
    >
      <h1 style={{ margin: 0, fontSize: 'var(--fs-lg)', fontWeight: 700 }}>TV Guide</h1>
      <span style={{ color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>
        {dayLabel(from)}
      </span>

      <div style={{ display: 'flex', gap: 4, marginLeft: 'var(--sp-3)' }}>
        <Button size="sm" onClick={onNow} icon="clock">Now</Button>
        <Button size="sm" onClick={() => onShift(-24)}>−24h</Button>
        <Button size="sm" onClick={() => onShift(-1)}>−1h</Button>
        <Button size="sm" onClick={() => onShift(1)}>+1h</Button>
        <Button size="sm" onClick={() => onShift(24)}>+24h</Button>
      </div>

      <div style={{ display: 'flex', gap: 4 }}>
        {[0, 1, 2].map((d) => (
          <Button key={d} size="sm" variant="ghost" onClick={() => onPrimeTime(d)}>
            {d === 0 ? 'Tonight 8pm' : `${dayLabel(Math.floor(Date.now() / 1000) + d * 86400)} 8pm`}
          </Button>
        ))}
      </div>

      <select
        value={group ?? ''}
        onChange={(e) => setGroup(e.target.value || undefined)}
        aria-label="Filter by category"
        style={{
          marginLeft: 'auto', padding: '6px 10px', background: 'var(--surface)',
          color: 'var(--text)', border: '1px solid var(--border-strong)',
          borderRadius: 'var(--r-md)', fontSize: 'var(--fs-sm)',
        }}
      >
        <option value="">All channels</option>
        {groups.map((g) => (
          <option key={g.name} value={g.name}>{g.name} ({g.count})</option>
        ))}
      </select>
    </div>
  );
}

function TimeHeader({ slots, from }: { slots: number[]; from: number }) {
  return (
    <div
      style={{
        position: 'sticky', top: 0, zIndex: 6, display: 'flex', height: 34,
        background: 'var(--bg-elevated)', borderBottom: '1px solid var(--border)',
      }}
    >
      <div
        style={{
          width: CHANNEL_W, flexShrink: 0, position: 'sticky', left: 0, zIndex: 7,
          background: 'var(--bg-elevated)', borderRight: '1px solid var(--border)',
          display: 'flex', alignItems: 'center', padding: '0 var(--sp-3)',
          fontSize: 'var(--fs-xs)', color: 'var(--text-faint)', fontWeight: 700,
          letterSpacing: '0.06em',
        }}
      >
        CHANNEL
      </div>
      {slots.map((t) => (
        <div
          key={t}
          style={{
            width: SLOT_W, flexShrink: 0, borderLeft: '1px solid var(--border)',
            display: 'flex', alignItems: 'center', padding: '0 var(--sp-2)',
            fontSize: 'var(--fs-xs)', color: 'var(--text-muted)',
            fontVariantNumeric: 'tabular-nums',
          }}
        >
          {clockTime(t)}
        </div>
      ))}
      <span style={{ display: 'none' }}>{from}</span>
    </div>
  );
}

function ChannelCell({
  channel, active, onClick,
}: { channel: Channel; active: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      style={{
        width: CHANNEL_W, flexShrink: 0, position: 'sticky', left: 0, zIndex: 4,
        display: 'flex', alignItems: 'center', gap: 'var(--sp-2)',
        padding: '0 var(--sp-3)', textAlign: 'left', cursor: 'pointer',
        background: active ? 'var(--surface-hover)' : 'var(--bg-elevated)',
        borderRight: '1px solid var(--border)',
        borderBottom: '1px solid var(--border)',
        borderTop: 'none', borderLeft: 'none', color: 'inherit',
      }}
    >
      <span
        style={{
          width: 34, textAlign: 'right', fontSize: 'var(--fs-sm)',
          color: 'var(--text-faint)', fontVariantNumeric: 'tabular-nums', fontWeight: 700,
        }}
      >
        {channel.number}
      </span>
      {channel.logo && (
        <img
          src={channel.logo} alt=""
          style={{
            width: 30, height: 30, borderRadius: 'var(--r-sm)', objectFit: 'cover',
            flexShrink: 0,
          }}
        />
      )}
      <span
        style={{
          fontSize: 'var(--fs-sm)', fontWeight: 600, overflow: 'hidden',
          textOverflow: 'ellipsis', whiteSpace: 'nowrap',
        }}
      >
        {channel.name}
      </span>
      {channel.hasCatchup && (
        <span
          role="img"
          aria-label="Catch-up available"
          title="Catch-up available"
          style={{ marginLeft: 'auto', display: 'flex', color: 'var(--text-faint)' }}
        >
          <Icon name="back10" size={13} />
        </span>
      )}
    </button>
  );
}

/**
 * The same channel at other qualities (README §7.3: "one entry with a quality
 * selector"). Only appears when the provider actually carries more than one, which is
 * also when collapsing duplicates has hidden the others from the list.
 */
function ChannelSources({
  channel, onTune,
}: { channel: Channel; onTune: (c: Channel) => void }) {
  const { data } = useCommand('library.alternates', { kind: 'live', id: channel.id },
    [channel.id]);
  const alternates = (data ?? []).filter((a) => a.id !== channel.id);
  if (alternates.length === 0) return null;

  return (
    <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', alignItems: 'center' }}>
      <span style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-faint)' }}>Also in</span>
      {alternates.map((a) => (
        <Button
          key={a.id}
          size="sm"
          variant="ghost"
          onClick={() => onTune({ ...channel, id: a.id, name: a.name, quality: a.quality })}
        >
          {a.quality ?? 'Unknown'}
        </Button>
      ))}
    </div>
  );
}

/** Genre colour coding with a legend (README §7.1). */
function genreTone(categories: string[]): string {
  const c = categories.map((x) => x.toLowerCase()).join(' ');
  if (c.includes('sport')) return 'color-mix(in srgb, #22c55e 18%, var(--surface))';
  if (c.includes('movie') || c.includes('film')) return 'color-mix(in srgb, #a855f7 18%, var(--surface))';
  if (c.includes('news')) return 'color-mix(in srgb, #3b82f6 18%, var(--surface))';
  if (c.includes('kid')) return 'color-mix(in srgb, #f59e0b 18%, var(--surface))';
  if (c.includes('document')) return 'color-mix(in srgb, #14b8a6 18%, var(--surface))';
  return 'var(--surface)';
}

function ProgrammeRow({
  channel, programmes, from, to, now, selectedId, recordingKeys, onSelect, onTune,
}: {
  channel: Channel;
  programmes: Programme[];
  from: number; to: number; now: number;
  selectedId: number | null;
  recordingKeys: Set<string>;
  onSelect: (p: Programme) => void;
  onTune: () => void;
}) {
  // Only render what overlaps the window — the horizontal half of the virtualization.
  const visible = programmes.filter((p) => p.start < to && p.stop > from);

  return (
    <div
      style={{
        position: 'relative', flex: 1, height: ROW_H,
        borderBottom: '1px solid var(--border)',
      }}
    >
      {visible.map((p) => {
        const left = Math.max(0, xFor(p.start, from));
        const right = xFor(Math.min(p.stop, to), from);
        const width = Math.max(2, right - left);
        const airing = p.start <= now && p.stop > now;
        const isSelected = selectedId === p.id;
        const recording = recordingKeys.has(`${channel.id}:${p.start}:${p.title}`);

        return (
          <button
            key={p.id}
            onClick={() => onSelect(p)}
            onDoubleClick={onTune}
            title={`${p.title} · ${clockTime(p.start)}–${clockTime(p.stop)}`}
            style={{
              position: 'absolute', left, width: width - 2, top: 3, height: ROW_H - 9,
              display: 'flex', flexDirection: 'column', justifyContent: 'center',
              alignItems: 'flex-start', gap: 2, overflow: 'hidden',
              padding: '0 var(--sp-3)', textAlign: 'left', cursor: 'pointer',
              background: genreTone(p.categories),
              border: `1px solid ${isSelected ? 'var(--accent)' : 'var(--border)'}`,
              boxShadow: isSelected ? '0 0 0 1px var(--accent)' : 'none',
              borderRadius: 'var(--r-sm)', color: 'var(--text)',
              opacity: p.stop < now ? 0.45 : 1,
            }}
          >
            <span
              style={{
                fontSize: 'var(--fs-sm)', fontWeight: 650, whiteSpace: 'nowrap',
                overflow: 'hidden', textOverflow: 'ellipsis', maxWidth: '100%',
                display: 'flex', alignItems: 'center', gap: 5,
              }}
            >
              {recording && (
                <span
                  aria-label="Recording scheduled"
                  title="Recording scheduled"
                  style={{
                    width: 7, height: 7, borderRadius: '50%', flexShrink: 0,
                    background: 'var(--live)',
                  }}
                />
              )}
              {p.isNew && <Badge tone="new" style={{ padding: '0 4px' }}>New</Badge>}
              {p.isLive && <Badge tone="live" style={{ padding: '0 4px' }}>Live</Badge>}
              {p.title}
            </span>
            {width > 110 && (
              <span
                style={{
                  fontSize: 'var(--fs-xs)', color: 'var(--text-faint)',
                  whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
                  maxWidth: '100%',
                }}
              >
                {clockTime(p.start)}–{clockTime(p.stop)}
                {p.season && p.episode ? ` · S${p.season}E${p.episode}` : ''}
              </span>
            )}
            {airing && (
              <div
                aria-hidden
                style={{
                  position: 'absolute', left: 0, bottom: 0, height: 2,
                  width: `${((now - p.start) / (p.stop - p.start)) * 100}%`,
                  background: 'var(--live)',
                }}
              />
            )}
          </button>
        );
      })}
    </div>
  );
}

/** Live video keeps playing while you browse the guide (README §7.1). */
function PreviewPane({
  channel, onTune,
}: { channel: Channel | null; onTune: (c: Channel) => void }) {
  const { data } = useCommand(
    'epg.nowNext',
    { channelId: channel?.id ?? 0 },
    [channel?.id],
  );

  if (!channel) return <div style={{ aspectRatio: '16/9', background: '#000' }} />;

  return (
    <div>
      <div
        style={{
          position: 'relative', aspectRatio: '16 / 9', background: '#000',
          overflow: 'hidden',
        }}
      >
        {/* On Windows this region is transparent and the mpv surface shows through;
            in the browser we render the channel art as a stand-in. */}
        {channel.logo && (
          <img
            src={channel.logo} alt=""
            style={{
              width: '100%', height: '100%', objectFit: 'cover', opacity: 0.5,
              filter: 'blur(1px)',
            }}
          />
        )}
        <div
          style={{
            position: 'absolute', inset: 0, display: 'flex', flexDirection: 'column',
            justifyContent: 'space-between', padding: 'var(--sp-3)',
            background: 'linear-gradient(to top, rgb(0 0 0 / 0.85), transparent 60%)',
          }}
        >
          <Badge tone="live" style={{ alignSelf: 'flex-start' }}>● Live</Badge>
          <div>
            <div style={{ fontWeight: 700, fontSize: 'var(--fs-sm)' }}>{channel.name}</div>
            {data?.now && (
              <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>
                {data.now.title}
              </div>
            )}
          </div>
        </div>
      </div>
      <div style={{ padding: 'var(--sp-3)', borderBottom: '1px solid var(--border)' }}>
        <Button
          variant="primary" size="sm" icon="tv"
          style={{ width: '100%' }}
          onClick={() => onTune(channel)}
        >
          Watch {channel.name}
        </Button>
      </div>
    </div>
  );
}

function InfoPane({
  selected, onTune, onCatchup, onSearch, dvr,
}: {
  selected: { ch: Channel; prog: Programme } | null;
  onTune: (c: Channel) => void;
  onCatchup: (channelId: number, start: number, stop: number) => Promise<void>;
  onSearch: (query: string) => void;
  dvr: ReturnType<typeof useDvrMarks>;
}) {
  const [catchupError, setCatchupError] = useState<string | null>(null);
  if (!selected) {
    return (
      <div
        style={{
          padding: 'var(--sp-5)', color: 'var(--text-faint)',
          fontSize: 'var(--fs-sm)', lineHeight: 1.6,
        }}
      >
        Select a programme to see details, or double-click it to watch that channel.
      </div>
    );
  }

  const { ch, prog } = selected;
  const now = Math.floor(Date.now() / 1000);
  const airing = prog.start <= now && prog.stop > now;
  // Nothing to schedule for a programme that is over; catch-up is a different button.
  const ended = prog.stop <= now;
  // Anything that has begun can be caught up on; what is still to come cannot.
  const started = prog.start <= now;
  const recording = dvr.recordingFor(ch, prog);
  const reminder = dvr.reminderFor(ch, prog);
  const rule = dvr.ruleFor(prog);

  return (
    <div style={{ padding: 'var(--sp-4)', overflowY: 'auto', flex: 1 }}>
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginBottom: 'var(--sp-3)' }}>
        {prog.isNew && <Badge tone="new">New</Badge>}
        {prog.isLive && <Badge tone="live">Live</Badge>}
        {prog.isPremiere && <Badge tone="accent">Premiere</Badge>}
        {prog.rating && <Badge tone="outline">{prog.rating}</Badge>}
        {ch.quality && <Badge tone="neutral">{ch.quality}</Badge>}
      </div>

      <h2 style={{ margin: '0 0 4px', fontSize: 'var(--fs-lg)', fontWeight: 700 }}>
        {prog.title}
      </h2>
      {prog.subTitle && (
        <div style={{ color: 'var(--text-muted)', marginBottom: 6 }}>{prog.subTitle}</div>
      )}
      <div
        style={{
          fontSize: 'var(--fs-sm)', color: 'var(--text-faint)',
          marginBottom: 'var(--sp-3)', fontVariantNumeric: 'tabular-nums',
        }}
      >
        {clockTime(prog.start)}–{clockTime(prog.stop)} · {duration(prog.stop - prog.start)}
        {prog.season && prog.episode ? ` · S${prog.season}E${prog.episode}` : ''}
      </div>

      <p
        style={{
          margin: '0 0 var(--sp-4)', fontSize: 'var(--fs-sm)', lineHeight: 1.6,
          color: 'var(--text-muted)',
        }}
      >
        {prog.description}
      </p>

      <div style={{ display: 'grid', gap: 6 }}>
        {airing && (
          <Button variant="primary" size="sm" icon="play" iconFilled onClick={() => onTune(ch)}>
            Watch now
          </Button>
        )}
        {/* Offered for anything already started, not only what is on now: the whole
            point of catch-up is the programme that finished an hour ago. */}
        {started && ch.hasCatchup && (
          <Button
            size="sm"
            icon="back10"
            onClick={() => {
              setCatchupError(null);
              void onCatchup(ch.id, prog.start, prog.stop).catch((e: unknown) =>
                setCatchupError(e instanceof Error ? e.message : String(e)));
            }}
          >
            {airing ? 'Watch from start' : 'Watch this'}
          </Button>
        )}
        {catchupError && (
          <div role="alert" style={{ color: 'var(--danger)', fontSize: 'var(--fs-sm)' }}>
            {catchupError}
          </div>
        )}

        <Button
          size="sm"
          icon="record"
          iconFilled={!!recording}
          variant={recording ? 'danger' : 'secondary'}
          disabled={dvr.busy || ended}
          onClick={() => void dvr.toggleRecord(ch, prog)}
        >
          {recording
            ? (recording.state === 'recording' ? 'Stop recording' : 'Cancel recording')
            : 'Record'}
        </Button>

        <Button
          size="sm"
          icon="stack"
          variant={rule ? 'primary' : 'secondary'}
          disabled={dvr.busy}
          onClick={() => void dvr.toggleSeries(ch, prog, false)}
        >
          {rule ? 'Stop recording series' : 'Record series'}
        </Button>

        {!airing && !ended && (
          <Button
            size="sm"
            icon="bell"
            iconFilled={reminder != null}
            variant={reminder != null ? 'primary' : 'secondary'}
            disabled={dvr.busy}
            onClick={() => void dvr.toggleReminder(ch, prog)}
          >
            {reminder != null ? 'Reminder set' : 'Remind me'}
          </Button>
        )}

        <ChannelSources channel={ch} onTune={onTune} />

        <Button
          size="sm"
          variant="ghost"
          icon="search"
          onClick={() => onSearch(prog.title)}
        >
          Search this title
        </Button>

        {dvr.error && (
          <div role="alert" style={{ color: 'var(--danger)', fontSize: 'var(--fs-sm)' }}>
            {dvr.error}
          </div>
        )}
      </div>
    </div>
  );
}
