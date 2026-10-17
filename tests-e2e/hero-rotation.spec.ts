/**
 * The billboard must not rotate under the hands of somebody using it.
 *
 * Pausing was hover-only — `onMouseEnter`/`onMouseLeave` and nothing else, swept — while
 * the billboard carries three large focusable buttons. So a viewer who tabbed to **Play**
 * and stopped to read the synopsis got twelve seconds (`ROTATE_MS`), or thirty-two if a
 * trailer had started, before `setIndex` advanced. The focused element is not remounted,
 * so focus stays put and the label changes under it: pressing Enter plays a film they did
 * not choose.
 *
 * WCAG 2.2.2 asks for a way to pause, stop or hide anything that moves for more than five
 * seconds. The dot indicators let a viewer *jump* but not stop (R-151).
 *
 * Driven with Playwright's clock, so twelve seconds cost nothing and are exact.
 */
import { expect, test, type Page } from '@playwright/test';

/**
 * Move the clock in steps, letting React schedule as it goes.
 *
 * One big `fastForward` is not enough. The billboard reschedules its own timer when the
 * trailer starts — twelve seconds becomes thirty-two — and that happens in a React
 * effect, which runs *after* `fastForward` has already advanced the clock to the end of
 * its window. The replacement timer is then due beyond it and never fires, so the test
 * saw a billboard that "never rotated" depending on whether the rail's first item had a
 * trailer key. Small steps give the effect a chance to run between them.
 */
async function advance(page: Page, ms: number, step = 4_000) {
  for (let moved = 0; moved < ms; moved += step) {
    await page.clock.fastForward(Math.min(step, ms - moved));
    // Real time, so React's scheduler gets a turn.
    await page.waitForTimeout(30);
  }
}

/** Which title the billboard is showing, by its Play button's label. */
async function featured(page: Page): Promise<string> {
  const hero = page.getByRole('region', { name: 'Featured' });
  const play = hero.getByRole('button', { name: /^Play/ }).first();
  await expect(play).toBeVisible();
  // The heading beside it is the title; the Play button's own label is generic.
  return hero.getByRole('heading').first().innerText();
}

// `@motion`, so the reduced-motion project skips it: there the billboard does not rotate
// at all, which is the subject of the test below rather than a failure of this one.
test('the billboard stops rotating while a keyboard user is on it @motion', async ({ page }) => {
  await page.clock.install();
  await page.goto('/#/');
  const hero = page.getByRole('region', { name: 'Featured' });
  await expect(hero).toBeVisible();

  // It does rotate when nobody is using it, which is the behaviour being preserved.
  //
  // Past `ROTATE_WITH_TRAILER_MS` rather than `ROTATE_MS`: a title with a trailer key
  // starts one after two seconds and then dwells for 32s instead of 12s, and whether the
  // rail's first item has a key depends on which item it is. Waiting 13s made this test
  // a coin flip on that — it passed alone and failed in a full run.
  const first = await featured(page);
  await advance(page, 40_000);
  await expect.poll(() => featured(page), { message: 'the billboard never rotated' })
    .not.toBe(first);

  // Now tab onto it, the way somebody reaching for Play does.
  const play = hero.getByRole('button', { name: /^Play/ }).first();
  await play.focus();
  await expect(play).toBeFocused();
  const held = await featured(page);

  // Well past two rotations' worth, trailer dwell included.
  await advance(page, 80_000);
  expect(await featured(page), 'the billboard rotated under a focused button')
    .toBe(held);
  // And focus is still where the viewer put it.
  await expect(play).toBeFocused();

  // Moving focus away lets it carry on. Past `ROTATE_WITH_TRAILER_MS`, not `ROTATE_MS`:
  // a trailer has started by now, and the dwell is 32s while one is playing.
  await page.getByRole('navigation', { name: 'Main' }).getByRole('link').first().focus();
  await advance(page, 40_000);
  await expect.poll(() => featured(page), { message: 'it never resumed' }).not.toBe(held);
});

test('reduce motion stops the billboard rotating at all', async ({ page }) => {
  await page.clock.install();
  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Appearance' })).toBeVisible();
  // Turn animations off, which is what README §12 means by reduce motion.
  const toggle = page.getByRole('switch', { name: 'Animations' });
  if ((await toggle.getAttribute('aria-checked')) === 'true') await toggle.click();
  await expect(toggle).toHaveAttribute('aria-checked', 'false');

  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  const first = await featured(page);

  // Three rotations' worth of nothing happening.
  await advance(page, 60_000);
  expect(await featured(page), 'the billboard rotated with animations off').toBe(first);
});
