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
 * One show's resume label must not appear on the next show's Play button.
 *
 * `DetailModal` is one mounted component whose `item` changes, so `Hero` is not
 * remounted when the title is *replaced* rather than closed — which is what the palette
 * does: `onPick` calls `openDetail` straight over the open modal. The effect that
 * fetches the resume point never cleared `label` on the way in, so the second show's
 * Play button read the first show's "Resume S02E05 from 41:12" for as long as the two
 * commands took, with the button live throughout. Pressing it in that window plays the
 * wrong thing.
 *
 * And the `!ep` path returned without clearing either, so a resume point naming an
 * episode the listing does not contain left the stale label up for good —
 * `watch_progress` carries no foreign key to `episodes`, deliberately, so the host can
 * really produce that (R-101).
 */
test('a resume label does not survive onto the next show', async ({ page }) => {
  await page.goto('/#/series');
  await expect(page.getByRole('heading', { name: 'Series' })).toBeVisible();

  // Watch into an episode of the first show, so it has something to resume.
  await page.getByTestId('catalog-card').first().click();
  const dialog = page.getByRole('dialog');
  const firstTitle = await dialog.getByRole('heading').first().innerText();
  await dialog.getByRole('tab', { name: 'episodes' }).click();
  await dialog.getByRole('button', { name: /^Mark S01E01 as played$/ }).click();
  await expect(dialog.getByRole('button', { name: /^Play S01E02$/ })).toBeVisible();
  const first = (await dialog.getByRole('button', { name: /^(Play|Resume) S\d+E\d+/ })
    .innerText()).trim();

  // Swap the title underneath the open modal, with the new one's resume lookup held
  // open. `__auroraHang` is what makes the window observable: against the mock the two
  // commands land in milliseconds, and on a real panel they do not.
  await page.evaluate(() => {
    (window as unknown as { __auroraHang: (name: string) => void })
      .__auroraHang('progress.resumePoint');
  });
  // The palette, which is the path a viewer has: `onPick` calls `openDetail` straight
  // over the modal that is already open, so `Hero` is never remounted.
  await page.keyboard.press('Control+k');
  const palette = page.getByRole('dialog', { name: 'Search' });
  await expect(palette).toBeVisible();
  await palette.getByRole('textbox').fill('the');
  // A *different* show: most fixture titles begin with "The", so the first hit is often
  // the one already open, and swapping a title for itself proves nothing.
  const hits = palette.getByTestId('palette-group-series').getByRole('button');
  await expect(hits.first()).toBeVisible();
  let picked = false;
  for (let i = 0; i < (await hits.count()); i += 1) {
    const text = (await hits.nth(i).innerText()).split('\n')[0]!.trim();
    if (text && text !== firstTitle.trim()) {
      await hits.nth(i).click();
      picked = true;
      break;
    }
  }
  expect(picked, 'the palette offered no show other than the one already open').toBe(true);
  await expect(dialog.getByRole('heading').first()).not.toHaveText(firstTitle);

  const play = dialog.getByRole('button', { name: /^(Play|Resume)/ }).first();
  await expect(play).toBeVisible();
  expect(
    (await play.innerText()).trim(),
    `the second show offered "${first}", which belongs to the first`,
  ).not.toBe(first);
  // And it says something of its own rather than nothing at all.
  await expect(play).toHaveText(/\S/);
});
