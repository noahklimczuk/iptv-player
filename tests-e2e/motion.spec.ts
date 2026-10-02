/**
 * Motion, and the switch that is supposed to govern it.
 *
 * `state/ui.ts` sets `data-motion` on `<html>` from the Animations switch, and no
 * stylesheet read it. So the switch turned off the framer-motion durations, which are
 * decided in JavaScript, and left every CSS animation running: the page fade, the
 * skeleton shimmer, the button lift. "Animations: Off" did about half of what it said.
 */
import { expect, test } from '@playwright/test';

test('turning animations off stops the CSS animations too, not just the JS ones', async ({
  page,
}) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();

  // `data-motion` is set from the switch and was read by no stylesheet at all, so the
  // page fade, the skeleton shimmer and the button lift carried on regardless.
  const sweeping = () =>
    page.evaluate(() => {
      const el = document.querySelector('.aurora-page');
      return el ? getComputedStyle(el).animationDuration : null;
    });

  await page.goto('/#/settings');
  await expect(page.getByRole('switch', { name: 'Animations' })).toHaveAttribute(
    'aria-checked',
    'true',
  );
  expect(await sweeping()).not.toBe('0.001s');

  await page.getByRole('switch', { name: 'Animations' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-motion', 'off');
  expect(await sweeping()).toBe('0.001s');
});
