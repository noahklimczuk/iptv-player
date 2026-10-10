/**
 * Full-screen EPG grid — README §7.1.
 *
 * Channels down the left, time across the top, blocks sized proportionally to duration,
 * a live "now" line, and a live video preview that keeps playing while you browse
 * (§7.1: "the single most cable-like detail; do not skip it").
 *
 * Both axes are virtualized: only visible channel rows are mounted, and each row renders
 * only the programmes overlapping the visible time window.
 *
 * ## The preview is real video, and that shapes the layout
 *
 * It used to be a 340px-wide panel down the left-hand side showing the channel's *logo*,
 * blurred, with a comment saying the region would be transparent on Windows and mpv
 * would show through. It would not have: the surface fills the whole window unless
 * something tells the host otherwise, and nothing did — so on a release that panel was
 * a blurred logo and never a picture.
 *
 * Now it is a picture. It sits top right, at a size worth looking at, and it works by
 * two things kept in step: `useInlaidPicture` tells the host to put the surface exactly
 * where the box is, and `SurfaceHole` paints the page's background *around* that
 * rectangle so nothing covers it. Both are needed — a hole with no surface under it is a
 * window onto the desktop, and a surface under an opaque page is invisible.
 *
 * Top right rather than top left because the grid reads left-to-right from the channel
 * column: putting the picture on the left pushed the thing you are actually scanning
 * into the middle of the screen, and the channel names ended up nowhere near the edge
 * they are indexed from.
 *
 * ## Clicking a channel
 *
 * First click previews it, second click goes full screen. That is the behaviour a
 * set-top box has, and the reason it is worth the extra state: a guide where one click
 * takes over the screen cannot be browsed — you cannot compare two channels without
 * leaving and coming back twice.
 */
import { useVirtualizer } from '@tanstack/react-virtual';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Channel, MosaicRect, Programme } from '@shared/ipc';
import { useDvrMarks } from '@/features/dvr/useDvrMarks';
import { Badge, Button, EmptyState, Select, Skeleton } from '@/components/Primitives';
import { Icon } from '@/components/Icon';
import { useCommand } from '@/hooks/useCommand';
import { clockTime, dayLabel, duration } from '@/lib/format';
import { SurfaceHole } from '@/components/SurfaceHole';
import { useInlaidPicture } from '@/hooks/useInlaidPicture';

const HALF_HOUR = 1800;
const PX_PER_MIN = 6;
const SLOT_W = (HALF_HOUR / 60) * PX_PER_MIN; // 180px per 30 min
const CHANNEL_W = 220;
const ROW_H = 64;
/**
 * How wide the preview column is.
 *
 * 460 rather than the 340 the old panel had, which makes the picture 460×259 instead of
 * 340×191 — a little over three times the area. It is the thing §7.1 calls the most
 * cable-like detail in the app, and at 340 it was a thumbnail.
 *
 * Fixed rather than a fraction: the grid beside it is measured in pixels per minute, so
 * a percentage column would change how much of the schedule is visible with the window
 * size, which is the one thing about a guide that should not move.
 */
const PREVIEW_W = 460;
/** How much time the grid holds in memory at once. */
const WINDOW_HOURS = 24;

const floorToSlot = (t: number) => Math.floor(t / HALF_HOUR) * HALF_HOUR;
const xFor = (t: number, from: number) => ((t - from) / 60) * PX_PER_MIN;

