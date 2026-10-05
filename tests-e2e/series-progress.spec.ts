/**
 * Marking episodes played, and what Play means for a show you are partway through.
 *
 * Progress is stored against the episode and the card is the show, so these two are the
 * same question asked from both ends: the list has to say what has been seen, and the
 * button has to open the thing the list implies.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

async function openSeries(page: Page) {
  await page.goto('/#/series');
  await expect(page.getByRole('heading', { name: 'Series' })).toBeVisible();
  await page.locator('[role="button"]').first().click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.getByRole('tab', { name: 'episodes' }).click();
  return dialog;
}

test('an episode can be marked played and unplayed again', async ({ page }) => {
  const dialog = await openSeries(page);

  const mark = dialog.getByRole('button', { name: /^Mark S\d+E\d+ as played$/ }).first();
  const which = (await mark.getAttribute('aria-label'))!
    .replace(/^Mark /, '').replace(/ as played$/, '');

  await mark.click();
  // The same control now offers the other direction, which is how the row says it took.
  await expect(dialog.getByRole('button', { name: `Mark ${which} as unplayed` })).toBeVisible();

  await page.screenshot({ path: `${SHOTS}/34-episode-marked.png` });

  await dialog.getByRole('button', { name: `Mark ${which} as unplayed` }).click();
  await expect(dialog.getByRole('button', { name: `Mark ${which} as played` })).toBeVisible();
});

test('Play opens the next unwatched episode, not the first one', async ({ page }) => {
  const dialog = await openSeries(page);

  // Watch the first two, the way somebody would who is three episodes in.
  for (const ep of ['S01E01', 'S01E02']) {
    await dialog.getByRole('button', { name: `Mark ${ep} as played` }).click();
    await expect(dialog.getByRole('button', { name: `Mark ${ep} as unplayed` })).toBeVisible();
  }

  // The button says which episode before it is pressed, so the promise is checkable.
  await expect(dialog.getByRole('button', { name: /^Play S01E03$/ })).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/35-series-resume.png` });
});

test('finishing a season moves Play on to the next one', async ({ page }) => {
  const dialog = await openSeries(page);

  // The list shows one season at a time, so this marks season one and nothing else.
  const marks = dialog.getByRole('button', { name: /^Mark S01E\d+ as played$/ });
  // `openSeries` returns as soon as the episodes tab is clicked, so seeding the loop
  // from an unwaited count can start it at zero: nothing is marked, and the assertion
  // below then reports Play as still on season one rather than saying why.
  await expect(marks.first()).toBeVisible();
  for (let left = await marks.count(); left > 0; left = await marks.count()) {
    await marks.first().click();
    await expect(marks).toHaveCount(left - 1);
  }

  // Which is the point: Play follows the show rather than the season on screen. The
  // episode it names is the next unwatched one wherever it lives.
  await expect(dialog.getByRole('button', { name: /^Play S02E01$/ })).toBeVisible();
});
