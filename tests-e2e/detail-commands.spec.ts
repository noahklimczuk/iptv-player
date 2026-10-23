/**
 * What opening a detail panel asks the host for.
 *
 * `DetailModal` calls `useSeriesProgress(item?.kind === 'series' ? item.id : 0)` — hooks
 * cannot be called conditionally, so anything that is not a series passes a sentinel id —
 * and the hook fetched from an effect with no guard on it. So the app asked the host which
 * episodes of series `0` had been watched. The answer is empty and harmless, which is why
 * nothing ever looked wrong (R-107).
 *
 * **The finding overstates it**, and the correction is the reason this file measures
 * rather than asserts a count. R-107 says "every film opened costs one round trip" and
 * "on a library browsed with the keyboard is a steady trickle". Measured with the guard
 * removed, the ids sent are `[0]` after a page load and still `[0]` after opening three
 * different films: `load` is memoized on `[profileId, seriesId]`, and opening a film
 * leaves `seriesId` at 0, so the effect does not re-run. The real cost is one wasted
 * command per page load. The fix is the same and still right; the trickle is not there.
 *
 * No journey could see this by looking at the screen, because the screen was right.
 * `__auroraCalls` is the seam that makes a command that should not have been sent visible
 * — the same approach `guide-payload.spec.ts` takes to a fetch that was too large.
 *
 * Deliberately *not* cleared before opening the panel. The first version of this test
 * cleared the log and then asserted an absence, which passed with the fix removed: the
 * call it was looking for had already happened at page load, before the clear. An
 * assertion about something not happening has to cover the whole window in which it
 * would have.
 */
import { expect, test, type Page } from '@playwright/test';

interface MockCall { name: string; args: Record<string, unknown>; bytes: number }

async function seriesIdsAsked(page: Page): Promise<unknown[]> {
  return await page.evaluate(
    () => (window as unknown as { __auroraCalls: () => MockCall[] }).__auroraCalls()
      .filter((c) => c.name === 'progress.forSeries')
      .map((c) => c.args.seriesId),
  );
}

test('nothing ever asks for the episodes of series zero', async ({ page }) => {
  await page.goto('/#/movies');
  const cards = page.getByTestId('catalog-card');
  await expect(cards.first()).toBeVisible();

  // Every id asked for since the page loaded, which is the window the stray call lives
  // in — it goes out when the modal first mounts with nothing selected.
  expect(await seriesIdsAsked(page), 'asked at page load').toEqual([]);

  for (const i of [0, 1, 2]) {
    await cards.nth(i).click();
    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole('heading', { level: 1 })).not.toHaveText('');
    await page.keyboard.press('Escape');
    await expect(dialog).toBeHidden();
  }

  expect(await seriesIdsAsked(page), 'asked while opening three films').toEqual([]);
});

test('opening a series still asks about its episodes', async ({ page }) => {
  // The other half of the guard: it must not have switched the fetch off for the kind
  // that needs it. Without this, the assertion above would pass just as well if the hook
  // never fetched at all, which is the failure R-159 describes.
  await page.goto('/#/series');
  const card = page.getByTestId('catalog-card').first();
  await expect(card).toBeVisible();

  await card.click();
  await expect(page.getByRole('dialog')).toBeVisible();

  await expect.poll(async () => (await seriesIdsAsked(page)).length).toBeGreaterThan(0);
  for (const id of await seriesIdsAsked(page)) {
    expect(Number(id), 'asked about a real series').toBeGreaterThan(0);
  }
});
