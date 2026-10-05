/**
 * The rail card expansion, measured rather than eyeballed.
 *
 * Two bugs lived here and both were invisible to every other test, because nothing was
 * looking at geometry — a screenshot test would have gone on passing with the bottom of
 * the card shaved off, since the shaved version is what it captured.
 *
 * 1. **The expanded card was clipped.** A scroll container cannot let overflow hang out
 *    of one axis: CSS forces `overflow-y` to compute as `auto` when `overflow-x` scrolls,
 *    whatever the stylesheet asks for. The scroller's padding was therefore the entire
 *    mechanism for making room, it was a hand-picked 28px, and the card needed 60 — so
 *    16px of poster and shadow were cut off.
 * 2. **The rail reflowed.** The expanded panel animated `height: 0 → auto`, which is
 *    layout, so the scroller grew from 308px to 412px and shoved every row below it down
 *    and back as the pointer moved along a rail.
 *
 * So these assert the two numbers that say it is fixed, and would fail again the moment
 * the card gains a line of text without `RAIL_EXPANSION_PAD` being told.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

interface Geometry {
  cardHeight: number;
  scrollerClientH: number;
  scrollerScrollH: number;
  overflowY: string;
}

async function geometry(page: Page): Promise<Geometry> {
  return page.evaluate(() => {
    const card = document.querySelector('[data-testid="catalog-card"]') as HTMLElement;
    const scroller = card.closest('.no-scrollbar') as HTMLElement;
    return {
      cardHeight: card.getBoundingClientRect().height,
      scrollerClientH: scroller.clientHeight,
      scrollerScrollH: scroller.scrollHeight,
      overflowY: getComputedStyle(scroller).overflowY,
    };
  });
}

async function homeWithRails(page: Page) {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await expect(page.locator('[data-testid="catalog-card"]').first()).toBeVisible();
  // The rails settle after their first paint; measuring mid-layout proves nothing.
  await page.waitForTimeout(900);
}

test('an expanded card is not clipped by the rail it is in', async ({ page }) => {
  await homeWithRails(page);
  const card = page.locator('[data-testid="catalog-card"]').first();

  const resting = await geometry(page);
  expect(resting.scrollerScrollH).toBeLessThanOrEqual(resting.scrollerClientH);

  await card.hover();
  // Past the 220ms expansion, and short of the 700ms trailer dwell.
  await page.waitForTimeout(450);

  const expanded = await geometry(page);

  // The assertion that matters: nothing sticks out of the scroll box, so nothing is cut.
  expect(
    expanded.scrollerScrollH,
    `the expanded card overflows its rail by ${
      expanded.scrollerScrollH - expanded.scrollerClientH
    }px — raise RAIL_EXPANSION_PAD in Rail.tsx`,
  ).toBeLessThanOrEqual(expanded.scrollerClientH);

  // And the reason the padding is load-bearing, stated as a test so the next person does
  // not "simplify" it away: the axis really does compute to `auto`.
  expect(expanded.overflowY).toBe('auto');

  await page.screenshot({ path: `${SHOTS}/rail-card-expanded.png` });
});

test('expanding a card does not move the rows below it', async ({ page }) => {
  await homeWithRails(page);
  const card = page.locator('[data-testid="catalog-card"]').first();

  // Brought into view *before* the first measurement, because `hover()` scrolls to the
  // element — and a scrolled page moves every viewport coordinate, which reads as the
  // jump this test is looking for. Measured 21px of it before this line existed.
  await card.scrollIntoViewIfNeeded();
  await page.waitForTimeout(250);

  /** The next rail's heading, relative to the scrolling pane rather than the viewport. */
  const headingOffset = () =>
    page.evaluate(() => {
      const heading = document.querySelectorAll('h2')[1] as HTMLElement | undefined;
      if (!heading) return null;
      const pane = heading.closest('main');
      return heading.getBoundingClientRect().top + (pane ? pane.scrollTop : 0);
    });

  const before = await geometry(page);
  const headingBefore = await headingOffset();

  await card.hover();
  await page.waitForTimeout(450);

  const after = await geometry(page);
  const headingAfter = await headingOffset();

  // The card grows by transform, which does not affect layout. Its box must not change.
  expect(after.cardHeight).toBeCloseTo(before.cardHeight, 0);
  expect(after.scrollerClientH).toBe(before.scrollerClientH);

  if (headingBefore != null && headingAfter != null) {
    expect(
      Math.abs(headingAfter - headingBefore),
      'the rail below moved when a card expanded',
    ).toBeLessThan(2);
  }
});

test('the expanded panel is reachable, not just visible', async ({ page }) => {
  await homeWithRails(page);
  const card = page.locator('[data-testid="catalog-card"]').first();
  await card.hover();
  await page.waitForTimeout(450);

  // The panel moved inside the card's own box to stop it being clipped, which is only
  // an improvement if its buttons can still be hit.
  const play = card.getByRole('button', { name: /^Play / });
  await expect(play).toBeVisible();
  const box = await play.boundingBox();
  expect(box, 'the Play button has no box to click').not.toBeNull();
  expect(box!.height).toBeGreaterThan(8);
});
