/**
 * Keep the video surface sitting inside a box on the page.
 *
 * The picture is not in the DOM. On Windows it is a child window of the app's own
 * window, a sibling of the WebView and *behind* it, so "video in this box" is really two
 * separate facts that have to be kept in step:
 *
 *   1. the host has put the surface at the box's rectangle, and
 *   2. nothing in the page is painted over that rectangle.
 *
 * This hook owns (1). It measures the element, converts to the physical pixels the
 * backend places surfaces in, and tells the host. (2) is `SurfaceHole`, which takes the
 * rectangle the host reports back — not the one we asked for, because the host clamps it.
 *
 * Why it re-measures rather than computing the box once: the guide's preview moves for
 * reasons that have nothing to do with the guide. The window resizes, the pill nav
 * reflows, a scrollbar appears when a group filter changes the row count, the display's
 * scale factor changes when the window is dragged to another monitor. A surface left
 * behind by any of those is a picture in the wrong place, and unlike a misplaced div it
 * does not simply look wrong — it covers whatever is actually there.
 */
import { useEffect, useRef, useState } from 'react';
import type { PipView } from '@shared/ipc';
import { invoke } from '@/ipc';
import { report } from '@/lib/errors';

/** A rectangle in physical pixels, which is what the host wants. */
interface PhysicalRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

function measure(el: HTMLElement): PhysicalRect | null {
  const box = el.getBoundingClientRect();
  // A box that has not been laid out yet, or is mid-collapse. Asking the host to put a
  // surface in it would be asking for a zero-sized child window.
  if (box.width < 1 || box.height < 1) return null;
  const dpr = window.devicePixelRatio || 1;
  return {
    x: Math.round(box.left * dpr),
    y: Math.round(box.top * dpr),
    width: Math.round(box.width * dpr),
    height: Math.round(box.height * dpr),
  };
}

const same = (a: PhysicalRect | null, b: PhysicalRect | null) =>
  a === b ||
  (!!a &&
    !!b &&
    a.x === b.x &&
    a.y === b.y &&
    a.width === b.width &&
    a.height === b.height);

export function useInlaidPicture(
  ref: React.RefObject<HTMLElement | null>,
  active: boolean,
): PipView['inlay'] {
  const [inlay, setInlay] = useState<PipView['inlay']>(null);
  /** The last rectangle actually sent, so an identical one is not sent again. */
  const sent = useRef<PhysicalRect | null>(null);

  useEffect(() => {
    const el = ref.current;
    if (!active || !el) {
      // Only clear if we are the ones who set it. Clearing unconditionally on every
      // render where `active` is false would fight whatever else is using the surface.
      if (sent.current) {
        sent.current = null;
        setInlay(null);
        invoke('preview.clear')
          .then((v) => setInlay(v.inlay))
          .catch(report('Could not give the picture back'));
      }
      return;
    }

    let alive = true;
    const push = () => {
      const rect = measure(el);
      if (!rect || same(rect, sent.current)) return;
      sent.current = rect;
      invoke('preview.place', rect)
        .then((v) => {
          // The host's rectangle, not ours: it trims anything outside the window, and
          // the frame has to be drawn where the picture actually went.
          if (alive) setInlay(v.inlay);
        })
        .catch((e) => {
          // A refusal — a mosaic is open, say — must not leave us believing we have the
          // surface, or the page would draw a hole onto the desktop.
          sent.current = null;
          if (alive) setInlay(null);
          report('Could not show the preview here')(e);
        });
    };

    push();

    // `ResizeObserver` catches the element changing size; it does not fire when the
    // element *moves* without resizing, which is what a scroll or a reflow above it
    // does. So the window's own events are watched too.
    const observer = new ResizeObserver(push);
    observer.observe(el);
    window.addEventListener('resize', push);
    window.addEventListener('scroll', push, true);

    return () => {
      alive = false;
      observer.disconnect();
      window.removeEventListener('resize', push);
      window.removeEventListener('scroll', push, true);
    };
  }, [active, ref]);

  // Give the surface back when the page goes away, which is the case that matters most:
  // leaving the guide with the picture still pinned to a box that no longer exists.
  useEffect(
    () => () => {
      if (sent.current) {
        sent.current = null;
        void invoke('preview.clear').catch(() => {});
      }
    },
    [],
  );

  return inlay;
}
