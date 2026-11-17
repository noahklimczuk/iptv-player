/**
 * The scrub area of the dock (C8–C10).
 *
 * `Timeline` picks what to draw from `model.timeline`: a seek bar with elapsed / total
 * for anything on demand, a window from the oldest moment held to live for buffered
 * live TV, and the programme's progress beside a LIVE badge when nothing is held.
 */
import { useEffect, useRef, useState } from 'react';
import type { PlayerState, SeekPreview } from '@shared/ipc';
import type { NowNext } from '@/features/live/useNowNext';
import { invoke } from '@/ipc';
import { duration } from '@/lib/format';
import { useClockTime } from '@/hooks/useClockTime';
import * as control from './playerControl';
import { LIVE_EDGE_SECS, timeline } from './model';

/* ── The seek preview ───────────────────────────────────────────────────── */

/**
 * The host's picture and words for a moment on the bar, while one is being pointed at.
 *
 * One request at a time. A pointer moves faster than frames can be found, so while one
 * is out the latest position is remembered and asked for when it comes back. A frame
 * the host is still looking for (`pending`) is asked for again shortly.
 */
export function useSeekPreview(at: number | null): SeekPreview | null {
  const [answer, setAnswer] = useState<SeekPreview | null>(null);
  const wanted = useRef<number | null>(at);
  const busy = useRef(false);
  const again = useRef<number | undefined>(undefined);
  wanted.current = at;

  useEffect(() => {
    window.clearTimeout(again.current);
    if (at == null) {
      setAnswer(null);
      return;
    }
    const ask = () => {
      const secs = wanted.current;
      if (secs == null || busy.current) return;
      busy.current = true;
      invoke('player.seekPreview', { positionSecs: secs })
        .then((r) => {
          busy.current = false;
          const now = wanted.current;
          if (now == null) return;
          setAnswer(r);
          if (Math.abs(now - secs) > 0.5) ask();
          else if (r.pending) again.current = window.setTimeout(ask, 150);
        })
        .catch(() => { busy.current = false; });
    };
    ask();
  }, [at]);

  useEffect(() => () => window.clearTimeout(again.current), []);
  return at == null ? null : answer;
}

const BUBBLE_W = 176;

function SeekBubble({
  x, width, time, lines, preview,
}: {
  x: number;
  width: number;
  time: string;
  lines: (string | null | undefined)[];
  preview: SeekPreview | null;
}) {
  // Kept inside the bar, so the bubble at either end is not cut off by the window.
  const half = BUBBLE_W / 2;
  const left = width > BUBBLE_W ? Math.min(Math.max(x, half), width - half) : width / 2;
  const image = preview?.image ?? null;
  const words = lines.filter((l): l is string => !!l && l.trim() !== '');
  return (
    <div
      data-testid="seek-preview"
      aria-hidden
      className="pl-bubble"
      style={{ left, width: image ? BUBBLE_W : 'auto', padding: image ? 8 : '6px 10px' }}
    >
      {image && (
        <img data-testid="seek-preview-frame" src={image} alt="" width={160} height={90} />
      )}
      <span data-testid="seek-preview-time" className="pl-bubble-time">{time}</span>
      {words.map((w) => <span key={w} className="pl-bubble-line">{w}</span>)}
    </div>
  );
}

/* ── The bar ────────────────────────────────────────────────────────────── */

/** The host's position is believed again once it is this close to where a seek went... */
const SETTLED_SECS = 2;
/** ...or after this long, because a seek can be refused. */
const SETTLE_LIMIT_MS = 4000;

/**
 * A range that seeks once, when it is let go (C8).
 *
 * While a hand is on it, the handle shows where the viewer is dragging, not where the
 * player is: the host's position only moves on its heartbeat, and binding the handle to
 * it made the handle snap back under the pointer. Nothing is sent until the drag ends,
 * because every value passed through on the way would be a real seek. After the seek
 * the handle stays where it was dropped until the host agrees, or until it is clear the
 * host never will.
 */
