/**
 * Continue Watching, and the progress that feeds it.
 *
 * `progress.save` was implemented on the host, registered, contract-tested, and given
 * a table with a completed-ratio rule — and called from exactly one place in the
 * repository: the mock transport. So every journey here showed a full Continue
 * Watching rail while a real machine saved nothing, ever, and the rail could only be
 * empty. The host's `library.rails` did not build the rail at all.
 *
 * The same shape as F-09 and F-12: the mock did the work the host was supposed to, and
 * the browser looked right the whole time.
 *
 * These assert the *outcome* rather than the traffic. Watching the transport is not
 * available here — `window.__auroraInvoke` is a copy of the binding the app imports,
 * so reassigning it intercepts nothing — and the outcome is the better subject: a film
 * that was not on the rail is on it, because it was watched.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

/** Each card's accessible name, one per card. */
async function titles(cards: ReturnType<Page['getByTestId']>): Promise<string[]> {
  const n = await cards.count();
  const out: string[] = [];
  for (let i = 0; i < n; i += 1) {
    const label = await cards.nth(i).getByRole('button').first().getAttribute('aria-label');
    if (label) out.push(label);
  }
  return out;
}

/**
 * The titles on the Continue Watching rail, reached *within* the app.
 *
 * Never `goto`: the mock's library lives in the page, so a reload would throw away
 * the progress the test just caused — which is the whole thing being measured.
 */
async function resumable(page: Page): Promise<string[]> {
  await page.keyboard.press('Escape');
  await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Home' }).click();
  // The home screen fetches its rails, so reading them the instant the link is
  // clicked gets whatever had arrived by then — which made this flap between three
  // and six items for reasons that had nothing to do with the feature.
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  const rail = page.getByRole('region', { name: 'Continue Watching' });
  if ((await rail.count()) === 0) return [];
  const cards = rail.getByTestId('catalog-card');
  await expect(cards.first()).toBeVisible();
  // Settled: the count has to stop moving before the list means anything.
  let seen = -1;
  for (let i = 0; i < 20 && seen !== (await cards.count()); i += 1) {
    seen = await cards.count();
    await page.waitForTimeout(100);
  }
  return titles(cards);
}

test('a film you watched joins Continue Watching, because the app saved where you got to', async ({
  page,
}) => {
  await page.goto('/#/');
  const before = await resumable(page);

  // A film that is not already on the rail, so the assertion cannot pass on a seeded
  // fixture alone.
  await page.getByRole('navigation', { name: 'Main' })
    .getByRole('link', { name: 'Movies' })
    .click();
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  const cards = page.getByTestId('catalog-card');
  const all = await titles(cards);
  const picked = all.find((t) => !before.includes(t)) ?? null;
  expect(picked, 'every film in the library was already resumable').not.toBeNull();
  await cards.nth(all.indexOf(picked!)).getByRole('button').first().click();
  await page.getByRole('button', { name: /^Play/ }).first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();

  // Watch a real way in. The rail deliberately ignores anything under a minute — a
  // film someone opened and closed again is not one they are partway through — so a
  // test that pauses on the opening frame is testing nothing.
  await page.evaluate(() => {
    const w = window as unknown as {
      __auroraInvoke: (c: string, a: unknown) => Promise<unknown>;
    };
    return w.__auroraInvoke('player.seek', { positionSecs: 600, relative: false });
  });

  // Pausing leaves `playing`, which is when the app flushes. The ten-second timer
  // will not have fired — and that is the point: the position worth keeping is the
  // one at the moment somebody stops.
  await page.getByRole('button', { name: 'Pause' }).click();
  await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeVisible();

  await expect
    .poll(async () => await resumable(page), { timeout: 15_000 })
    .toContain(picked!);

  await page.screenshot({ path: `${SHOTS}/47-continue-watching.png` });
});

test('the rail comes before the others and shows how far in', async ({ page }) => {
  await page.goto('/#/');
  const rail = page.getByRole('region', { name: 'Continue Watching' });
  await expect(rail).toBeVisible();

  // Ahead of the rails that are about the library rather than about the viewer.
  const names = (
    await Promise.all(
      (await page.getByRole('region').all()).map((r) => r.getAttribute('aria-label')),
    )
  ).filter((n): n is string => !!n);
  const mine = names.indexOf('Continue Watching');
  const library = names.indexOf('Recently added');
  expect(mine).toBeGreaterThanOrEqual(0);
  if (library >= 0) expect(mine).toBeLessThan(library);

  // The bar is the whole point of the rail. It used to be read from the mock module's
  // own memory, so it drew here and was dead on a real machine; it now comes from the
  // rail the host sent.
  await expect(rail.getByTestId('card-progress').first()).toBeVisible();
});

test('live TV is not recorded — there is no position to come back to', async ({ page }) => {
  await page.goto('/#/');
  const before = await resumable(page);

  await page.getByRole('navigation', { name: 'Main' })
    .getByRole('link', { name: 'Live TV' })
    .click();
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();
  await page.getByTestId('channel-row').first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await page.getByRole('button', { name: 'Pause' }).click();
  await page.waitForTimeout(500);

  // A channel has no end to be a fraction of and no position worth resuming, so the
  // rail must be exactly as it was.
  expect(await resumable(page)).toEqual(before);
});
