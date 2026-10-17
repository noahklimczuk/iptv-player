/**
 * A channel the provider has stopped listing says so on its row.
 *
 * The setup wizard has always told the viewer that these are "kept and flagged", and
 * half of that was true: `channels::stale` counted them for the refresh report and
 * nothing marked the rows. No query returned a staleness column, `ChannelRow` had no
 * field for one, and nothing in the UI read one — so a dropped channel sat in Live TV
 * looking exactly like a working one, and the only sign was a tune that failed with a
 * provider error. Being sent to look for a flag that does not exist is worse than being
 * told nothing (R-53).
 */
import { expect, test } from '@playwright/test';

test('a dropped channel is marked in the live list', async ({ page }) => {
  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();

  const rows = page.getByTestId('channel-row');
  await expect(rows.first()).toBeVisible();

  // The badge exists on the rows the host marked, and on no others. Which channels
  // those are is the fixture's business; that some of them are is the point, because a
  // real library acquires them on the first refresh that drops anything.
  const dropped = page.getByText('Dropped', { exact: true });
  const count = await dropped.count();
  expect(count, 'no channel is marked dropped, so this proves nothing').toBeGreaterThan(0);
  await expect(dropped.first()).toBeVisible();

  // It is inside a channel row rather than loose on the page.
  const marked = rows.filter({ has: page.getByText('Dropped', { exact: true }) });
  expect(await marked.count()).toBe(count);

  // And it says what it means to somebody who hovers it, since "Dropped" alone is a
  // word and not an explanation.
  await expect(
    marked.first().getByTitle('Your provider no longer lists this channel'),
  ).toHaveCount(1);
});
