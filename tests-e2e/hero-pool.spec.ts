/**
 * The billboard offers titles the viewer has not already decided about.
 *
 * `HomePage` excluded `continueWatching`, `upNext` and `myList` — by rail *kind*, which
 * is not the same as by title. A film on My List is also in `recentlyAdded` and in its
 * genre rail, so it entered the hero pool through one of those and could headline the
 * billboard: the screen inviting you to start something you had already saved, or
 * already begun. The comment in that file said why neither belongs there and the code
 * only managed it for titles that appeared nowhere else.
 *
 * Found as a flake, not by reading. `lists.spec.ts` opens the first billboard item and
 * asserts that liking it does not put it on My List — and failed whenever the shuffle
 * happened to put an already-listed title first. `heroSeed` is `Date.now()` once per
 * session, so that was roughly one run in six: a test that passed for reasons unrelated
 * to what it was testing, which is the R-159 pattern.
 *
 * So the invariant is asserted directly here, across every dot rather than whichever one
 * the shuffle put first.
 */
import { expect, test } from '@playwright/test';

test('no billboard title is one the viewer has already decided about', async ({ page }) => {
  await page.goto('/#/');
  const hero = page.getByRole('region', { name: 'Featured' });
  await expect(hero).toBeVisible();

  const dots = hero.getByRole('button', { name: /^Show / });
  const count = await dots.count();
  expect(count, 'the billboard should be showing several titles').toBeGreaterThan(1);

  for (let i = 0; i < count; i++) {
    await dots.nth(i).click();
    await hero.getByRole('button', { name: 'More Info' }).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    const title = await dialog.getByRole('heading', { level: 1 }).innerText();

    // The "Add" form of the button, not the "Remove" form: this title is not on My List.
    // Asserted as what is there rather than as what is absent — a `toHaveCount(0)` on
    // the Remove button would also pass if the modal never opened (R-159).
    await expect(
      dialog.getByRole('button', { name: `Add ${title} to My List` }),
      `billboard item ${i + 1} (${title}) is already on My List`,
    ).toBeVisible();

    await page.keyboard.press('Escape');
    await expect(dialog).toBeHidden();
  }
});

test('the billboard still has something to show', async ({ page }) => {
  // The exclusion removes titles from the pool, so it could in principle empty it. It
  // must not: a home screen whose billboard is blank is worse than one offering a film
  // you have seen.
  await page.goto('/#/');
  const hero = page.getByRole('region', { name: 'Featured' });
  await expect(hero).toBeVisible();
  await expect(hero.getByRole('heading', { level: 1 })).not.toHaveText('');
  await expect(hero.getByRole('button', { name: 'More Info' })).toBeVisible();
});
