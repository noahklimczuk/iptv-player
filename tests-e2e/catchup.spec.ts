/**
 * Catch-up playback (README §7.1): "Watch from start" in the guide.
 *
 * The button is only half the feature — the other half is being told why a programme
 * cannot be replayed, so these journeys cover both what plays and what the viewer sees
 * when the provider's window has closed. The mock transport mirrors the host's
 * refusals (aurora-app::window::resolve_catchup) word for word.
 */
import { expect, test, type Locator, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

/** Channel cells lead every guide row and begin with the channel number. */
const channelCells = (page: Page) => page.getByRole('button', { name: /^\d{3}\s/ });

const withCatchup = (page: Page) =>
  channelCells(page).filter({ has: page.getByTitle('Catch-up available') });

const withoutCatchup = (page: Page) =>
  channelCells(page).filter({ hasNot: page.getByTitle('Catch-up available') });

/** Programme cells carry `title="<name> · HH:MM–HH:MM"`; channel cells carry none. */
const programmesIn = (cell: Locator) =>
  cell.locator('xpath=..').locator('button[title*="·"]');

async function openGuide(page: Page) {
  await page.goto('/#/guide');
  await expect(page.getByRole('heading', { name: 'TV Guide' })).toBeVisible();
  // The grid virtualizes; wait for the first row before locating anything in it.
  await expect(channelCells(page).first()).toBeVisible();
}

/** Walk the guide back a whole number of days. */
async function goBackDays(page: Page, days: number) {
  const back = page.getByRole('button', { name: '−24h' });
  for (let i = 0; i < days; i += 1) await back.click();
}

/** The OSD hides itself after a few idle seconds; a mouse move brings it back. */
async function showOsd(page: Page) {
  await page.mouse.move(800, 500);
}

/** "14:00–15:30 · 1:30:00 · S2E4" → 5400. */
function selectedDurationSecs(meta: string): number {
  const part = meta.split('·').map((s) => s.trim()).find((s) => /^\d+(:\d{2})+$/.test(s));
  if (!part) throw new Error(`no duration in info pane meta: ${meta}`);
  return part.split(':').reduce((acc, n) => acc * 60 + Number(n), 0);
}

test('Watch from start replays the running programme with a scrubber, not a live edge', async ({
  page,
}) => {
  await openGuide(page);

  const cell = withCatchup(page).first();
  await programmesIn(cell).first().click();

  const info = page.locator('aside');
  const meta = await info.getByText(/\d{2}:\d{2}–\d{2}:\d{2} ·/).first().textContent();
  const expected = selectedDurationSecs(meta ?? '');

  const watch = page.getByRole('button', { name: 'Watch from start' });
  await expect(watch).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/24-guide-catchup.png` });
  await watch.click();

  await showOsd(page);
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();

  // A live tune renders no scrubber at all (PlayerOverlay's Scrubber branches on
  // isLive), so the slider existing is the assertion that this is a replay — and its
  // range is the programme, not the channel.
  const seek = page.getByRole('slider', { name: 'Seek' });
  await expect(seek).toBeVisible();
  await expect(seek).toHaveAttribute('max', String(expected));
  await expect(seek).toHaveValue('0');
  await page.screenshot({ path: `${SHOTS}/25-catchup-playing.png` });
});

test('a channel without catch-up offers live only', async ({ page }) => {
  await openGuide(page);

  const cell = withoutCatchup(page).first();
  await programmesIn(cell).first().click();

  // The same programme, on a channel that cannot replay it: no dead button offered.
  await expect(page.getByRole('button', { name: 'Watch now' })).toBeVisible();
  await expect(page.getByRole('button', { name: /^Watch (from start|this)$/ })).toBeHidden();
});

test('a programme that has not started yet cannot be caught up', async ({ page }) => {
  await openGuide(page);

  const cell = withCatchup(page).first();
  // The leftmost cell is on air, so the one after it has not begun.
  await programmesIn(cell).nth(1).click();

  await expect(page.getByRole('button', { name: /remind me|reminder set/i })).toBeVisible();
  await expect(page.getByRole('button', { name: /^Watch (from start|this)$/ })).toBeHidden();
});

test('a programme that finished earlier can still be caught up', async ({ page }) => {
  await openGuide(page);
  await goBackDays(page, 1);

  const cell = withCatchup(page).first();
  await programmesIn(cell).first().click();

  // Yesterday, so the wording changes: there is no "start" to return to.
  const watch = page.getByRole('button', { name: 'Watch this' });
  await expect(watch).toBeVisible();
  await watch.click();

  await showOsd(page);
  await expect(page.getByRole('slider', { name: 'Seek' })).toBeVisible();
});

test('past the provider window, the guide says so instead of opening a blank player', async ({
  page,
}) => {
  await openGuide(page);
  // Beyond the deepest window any channel in the fixtures offers.
  await goBackDays(page, 8);

  const cell = withCatchup(page).first();
  await programmesIn(cell).first().click();

  await page.getByRole('button', { name: 'Watch this' }).click();

  const alert = page.getByRole('alert').filter({ hasText: /catch-up window/ });
  await expect(alert).toBeVisible();
  await expect(alert).toContainText(/\d+-day catch-up window/);
  // The refusal stays in the guide: no player, no scrubber, nothing to dismiss.
  await expect(page.getByRole('slider', { name: 'Seek' })).toBeHidden();
  await page.screenshot({ path: `${SHOTS}/26-catchup-expired.png` });
});
