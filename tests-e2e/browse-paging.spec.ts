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
  // The count is empty until the host has counted, and `Number('')` is 0 — so reading it
  // on the strength of the heading alone made `total` zero and every comparison below
  // meaningless. The first test in this file already waited; the rest did not.
  await expect(count).not.toHaveText('');
  const total = Number((await count.innerText()).replace(/[^0-9]/g, ''));

  // The biggest shelf in this library: the sidebar is ordered by size, and "All" is
  // pinned above the list rather than being its first row.
  const sidebar = page.getByRole('navigation', { name: 'Film categories' });
  const shelves = sidebar.getByTestId('group-list').getByRole('button');
  await shelves.first().click();

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

  await sidebar.getByRole('button', { name: /^All/ }).click();
  await expect
    .poll(async () => Number((await count.innerText()).replace(/[^0-9]/g, '')))
    .toBe(total);
});

test('the selected category is named, and stays visible while you scroll', async ({ page }) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();

  const sidebar = page.getByRole('navigation', { name: 'Film categories' });
  const shelf = sidebar.getByTestId('group-list').getByRole('button').first();
  const name = (await shelf.innerText()).split('\n')[0]!.trim();
  await shelf.click();

  // The heading says which shelf, rather than "Movies" over one shelf of them.
  await expect(page.getByRole('heading', { level: 1 })).toHaveText(name);
  // And it is marked as the one that is on, for anything that cannot see the highlight.
  await expect(shelf).toHaveAttribute('aria-pressed', 'true');

  // The sidebar is sticky, so scrolling the grid does not take the group list away —
  // which is what a horizontal strip of chips did as soon as you moved down the page.
  await page.mouse.wheel(0, 2400);
  await expect(shelf).toBeInViewport();
});

test('a long category list is filtered in place', async ({ page }) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();

  const sidebar = page.getByRole('navigation', { name: 'Film categories' });
  const filter = sidebar.getByTestId('group-filter');
  test.skip(await filter.count() === 0, 'this library has few enough shelves to need no filter');

  const rows = sidebar.getByTestId('group-list').getByRole('button');
  const before = await rows.count();
  expect(before).toBeGreaterThan(1);

  const name = (await rows.first().innerText()).split('\n')[0]!.trim();
  await filter.fill(name.slice(0, 4));
  await expect.poll(async () => rows.count()).toBeLessThan(before);
  // What was typed still matches the row it was taken from.
  await expect(rows.first()).toContainText(name.slice(0, 4));

  // Nothing matching says so rather than showing an empty column.
  await filter.fill('zzzqqq');
  await expect(sidebar.getByText(/No group matches/)).toBeVisible();
});

test('the A–Z bar narrows to a letter, and only where that means something', async ({ page }) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();

  const bar = page.getByRole('group', { name: 'Jump to letter' });
  const count = page.getByTestId('browse-count');

  // Films default to Recently added, where a letter would narrow to a set ordered by
  // something else entirely. The bar is visible and inert, with the reason in its title.
  await expect(bar).toHaveAttribute('title', /Sort by A–Z/);

  await page.getByRole('button', { name: 'A–Z', exact: true }).click();
  await expect(page.getByTestId('card-title').first()).toBeVisible();
  await expect(count).not.toHaveText('');
  const total = Number((await count.innerText()).replace(/[^0-9]/g, ''));

  // A letter this library actually has, taken from the data rather than chosen in
  // advance: the fixtures are mostly "The …", so picking one by hand tests an empty
  // result and calls it a failure. Skipping the titles that start with a bracket or a
  // digit, which are the `#` bucket's and have no letter of their own.
  const shown = await page.getByTestId('card-title').allInnerTexts();
  const lettered = shown.map((t) => t.trim()).find((t) => /^[A-Za-z]/.test(t));
  expect(lettered, 'no film in the fixtures starts with a letter').toBeTruthy();
  const pick = lettered!.charAt(0).toUpperCase();

  await bar.getByRole('button', { name: pick, exact: true }).click();
  await expect
    .poll(async () => Number((await count.innerText()).replace(/[^0-9]/g, '')))
    .toBeLessThan(total);

  // Every title on screen starts with it — the host filtered, rather than the page
  // scrolling to roughly the right place.
  const titles = await page.getByTestId('card-title').allInnerTexts();
  expect(titles.length).toBeGreaterThan(0);
  for (const title of titles) {
    expect(
      title.trim().toUpperCase().startsWith(pick),
      `${title} does not start with ${pick}`,
    ).toBe(true);
  }

  // Pressing it again clears it: a filter needs an off.
  await bar.getByRole('button', { name: pick, exact: true }).click();
  await expect
    .poll(async () => Number((await count.innerText()).replace(/[^0-9]/g, '')))
    .toBe(total);
});

test('the hash bucket holds the titles no letter would reach', async ({ page }) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  await page.getByRole('button', { name: 'A–Z', exact: true }).click();

  const bar = page.getByRole('group', { name: 'Jump to letter' });
  await bar.getByRole('button', { name: /number or symbol/ }).click();

  // A provider's library is full of these: "[SPANISH] La Casa del Lago" in the fixtures,
  // and `[4K] …`, `2001 …`, `|UK| …` on a real one.
  const titles = await page.getByTestId('card-title').allInnerTexts();
  expect(titles.length).toBeGreaterThan(0);
  for (const title of titles) {
    const first = title.trim().charAt(0).toUpperCase();
    expect(first < 'A' || first > 'Z', `${title} starts with a letter`).toBe(true);
  }
});

test('searching narrows within the list without leaving the page', async ({ page }) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  const count = page.getByTestId('browse-count');
  // The count is empty until the host has counted, and `Number('')` is 0 — so reading it
  // on the strength of the heading alone made `total` zero and every comparison below
  // meaningless. The first test in this file already waited; the rest did not.
  await expect(count).not.toHaveText('');
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
  await page.getByRole('button', { name: 'Clear' }).click();
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
    expect(await page.getByTestId('select-row').count()).toBeGreaterThan(1);
  }
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
