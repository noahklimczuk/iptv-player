/**
 * An opaque backdrop with one rectangle missing, so the video behind the page shows
 * through there and nowhere else.
 *
 * You cannot make a hole by putting a transparent element on top of an opaque one. CSS
 * has no way to erase what is already painted: a child with `background: transparent`
 * shows its *parent's* background, not the window behind it. For the picture to be
 * visible in a rectangle, every layer over that rectangle has to not paint — the app
 * shell included, which is why the caller also has to go transparent while this is up.
 *
 * So the opaque background is drawn by this component instead, as four bands around the
 * hole, and the hole is left alone. Four bands rather than one sheet with a `clip-path`
 * cutout: the same result, no dependence on `polygon(evenodd, …)`, and each band is a
 * rectangle that can be reasoned about on its own.
 *
 * **The z-index is the whole design.** These sit *behind* the page's content, so the
 * page is drawn over them normally and only the hole is see-through. Putting them in
 * front would cover the page — which is a real mistake and not a hypothetical one: it is
 * what `PipTile` does, and it is why the small picture has never shown anything. That is
 * a separate problem from this one, because a picture-in-picture tile floats *over* the
 * content it is meant to let you read, so it cannot be solved by painting behind it.
 * Here the rectangle is a region of the layout that nothing else is in, which is exactly
 * the case four bands behind the content do solve.
 */
import type { MosaicRect } from '@shared/ipc';

/** Physical pixels to CSS pixels, read live so a move between displays is handled. */
function cssRect(rect: MosaicRect, dpr: number) {
  return {
    left: rect.x / dpr,
    top: rect.y / dpr,
    width: rect.width / dpr,
    height: rect.height / dpr,
  };
}

/**
 * A `clip-path` for a layer that should paint everywhere except one rectangle.
 *
 * The other way to make a hole, and the one to reach for when the rectangle sits *over*
 * page content rather than in a region of its own. Bands cannot help there: content
 * drawn above them covers the hole, and bands drawn above the content cover the page.
 * Clipping removes the rectangle from the layer itself, so whatever is inside it —
 * background, text, cards — simply is not painted there.
 *
 * `evenodd` with the outer ring followed by the inner one is what makes the inner ring a
 * hole rather than a second filled shape.
 *
 * It also makes the clipped element a containing block for its `position: fixed`
 * descendants. That is harmless here *because the element is exactly the viewport* — the
 * app shell is `height: 100%` with its own scrolling pane inside — so a fixed child's
 * containing block changes to a box of the same size in the same place. On an element
 * that is not viewport-sized this would move things.
 *
 * Anything that must stay *visible over* the rectangle has to be outside the clipped
 * subtree: it is clipped too, and a tile's own controls sit inside the hole by
 * definition. `PipTile` is portalled to `document.body` for that reason.
 */
export function holeClipPath(rect: MosaicRect, dpr: number): string {
  const left = rect.x / dpr;
  const top = rect.y / dpr;
  const right = left + rect.width / dpr;
  const bottom = top + rect.height / dpr;
  return (
    'polygon(evenodd, 0% 0%, 100% 0%, 100% 100%, 0% 100%, 0% 0%, ' +
    `${left}px ${top}px, ${left}px ${bottom}px, ` +
    `${right}px ${bottom}px, ${right}px ${top}px, ${left}px ${top}px)`
  );
}

export function SurfaceHole({
  rect,
  dpr = window.devicePixelRatio || 1,
  background = 'var(--bg)',
  zIndex = 0,
}: {
  /** Where the picture is, in physical pixels, as the host reported it. */
  rect: MosaicRect | null;
  dpr?: number;
  background?: string;
  zIndex?: number;
}) {
  if (!rect || rect.width === 0 || rect.height === 0) return null;
  const box = cssRect(rect, dpr);

  const band = (style: React.CSSProperties): React.CSSProperties => ({
    position: 'fixed',
    background,
    zIndex,
    // The bands are scenery. Clicks belong to whatever is drawn over them.
    pointerEvents: 'none',
    ...style,
  });

  return (
    <div data-testid="surface-hole" aria-hidden>
      <div style={band({ left: 0, right: 0, top: 0, height: box.top })} />
      <div style={band({ left: 0, right: 0, top: box.top + box.height, bottom: 0 })} />
      <div style={band({ left: 0, width: box.left, top: box.top, height: box.height })} />
      <div
        style={band({
          left: box.left + box.width,
          right: 0,
          top: box.top,
          height: box.height,
        })}
      />
    </div>
  );
}
