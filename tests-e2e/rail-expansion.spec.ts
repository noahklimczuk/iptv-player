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

/**
 * A card is the same width whatever is written under it.
 *
 * Reported as "the Because of what you watch section is massive", with three posters
 * across a 1920px screen where every other rail showed ten. The cards carry their
 * recommendation reason as a single `white-space: nowrap` line, and a flex item's
 * `min-width` defaults to `auto` — it may not be narrower than its own content. So the
 * card grew to fit the whole sentence, and the ellipsis that was supposed to cut the
 * reason short could never fire, because nothing overflowed.
 *
 * Only the recommendation rail passes a reason, which is why only it was affected, and
 * why no existing test saw it: the rails the suite measures have nothing under the
 * poster but a year.
 */
test('a reason under a card does not make the card wider', async ({ page }) => {
  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
  await page.evaluate(async () => {
    const w = window as unknown as { __auroraInvoke: (c: string, a: unknown) => Promise<unknown> };
    await w.__auroraInvoke('gemini.setKey', { key: 'test-key' });
  });
  await page.getByRole('navigation', { name: 'Main' })
    .getByRole('link', { name: 'Home' })
    .click();
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await page.waitForTimeout(900);

  const withReason = page.getByRole('region', { name: 'Because of what you watch' });
  await expect(withReason).toBeVisible();
  await expect(withReason.getByTestId('card-reason').first()).toBeVisible();

  const cards = withReason.getByTestId('catalog-card');
  const count = Math.min(await cards.count(), 4);
  expect(count).toBeGreaterThan(1);

  // Against the rail's own card width rather than another rail's, so the assertion does
  // not depend on which rails the fixture happens to produce. `CARD_W` is 168; the bug
  // rendered these at 265 and, on a real library with longer reasons, at over 700.
  for (let i = 0; i < count; i += 1) {
    const box = (await cards.nth(i).boundingBox())!;
    expect(box.width, `a card with a reason is ${box.width}px, not a card width`)
      .toBeLessThan(200);
  }

  // And the reason is cut short rather than setting the width.
  const cut = await withReason.getByTestId('card-reason').first().evaluate(
    (el) => el.scrollWidth > el.clientWidth || el.textContent!.length < 30,
  );
  expect(cut, 'the reason is neither ellipsed nor short').toBe(true);
});

test('a rail opens at its start, with its first card level with the heading', async ({ page }) => {
  // Cards snap to `start`, and without scroll padding "start" was the scroller's edge,
  // so every rail snapped 24px in as it laid out: its first card cut against the
  // panel and a "Scroll left" arrow over it, though nothing had been scrolled.
  await homeWithRails(page);
  // A rail long enough to scroll: one that fits has nowhere to snap to, so it never drifted.
  const rail = page.locator('section')
    .filter({ has: page.getByRole('button', { name: 'Scroll right' }) }).first();
  await rail.scrollIntoViewIfNeeded();
  await page.waitForTimeout(600);
  await expect(rail.getByRole('button', { name: 'Scroll left' })).toHaveCount(0);

  const { scrollLeft, cardX, headingX } = await rail.evaluate((el) => {
    const scroller = el.querySelector('.no-scrollbar') as HTMLElement;
    return {
      scrollLeft: scroller.scrollLeft,
      cardX: el.querySelector('[data-testid="catalog-card"]')!.getBoundingClientRect().x,
      headingX: el.querySelector('h2')!.getBoundingClientRect().x,
    };
  });
  expect(scrollLeft).toBe(0);
  expect(Math.abs(cardX - headingX)).toBeLessThan(2);
});

test('paging a rail lands a whole card at the gutter', async ({ page }) => {
  await homeWithRails(page);
  const rail = page.locator('section')
    .filter({ has: page.getByRole('button', { name: 'Scroll right' }) }).first();
  await rail.getByRole('button', { name: 'Scroll right' }).click();
  await expect(rail.getByRole('button', { name: 'Scroll left' })).toBeVisible();
  await page.waitForTimeout(900);

  const { offset } = await rail.evaluate((el) => {
    const scroller = el.querySelector('.no-scrollbar') as HTMLElement;
    const left = scroller.getBoundingClientRect().x + 24;
    // The first card whose right edge is past the gutter is the one at the start.
    const first = [...el.querySelectorAll('[data-testid="catalog-card"]')]
      .map((c) => c.getBoundingClientRect())
      .find((r) => r.right > left)!;
    return { offset: first.x - left };
  });
  // Ended mid-card before: `clientWidth - CARD_W` is not a whole number of cards.
  expect(Math.abs(offset)).toBeLessThan(2);
});

test('an open Top 10 card says its rank instead of hiding the numeral under its panel', async ({ page }) => {
  // The numeral sits on the foot of the poster, which is where the expanded panel goes:
  // the panel hid most of it and the part past the card's left edge was a stray stroke.
  await homeWithRails(page);
  const rail = page.getByRole('region', { name: 'Top 10 This Week' });
  await rail.scrollIntoViewIfNeeded();
  const card = rail.getByTestId('catalog-card').nth(3);
  await expect(card.getByTestId('card-rank')).toHaveCSS('opacity', '1');

  await card.hover();
  await expect(card.getByTestId('card-rank-text')).toHaveText('#4 in Top 10');
  await expect(card.getByTestId('card-rank')).toHaveCSS('opacity', '0');
  // Only the open card: its neighbours keep their numerals.
  await expect(rail.getByTestId('catalog-card').nth(4).getByTestId('card-rank')).toHaveCSS('opacity', '1');
});

test('a ranked or recommended rail shows no count, and a shelf does', async ({ page }) => {
  // "Top 10 This Week 10" said nothing, and "Because You Watched MobLand 4,073" was the
  // size of the pool the ranking scored, read as a claim about MobLand.
  await homeWithRails(page);
  for (const name of ['Top 10 This Week', 'Top Picks For You']) {
    await expect(page.getByRole('region', { name }).getByTestId('rail-count')).toHaveCount(0);
  }
  for (const name of ['Recently Added', 'My List']) {
    await expect(page.getByRole('region', { name }).getByTestId('rail-count')).toHaveCount(1);
  }
});
