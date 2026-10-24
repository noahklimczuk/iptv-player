/**
 * Drag to reorder favourites (README §4).
 *
 * The data layer has been there the whole time and nothing used it: `favorites` has a
 * `sort_order` column, `lists::favorite_channel_ids` ends `ORDER BY sort_order,
 * added_at` — and both inserts wrote a literal `0`, so every row tied and the tie-break
 * did all the work. The list was in the order things were hearted and could not be in
 * any other.
 *
 * Two halves had to meet for this to show at all. The host needed something that writes
 * the column, and `channels::list` needed to *read* it in the favourites view — it
 * ordered by channel number there, so a reorder would have been saved and then
 * overruled by the only query the screen uses.
 */
import { expect, test, type Page } from '@playwright/test';

/** The favourites view, and the names in it from top to bottom. */
async function favourites(page: Page) {
  await page.goto('/#/live');
  await expect(page.getByTestId('channel-row').first()).toBeVisible();
  await page.getByRole('button', { name: /^Favourites$/ }).click();
  await expect(page.getByTestId('channel-row').first()).toBeVisible();
  return page.getByTestId('channel-row');
}

/** The channel names in view order. */
async function names(rows: ReturnType<Page['getByTestId']>) {
  return (await rows.evaluateAll((els) =>
    els.map((e) => e.getAttribute('aria-label') ?? ''),
  )).map((l) => l.replace(/^Watch /, ''));
}

test('a favourite dragged onto another takes its place', async ({ page }) => {
  const rows = await favourites(page);
  const before = await names(rows);
  test.skip(before.length < 2, 'needs two favourites to have an order at all');

  // The second row onto the first.
  await rows.nth(1).dragTo(rows.nth(0));

  await expect
    .poll(async () => (await names(rows))[0])
    .toBe(before[1]);

  const after = await names(rows);
  expect(after[1]).toBe(before[0]);
  // Nothing was lost or duplicated in the move, which a splice gets wrong easily.
  expect([...after].sort()).toEqual([...before].sort());
});

test('the new order survives leaving the screen and coming back', async ({ page }) => {
  const rows = await favourites(page);
  const before = await names(rows);
  test.skip(before.length < 2, 'needs two favourites');

  await rows.nth(1).dragTo(rows.nth(0));
  await expect.poll(async () => (await names(rows))[0]).toBe(before[1]);

  // Away and back, so the order comes from the host rather than from React state.
  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: /Settings/i }).first()).toBeVisible();
  const again = await favourites(page);
  expect((await names(again))[0]).toBe(before[1]);
});

test('the ordinary channel list is not draggable', async ({ page }) => {
  // Everywhere but favourites the order is the provider's numbering, which is not the
  // viewer's to rearrange — a drag there would either do nothing or silently renumber
  // somebody's channels.
  await page.goto('/#/live');
  const rows = page.getByTestId('channel-row');
  await expect(rows.first()).toBeVisible();
  await expect(rows.first()).toHaveAttribute('draggable', 'false');

  // ...and in favourites it is.
  const favs = await favourites(page);
  await expect(favs.first()).toHaveAttribute('draggable', 'true');
});
