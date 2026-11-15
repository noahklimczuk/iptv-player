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
import { useGuideSlice } from './useGuideSlice';
import { nameGuide } from './nameGuide';
import {
  Badge, Button, ChannelTile as LogoTile, EmptyState, Skeleton,
} from '@/components/Primitives';
import { Icon } from '@/components/Icon';
import { useCommand } from '@/hooks/useCommand';
import { dayLabel, duration } from '@/lib/format';
import { useClockTime } from '@/hooks/useClockTime';
import { SurfaceHole } from '@/components/SurfaceHole';
import { useInlaidPicture } from '@/hooks/useInlaidPicture';
import { isTyping } from '@/hooks/useHotkeys';
import { activeProfileId } from '@/state/profile';
import {
  applyFilters, hostGroup, readFilters, toggleGroup, writeFilters, type GuideFilters,
} from './guideFilters';
import { GuideFiltersButton } from './GuideFiltersPanel';

const HALF_HOUR = 1800;
const PX_PER_MIN = 6;
const SLOT_W = (HALF_HOUR / 60) * PX_PER_MIN; // 180px per 30 min
const CHANNEL_W = 264;
const ROW_H = 56;
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
/**
 * How long the keyboard selection has to rest on a channel before it is previewed.
 * Long enough that running down the list opens nothing on the way, short enough that
 * stopping feels like the preview simply follows.
 */
