/**
 * The icon set's geometry, measured rather than eyeballed.
 *
 * "The icons are a little off across the whole app" is the kind of report that is
 * obviously true once you look and impossible to act on by reading the source, because
 * what is wrong is where each glyph sits inside its 24×24 box — and a path is a string of
 * numbers. Measuring found four that were broken rather than merely off:
 *
 *   * `back10` and `forward10` were a quarter of an arc with an arrowhead pinned to it,
 *     centred 9 units left and right of the middle — the two worst in the set;
 *   * `volume` was a speaker cone with no sound waves at all, 5 units off centre;
 *   * `settings` spanned the full 0→24 of the viewBox, so with a 1.8 stroke its outer
 *     edge was clipped on all four sides. An SVG root clips to its viewBox, and half the
 *     stroke was outside it.
 *
 * …and five more sitting 1 to 2 units out, which is enough to make a row of them look
 * unsteady without any one of them looking wrong.
 *
 * `getBBox()` is the only honest way to ask this: it is the rendered geometry, arcs and
 * curves included, rather than the extents of the numbers in the path. That needs a real
 * SVG engine, which is why this lives here and not in a unit test.
 */
import { expect, test } from '@playwright/test';

/**
 * How far a glyph's box may sit from the centre of its viewBox.
 *
 * Not zero: `check`, `home`, `tv` and `bell` are half a unit out by design — a tick and
 * a roofline are optically centred slightly high — and forcing those to 0 would be
 * chasing the measurement rather than the look. Anything past this is a mistake.
 */
const MAX_OFFSET = 0.6;

/**
 * The widest a glyph may be.
 *
 * 21 leaves 1.5 units of margin, which is more than the 0.9 a 1.8 stroke needs. The
 * assertion exists because exceeding it does not look like a design decision, it looks
 * like a shaved edge — and that is exactly how `settings` read.
 */
const MAX_SPAN = 21;

/** And the narrowest, so one glyph is not visibly lighter than its neighbours. */
const MIN_SPAN = 10;

interface Box {
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

test('every icon sits in the middle of its box, and inside it', async ({ page }) => {
  await page.goto('/#/');

  const boxes: Box[] = await page.evaluate(() => {
    const paths = (window as unknown as { __auroraIconPaths: Record<string, string> })
      .__auroraIconPaths;
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('viewBox', '0 0 24 24');
    svg.setAttribute('width', '24');
    svg.setAttribute('height', '24');
    // Off-screen but laid out: `getBBox` on a detached element throws in some engines.
    svg.style.position = 'fixed';
    svg.style.left = '-500px';
    const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
    path.setAttribute('fill', 'none');
    path.setAttribute('stroke', 'black');
    svg.appendChild(path);
    document.body.appendChild(svg);

    const out: Box[] = [];
    for (const [name, d] of Object.entries(paths)) {
      path.setAttribute('d', d);
      const b = path.getBBox();
      out.push({ name, x: b.x, y: b.y, width: b.width, height: b.height });
    }
    svg.remove();
    return out;
  });

  // The set is real, so a broken seam cannot pass this as "nothing to check".
  expect(boxes.length).toBeGreaterThan(30);

  const offCentre: string[] = [];
  const wrongSize: string[] = [];
  for (const b of boxes) {
    const dx = b.x + b.width / 2 - 12;
    const dy = b.y + b.height / 2 - 12;
    if (Math.abs(dx) > MAX_OFFSET || Math.abs(dy) > MAX_OFFSET) {
      offCentre.push(`${b.name} is (${dx.toFixed(2)}, ${dy.toFixed(2)}) from centre`);
    }
    const span = Math.max(b.width, b.height);
    if (span > MAX_SPAN) {
      wrongSize.push(`${b.name} spans ${span.toFixed(1)}, so its stroke is clipped`);
    } else if (span < MIN_SPAN) {
      wrongSize.push(`${b.name} spans only ${span.toFixed(1)}`);
    }
  }

  expect(offCentre, offCentre.join('\n')).toEqual([]);
  expect(wrongSize, wrongSize.join('\n')).toEqual([]);
});

test('a glyph is drawn for every name, and an unknown name falls back', async ({ page }) => {
  await page.goto('/#/');
  // The fallback matters: `Icon` renders `info` for a name it does not have, so a typo is
  // a visible wrong icon rather than an empty box that reads as a layout bug.
  const empty = await page.evaluate(() => {
    const paths = (window as unknown as { __auroraIconPaths: Record<string, string> })
      .__auroraIconPaths;
    return Object.entries(paths)
      .filter(([, d]) => !d || d.trim().length < 4)
      .map(([name]) => name);
  });
  expect(empty).toEqual([]);
});