function ScrubBar({
  min, max, value, buffered, label, describe,
}: {
  min: number;
  max: number;
  value: number;
  /** How far ahead of `min` is already downloaded, for the lighter part of the track. */
  buffered?: number;
  label: string;
  describe: (secs: number, preview: SeekPreview | null) => { time: string; lines: (string | null)[] };
}) {
  /** Where the handle is being held, or was dropped and not yet reached. */
  const [draft, setDraftState] = useState<number | null>(null);
  const draftNow = useRef<number | null>(null);
  const setDraft = (v: number | null) => { draftNow.current = v; setDraftState(v); };
  /** A pointer or key is on the bar: changes are the viewer's, and commit on release. */
  const handling = useRef(false);
  /** A seek was sent and the handle waits for the host; when it was sent. */
  const sentAt = useRef(0);
  const [hover, setHover] = useState<number | null>(null);
  const [held, setHeld] = useState(false);
  const bar = useRef<HTMLDivElement>(null);

  const span = Math.max(0, max - min);
  const at = (clientX: number) => {
    const el = bar.current;
    if (!el || span <= 0) return null;
    const r = el.getBoundingClientRect();
    return min + Math.min(1, Math.max(0, (clientX - r.left) / Math.max(1, r.width))) * span;
  };

  // Release the handle once the host has caught up, or once it is clear it will not. A
  // timer owns the limit, so it fires even if nothing else changes (a refused seek on a
  // paused stream leaves the position still).
  useEffect(() => {
    if (draft === null || sentAt.current === 0) return;
    const release = () => { sentAt.current = 0; setDraft(null); };
    if (Math.abs(value - draft) <= SETTLED_SECS) {
      release();
      return;
    }
    const t = window.setTimeout(release, Math.max(0, SETTLE_LIMIT_MS - (Date.now() - sentAt.current)));
    return () => window.clearTimeout(t);
  }, [value, draft]);

  const commit = () => {
    setHeld(false);
    if (!handling.current) return;
    handling.current = false;
    const to = draftNow.current;
    if (to === null) return;
    sentAt.current = Date.now();
    control.seekTo(to);
  };
  const begin = () => {
    handling.current = true;
    sentAt.current = 0;
  };

  const shown = draft ?? value;
  const fill = span > 0 ? ((shown - min) / span) * 100 : 0;
  const buf = span > 0 && buffered != null ? Math.max(fill, ((buffered - min) / span) * 100) : fill;
  const pointAt = held && draft !== null ? draft : hover;
  const preview = useSeekPreview(pointAt);
  const width = bar.current?.offsetWidth ?? 0;
  const words = pointAt !== null ? describe(pointAt, preview) : null;

  return (
    <div
      ref={bar}
      className="pl-scrub"
      onPointerMove={(e) => setHover(at(e.clientX))}
      onPointerLeave={() => setHover(null)}
    >
      {words && (
        <SeekBubble
          x={span > 0 && pointAt !== null ? ((pointAt - min) / span) * width : 0}
          width={width}
          time={words.time}
          lines={words.lines}
          preview={preview}
        />
      )}
      <input
        type="range"
        className="dl-range"
        aria-label={label}
        aria-valuetext={describe(shown, null).time}
        min={min}
        max={Math.max(min + 1, max)}
        step="any"
        value={shown}
        onPointerDown={() => { begin(); setHeld(true); }}
        onKeyDown={(e) => { if (/^(Home|End|Page)/.test(e.key)) begin(); }}
        onChange={(e) => {
          const v = Number(e.target.value);
          if (handling.current) {
            setDraft(v);
            return;
          }
          // No pointer and no key: an assistive technology setting the value. That is
          // a whole move in one go, so it seeks now. A change while a sent seek is
          // still settling is the browser echoing the drag that sent it, and is not.
          if (sentAt.current === 0) {
            handling.current = true;
            setDraft(v);
            commit();
          }
        }}
        onPointerUp={commit}
        onLostPointerCapture={commit}
        onKeyUp={commit}
        onBlur={commit}
        style={{ ['--fill' as string]: `${fill}%`, ['--buf' as string]: `${buf}%` }}
      />
    </div>
  );
}

/* ── The timelines ──────────────────────────────────────────────────────── */

export function Timeline({ player, guide }: { player: PlayerState; guide: NowNext | null }) {
  const clock = useClockTime();
  const t = timeline(player);

  if (t.kind === 'vod') {
    return (
      <div className="pl-timeline">
        <div className="pl-readout">
          <span>{duration(t.position)}</span>
          <span className="pl-muted">{duration(t.duration)}</span>
        </div>
        <ScrubBar
          min={0}
          max={t.duration}
          value={t.position}
          buffered={t.buffered}
          label="Seek"
          describe={(secs, p) => ({ time: duration(secs), lines: [p?.label ?? null] })}
        />
      </div>
    );
  }

  if (t.kind === 'timeshift') {
    const held = t.live - t.start;
    const atLive = t.behind === 0;
    return (
      <div className="pl-timeline">
        <div className="pl-readout">
          <span aria-label="Position relative to live" className="pl-live-text">
            {atLive && <span aria-hidden className="pl-dot" />}
            {atLive ? 'Live' : `−${duration(t.behind)}`}
          </span>
          <span className="pl-muted pl-small">
            {held > 0 ? `${duration(held)} buffered` : 'buffering…'}
          </span>
        </div>
        <div className="pl-edge-wrap">
          <ScrubBar
            min={Math.floor(t.start)}
            max={Math.ceil(t.live)}
            value={t.position}
            label="Timeshift"
            describe={(secs, p) => {
              const back = Math.max(0, t.live - secs);
              return {
                time: back <= LIVE_EDGE_SECS ? 'Live' : `−${duration(back)}`,
                lines: [
                  p?.clockUnix != null ? clock(p.clockUnix) : null,
                  p?.programme ?? null,
                  p?.label ?? null,
                ],
              };
            }}
          />
          <span aria-hidden title="Live edge" className="pl-edge" />
        </div>
      </div>
    );
  }

  if (t.kind === 'live') {
    const now = Math.floor(Date.now() / 1000);
    const prog = guide?.now ?? null;
    const pct = prog ? ((now - prog.start) / Math.max(1, prog.stop - prog.start)) * 100 : 100;
    return (
      <div className="pl-timeline">
        <div className="pl-readout">
          <span>{prog ? clock(prog.start) : ''}</span>
          <span className="pl-live-text">
            <span aria-hidden className="pl-dot" />
            <span>Live · {clock(now)}</span>
          </span>
          <span className="pl-muted">{prog ? clock(prog.stop) : ''}</span>
        </div>
        <div
          role="progressbar"
          aria-label="Programme progress"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(Math.max(0, Math.min(100, pct)))}
          className="pl-progress"
        >
          <div style={{ width: `${Math.max(0, Math.min(100, pct))}%` }} />
        </div>
      </div>
    );
  }

  // On demand, with no duration known: where it is, and nothing to drag.
  return (
    <div className="pl-timeline">
      <div className="pl-readout"><span>{duration(t.position)}</span></div>
      <div aria-hidden className="pl-progress" />
    </div>
  );
}
