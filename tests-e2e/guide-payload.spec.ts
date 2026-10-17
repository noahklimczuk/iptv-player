/**
 * What the guide asks for, rather than what it draws.
 *
 * `GuidePage` fetched `epg.gridSlice` with `channelIds: []`, which the host reads as
 * every channel, over a 24-hour window. Measured against the live panel — the real
 * 165 MB XMLTV and the real channel list, in the exact shape the host serialises —
 * that is 35,576 programmes and **14.82 MB in one reply**: 13.10 MB of programmes and
 * 1.72 MB of channels. Then re-fetched on every change to `from`, which is what all
 * eight navigation controls do, so walking forward an hour at a time across an evening
 * cost 14.8 MB a press. The virtualiser mounts about 14 of 6,699 rows, so over 99% of
 * it was parsed and never read; the category filter was applied here afterwards, so
 * narrowing to one of 67 groups narrowed the drawing and not the fetch (R-130).
 *
 * No journey could see this, because a journey asserts on the screen and the screen was
 * right. `__auroraCalls` is the seam that makes the cost visible.
 */
import { expect, test, type Page } from '@playwright/test';

interface MockCall { name: string; args: { channelIds?: number[]; group?: string }; bytes: number }

async function calls(page: Page, name: string): Promise<MockCall[]> {
  const all = await page.evaluate(
    () => (window as unknown as { __auroraCalls: () => MockCall[] }).__auroraCalls(),
  );
  return all.filter((c) => c.name === name);
}

async function openGuide(page: Page) {
  await page.goto('/#/guide');
  await expect(page.getByText('CHANNEL', { exact: true })).toBeVisible();
  // The hook settles for 120ms before it asks, so the first fetch needs waiting for.
  await expect.poll(async () => (await calls(page, 'epg.gridSlice')).length).toBeGreaterThan(0);
}

test('the guide asks only about the rows it has mounted', async ({ page }) => {
  await openGuide(page);

  const total = (await page.evaluate(
    () => (window as unknown as { __auroraCalls: () => MockCall[] }).__auroraCalls(),
  )).filter((c) => c.name === 'channels.list').length;
  expect(total, 'the channel rows no longer come from the guide slice').toBeGreaterThan(0);

  const slices = await calls(page, 'epg.gridSlice');
  const mounted = await page.getByTestId('guide-row').count();
  expect(mounted, 'no rows are mounted').toBeGreaterThan(0);

  for (const { args, bytes } of slices) {
    // `[]` is the whole finding: the host reads it as every channel in the library.
    expect(args.channelIds, 'epg.gridSlice asked for every channel').not.toEqual([]);
    expect(
      args.channelIds!.length,
      `asked about ${args.channelIds!.length} channels for ${mounted} mounted rows`,
    ).toBeLessThanOrEqual(mounted + 12);
    // Against the old code this reply is 14.8 MB on a real library. The fixtures are
    // far smaller, so the assertion is on the shape: a screenful, not a library.
    expect(bytes, `one guide reply was ${bytes} bytes`).toBeLessThan(400_000);
  }
});

test('walking the clock forward does not refetch the whole library', async ({ page }) => {
  await openGuide(page);
  await page.evaluate(
    () => (window as unknown as { __auroraClearCalls: () => void }).__auroraClearCalls(),
  );

  // Four of the eight controls that move `from`, each of which was a full refetch.
  for (const label of ['+1h', '+1h', '+24h', 'Now']) {
    await page.getByRole('button', { name: label, exact: true }).click();
    await page.waitForTimeout(250);
  }

  const slices = await calls(page, 'epg.gridSlice');
  expect(slices.length, 'moving the clock asked for nothing at all').toBeGreaterThan(0);
  const biggest = Math.max(...slices.map((c) => c.bytes));
  expect(biggest, `the largest reply while navigating was ${biggest} bytes`)
    .toBeLessThan(400_000);
  for (const { args } of slices) {
    expect(args.channelIds, 'a navigation refetched every channel').not.toEqual([]);
  }
});

test('choosing a category narrows the query, not just the drawing', async ({ page }) => {
  await openGuide(page);
  await page.evaluate(
    () => (window as unknown as { __auroraClearCalls: () => void }).__auroraClearCalls(),
  );

  // The guide's own category filter, which used to be applied to the answer.
  await page.getByTestId('select-category').click();
  const row = page.getByRole('listbox', { name: 'Category' }).getByTestId('select-row');
  // The first row is the "All channels" placeholder; the second is a real group.
  const chosen = (await row.nth(1).innerText()).split('\n')[0]!.trim();
  await row.nth(1).click();
  await page.waitForTimeout(300);

  const lists = await calls(page, 'channels.list');
  expect(lists.length, 'the category did not reach the host at all').toBeGreaterThan(0);
  expect(lists.at(-1)!.args.group, `asked the host for ${chosen}`).toBe(chosen);
});
