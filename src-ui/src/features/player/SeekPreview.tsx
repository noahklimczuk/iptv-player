/**
 * The seek preview: the bubble over the scrub bar (asked for as "add a seek preview to
 * the player", settled as frames and time).
 *
 * The host decides what can be shown (`player.seekPreview`, `seekpreview.rs`): a
 * picture of what has already played or of any moment of a recording, the chapter or
 * intro/credits marker, and on live TV the time and programme. A film's unplayed
 * future has no picture -- that would take a second stream, and the line allows one --
 * so there the bubble is just the time and what it knows about the moment.
 */
import { useEffect, useRef, useState } from 'react';
import type { SeekPreview } from '@shared/ipc';
import { invoke } from '@/ipc';

/**
 * The preview for `secs`, or `null` while nothing is hovered.
 *
 * One request in flight at a time, always for the latest position: the pointer moves
 * far faster than a frame can be decoded from a recording, and answering every
 * position it crossed would arrive long after it had moved on. A `pending` answer --
 * the host is decoding that frame now -- is asked again shortly.
 */
export function useSeekPreview(secs: number | null): SeekPreview | null {
  const [data, setData] = useState<SeekPreview | null>(null);
  const want = useRef<number | null>(secs);
  const inFlight = useRef(false);
  const retry = useRef(0);
  want.current = secs;

  useEffect(() => {
    if (secs == null) {
      window.clearTimeout(retry.current);
      setData(null);
      return;
    }
    const ask = () => {
      const at = want.current;
      if (at == null || inFlight.current) return;
      inFlight.current = true;
      invoke('player.seekPreview', { positionSecs: at })
        .then((r) => {
          inFlight.current = false;
          if (want.current == null) return;
          setData(r);
          if (Math.abs(want.current - at) > 0.5) ask();
          else if (r.pending) {
            window.clearTimeout(retry.current);
            retry.current = window.setTimeout(ask, 150);
          }
        })
        .catch(() => {
          // A bubble with only the time in it is still a bubble; nothing to report.
          inFlight.current = false;
        });
    };
    ask();
  }, [secs]);

  useEffect(() => () => window.clearTimeout(retry.current), []);
  return secs == null ? null : data;
}

const BUBBLE_W = 176;

/**
 * The bubble itself, centred over `x` (pixels from the left of the bar) and kept inside
 * the bar's width so it never runs off the dock.
 */
export function SeekBubble({
  x, width, time, lines, preview,
}: {
  x: number;
  width: number;
  time: string;
  lines: (string | null | undefined)[];
  preview: SeekPreview | null;
}) {
  const half = BUBBLE_W / 2;
  const left = width > BUBBLE_W ? Math.min(Math.max(x, half), width - half) : width / 2;
  const image = preview?.image ?? null;
  const shown = lines.filter((l): l is string => !!l && l.trim() !== '');
  return (
    <div
      data-testid="seek-preview"
      aria-hidden
      style={{
        position: 'absolute', bottom: 'calc(100% + 14px)', left,
        transform: 'translateX(-50%)', width: image ? BUBBLE_W : 'auto',
        minWidth: 64, maxWidth: BUBBLE_W, padding: image ? 8 : '6px 10px',
        borderRadius: 16, background: 'var(--dock-bg)', color: 'var(--dock-text)',
        boxShadow: 'var(--shadow-dock)', pointerEvents: 'none', zIndex: 3,
        display: 'grid', gap: 6, justifyItems: 'center', textAlign: 'center',
      }}
    >
      {image && (
        <img
          data-testid="seek-preview-frame"
          src={image}
          alt=""
          width={160}
          height={90}
          style={{
            width: 160, height: 90, objectFit: 'cover', borderRadius: 10,
            display: 'block', background: '#000',
          }}
        />
      )}
      <span
        data-testid="seek-preview-time"
        style={{ fontWeight: 800, fontSize: 'var(--fs-sm)', fontVariantNumeric: 'tabular-nums' }}
      >
        {time}
      </span>
      {shown.map((line) => (
        <span
          key={line}
          style={{
            fontSize: 'var(--fs-xs)', fontWeight: 700, color: 'var(--dock-muted)',
            maxWidth: BUBBLE_W - 16, overflow: 'hidden', textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {line}
        </span>
      ))}
    </div>
  );
}
