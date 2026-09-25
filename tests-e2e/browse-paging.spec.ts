/**
 * Movies and Series page in; they do not stop at the first request.
 *
 * `BrowsePage` asked for `limit: 120, offset: 0` and never asked again, then printed
 * `items.length` beside the heading — so a library of twenty thousand films announced
 * itself as "120" and the 121st could not be reached by scrolling, searching within
 * the page, or any other means. The host commands had taken `limit` and `offset` from
 * the beginning; the second page was simply never requested.
 *
 * The mock library holds more films than one page, so a complete read is more than one
 * request and the count has to move. The assertions are about that movement and about
 * the count agreeing with what is on screen, rather than about a fixture size that is
 * free to change.
 */
import { expect, test } from '@playwright/test';

const SHOTS = 'screenshots';
/** Must match `PAGE` in `BrowsePage.tsx`: the size of one request. */
const PAGE = 120;

test('the film count is not the page size, and the rest can be reached', async ({
  page,
}) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();

  const count = page.getByTestId('browse-count');
  // Honest while incomplete: "120+" says there are at least this many and more to
  // come, where a bare "120" claimed to be the whole library.
  await expect(count).toHaveText('120+');

  // Scrolling to the end brings the rest in, and the count settles on the truth.
  await page.getByTestId('browse-sentinel').scrollIntoViewIfNeeded();
  await expect(count).not.toHaveText(/\+$/, { timeout: 10_000 });
  const total = Number(await count.innerText());
  expect(total, 'paging should have found more than the first page').toBeGreaterThan(PAGE);

  // The sentinel retires once there is nothing left to fetch, so it cannot sit at the
  // bottom of a complete list saying "Loading more…" forever.
  await expect(page.getByTestId('browse-sentinel')).toHaveCount(0);

  await page.screenshot({ path: `${SHOTS}/45-browse-paged.png` });
});

test('a film past the first page is really on the page, not just counted', async ({
  page,
}) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();

  const cards = page.getByTestId('catalog-card');
  expect(
    await cards.count(),
    'the first page should be a page, not the whole library',
  ).toBe(PAGE);

  const count = page.getByTestId('browse-count');
  await page.getByTestId('browse-sentinel').scrollIntoViewIfNeeded();
  await expect(count).not.toHaveText(/\+$/, { timeout: 10_000 });

  // The number beside the heading has to be the number of cards, not a page size:
  // that equality is the whole bug.
  const total = Number(await count.innerText());
  expect(total).toBeGreaterThan(PAGE);
  expect(await cards.count()).toBe(total);
});

test('changing the genre starts the list again rather than appending to it', async ({
  page,
}) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  const count = page.getByTestId('browse-count');
  await page.getByTestId('browse-sentinel').scrollIntoViewIfNeeded();
  await expect(count).not.toHaveText(/\+$/, { timeout: 10_000 });
  const total = Number(await count.innerText());

  // A filtered list is a different list. Pages from the old one must not survive into
  // it — that is the race the generation counter in `usePages` exists for.
  const genre = page.getByLabel('Genre');
  const options = await genre.locator('option').allTextContents();
  const pick = options.find((o) => o && o !== 'All genres');
  test.skip(!pick, 'the fixture library has no genres to filter by');
  await genre.selectOption({ label: pick! });

  const after = await page.getByTestId('catalog-card').count();
  expect(after, 'a filtered list should be smaller than the whole library').toBeLessThan(total);
});

test('series page in too, and a short library says its real size at once', async ({
  page,
}) => {
  // The mock holds fewer series than one page, so a single request is the whole list:
  // the count is final immediately and there is no sentinel to scroll to.
  await page.goto('/#/series');
  await expect(page.getByRole('heading', { name: 'Series' })).toBeVisible();
  const count = page.getByTestId('browse-count');
  await expect(count).not.toHaveText(/\+$/);
  expect(Number(await count.innerText())).toBe(
    await page.getByTestId('catalog-card').count(),
  );
  await expect(page.getByTestId('browse-sentinel')).toHaveCount(0);
});
