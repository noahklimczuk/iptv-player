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

/** Which title the billboard is showing, by its Play button's label. */
async function featured(page: Page): Promise<string> {
  const hero = page.getByRole('region', { name: 'Featured' });
  const play = hero.getByRole('button', { name: /^Play/ }).first();
  await expect(play).toBeVisible();
  // The heading beside it is the title; the Play button's own label is generic.
  return hero.getByRole('heading').first().innerText();
}

test('the billboard stops rotating while a keyboard user is on it', async ({ page }) => {
  await page.clock.install();
  await page.goto('/#/');
  const hero = page.getByRole('region', { name: 'Featured' });
  await expect(hero).toBeVisible();

  // It does rotate when nobody is using it, which is the behaviour being preserved.
  const first = await featured(page);
  await page.clock.fastForward(13_000);
  await expect.poll(() => featured(page), { message: 'the billboard never rotated' })
    .not.toBe(first);

  // Now tab onto it, the way somebody reaching for Play does.
  const play = hero.getByRole('button', { name: /^Play/ }).first();
  await play.focus();
  await expect(play).toBeFocused();
  const held = await featured(page);

  // Well past two rotations' worth.
  await page.clock.fastForward(40_000);
  expect(await featured(page), 'the billboard rotated under a focused button')
    .toBe(held);
  // And focus is still where the viewer put it.
  await expect(play).toBeFocused();

  // Moving focus away lets it carry on. Past `ROTATE_WITH_TRAILER_MS`, not `ROTATE_MS`:
  // a trailer has started by now, and the dwell is 32s while one is playing.
  await page.getByRole('navigation', { name: 'Main' }).getByRole('link').first().focus();
  await page.clock.fastForward(35_000);
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
  await page.clock.fastForward(40_000);
  expect(await featured(page), 'the billboard rotated with animations off').toBe(first);
});
