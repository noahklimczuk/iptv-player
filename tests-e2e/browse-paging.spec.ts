/**
 * Browse pages in, counts honestly, and narrows by the things a real library has.
 *
 * `BrowsePage` asked for `limit: 120, offset: 0` and never asked again, then printed
 * `items.length` beside the heading — so a library of twenty thousand films announced
 * itself as "120" and the 121st could not be reached at all. The host commands had
 * taken `limit` and `offset` from the beginning; the second page was never requested.
 *
 * Driving the page against a real subscription then showed the rest of it: the genre
 * dropdown was empty (genres come from TMDB enrichment, which needs a key), the count
 * climbed while you scrolled, and there was no way to narrow a hundred thousand rows
 * at all. The count now comes from the host, and the filters are the provider's own
 * categories.
 */
import { expect, test } from '@playwright/test';

const SHOTS = 'screenshots';
/** Must match `PAGE` in `BrowsePage.tsx`: the size of one request. */
const PAGE = 120;

test('the count is the library, not the page size', async ({ page }) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();

  // Counted by the host before a single poster arrives, so it never reads as the page
  // size and never climbs while you scroll.
  const count = page.getByTestId('browse-count');
  await expect(count).not.toHaveText('');
  const total = Number((await count.innerText()).replace(/[^0-9]/g, ''));
  expect(total, 'the fixture library is larger than one page').toBeGreaterThan(PAGE);

  // The first request really is a page of it.
  expect(await page.getByTestId('catalog-card').count()).toBe(PAGE);

  // Scrolling brings the rest in, and the count does not move because it was right.
  await page.getByTestId('browse-sentinel').scrollIntoViewIfNeeded();
  await expect(page.getByTestId('browse-sentinel')).toHaveCount(0, { timeout: 10_000 });
  expect(await page.getByTestId('catalog-card').count()).toBe(total);
  expect(Number((await count.innerText()).replace(/[^0-9]/g, ''))).toBe(total);

  await page.screenshot({ path: `${SHOTS}/45-browse-paged.png` });
});

test('a category narrows the list, and clears again', async ({ page }) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  const count = page.getByTestId('browse-count');
  const total = Number((await count.innerText()).replace(/[^0-9]/g, ''));

  // The first chip after "All" — the biggest shelf in this library.
  const chips = page.getByTestId('browse-category');
  await chips.nth(1).click();

  await expect
    .poll(async () => Number((await count.innerText()).replace(/[^0-9]/g, '')))
    .toBeLessThan(total);
  const narrowed = Number((await count.innerText()).replace(/[^0-9]/g, ''));
  expect(narrowed).toBeGreaterThan(0);

  // A filtered list is a different list: pages from the old one must not survive into
  // it, which is the race the generation counter in `usePages` exists for.
  await expect
    .poll(async () => page.getByTestId('catalog-card').count())
    .toBeLessThanOrEqual(Math.min(narrowed, PAGE));

  await chips.first().click();
  await expect
    .poll(async () => Number((await count.innerText()).replace(/[^0-9]/g, '')))
    .toBe(total);
});

test('searching narrows within the list without leaving the page', async ({ page }) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  const count = page.getByTestId('browse-count');
  const total = Number((await count.innerText()).replace(/[^0-9]/g, ''));

  const title = await page.getByTestId('card-title').first().innerText();
  const needle = title.split(' ')[0]!;
  await page.getByTestId('browse-search').fill(needle);

  await expect
    .poll(async () => Number((await count.innerText()).replace(/[^0-9]/g, '')), {
      timeout: 5_000,
    })
    .toBeLessThan(total);
  expect(Number((await count.innerText()).replace(/[^0-9]/g, ''))).toBeGreaterThan(0);

  // Every card on screen matches what was typed.
  const titles = await page.getByTestId('card-title').allInnerTexts();
  expect(titles.length).toBeGreaterThan(0);
  for (const t of titles) {
    expect(t.toLowerCase()).toContain(needle.toLowerCase());
  }

  // Clearing puts the library back.
  await page.getByRole('button', { name: 'Clear search' }).click();
  await expect
    .poll(async () => Number((await count.innerText()).replace(/[^0-9]/g, '')), {
      timeout: 5_000,
    })
    .toBe(total);
});

test('series browse has the same controls films do', async ({ page }) => {
  // Series used to be locked to A–Z while films had four sorts, for no reason anybody
  // could name.
  await page.goto('/#/series');
  await expect(page.getByRole('heading', { name: 'Series' })).toBeVisible();
  await expect(page.getByTestId('browse-count')).not.toHaveText('');
  await expect(page.getByTestId('browse-search')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Year' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Rating' })).toBeVisible();
});

test('an empty genre filter is not shown at all', async ({ page }) => {
  // On a library with no TMDB key there are no genres, and a control reading "All
  // genres" with nothing under it is one that looks broken and is.
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  const genre = page.getByRole('button', { name: 'Genre' });
  if (await genre.count() > 0) {
    await genre.click();
    // The "all" row plus at least one real genre.
    expect(await page.getByTestId('browse-picker-row').count()).toBeGreaterThan(1);
  }
});

test('a long category list is searchable rather than a two-hundred-row dropdown', async ({
  page,
}) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();

  const picker = page.getByTestId('browse-picker-category');
  test.skip(await picker.count() === 0, 'this library has few enough shelves to fit in chips');
  await picker.click();

  const rows = page.getByTestId('browse-picker-row');
  const before = await rows.count();
  expect(before).toBeGreaterThan(1);

  // Typing narrows it, and the "all" row goes with the rest.
  const name = await rows.nth(1).innerText();
  await page.getByLabel('Filter category').fill(name.split(/\s+/)[0]!);
  await expect.poll(async () => rows.count()).toBeLessThan(before);

  // Picking one closes the popover and narrows the library.
  const count = page.getByTestId('browse-count');
  const total = Number((await count.innerText()).replace(/[^0-9]/g, ''));
  await rows.last().click();
  await expect(rows).toHaveCount(0);
  await expect
    .poll(async () => Number((await count.innerText()).replace(/[^0-9]/g, '')))
    .toBeLessThan(total);
});

test('the count is grouped, so a six-figure library is readable', async ({ page }) => {
  // `toLocaleString()` produced "117508" under the WebView this ships inside, because
  // a process with no locale configured groups by nothing.
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  const shown = await page.getByTestId('browse-count').innerText();
  const value = Number(shown.replace(/[^0-9]/g, ''));
  if (value >= 1000) {
    expect(shown, 'a four-figure count needs a separator').toContain(',');
  }
});