const PREVIEW_SETTLE_MS = 600;

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
  // Half an hour back, as the Now button does: opening on the current slot put the
  // "now" line over the first letter of every title.
  const [from, setFrom] = useState(() => floorToSlot(Math.floor(Date.now() / 1000) - HALF_HOUR));
  const [filters, setFiltersState] = useState<GuideFilters>(readFilters);
  const setFilters = useCallback((f: GuideFilters) => {
    setFiltersState(f);
    writeFilters(f);
  }, []);
  const profileId = activeProfileId();
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
  /**
   * The rows, from `channels.list`, with the category narrowing the *query*.
   *
   * Both halves of this used to come out of one `epg.gridSlice` with
   * `channelIds: []` — which the host reads as every channel — and the category was
   * then applied here, client-side, to what had already crossed the bridge. So
   * choosing one of 67 groups narrowed what was drawn and not what was fetched
   * (R-130).
   */
  const group = hostGroup(filters);
  const { data: channelRows, loading } = useCommand(
    'channels.list',
    // The profile, so `favorite` is filled in for the "Favourites only" filter.
    { group, profileId },
    [group, profileId],
  );

  /**
   * Narrowed by the filters (`guideFilters.ts`): the groups shown or hidden, and
   * "With listings" -- on a real line more than half the channels have no guide
   * anywhere, and browsing through 3,600 rows of event slots and loops is the slow way
   * -- with favourites, catch-up, radio and quality beside it. Client-side, because
   * the list is already here whole and every one of those is a field of the channel.
   */
  const channels = useMemo(
    () => applyFilters(channelRows ?? [], filters),
    [channelRows, filters],
  );

  // A filter that takes the selected channel away moves the selection to the top of
  // what is left, rather than leaving it on a row that is no longer drawn.
  useEffect(() => {
    if (!channels.length) return;
    if (!previewChannel || !channels.some((c) => c.id === previewChannel.id)) {
      setPreviewChannel(channels[0]!);
      setSelected(null);
    }
  }, [channels, previewChannel]);

  const rowVirt = useVirtualizer({
    count: channels.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_H,
    overscan: 6,
  });

  // The programmes for the rows the virtualiser has actually mounted, plus its
  // overscan. About 14 rows of 6,699 on a 950px window, which is the difference
  // between a 14.8 MB reply and roughly 30 KB of it.
  const visibleIds = rowVirt
    .getVirtualItems()
    .map((vr) => channels[vr.index]?.id)
    .filter((id): id is number => id != null);
  const guide = useGuideSlice(from, to, visibleIds);

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

  /**
   * The keyboard: arrows walk the guide, and resting on a channel previews it.
   *
   * Up and Down move between channels, PageUp and PageDown a screenful at a time, Left
   * and Right through the selected channel's programmes, and Enter goes full screen.
   * The preview follows the selection on its own, but only once the viewer stops on a
   * channel (`PREVIEW_SETTLE_MS`): this line allows one connection, and opening a
   * stream on every keypress while someone runs down the list would cost each of them
   * a connect-and-close the panel is slow to forget.
   *
   * Captured on the window ahead of the app-wide hotkeys, which give these keys to
   * volume, seeking and channel up/down -- right for the player, wrong here. Only while
   * the guide is actually on screen: it stays mounted under a full-screen player, where
   * the arrows belong to the player again.
   */
  const rootRef = useRef<HTMLDivElement>(null);
  const settle = useRef<number | undefined>(undefined);
  const previewSoon = useCallback(
    (ch: Channel) => {
      window.clearTimeout(settle.current);
      settle.current = window.setTimeout(() => {
        if (playingRef.current === ch.id) return;
        playingRef.current = ch.id;
        setPlayingId(ch.id);
        onPreview(ch);
      }, PREVIEW_SETTLE_MS);
    },
    [onPreview],
  );
  const playingRef = useRef<number | null>(playingId);
  playingRef.current = playingId;
  useEffect(() => () => window.clearTimeout(settle.current), []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey || isTyping(e.target)) return;
      if (!['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'PageUp', 'PageDown', 'Enter']
        .includes(e.key)) return;
      // Not under a full-screen player, a dialog, or the command palette.
      const root = rootRef.current;
      if (!root || root.offsetParent === null) return;
      if (document.querySelector('[aria-modal="true"]')) return;
      if (channels.length === 0) return;

      const at = Math.max(0, channels.findIndex((c) => c.id === previewChannel?.id));
      const page = Math.max(1, Math.floor((scrollRef.current?.clientHeight ?? 560) / ROW_H) - 1);
      const moveTo = (i: number) => {
        const next = channels[Math.min(channels.length - 1, Math.max(0, i))]!;
        setPreviewChannel(next);
        // Keep the time. With a programme selected, Up and Down land on whatever is on
        // the next channel at that moment, as a set-top box guide does -- dropping the
        // selection on every row change is what made it hard to see where you were.
        const at = selected ? Math.max(selected.prog.start, from) : null;
        const list = next.epgChannelId ? (guide.byEpgId[next.epgChannelId] ?? []) : [];
        const same = at != null ? list.find((p) => p.start <= at && p.stop > at) : undefined;
        setSelected(same ? { ch: next, prog: same } : null);
        rowVirt.scrollToIndex(channels.indexOf(next), { align: 'auto' });
        previewSoon(next);
      };

      const ch = channels[at]!;
      const stepProgramme = (dir: 1 | -1) => {
        const list = ch.epgChannelId ? (guide.byEpgId[ch.epgChannelId] ?? []) : [];
        if (list.length === 0) return;
        const sorted = [...list].sort((a, b) => a.start - b.start);
        const cur = selected?.ch.id === ch.id ? selected.prog : null;
        const base = cur
          ? sorted.findIndex((p) => p.id === cur.id)
          // From nothing selected, start on what is on now.
          : sorted.findIndex((p) => p.start <= now && p.stop > now) - (dir === 1 ? 1 : -1);
        const prog = sorted[Math.min(sorted.length - 1, Math.max(0, base + dir))];
        if (!prog) return;
        setSelected({ ch, prog });
        const el = scrollRef.current;
        if (el) {
          const x = xFor(Math.max(prog.start, from), from);
          const visibleW = el.clientWidth - CHANNEL_W;
          if (x < el.scrollLeft || x + 120 > el.scrollLeft + visibleW) {
            el.scrollLeft = Math.max(0, x - 24);
          }
        }
      };

      e.preventDefault();
      e.stopPropagation();
      switch (e.key) {
        case 'ArrowUp': moveTo(at - 1); break;
        case 'ArrowDown': moveTo(at + 1); break;
        case 'PageUp': moveTo(at - page); break;
        case 'PageDown': moveTo(at + page); break;
        case 'ArrowLeft': stepProgramme(-1); break;
        case 'ArrowRight': stepProgramme(1); break;
        case 'Enter':
          window.clearTimeout(settle.current);
          onTune(ch);
          break;
      }
    };
    window.addEventListener('keydown', onKey, { capture: true });
    return () => window.removeEventListener('keydown', onKey, { capture: true });
  }, [channels, previewChannel, selected, guide.byEpgId, now, from, rowVirt, previewSoon, onTune]);

  const timeSlots = useMemo(() => {
    const out: number[] = [];
    for (let t = from; t < to; t += HALF_HOUR) out.push(t);
    return out;
  }, [from, to]);

  return (
    <div
      ref={rootRef}
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
      {/*
        * `zIndex: 2`, one above the row below it, because the toolbar's dropdowns open
        * downwards *into* that row. Both were 1, and with equal z-index the later
        * sibling wins, so the Category list opened underneath the preview panel it sits
        * directly above: `elementFromPoint` on a row of that list returns
        * `guide-preview`, and the click never reached it. The guide's only category
        * filter could not be chosen at all. Found by the journey added for R-130, which
        * needed to choose one to prove the filter reaches the host.
        */}
      <div style={{ position: 'relative', zIndex: 2 }}>
      <GuideToolbar
        from={from}
        groups={groups ?? []}
        filters={filters}
        setFilters={setFilters}
        shown={channels.length}
        onNow={scrollToNow}
        onShift={(hours) => jumpTo(from + hours * 3600)}
        onPrimeTime={primeTime}
      />
      </div>

      <div
        style={{
          display: 'flex', flex: 1, minHeight: 0, position: 'relative', zIndex: 1,
          gap: 16, padding: '0 36px var(--navpill-space)',
        }}
      >
        {/* The grid, on the side the channel column is indexed from. Daylight puts it in
            one white rounded panel; the preview column beside it is not inside this, so
            the panel's paint never reaches the picture. */}
        <div
          className="dl-panel"
          style={{
            flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column',
            overflow: 'hidden', padding: '8px 0',
          }}
        >
          {loading && channels.length === 0 ? (
            <div style={{ padding: 'var(--sp-5)', display: 'grid', gap: 8 }}>
              {Array.from({ length: 10 }, (_, i) => <Skeleton key={i} h={ROW_H - 8} r={14} />)}
            </div>
          ) : channels.length === 0 ? (
            <EmptyState
              title={
                filters.withListings
                  ? 'No channels with listings here'
                  : 'No channels match these filters'
              }
            />
          ) : (
            <div
              ref={scrollRef}
              style={{ flex: 1, overflow: 'auto', position: 'relative' }}
            >
              <div
                style={{
                  width: CHANNEL_W + totalWidth, position: 'relative',
                  ['--guide-channel-w' as string]: `${CHANNEL_W}px`,
                }}
              >
                <TimeHeader
                  slots={timeSlots}
                  mark={
                    selected && selected.prog.stop > from && selected.prog.start < to
                      ? {
                          left: CHANNEL_W + xFor(Math.max(selected.prog.start, from), from),
                          right: CHANNEL_W + xFor(Math.min(selected.prog.stop, to), from),
                        }
                      : null
                  }
                />

                {/* Where you are, across: a faint band down the selected programme's
                    time, so the cursor reads as a crosshair with the row band below. */}
                {selected && selected.prog.stop > from && selected.prog.start < to && (
                  <div
                    aria-hidden
                    data-testid="guide-time-band"
                    style={{
                      position: 'absolute', top: 36, bottom: 0,
                      left: CHANNEL_W + xFor(Math.max(selected.prog.start, from), from),
                      width: xFor(Math.min(selected.prog.stop, to), from)
                        - xFor(Math.max(selected.prog.start, from), from),
                      background: 'color-mix(in srgb, var(--accent) 7%, transparent)',
                      zIndex: 3, pointerEvents: 'none',
                    }}
                  />
                )}

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
                    // No fallback to the row id.
                    //
                    // `byEpgId` is keyed by `epg_programmes.channel_id`, the XMLTV id the
                    // provider chose; `String(ch.id)` is the channel's SQLite rowid. The
                    // two are different namespaces, so the fallback asked for a key that
                    // does not describe the thing being looked up -- and usually got
                    // `[]`, which is the right answer for a channel with no mapping, by
                    // accident.
                    //
                    // Where it is not: an XMLTV id that happens to be a bare number
                    // equal to some channel's rowid. That channel, which has *no* guide
                    // mapping, would be drawn with another channel's programmes. None of
                    // this guide's 4,827 distinct ids is purely numeric, but the
                    // fallback ran for 4,874 of 6,699 channels, so the exposure was most
                    // of the grid (R-89).
                    const progs = ch.epgChannelId ? guide.byEpgId[ch.epgChannelId] ?? [] : [];
                    const current = previewChannel?.id === ch.id;
                    return (
                      <div
                        key={ch.id}
                        data-testid="guide-row"
                        data-current={current ? 'true' : undefined}
                        className={current ? 'guide-row-current' : undefined}
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
            width: PREVIEW_W, flexShrink: 0, gap: 16,
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
  from, groups, filters, setFilters, shown, onNow, onShift, onPrimeTime,
}: {
  from: number;
  groups: { name: string; count: number }[];
  filters: GuideFilters;
  setFilters: (f: GuideFilters) => void;
  /** How many channels the filters leave. */
  shown: number;
  onNow: () => void;
  onShift: (hours: number) => void;
  onPrimeTime: (offsetDays: number) => void;
}) {
  return (
    <div style={{ padding: '4px 36px 14px', display: 'grid', gap: 12 }}>
      {/* The top bar already says "Guide" on screen; the heading is for assistive
          technology, and its name stays "TV Guide". */}
      <h1 className="sr-only">TV Guide</h1>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
        <span className="dl-display" style={{ fontSize: 'var(--fs-2xl)', color: 'var(--text)' }}>
          {dayLabel(from)}
        </span>
        <div className="dl-seg" style={{ marginLeft: 'auto' }}>
          <button type="button" onClick={() => onShift(-24)}>−24h</button>
          <button type="button" onClick={() => onShift(-1)}>−1h</button>
          <button type="button" data-active="true" onClick={onNow}>
            <Icon name="clock" size={15} />
            Now
          </button>
          <button type="button" onClick={() => onShift(1)}>+1h</button>
          <button type="button" onClick={() => onShift(24)}>+24h</button>
        </div>
        <div className="dl-seg">
          <button type="button" onClick={() => onPrimeTime(0)}>
            <Icon name="moon" size={15} />
            Tonight 8pm
          </button>
        </div>
      </div>

      {/* Groups as chips rather than a dropdown: browsing is choosing between them, and
          a list you have to open to see is a list nobody reads. Each chip adds its
          group to the choice or takes it out, so several can be on at once; the
          Filters panel has the rest -- hiding groups instead, searching them, and
          the channel filters. */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, minWidth: 0 }}>
        <GuideFiltersButton
          groups={groups}
          filters={filters}
          setFilters={setFilters}
          shown={shown}
        />
        <div className="guide-chips" role="group" aria-label="Category">
          <button
            type="button"
            className="dl-filter"
            aria-pressed={filters.groups.length === 0}
            onClick={() => setFilters({ ...filters, groups: [] })}
          >
            All channels
          </button>
          {groups.map((g) => (
            <button
              key={g.name}
              type="button"
              className="dl-filter"
              data-testid="guide-group"
              data-mode={filters.groupMode}
              aria-pressed={filters.groups.includes(g.name)}
              onClick={() => setFilters(toggleGroup(filters, g.name))}
            >
              {g.name}
              <span className="guide-chip-count">{g.count}</span>
            </button>
          ))}
        </div>
        <div className="guide-listings">
          <button
            type="button"
            role="switch"
            className="dl-switch"
            aria-checked={filters.withListings}
            aria-label="Only channels with listings"
            onClick={() => setFilters({ ...filters, withListings: !filters.withListings })}
          />
          <span aria-hidden>With listings</span>
        </div>
      </div>
    </div>
  );
}

/**
 * The hour labels above the grid.
 *
 * `from` used to be a prop, used by nothing, kept alive by a
 * `<span style={{ display: 'none' }}>{from}</span>` on the last line — a Unix timestamp
 * rendered into the sticky header on every guide paint so that the parameter counted as
 * used. Invisible, harmless, and completely unguessable for anyone wondering why a
 * timestamp is in the markup. The honest fix is to not take the prop (R-131).
 */
function TimeHeader({
  slots, mark,
}: {
  slots: number[];
  /** The selected programme's span, in pixels from the left of the grid. */
  mark: { left: number; right: number } | null;
}) {
  // Bound to the 12/24-hour preference, shadowing the pure import (R-136).
  const clockTime = useClockTime();
  return (
    <div
      style={{
        position: 'sticky', top: 0, zIndex: 6, display: 'flex', height: 36,
        background: 'var(--surface)',
      }}
    >
      {mark && (
        <div
          aria-hidden
          data-testid="guide-time-mark"
          style={{
            position: 'absolute', bottom: 0, height: 4, borderRadius: 2,
            left: mark.left + 2, width: Math.max(4, mark.right - mark.left - 4),
            background: 'var(--accent)', pointerEvents: 'none',
          }}
        />
      )}
      <div
        style={{
          width: CHANNEL_W, flexShrink: 0, position: 'sticky', left: 0, zIndex: 7,
          background: 'var(--surface)',
          display: 'flex', alignItems: 'center', padding: '0 16px',
          fontSize: 'var(--fs-xs)', color: 'var(--text-faint)', fontWeight: 800,
          letterSpacing: '0.06em',
        }}
      >
        CHANNEL
      </div>
      {slots.map((t) => (
        <div
          key={t}
          style={{
            width: SLOT_W, flexShrink: 0,
            display: 'flex', alignItems: 'center', padding: '0 var(--sp-2)',
            fontSize: 'var(--fs-md)', fontWeight: 600, color: 'var(--text-muted)',
            fontVariantNumeric: 'tabular-nums',
          }}
        >
          {clockTime(t)}
        </div>
      ))}
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
        display: 'flex', alignItems: 'center', gap: 10,
        padding: '0 12px 0 8px', textAlign: 'left', cursor: 'pointer',
        // The row you are on: accent-tinted with a bar down its edge. It was the hover
        // grey, a step away from every other row, and the cursor could not be found.
        background: active
          ? 'color-mix(in srgb, var(--accent) 22%, var(--surface))'
          : 'var(--surface)',
        boxShadow: active ? 'inset 4px 0 0 var(--accent)' : 'none',
        border: 'none', borderRadius: '0 var(--r-md) var(--r-md) 0',
        color: active ? 'var(--text)' : 'inherit',
      }}
      data-current={active ? 'true' : undefined}
    >
      <span
        style={{
          width: 30, textAlign: 'right', fontSize: 'var(--fs-xs)',
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
      <LogoTile name={channel.name} number={channel.number} logo={channel.logo} size={40} />
      <span
        style={{
          fontSize: 'var(--fs-sm)', fontWeight: 800, lineHeight: 1.2, overflow: 'hidden',
          display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical',
          wordBreak: 'break-word',
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
  // Bound to the 12/24-hour preference, shadowing the pure import (R-136).
  const clockTime = useClockTime();
  // Only render what overlaps the window — the horizontal half of the virtualization.
  const visible = programmes.filter((p) => p.start < to && p.stop > from);

  return (
    <div
      style={{
        position: 'relative', flex: 1, height: ROW_H,
      }}
    >
      {/* A channel with no guide for this window used to be an empty strip, and a
          screenful of them reads as the guide having failed to load. On a real
          subscription this is most of it: EPG coverage measured 42.8% on the panel
          this was built against, so whichever end of the list you open is likely to
          be blank. Saying so is the difference between "nothing is on" and "nothing
          is known". */}
      {visible.length === 0 && <FromTheName channel={channel} />}

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
            aria-pressed={isSelected}
            onClick={() => onSelect(p)}
            onDoubleClick={onTune}
            title={`${p.title} · ${clockTime(p.start)}–${clockTime(p.stop)}`}
            style={{
              position: 'absolute', left: left + 2, width: width - 4, top: 4,
              height: ROW_H - 8,
              display: 'flex', flexDirection: 'column', justifyContent: 'center',
              alignItems: 'flex-start', gap: 0, overflow: 'hidden',
              padding: '0 12px', textAlign: 'left', cursor: 'pointer', lineHeight: 1.3,
              // Daylight: the airing slot is white, everything else a sunk pill, and the
              // selected programme is the accent.
              // What is on now is what somebody browsing is looking for, so it is the
              // one that stands out: accent-tinted, with its progress along the foot.
              background: isSelected
                ? 'var(--accent)'
                : airing
                  ? 'color-mix(in srgb, var(--accent) 16%, var(--surface-sunk))'
                  : 'var(--surface-sunk)',
              border: 'none',
              boxShadow: isSelected
                // A ring that stands off the pill, and a glow: the selected programme is
                // the cursor, and it has to be findable at a glance across the grid.
                ? '0 0 0 2px var(--surface), 0 0 0 4px var(--accent), '
                  + '0 6px 20px color-mix(in srgb, var(--accent) 45%, transparent)'
                : airing
                  ? 'inset 0 0 0 1.5px color-mix(in srgb, var(--accent) 45%, transparent)'
                  : 'none',
              zIndex: isSelected ? 2 : undefined,
              borderRadius: 'var(--r-md)',
              color: isSelected ? 'var(--accent-text)' : 'var(--text)',
              opacity: p.stop < now ? 0.45 : 1,
            }}
          >
            <span
              style={{
                fontSize: 'var(--fs-sm)', fontWeight: 800, whiteSpace: 'nowrap',
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
                  fontSize: 'var(--fs-xs)', fontWeight: 700,
                  color: isSelected ? 'var(--accent-text)' : 'var(--text-faint)',
                  opacity: isSelected ? 0.85 : 1,
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
                  position: 'absolute', left: 0, bottom: 0, height: 3,
                  width: `${((now - p.start) / (p.stop - p.start)) * 100}%`,
                  background: isSelected ? 'var(--accent-text)' : 'var(--accent)',
                }}
              />
            )}
          </button>
        );
      })}
    </div>
  );
}

