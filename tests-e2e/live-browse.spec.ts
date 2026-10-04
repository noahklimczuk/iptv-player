/**
 * Finding a channel among thousands.
 *
 * Live TV put its groups in a strip that scrolled sideways and gave you no way to search
 * within one. That is fine for the eight groups a fixture has; a real subscription has
 * several hundred — one per country per genre per quality — and 22,305 channels behind
 * them. The group you had chosen scrolled out of sight as soon as you moved down the list,
 * and the only way back to the first group was to drag the strip all the way left.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

async function live(page: Page) {
  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();
  await expect(page.getByTestId('channel-row').first()).toBeVisible();
}

test('groups are a list beside the channels, with their counts', async ({ page }) => {
  await live(page);

  const sidebar = page.getByRole('navigation', { name: 'Channel groups' });
  await expect(sidebar).toBeVisible();
  // All and Favourites are pinned above the groups, where they do not move.
  await expect(sidebar.getByRole('button', { name: 'All channels' })).toHaveAttribute(
    'aria-pressed',
    'true',
  );

  const groups = sidebar.getByTestId('group-list').getByRole('button');
  // Polled, not counted once. `live()` waits for a channel row, which says the channels
  // have arrived but not that the groups derived from them have been laid out — and the
  // sidebar is virtualised, so this counts what is mounted rather than what exists.
  // `count()` is the one read here that does not retry.
  await expect.poll(async () => groups.count()).toBeGreaterThan(1);
  // Each row carries how many channels are in it, which the strip only did in brackets.
  await expect(groups.first()).toHaveText(/\d/);

  const name = (await groups.first().innerText()).split('\n')[0]!.trim();
  const before = await page.getByTestId('channel-row').count();
  await groups.first().click();

  // The heading says which group, and the list is narrowed to it.
  await expect(page.getByRole('heading', { level: 1 })).toHaveText(name);
  await expect(groups.first()).toHaveAttribute('aria-pressed', 'true');
  expect(await page.getByTestId('channel-row').count()).toBeLessThanOrEqual(before);

  await page.screenshot({ path: `${SHOTS}/50-live-groups.png` });
});

test('searching narrows the channels without leaving the group', async ({ page }) => {
  await live(page);

  const name = await page
    .getByTestId('channel-row')
    .first()
    .getByRole('strong')
    .innerText()
    .catch(async () => (await page.getByTestId('channel-row').first().innerText()).split('\n')[1]!);
  const needle = name.trim().split(' ')[0]!;

  await page.getByTestId('channel-search').fill(needle);
  await expect
    .poll(async () => page.getByTestId('channel-row').count())
    .toBeGreaterThan(0);

  for (const row of await page.getByTestId('channel-row').all()) {
    expect((await row.getAttribute('aria-label'))!.toLowerCase()).toContain(
      needle.toLowerCase(),
    );
  }

  // A search that matches nothing says so, and offers the way out — rather than the
  // "No channels — add a provider in Settings" that an empty list used to produce.
  await page.getByTestId('channel-search').fill('zzzqqqxx');
  await expect(page.getByText('No channel matches')).toBeVisible();
  await page.getByRole('button', { name: 'Clear search' }).click();
  await expect(page.getByTestId('channel-row').first()).toBeVisible();
});

test('the A–Z bar works once the list is in alphabetical order', async ({ page }) => {
  await live(page);

  const bar = page.getByRole('group', { name: 'Jump to letter' });
  // Channels are in the provider's own numbering by default, which is what digit entry
  // addresses — so a letter would narrow to a set ordered by something else.
  await expect(bar).toHaveAttribute('title', /Sort by A–Z/);

  await page.getByRole('button', { name: 'A–Z', exact: true }).click();
  const names = await page.getByTestId('channel-row').evaluateAll((rows) =>
    rows.map((r) => r.getAttribute('aria-label')!.replace(/^Watch /, '')),
  );
  const lettered = names.find((n) => /^[A-Za-z]/.test(n));
  expect(lettered, 'no channel in the fixtures starts with a letter').toBeTruthy();
  const pick = lettered!.charAt(0).toUpperCase();

  await bar.getByRole('button', { name: pick, exact: true }).click();
  await expect.poll(async () => page.getByTestId('channel-row').count()).toBeGreaterThan(0);
  for (const row of await page.getByTestId('channel-row').all()) {
    const label = (await row.getAttribute('aria-label'))!.replace(/^Watch /, '');
    expect(label.charAt(0).toUpperCase(), `${label} is not a ${pick}`).toBe(pick);
  }
});

test('favourites are a pinned entry rather than a button that fights the group', async ({
  page,
}) => {
  await live(page);
  const sidebar = page.getByRole('navigation', { name: 'Channel groups' });

  // Make sure exactly the first channel is favourited, whichever way the fixtures left
  // it: the heart's label is "Add" or "Remove" depending on that, so pressing by label
  // only works in one of the two cases.
  const first = page.getByTestId('channel-row').first();
  const label = (await first.getAttribute('aria-label'))!.replace(/^Watch /, '');
  const heart = first.getByRole('button', { name: /favourites$/ });
  if ((await heart.getAttribute('aria-pressed')) !== 'true') {
    await heart.click();
    await expect(heart).toHaveAttribute('aria-pressed', 'true');
  }

  await sidebar.getByRole('button', { name: 'Favourites' }).click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Favourites');
  await expect(
    page.getByTestId('channel-row').filter({ has: page.getByText(label, { exact: true }) }),
  ).toHaveCount(1);

  // And back out again.
  await sidebar.getByRole('button', { name: 'All channels' }).click();
  await expect
    .poll(async () => page.getByTestId('channel-row').count())
    .toBeGreaterThan(1);
});
