/**
 * "More Like This" was the same nine films under every title in the library.
 *
 * `Similar` took no props, so it could not be about the item. Its query was a constant —
 * `library.movies` by rating, nine of them, `offset: 12` — with an empty dependency
 * array, which makes the grid identical for all 27,494 films and all 8,086 shows. A
 * series got nine films. And it is the default tab for a film, so those nine strangers
 * were the first thing the detail view showed (R-105).
 *
 * The three assertions here are the three claims the heading makes: the grid changes with
 * the title, it does not contain the title itself, and a show is compared to shows.
 */
import { expect, test, type Page } from '@playwright/test';

/**
 * The titles the "More Like This" grid is showing, in order, or null for a title that
 * has no grid because nothing has enriched it.
 *
 * Similarity here is the item's first genre, and `genres` is one of the eight fields
 * that come from TMDB and nothing else. One fixture row in six is left as an import
 * writes it, which is the majority state of a real library (R-157), and such a title
 * says so in words rather than drawing nine strangers. Walking past those is this
 * test's business: the claim it makes is about titles there is something to compare.
 */
async function similarTitles(page: Page): Promise<string[] | null> {
  const dialog = page.getByRole('dialog');
  await dialog.getByRole('tab', { name: 'More Like This' }).click();
  const grid = dialog.getByTestId('similar-grid');
  const unenriched = dialog.getByText('finding similar titles needs genres');
  // Whichever of the two the tab settles on, rather than timing out on the grid.
  await expect(grid.or(unenriched).first()).toBeVisible();
  if ((await unenriched.count()) > 0) return null;
  // Settles the fetch the tab triggers before reading the cards.
  await expect(dialog.getByTestId('similar-card').first()).toBeVisible();
  return dialog.getByTestId('similar-title').allTextContents();
}

/** Opens the nth card on a browse page and returns its title. */
async function openCard(page: Page, nth: number): Promise<string> {
  const card = page.getByTestId('catalog-card').nth(nth);
  const label = await card.getByRole('button').first().getAttribute('aria-label');
  await card.click();
  await expect(page.getByRole('dialog')).toBeVisible();
  // The label carries ", 1999" when there is a year; the title is what precedes it.
  return (label ?? '').replace(/, \d{4}$/, '');
}

test('More Like This is about the title it sits under', async ({ page }) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();

  // Five films that have a grid, which is not the same as the first five cards: a card
  // in six is unenriched and has none.
  const seen: string[][] = [];
  for (let nth = 0; seen.length < 5 && nth < 20; nth += 1) {
    const title = await openCard(page, nth);
    const titles = await similarTitles(page);

    if (titles !== null) {
      // A title is never like itself. The old query could return the open film in its
      // own grid, since it knew nothing about it.
      expect(titles, `${title} is in its own More Like This`).not.toContain(title);
      seen.push(titles);
    }

    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog')).toHaveCount(0);
  }
  expect(seen.length, 'the first twenty films produced fewer than five grids').toBe(5);

  // The point of the finding: five films, and not one grid in common. Against the old
  // code every one of these lists was byte-identical.
  const distinct = new Set(seen.map((t) => t.join('|')));
  expect(
    distinct.size,
    `five films produced ${distinct.size} distinct grids: ${[...distinct].join(' / ')}`,
  ).toBeGreaterThan(1);
});

test('a show is compared to shows, not to films', async ({ page }) => {
  await page.goto('/#/series');
  await expect(page.getByRole('heading', { name: 'Series' })).toBeVisible();

  // The first show with genres, for the same reason as above.
  let titles: string[] | null = null;
  for (let nth = 0; titles === null && nth < 20; nth += 1) {
    await openCard(page, nth);
    titles = await similarTitles(page);
    if (titles === null) {
      await page.keyboard.press('Escape');
      await expect(page.getByRole('dialog')).toHaveCount(0);
    }
  }
  expect(titles, 'none of the first twenty shows had a grid').not.toBeNull();
  expect(titles!.length).toBeGreaterThan(0);

  // Every row in a show's grid is a show, which is what the season count proves: a film
  // row shows a running time there instead.
  const dialog = page.getByRole('dialog');
  const cards = await dialog.getByTestId('similar-card').allInnerTexts();
  for (const card of cards) {
    expect(card, `"${card}" is not a show`).toMatch(/\d+ seasons?/);
  }
});