export function GuidePage({
  onTune,
  onPreview,
  pictureReady,
  onInlay,
  onCatchup,
  onSearch,
}: {
  /** Play it full screen. The second click on an already-previewing channel. */
  onTune: (channel: Channel) => void;
  /**
   * Start playing it without surfacing the player, so it lands in the preview box.
   *
   * A separate prop rather than a flag on `onTune` because the two are different
   * intentions and the page says which it means at each call site.
   */
  onPreview: (channel: Channel) => void;
  /**
   * Whether the video surface actually has a frame on it.
   *
   * Not the same as "we asked it to play". The preview box is a hole in an otherwise
   * opaque page -- that is how the picture behind the WebView shows at all -- so a box
   * made see-through before the stream has opened is a rectangle of desktop. This is the
   * host's own answer, and the box stays black until it is true.
   */
  pictureReady: boolean;
  /**
   * Where the picture ended up, or `null` when the guide no longer has it.
   *
   * The shell paints its own opaque background behind every page, and that background
   * is over the video too — so the hole this page punches is only a hole if the shell
   * stops painting as well. It is told rather than asked because only this page knows
   * when it has the surface.
   */
  onInlay: (rect: MosaicRect | null) => void;
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
  /**
   * The channel whose stream is actually in the preview, as opposed to merely selected.
   *
   * Two different things, and conflating them is what makes the second click wrong.
   * Moving the selection with the arrow keys or clicking a *programme* highlights a
   * channel without tuning it; only a click on the channel starts a stream. The second
   * click goes full screen just when this says the picture is already live, so a click
   * on a channel nobody has tuned yet never jumps straight past the preview.
   */
  const [playingId, setPlayingId] = useState<number | null>(null);

  const previewRef = useRef<HTMLDivElement>(null);
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

  /**
   * Ask the host for the picture, but only once there is one.
   *
   * Gated on `playingId` rather than being on whenever the guide is open, because the
   * hole and the surface are two halves of the same thing: a hole punched before a
   * stream is playing is a rectangle of desktop showing through the app.
   */
  const inlay = useInlaidPicture(previewRef, pictureReady && playingId != null);

  // Reported through an effect rather than from the hook's callback so that leaving the
  // page also says so: the cleanup runs on unmount, and a shell left transparent after
  // the guide has gone is a window onto the desktop.
  useEffect(() => {
    onInlay(inlay);
    return () => onInlay(null);
  }, [inlay, onInlay]);

  const totalWidth = ((to - from) / 60) * PX_PER_MIN;

  const jumpTo = useCallback((target: number) => {
    setFrom(floorToSlot(target));
    requestAnimationFrame(() => {
      if (scrollRef.current) scrollRef.current.scrollLeft = 0;
    });
  }, []);

  /**
   * One click previews, a second on the same channel goes full screen.
   *
   * Keyed on `playingId` rather than on the selection, so a channel that is only
   * highlighted — by the arrow keys, or by clicking one of its programmes — still takes
   * two clicks to reach full screen. The alternative made the guide unusable with a
   * remote: moving down a column and pressing OK would have gone straight to full
   * screen on a channel that had never been previewed.
   */
  const clickChannel = useCallback(
    (ch: Channel) => {
      setPreviewChannel(ch);
      if (playingId === ch.id) {
        onTune(ch);
        return;
      }
      setPlayingId(ch.id);
      onPreview(ch);
    },
    [playingId, onPreview, onTune],
  );

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
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        // Transparent while the picture is inlaid, with `SurfaceHole` painting the
        // background around it instead. An opaque root here is painted over the video,
        // and no amount of transparency on the box inside it would help: a transparent
        // child shows its parent's background, not the window behind the page.
        background: inlay ? 'transparent' : 'var(--bg)',
        position: 'relative',
      }}
    >
      {/* Behind the content, so the page draws over it and only the box is see-through. */}
      <SurfaceHole rect={inlay} />

      {/*
        * Above the bands, explicitly. A `position: fixed` band with `z-index: 0` paints
        * *above* in-flow content that has no z-index of its own — which is the trap
        * `PipTile` fell into — so everything that must stay visible says so.
        */}
      <div style={{ position: 'relative', zIndex: 1 }}>
      <GuideToolbar
        from={from}
        groups={groups ?? []}
        group={group}
        setGroup={setGroup}
        onNow={scrollToNow}
        onShift={(hours) => jumpTo(from + hours * 3600)}
        onPrimeTime={primeTime}
      />
      </div>

      <div style={{ display: 'flex', flex: 1, minHeight: 0, position: 'relative', zIndex: 1 }}>
        {/* The grid, on the side the channel column is indexed from. */}
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
                          live={playingId === ch.id}
                          onClick={() => clickChannel(ch)}
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

        {/* Preview + info, top right. */}
        <aside
          style={{
            width: PREVIEW_W, flexShrink: 0, borderLeft: '1px solid var(--border)',
            display: 'flex', flexDirection: 'column', minHeight: 0,
            // Deliberately no background. An opaque panel here would be painted over
            // the picture — see `SurfaceHole`. The panels inside it carry their own.
            background: 'transparent',
          }}
        >
          <PreviewPane
            boxRef={previewRef}
            channel={previewChannel}
            live={pictureReady && playingId != null && playingId === previewChannel?.id}
            onTune={onTune}
            onPreview={(ch) => {
              setPlayingId(ch.id);
              onPreview(ch);
            }}
          />
          <InfoPane
            selected={selected}
            onTune={onTune}
            onCatchup={onCatchup}
            onSearch={onSearch}
            dvr={dvr}
          />
        </aside>
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

      <Select
        label="Category"
        placeholder="All channels"
        options={groups.map((g) => ({
          value: g.name, label: g.name, hint: String(g.count),
        }))}
        value={group}
        onChange={setGroup}
        style={{ marginLeft: 'auto' }}
      />
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
  channel, active, live, onClick,
}: {
  channel: Channel;
  /** Selected — its programmes are what the info pane is describing. */
  active: boolean;
  /** Its stream is the one in the preview, so the next click goes full screen. */
  live: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      data-testid="guide-channel"
      data-channel-name={channel.name}
      aria-current={live ? 'true' : undefined}
      data-live={live ? 'true' : 'false'}
      // Says what the next click will do, which is the only way a two-step click is
      // discoverable rather than surprising.
      title={live ? `Watch ${channel.name} full screen` : `Preview ${channel.name}`}
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
      {live && (
        <span
          aria-hidden
          title="Playing in the preview"
          style={{
            width: 6, height: 6, borderRadius: '50%', background: 'var(--live)',
            flexShrink: 0,
          }}
        />
      )}
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
      {/* A channel with no guide for this window used to be an empty strip, and a
          screenful of them reads as the guide having failed to load. On a real
          subscription this is most of it: EPG coverage measured 42.8% on the panel
          this was built against, so whichever end of the list you open is likely to
          be blank. Saying so is the difference between "nothing is on" and "nothing
          is known". */}
      {visible.length === 0 && (
        <div
          style={{
            position: 'absolute', inset: '3px 2px 6px 2px',
            display: 'flex', alignItems: 'center', paddingLeft: 'var(--sp-3)',
            borderRadius: 'var(--r-sm)',
            border: '1px dashed var(--border)',
            color: 'var(--text-faint)', fontSize: 'var(--fs-sm)',
          }}
        >
          No guide data for this channel
        </div>
      )}

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
            data-testid="programme"
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
/**
 * The preview box, and the frame around it.
 *
 * **There is no video element in here.** When something is playing, the rectangle is
 * where the host has put the real surface, and everything this draws has to stay out of
 * it — so the controls sit in a bar under the picture rather than over it, and the box
 * itself paints nothing at all.
 *
 * When nothing is playing it is an ordinary panel: the channel's logo, dimmed, and an
 * invitation to press play. That is also the honest state in a browser, where there is
 * no surface to inlay and `preview.place` only records the request.
 */
function PreviewPane({
  boxRef,
  channel,
  live,
  onTune,
  onPreview,
}: {
  boxRef: React.Ref<HTMLDivElement>;
  channel: Channel | null;
  /** Whether this channel's stream is the one in the box right now. */
  live: boolean;
  onTune: (c: Channel) => void;
  onPreview: (c: Channel) => void;
}) {
  const { data } = useCommand(
    'epg.nowNext',
    { channelId: channel?.id ?? 0 },
    [channel?.id],
  );

  return (
    <div
      style={{
        borderBottom: '1px solid var(--border)',
        background: 'var(--bg-elevated)',
        // The panel's background stops at the picture: see the box below.
        display: 'grid',
      }}
    >
      <div
        ref={boxRef}
        data-testid="guide-preview"
        data-live={live ? 'true' : 'false'}
        style={{
          position: 'relative',
          aspectRatio: '16 / 9',
          // Transparent while the picture is live, because the surface is behind the
          // page and anything painted here is painted over it. Black otherwise, so an
          // empty box reads as a screen rather than as a gap in the layout.
          background: live ? 'transparent' : '#000',
          overflow: 'hidden',
        }}
      >
        {!live && channel?.logo && (
          <img
            src={channel.logo}
            alt=""
            style={{
              width: '100%', height: '100%', objectFit: 'contain',
              opacity: 0.35, padding: '12%', boxSizing: 'border-box',
            }}
          />
        )}
        {!live && (
          <div
            style={{
              position: 'absolute', inset: 0, display: 'grid', placeItems: 'center',
            }}
          >
            {channel ? (
              <Button
                variant="primary"
                icon="play"
                iconFilled
                data-testid="guide-preview-play"
                onClick={() => onPreview(channel)}
              >
                Preview {channel.name}
              </Button>
            ) : (
              <span style={{ color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>
                Pick a channel
              </span>
            )}
          </div>
        )}
      </div>

      {/*
        * Under the picture, not over it. A gradient and a title drawn across the bottom
        * of the box would be drawn across the video — this is the one panel in the app
        * where an overlay is not available.
        */}
      <div
        style={{
          display: 'flex', alignItems: 'center', gap: 'var(--sp-3)',
          padding: 'var(--sp-3)', minHeight: 56,
        }}
      >
        <div style={{ flex: 1, minWidth: 0 }}>
          <div
            style={{
              fontWeight: 700, fontSize: 'var(--fs-sm)',
              overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
            }}
          >
            {channel?.name ?? 'Nothing selected'}
          </div>
          <div
            style={{
              fontSize: 'var(--fs-xs)', color: 'var(--text-muted)',
              overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
            }}
          >
            {data?.now?.title ?? (channel ? 'No guide data' : 'Click a channel to preview it')}
          </div>
        </div>
        {live && <Badge tone="live">● Live</Badge>}
        {channel && (
          <Button
            variant="primary"
            size="sm"
            icon="fullscreen"
            data-testid="guide-preview-fullscreen"
            onClick={() => onTune(channel)}
          >
            Full screen
          </Button>
        )}
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
