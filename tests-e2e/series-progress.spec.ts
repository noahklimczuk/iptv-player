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

/**
 * The resume bar on an episode without artwork, which was being clipped to nothing.
 *
 * The thumbnail box's only in-flow child was a conditional `<img>`. With `ep.still`
 * null it was 148px wide and 0px tall with `overflow: hidden`, so the play affordance
 * and the resume bar — both absolutely positioned — were clipped away. `partway` is
 * computed from real host data at the row above, rendered, and then vanished; the
 * episode list is where you find the one you were partway through, and the bar is the
 * only sign of it other than the tick.
 *
 * Invisible from here until now because every fixture episode carried a still, which no
 * real library does: stills come from TMDB enrichment and nothing else (R-106).
 */
test('an episode without artwork still has a thumbnail box to draw progress in', async ({ page }) => {
  const dialog = await openSeries(page);

  const thumbs = dialog.getByTestId('episode-thumb');
  const count = await thumbs.count();
  expect(count, 'no episodes to measure').toBeGreaterThan(0);

  // Every row, because which ones lack a still is the fixture's business and not this
  // test's: a box of zero height is wrong whatever the reason for it.
  let withoutImage = 0;
  for (let i = 0; i < count; i++) {
    const thumb = thumbs.nth(i);
    if ((await thumb.locator('img').count()) === 0) withoutImage++;
    const box = await thumb.boundingBox();
    expect(box, `episode ${i + 1} has no thumbnail box at all`).not.toBeNull();
    expect(box!.height, `episode ${i + 1}'s thumbnail is ${box!.height}px tall`)
      .toBeGreaterThan(20);
    // And the play affordance inside it is on screen rather than clipped.
    await expect(thumb.locator('svg').first()).toBeVisible();
  }

  // If this is 0 the fixtures have gone back to enriching everything and the test above
  // is no longer testing the case it was written for.
  expect(withoutImage, 'every fixture episode has a still again').toBeGreaterThan(0);
});
