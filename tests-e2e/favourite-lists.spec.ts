/**
 * Named lists of favourites (README §4).
 *
 * `favorites.list_name` has been a column with a `DEFAULT 'Favorites'` since the table
 * was created, and `lists.rs` took it as a parameter throughout — while every caller
 * passed the same constant and `channels::list` hardcoded
 * `AND fav.list_name = 'Favorites'`. One list was all there could ever be.
 *
 * A list exists because something is in it, which is how the host derives them: there is
 * no empty-list state on either side, and creating a list and filling it are the same
 * gesture.
 */
import { expect, test, type Page } from '@playwright/test';

/** The channel names currently on screen, top to bottom. */
async function channelNames(page: Page): Promise<string[]> {
  const rows = page.getByTestId('channel-row');
  await expect(rows.first()).toBeVisible();
  return (
    await rows.evaluateAll((els) => els.map((e) => e.getAttribute('aria-label') ?? ''))
  ).map((l) => l.replace(/^Watch /, ''));
}

/** The sidebar, where the groups and the named lists live. */
function sidebar(page: Page) {
  return page.getByRole('navigation', { name: 'Channel groups' });
}

/** Open Live TV with the channel list up. */
async function live(page: Page) {
  await page.goto('/#/live');
  await expect(page.getByTestId('channel-row').first()).toBeVisible();
}

test('a channel can be put in a list that does not exist yet', async ({ page }) => {
  await live(page);
  const names = await channelNames(page);
  const first = names[0]!;

  page.once('dialog', (d) => d.accept('Sports'));
  await page.getByRole('button', { name: `Add ${first} to a new list` }).first().click();

  // The list is now the one on screen, and the channel is in it.
  await expect(page.getByRole('heading', { name: 'Sports' })).toBeVisible();
  await expect.poll(async () => (await channelNames(page))[0]).toBe(first);

  // And it is offered in the sidebar with its count, so it can be returned to.
  await expect(sidebar(page).getByRole('button', { name: /^Sports \(1\)/ })).toBeVisible();
});

test('the lists are separate, and the heart means "in this list"', async ({ page }) => {
  await live(page);
  const names = await channelNames(page);
  const [first, second] = [names[0]!, names[1]!];

  // What the default list holds before any of this.
  await sidebar(page).getByRole('button', { name: 'Favourites' }).click();
  await expect(page.getByRole('heading', { name: 'Favourites' })).toBeVisible();
  const defaultBefore = await channelNames(page);

  await sidebar(page).getByRole('button', { name: 'All channels' }).click();
  page.once('dialog', (d) => d.accept('Sports'));
  await page.getByRole('button', { name: `Add ${first} to a new list` }).first().click();
  await expect(page.getByRole('heading', { name: 'Sports' })).toBeVisible();

  // Sports holds one channel, and it is not the second one.
  const inSports = await channelNames(page);
  expect(inSports).toContain(first);
  expect(inSports).not.toContain(second);

  // The default list is untouched by any of that. Compared against what it held before
  // rather than asserted to lack the channel: the fixture seeds some channels as
  // favourites already, so "not in it" would be a claim about the fixture.
  await sidebar(page).getByRole('button', { name: 'Favourites' }).click();
  await expect(page.getByRole('heading', { name: 'Favourites' })).toBeVisible();
  expect(await channelNames(page)).toEqual(defaultBefore);
});

test('emptying a list makes it stop existing', async ({ page }) => {
  await live(page);
  const first = (await channelNames(page))[0]!;

  page.once('dialog', (d) => d.accept('Temporary'));
  await page.getByRole('button', { name: `Add ${first} to a new list` }).first().click();
  await expect(sidebar(page).getByRole('button', { name: /^Temporary \(1\)/ })).toBeVisible();

  // Take the only channel back out. Asserted in two steps on purpose: that the toggle
  // landed, and only then that the list went with it -- otherwise a click that missed
  // and a list that refused to disappear look identical.
  await page.getByRole('button', { name: `Remove ${first} from favourites` }).first().click();
  await expect(page.getByTestId('channel-row')).toHaveCount(0);

  // A list is its rows, so the list goes with it. There is no empty list to tidy up.
  await expect(sidebar(page).getByRole('button', { name: /^Temporary/ })).toHaveCount(0);

  // The default list is still offered, because the heart has to have somewhere to put
  // things.
  await expect(sidebar(page).getByRole('button', { name: 'Favourites' })).toBeVisible();
});
