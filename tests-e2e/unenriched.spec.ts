/**
 * The library a viewer actually has on the day they install, which no journey had drawn.
 *
 * An import writes ten fields for a film — provider key, title, match key, year, quality,
 * group, url, poster, added date, and the provider's own rating. Eight of the fields the
 * fixtures hand to every journey come from TMDB enrichment and from nothing else:
 * `backdrop`, `logoArt`, `overview`, `runtimeMins`, `certification`, `genres`, `cast` and
 * `trailerKey`. That needs an API key a viewer may never set.
 *
 * And it is not a transient first-run state. `docs/ROADMAP.md` records a real sweep
 * matching **39 of 80** titles, so after a complete pass about half a real library still
 * has none of those fields, permanently. The unenriched row is the *majority* state of a
 * real library, and it was the common cause behind five findings in the audit (R-157).
 *
 * Two mechanisms, both exercised here: one row in six is always unenriched, so every
 * journey that draws a rail draws one; and `?unenriched` strips every row, so the app can
 * be walked through end to end as it looks before enrichment.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

/** Walk the main screens and fail on any crash or unhandled rejection. */
async function walkWithoutBreaking(page: Page, query: string) {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(`console: ${m.text()}`);
  });

  for (const [route, anchor] of [
    ['/', 'region:Featured'],
    ['/movies', 'heading:Movies'],
    ['/series', 'heading:Series'],
    ['/live', 'heading:Live TV'],
  ] as const) {
    await page.goto(`/${query}#${route}`);
    const [kind, name] = anchor.split(':') as ['region' | 'heading', string];
    await expect(page.getByRole(kind, { name })).toBeVisible();
  }

  // And a detail view, which is where most of the eight fields are drawn.
  await page.goto(`/${query}#/movies`);
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  await page.getByTestId('catalog-card').first().click();
  await expect(page.getByRole('dialog')).toBeVisible();
  await page.getByRole('dialog').getByRole('tab', { name: /details/i }).click();
  await expect(page.getByRole('dialog')).toContainText('Title');

  return errors;
}

test('the app draws a library nothing has enriched', async ({ page }) => {
  const errors = await walkWithoutBreaking(page, '?unenriched');
  expect(errors, 'the unenriched library broke something').toEqual([]);
  await page.screenshot({ path: `${SHOTS}/60-unenriched-detail.png` });
});

test('an unenriched card is on screen in an ordinary run too', async ({ page }) => {
  // One row in six, so a rail of twenty has three of them. This is what makes the state
  // reachable without a flag — a journey that only ever saw rich rows is how five
  // findings survived.
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();

  const cards = page.getByTestId('catalog-card');
  await expect(cards.first()).toBeVisible();
  const n = Math.min(await cards.count(), 24);
  expect(n, 'no cards to look at').toBeGreaterThan(6);

  // A card with no backdrop and no genres is the unenriched one. Found by asking the
  // host rather than by reading pixels: the mock is the transport, so this is the same
  // answer the screen is drawn from.
  const unenriched = await page.evaluate(async () => {
    const w = window as unknown as {
      __auroraInvoke: (c: string, a: unknown) => Promise<
        { id: number; backdrop: string | null; genres: string[]; overview: string | null }[]
      >;
    };
    const rows = await w.__auroraInvoke('library.movies', {
      sort: 'recentlyAdded', limit: 24, offset: 0,
    });
    return rows.filter((r) => r.backdrop === null && r.genres.length === 0).length;
  });
  expect(unenriched, 'every fixture film is still fully enriched').toBeGreaterThan(0);
});

// The case this most wants next — "More Like This" on a title with no genres, which has
// to say there is nothing to compare rather than show nine strangers — belongs with the
// fix for it (R-105) rather than here. That fix is in a separate branch; this one would
// pass against its `Similar` and fail against main's, which is the wrong way round for a
// test about fixtures.
