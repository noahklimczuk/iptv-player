/**
 * The floating navigation pill (replacing the left rail).
 *
 * The look is a matter of taste and not something to assert. What these pin down is the
 * part that is easy to get wrong when navigation stops being part of the layout and
 * starts floating over it:
 *
 * - it is *over* the content rather than beside it, and centred;
 * - the content pane keeps room for it, so the last row of a grid is reachable instead
 *   of parked underneath;
 * - it gets out of the way of video, exactly as the rail did;
 * - and it is still a `navigation` landmark full of links, because a remote control and
 *   the rest of the suite both depend on that.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

const pill = (page: Page) => page.getByTestId('nav-pill');

test('the nav floats near the bottom, centred over the content', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await expect(pill(page)).toBeVisible();

  const box = (await pill(page).boundingBox())!;
  const viewport = page.viewportSize()!;

  // Near the bottom, not at it: a pill flush with the edge is a bar.
  expect(box.y + box.height).toBeLessThan(viewport.height);
  expect(viewport.height - (box.y + box.height)).toBeLessThan(80);
  expect(box.y).toBeGreaterThan(viewport.height * 0.6);

  // Horizontally centred, within a pixel or two of rounding.
  const centre = box.x + box.width / 2;
  expect(Math.abs(centre - viewport.width / 2)).toBeLessThan(3);

  // Fully rounded, which is the whole visual idea.
  const radius = await pill(page).evaluate((el) => getComputedStyle(el).borderRadius);
  expect(radius).not.toBe('0px');

  await page.screenshot({ path: `${SHOTS}/nav-pill.png` });
});

test('the content pane keeps room so nothing hides under the pill', async ({ page }) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  await page.waitForTimeout(900);

  const padding = await page.evaluate(() => {
    const pane = document.querySelector('main') as HTMLElement;
    return {
      paddingBottom: parseFloat(getComputedStyle(pane).paddingBottom),
      scrollPaddingBottom: getComputedStyle(pane).scrollPaddingBottom,
    };
  });
  const box = (await pill(page).boundingBox())!;

  // The reserved space has to actually cover the pill, or the last row of the grid is
  // behind it and cannot be clicked.
  expect(padding.paddingBottom).toBeGreaterThanOrEqual(box.height);
  // And keyboard navigation scrolls a focused card into view — "in view" has to mean
  // "not under the nav", which is what scroll padding is for.
  expect(padding.scrollPaddingBottom).not.toBe('0px');

  // Deliberately *not* asserted here: that the last card on screen sits above the pill
  // after scrolling to the end. The browse grid loads more as it is scrolled, so there
  // is always another row below — the first version of this test scrolled to the bottom,
  // found a card at y=2736, and was measuring lazy loading rather than layout. The
  // padding is the contract; whether a particular card is under the nav at a particular
  // scroll position is not.
});

test('the pill gets out of the way of video', async ({ page }) => {
  // `?video` composes as though libmpv were rendering behind the page, which is the only
  // way a browser can reach this state: `showingPicture` requires a real video surface,
  // so without the flag the chrome stays up — correctly, since hiding it over nothing
  // would leave a hole through to the desktop.
  await page.goto('/?video#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();
  await expect(pill(page)).toBeVisible();

  await page.getByRole('button', { name: 'Watch Meridian News' }).first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();

  // Same rule the rail followed: while there is a picture behind the page, the chrome
  // is not drawn over it.
  await expect(pill(page)).toBeHidden();
});

test('it is still a navigation landmark of links', async ({ page }) => {
  await page.goto('/#/');
  const nav = page.getByRole('navigation', { name: 'Main' });
  await expect(nav).toBeVisible();

  // Every destination, reachable by its name — which is what a remote control and the
  // rest of this suite both use.
  for (const label of ['Home', 'Live TV', 'Guide', 'Movies', 'Series', 'Settings']) {
    await expect(nav.getByRole('link', { name: label })).toBeVisible();
  }

  await nav.getByRole('link', { name: 'Guide' }).click();
  await expect(page.getByRole('heading', { name: 'TV Guide' })).toBeVisible();

  // The current destination is marked, not just coloured differently by luck.
  await expect(nav.getByRole('link', { name: 'Guide' })).toHaveAttribute(
    'aria-current',
    'page',
  );
});
