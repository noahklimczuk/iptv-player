/**
 * The lens in Liquid Glass: the page behind a glass surface bends at its edges.
 *
 * Asked for: "make the liquid glass identical to the iOS one". Blur and tint are what
 * every frosted surface has; what makes Apple's glass read as a piece of glass rather
 * than a pane of frost is that it *refracts*. Near the rim the content behind is pulled
 * inward, as through the curved edge of a lens, and the middle stays clear.
 *
 * The browser has no lens, but Chromium (which is what WebView2 is) accepts an SVG
 * filter in `backdrop-filter`. So each surface gets a displacement map the size and
 * shape of itself -- neutral grey in the middle, pushing inward along the rim -- and
 * `styles/glass.css` puts it in front of the blur through `--glass-lens`. Anywhere that
 * cannot take it (no SVG backdrop filters, no canvas) the variable stays unset and the
 * surface is the same glass without the bend.
 */
import { useEffect, useRef, type RefObject } from 'react';

const SVG_NS = 'http://www.w3.org/2000/svg';
let defsHost: SVGSVGElement | null = null;
let nextId = 0;

function defs(): SVGSVGElement {
  if (defsHost && defsHost.isConnected) return defsHost;
  const svg = document.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('width', '0');
  svg.setAttribute('height', '0');
  svg.style.position = 'fixed';
  svg.style.pointerEvents = 'none';
  document.body.appendChild(svg);
  defsHost = svg;
  return svg;
}

let supported: boolean | null = null;
/** Whether this engine takes an SVG filter in `backdrop-filter`, and can draw a map. */
export function lensSupported(): boolean {
  if (supported !== null) return supported;
  try {
    supported =
      typeof CSS !== 'undefined' &&
      CSS.supports('backdrop-filter', 'url(#x) blur(1px)') &&
      !!document.createElement('canvas').getContext('2d');
  } catch {
    supported = false;
  }
  return supported;
}

/**
 * A displacement map for a rounded rectangle of `w` x `h` with corner radius `r`.
 *
 * Red and green carry the horizontal and vertical push (128 is none). Inside the band
 * `bezel` pixels deep along the rim, each pixel points inward along the edge's normal,
 * strongest at the rim and easing to nothing with the square of the depth -- the
 * profile of a rounded lens edge, which is what gives the bend its soft roll instead of
 * a step. Only the band is computed; the middle is filled flat.
 */
export function lensMap(w: number, h: number, r: number, bezel: number): ImageData | null {
  const canvas = document.createElement('canvas');
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext('2d');
  if (!ctx) return null;
  const img = ctx.createImageData(w, h);
  const px = new Uint32Array(img.data.buffer);
  // Little-endian RGBA: alpha 255, blue/green/red 128.
  px.fill(0xff808080);
  const hw = w / 2;
  const hh = h / 2;
  // Signed distance to the rounded rectangle's edge, negative inside.
  const sdf = (x: number, y: number) => {
    const qx = Math.abs(x - hw) - (hw - r);
    const qy = Math.abs(y - hh) - (hh - r);
    return Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - r;
  };
  const band = Math.max(r, bezel);
  for (let j = 0; j < h; j += 1) {
    const rowInBand = j < bezel || j >= h - bezel;
    for (let i = 0; i < w; i += 1) {
      if (!rowInBand && i >= band && i < w - band) {
        i = w - band - 1;
        continue;
      }
      const x = i + 0.5;
      const y = j + 0.5;
      const depth = -sdf(x, y);
      if (depth <= 0 || depth >= bezel) continue;
      const gx = sdf(x + 1, y) - sdf(x - 1, y);
      const gy = sdf(x, y + 1) - sdf(x, y - 1);
      const len = Math.hypot(gx, gy) || 1;
      const t = 1 - depth / bezel;
      const m = t * t * 127;
      const red = Math.round(128 - (gx / len) * m);
      const green = Math.round(128 - (gy / len) * m);
      px[j * w + i] = (0xff << 24) | (128 << 16) | (green << 8) | red;
    }
  }
  return img;
}

