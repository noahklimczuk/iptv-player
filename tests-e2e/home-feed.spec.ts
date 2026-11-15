/**
 * The home feed: every rail holds everything that belongs on it, and pages it in.
 *
 * Asked for: "make it so that each rail doesn't have a limit to how many items it
 * holds". Rails used to be cut at 24. Now the host builds each one whole and the screen
 * asks for the next page as a rail is scrolled, so these hold the screen to that: the
 * heading counts the whole rail, scrolling reaches past the first page, and a title the
 * viewer says "not for me" about leaves every rail at once.
 */
import { expect, test, type Page } from '@playwright/test';

interface MockCall { name: string; args: Record<string, unknown> }

const calls = (page: Page) =>
  page.evaluate(() => (window as unknown as { __auroraCalls: () => MockCall[] }).__auroraCalls());

async function openHome(page: Page) {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
}

/** Into the middle of the screen: at the bottom edge the tab bar covers a card's actions. */
async function centre(page: Page, rail: string) {
  const r = page.getByRole('region', { name: rail });
  await r.evaluate((el) => el.scrollIntoView({ block: 'center' }));
  return r;
}

const cards = (page: Page, rail: string) =>
  page.getByRole('region', { name: rail }).getByTestId('catalog-card');

test('a rail counts all of itself and scrolling it loads past the first page', async ({ page }) => {
  await openHome(page);
  const rail = await centre(page, 'Recently Added');
  const count = Number((await rail.getByTestId('rail-count').innerText()).replace(/,/g, ''));
  // The fixture library is 160 films; the first answer carries 30 of them.
  expect(count, 'the heading counts the whole rail').toBeGreaterThan(100);

  const scroller = rail.locator('.no-scrollbar');
  for (let i = 0; i < 8; i += 1) {
    await scroller.evaluate((el) => { el.scrollLeft = el.scrollWidth; });
    await page.waitForTimeout(120);
  }
  await expect.poll(async () => cards(page, 'Recently Added').count()).toBeGreaterThan(60);
  const pages = (await calls(page)).filter((c) => c.name === 'library.railPage');
  expect(pages.length).toBeGreaterThan(0);
  expect(pages[0]!.args.railId).toBe('recently-added');
  expect(pages[0]!.args.offset).toBe(30);
});

test('not for me takes a title off every rail at once', async ({ page }) => {
  await openHome(page);
  await centre(page, 'Recently Added');
  const first = cards(page, 'Recently Added').first();
  const name = (await first.getByRole('button').first().getAttribute('aria-label'))!;
  const title = name.replace(/, \d{4}$/, '');
  await first.hover();
  const notForMe = first.getByRole('button', { name: `Not for me: ${title}` });
  await expect(notForMe).toBeVisible();
  await notForMe.click();

  await expect(page.getByRole('button', { name, exact: true })).toHaveCount(0);
  const asked = (await calls(page)).filter((c) => c.name === 'library.notInterested');
  expect(asked).toHaveLength(1);
});

test('rails further down are drawn as they come near', async ({ page }) => {
  await openHome(page);
  const placeholders = page.getByTestId('rail-placeholder');
  const before = await placeholders.count();
  expect(before, 'not every rail is drawn up front').toBeGreaterThan(0);
  await page.getByRole('region', { name: 'Documentary' }).scrollIntoViewIfNeeded();
  await expect(cards(page, 'Documentary').first()).toBeVisible();
  expect(await placeholders.count()).toBeLessThan(before);
});
