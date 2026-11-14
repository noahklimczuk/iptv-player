/**
 * The home rails name what is on them.
 *
 * Their cards were artwork only -- titles were printed under the posters in Browse and
 * nowhere on the home screen -- so a rail was a row of pictures to recognise, and a
 * film whose poster carries no name could not be told apart. Asked for directly.
 */
import { expect, test } from '@playwright/test';

test('every card on a home rail carries its title under the poster', async ({ page }) => {
  await page.goto('/#/');
  const rail = page.locator('section[aria-label]').filter({
    has: page.getByTestId('card-title'),
  }).first();
  await expect(rail).toBeVisible();

  const cards = rail.locator('[aria-label]').filter({ has: page.getByTestId('card-title') });
  const count = await cards.count();
  expect(count, 'a home rail with titled cards').toBeGreaterThan(0);

  // The printed title is the card's own, not a neighbour's: its accessible name leads
  // with the title, optionally followed by ", <year>".
  for (let i = 0; i < Math.min(count, 5); i += 1) {
    const card = cards.nth(i);
    const printed = (await card.getByTestId('card-title').innerText()).trim();
    expect(printed.length).toBeGreaterThan(0);
    expect(await card.getAttribute('aria-label')).toContain(printed);
  }
});