function toDataUrl(img: ImageData): string {
  const canvas = document.createElement('canvas');
  canvas.width = img.width;
  canvas.height = img.height;
  canvas.getContext('2d')!.putImageData(img, 0, 0);
  return canvas.toDataURL();
}

/**
 * Give an element the lens, kept to its size, and the light that follows the pointer.
 *
 * The map is rebuilt when the element changes size (a palette's results filtering, a
 * window resized), at most once a frame. The pointer sets `--lg-x`/`--lg-y`, which the
 * material uses for the highlight that tracks a finger or cursor across the glass.
 */
export function useLiquidGlass<T extends HTMLElement>(
  opts: {
    bezel?: number;
    /** An element the caller already holds a ref to. */
    ref?: RefObject<T>;
    /** For an element rendered conditionally: when it is there. */
    shown?: boolean;
  } = {},
) {
  const own = useRef<T>(null);
  const ref = opts.ref ?? own;
  const bezelOpt = opts.bezel;
  const shown = opts.shown ?? true;
  useEffect(() => {
    const el = ref.current;
    if (!el) return;

    const move = (e: PointerEvent) => {
      const box = el.getBoundingClientRect();
      el.style.setProperty('--lg-x', `${e.clientX - box.left}px`);
      el.style.setProperty('--lg-y', `${e.clientY - box.top}px`);
    };
    const leave = () => {
      el.style.removeProperty('--lg-x');
      el.style.removeProperty('--lg-y');
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerleave', leave);

    if (!lensSupported() || typeof ResizeObserver === 'undefined') {
      return () => {
        el.removeEventListener('pointermove', move);
        el.removeEventListener('pointerleave', leave);
      };
    }

    const id = `aurora-lens-${(nextId += 1)}`;
    const filter = document.createElementNS(SVG_NS, 'filter');
    filter.setAttribute('id', id);
    filter.setAttribute('x', '0');
    filter.setAttribute('y', '0');
    filter.setAttribute('width', '100%');
    filter.setAttribute('height', '100%');
    filter.setAttribute('primitiveUnits', 'userSpaceOnUse');
    filter.setAttribute('color-interpolation-filters', 'sRGB');
    const image = document.createElementNS(SVG_NS, 'feImage');
    image.setAttribute('x', '0');
    image.setAttribute('y', '0');
    image.setAttribute('result', 'map');
    image.setAttribute('preserveAspectRatio', 'none');
    const displace = document.createElementNS(SVG_NS, 'feDisplacementMap');
    displace.setAttribute('in', 'SourceGraphic');
    displace.setAttribute('in2', 'map');
    displace.setAttribute('xChannelSelector', 'R');
    displace.setAttribute('yChannelSelector', 'G');
    filter.append(image, displace);
    defs().appendChild(filter);

    let drawn = '';
    let frame = 0;
    const draw = () => {
      frame = 0;
      const w = Math.round(el.offsetWidth);
      const h = Math.round(el.offsetHeight);
      if (w < 8 || h < 8) return;
      const radius = Math.min(parseFloat(getComputedStyle(el).borderTopLeftRadius) || 0, w / 2, h / 2);
      const bezel = Math.max(4, Math.min(bezelOpt ?? 18, h / 3, w / 3));
      const key = `${w}x${h}r${radius}b${bezel}`;
      if (key === drawn) return;
      const map = lensMap(w, h, radius, bezel);
      if (!map) return;
      drawn = key;
      image.setAttribute('width', String(w));
      image.setAttribute('height', String(h));
      image.setAttribute('href', toDataUrl(map));
      // The push at the rim, in pixels, is half the scale.
      displace.setAttribute('scale', String(Math.round(bezel * 1.6)));
      el.style.setProperty('--glass-lens', `url(#${id})`);
    };
    const ro = new ResizeObserver(() => {
      if (!frame) frame = requestAnimationFrame(draw);
    });
    ro.observe(el);
    draw();

    return () => {
      ro.disconnect();
      if (frame) cancelAnimationFrame(frame);
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerleave', leave);
      el.style.removeProperty('--glass-lens');
      filter.remove();
    };
  }, [bezelOpt, shown, ref]);
  return ref;
}