/**
 * The row for a channel the guide knows nothing about, read from its name.
 *
 * It was a dashed strip saying "No guide data for this channel", and on a real line that
 * was more than half the guide. Most of those channels say what is on in their name --
 * a fixture, a 24/7 loop -- so the row says that, marked as coming from the name so it
 * is never mistaken for a schedule. Not a button: there is no programme to select, set a
 * reminder on or record.
 */
function FromTheName({ channel }: { channel: Channel }) {
  const g = useMemo(() => nameGuide(channel.name), [channel.name]);
  const clockTime = useClockTime();
  const when =
    g.kind === 'event'
      ? g.at != null
        ? `${dayLabel(g.at)} ${clockTime(g.at)}`
        : g.detail
      : g.kind === 'loop'
        ? 'All day'
        : null;
  return (
    <div
      data-testid="guide-from-name"
      data-kind={g.kind}
      className="guide-from-name"
      title={`From the channel name: ${channel.name}`}
    >
      <div className="guide-from-name-body">
        <Icon name={g.kind === 'event' ? 'star' : g.kind === 'loop' ? 'refresh' : 'tv'} size={14} />
        <span className="guide-from-name-title">{g.title}</span>
        {when && <span className="guide-from-name-when">{when}</span>}
        <span className="guide-from-name-tag">from channel name</span>
      </div>
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
      data-testid="guide-preview-panel"
      style={{
        // **No background on the panel.** A parent's background is painted across the
        // whole of it, the box included, and the picture is *behind* the page -- so a
        // panel colour here covered the video completely. It was only ever seen because
        // the surface sat a little out of line with the box and showed round the edge
        // of the panel; once the two lined up exactly, the preview played unseen
        // ("the guide preview still plays but I can't see it in the box"). The colour
        // is on the bar under the picture instead, and the box paints its own black
        // while nothing is playing.
        borderRadius: 'var(--r-xl)', overflow: 'hidden',
        boxShadow: 'var(--shadow-1)',
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
          background: live ? 'transparent' : 'var(--player-bg)',
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
          padding: '12px 16px', minHeight: 64,
          background: 'var(--bg-elevated)',
        }}
      >
        <div style={{ flex: 1, minWidth: 0 }}>
          <div
            style={{
              fontWeight: 800, fontSize: 'var(--fs-md)',
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
  // Bound to the 12/24-hour preference, shadowing the pure import (R-136).
  const clockTime = useClockTime();
  const [catchupError, setCatchupError] = useState<string | null>(null);
  if (!selected) {
    return (
      <div
        className="dl-panel"
        style={{
          padding: 20, color: 'var(--text-muted)',
          fontSize: 'var(--fs-sm)', lineHeight: 1.6, fontWeight: 600,
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
    <div
      className="dl-panel"
      style={{ padding: 20, overflowY: 'auto', flex: 1, minHeight: 0 }}
    >
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginBottom: 'var(--sp-3)' }}>
        {prog.isNew && <Badge tone="new">New</Badge>}
        {prog.isLive && <Badge tone="live">Live</Badge>}
        {prog.isPremiere && <Badge tone="accent">Premiere</Badge>}
        {prog.rating && <Badge tone="outline">{prog.rating}</Badge>}
        {ch.quality && <Badge tone="neutral">{ch.quality}</Badge>}
      </div>

      <h2
        className="dl-display"
        style={{ margin: '0 0 6px', fontSize: 'var(--fs-2xl)', color: 'var(--text)' }}
      >
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
