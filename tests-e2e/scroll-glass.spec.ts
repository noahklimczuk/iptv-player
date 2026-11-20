/**
 * The glass stands down while Home scrolls, and comes back when it stops.
 *
 * Reported as "scrolling on the home page is very laggy". A backdrop filter over moving
 * content is re-blurred every frame, and so is a poster under the billboard's running
 * zoom; `lib/scrolling.ts` flags the root while a scroll is under way so both can hold
 * off. What is checked here is the contract the styles rest on: the flag goes up on a
 * real wheel, the tab bar is unfiltered and the drift paused while it is up, and it all
 * comes back once the page is still -- a flag that never came down would leave the
 * whole app without its glass.
 */
import { expect, test, type Page } from '@playwright/test';

const flagged = (page: Page) =>
  page.evaluate(() => document.documentElement.dataset.scrolling !== undefined);
const pillFilter = (page: Page) =>
  page.getByTestId('nav-pill').evaluate((el) => getComputedStyle(el).backdropFilter);
const drift = (page: Page) =>
  page.evaluate(() => {
    const el = document.querySelector('.hero-drift');
    return el ? getComputedStyle(el).animationPlayState : null;
  });

test('scrolling Home sets the glass aside, and stopping brings it back', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByTestId('nav-pill')).toBeVisible();
  await expect(page.getByRole('heading', { level: 2 }).first()).toBeVisible();
  await expect.poll(() => pillFilter(page)).toContain('blur');
  expect(await flagged(page)).toBe(false);

  await page.mouse.move(700, 600);
  // Keep the scroll going while it is looked at: one notch settles in 160 ms.
  let going = true;
  const wheel = (async () => {
    while (going) {
      await page.mouse.wheel(0, 120);
      await page.waitForTimeout(40);
    }
  })();
  try {
    await expect.poll(() => flagged(page), { timeout: 3000 }).toBe(true);
    expect(await pillFilter(page)).toBe('none');
    const state = await drift(page);
    // The drift runs only while animations are on and a title has artwork.
    if (state !== null) expect(state).toBe('paused');
  } finally {
    going = false;
    await wheel;
  }

  await expect.poll(() => flagged(page), { timeout: 3000 }).toBe(false);
  await expect.poll(() => pillFilter(page)).toContain('blur');
  const after = await drift(page);
  if (after !== null) expect(after).toBe('running');
});
