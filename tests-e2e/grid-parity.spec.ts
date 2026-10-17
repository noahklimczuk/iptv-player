/**
 * The grid view offers what the list view offers.
 *
 * `ChannelRow` renders the catch-up badge and the favourite heart and takes
 * `onToggleFavorite`; `ChannelTile` took only `channel`, `guide` and `onTune` and
 * rendered neither. So switching Live TV to Grid silently removed the only way to
 * favourite a channel from that screen — while the empty state for favourites tells the
 * viewer exactly where the control is, "use the heart on any channel, to add it here"
 * (R-114).
 */
import { expect, test, type Page } from '@playwright/test';

async function openGrid(page: Page) {
  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();
  await expect(page.getByTestId('channel-row').first()).toBeVisible();
  await page.getByRole('button', { name: 'Grid' }).click();
  await expect(page.getByTestId('channel-tile').first()).toBeVisible();
}

test('a channel can be favourited from the grid', async ({ page }) => {
  await openGrid(page);

  const tile = page.getByTestId('channel-tile').first();
  const heart = tile.getByRole('button', { name: /favourites$/ });
  await expect(heart).toBeVisible();

  const before = await heart.getAttribute('aria-pressed');
  await heart.click();
  await expect(heart).toHaveAttribute('aria-pressed', before === 'true' ? 'false' : 'true');

  // It really went to the host rather than only to the button: the list view, which is
  // a different component reading the same channel, agrees.
  const name = (await tile.locator('strong').innerText()).trim();
  await page.getByRole('button', { name: 'List' }).click();
  const row = page.getByTestId('channel-row').filter({ hasText: name }).first();
  await expect(row.getByRole('button', { name: /favourites$/ }))
    .toHaveAttribute('aria-pressed', before === 'true' ? 'false' : 'true');
});

test('the grid says the same things about a channel as the list', async ({ page }) => {
  await openGrid(page);

  // Everything below is pinned to a named channel rather than to whatever the grid
  // happens to have rendered: it is virtualised, so "the first tile" depends on the
  // scroll position and on how much guide data has arrived, which made an earlier
  // version of this test fail about once in five full runs while passing on its own.
  const flagged = await page.evaluate(async () => {
    const inv = (window as unknown as {
      __auroraInvoke: (c: string, a: unknown) => Promise<unknown>;
    }).__auroraInvoke;
    const list = (await inv('channels.list', {})) as {
      name: string; stale: boolean; hasCatchup: boolean; epgChannelId: string | null;
    }[];
    return {
      stale: list.find((c) => c.stale)?.name ?? null,
      catchup: list.find((c) => c.hasCatchup)?.name ?? null,
      unmapped: list.find((c) => !c.epgChannelId)?.name ?? null,
    };
  });

  // "No guide data", not an em dash: the same channel read as unknown in one view and
  // as a dash in the other, and with most of this subscription carrying no guide
  // mapping the dash was most of the grid.
  if (flagged.unmapped) {
    const filter = page.getByPlaceholder('Search channels…');
    await filter.fill(flagged.unmapped);
    const tile = page.getByTestId('channel-tile').filter({ hasText: flagged.unmapped }).first();
    await expect(tile).toBeVisible();
    await expect(tile).toContainText('No guide data');
    await expect(tile).not.toContainText('—');
    await filter.fill('');
  }

  // And the two badges the row carries are here too. Filtered down to a channel that
  // has each, rather than hoping one is inside the virtualised window: the grid renders
  // the rows it can see, so "the first tile with a Dropped badge" depends on where the
  // list happens to be scrolled.
  expect(flagged.stale, 'no channel is marked dropped, so this proves nothing').toBeTruthy();
  expect(flagged.catchup, 'no channel has catch-up, so this proves nothing').toBeTruthy();

  const filter = page.getByPlaceholder('Search channels…');
  await filter.fill(flagged.stale!);
  const dropped = page.getByTestId('channel-tile').filter({ hasText: flagged.stale! }).first();
  await expect(dropped).toBeVisible();
  await expect(dropped.getByText('Dropped', { exact: true })).toBeVisible();

  await filter.fill(flagged.catchup!);
  const catchup = page.getByTestId('channel-tile').filter({ hasText: flagged.catchup! }).first();
  await expect(catchup).toBeVisible();
  await expect(catchup.getByText('Catch-up', { exact: true })).toBeVisible();
});
