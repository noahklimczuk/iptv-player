/**
 * DVR journeys (README §7.7): scheduling from the guide, the recordings library, and
 * the series rules screen. Runs against the mock transport, which mirrors the host's
 * padding, duplicate rule and conflict arithmetic.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

async function settle(page: Page, ms = 700) {
  await page.waitForLoadState('networkidle').catch(() => {});
  await page.waitForTimeout(ms);
}

/** Select a guide cell for a programme that has not started yet. */
async function selectUpcoming(page: Page) {
  await page.goto('/#/guide');
  await settle(page);
  // The first cell in the grid is usually on air; pick one further right so Record
  // and Remind me are both meaningful.
  const cells = page.locator('button[title*="·"]');
  const count = await cells.count();
  for (let i = 0; i < count; i += 1) {
    const cell = cells.nth(i);
    if (!(await cell.isVisible())) continue;
    await cell.click();
    const remind = page.getByRole('button', { name: /remind me|reminder set/i });
    if (await remind.isVisible().catch(() => false)) return cell;
  }
  throw new Error('no upcoming programme found in the guide');
}

test('recordings library groups what is watchable, what is coming, and what failed', async ({
  page,
}) => {
  await page.goto('/#/recordings');
  await expect(page.getByRole('heading', { name: 'Recordings' })).toBeVisible();

  // Recorded is the landing tab: the question people open this page to answer.
  await expect(page.getByRole('tab', { name: /Recorded/ })).toHaveAttribute(
    'aria-selected',
    'true',
  );
  await expect(page.getByRole('button', { name: /^Play$/ }).first()).toBeVisible();

  await settle(page);
  await page.screenshot({ path: `${SHOTS}/20-recordings.png` });
});

test('deleting a recording asks first, and backing out keeps the file', async ({ page }) => {
  await page.goto('/#/recordings');
  await expect(page.getByRole('heading', { name: 'Recordings' })).toBeVisible();

  const plays = page.getByRole('button', { name: /^Play$/ });
  const before = await plays.count();
  expect(before).toBeGreaterThan(0);

  // Identify one recording by name and follow that one, rather than counting rows: the
  // row being asked about swaps its controls for the question, so a count of Play
  // buttons legitimately dips by one while it is open.
  const first = page.getByRole('button', { name: /^Delete / }).first();
  const name = (await first.getAttribute('aria-label'))!.replace(/^Delete /, '');

  // The delete control sits beside Keep, so the first click must not be the last word.
  await first.click();
  await expect(page.getByText(/Delete permanently, freeing/)).toBeVisible();
  await expect(page.getByText(name, { exact: true })).toBeVisible();

  await settle(page, 300);
  await page.screenshot({ path: `${SHOTS}/20-recordings-confirm-delete.png` });

  await page.getByRole('button', { name: 'Keep', exact: true }).click();
  await expect(page.getByText(/Delete permanently, freeing/)).toHaveCount(0);
  await expect(plays).toHaveCount(before);
  await expect(page.getByText(name, { exact: true })).toBeVisible();

  // And confirming does go through.
  await page.getByRole('button', { name: `Delete ${name}` }).click();
  await page.getByRole('button', { name: 'Delete', exact: true }).click();
  await expect(plays).toHaveCount(before - 1);
  await expect(page.getByText(name, { exact: true })).toHaveCount(0);
});

test('a recording in flight shows live progress, and failures say why', async ({ page }) => {
  await page.goto('/#/recordings');
  await page.getByRole('tab', { name: /Scheduled/ }).click();

  // The one in flight is badged and has a progress bar, not just a countdown.
  await expect(page.getByText('Recording', { exact: true })).toBeVisible();
  await expect(page.getByRole('progressbar').first()).toBeVisible();
  await expect(page.getByRole('button', { name: 'Stop' })).toBeVisible();

  // A failure is never silent: the reason is on the row.
  await expect(page.getByRole('heading', { name: /Didn.t record/ })).toBeVisible();
  await expect(page.getByText('Aurora was not running when this was due')).toBeVisible();

  await settle(page);
  await page.screenshot({ path: `${SHOTS}/21-recordings-scheduled.png` });
});

test('a clash is announced before it happens, naming what loses', async ({ page }) => {
  await page.goto('/#/recordings');
  await page.getByRole('tab', { name: /Scheduled/ }).click();

  const notice = page.getByRole('status');
  await expect(notice).toBeVisible();
  await expect(notice).toContainText(/more than your subscription allows/);
  await expect(notice).toContainText(/will be dropped/);
});

test('series rules list what they cover and can be paused', async ({ page }) => {
  await page.goto('/#/recordings');
  await page.getByRole('tab', { name: /Series rules/ }).click();

  await expect(page.getByText('New episodes only')).toBeVisible();
  const pause = page.getByRole('button', { name: 'Pause' }).first();
  await pause.click();
  await expect(page.getByRole('button', { name: 'Resume' }).first()).toBeVisible();

  await settle(page);
  await page.screenshot({ path: `${SHOTS}/22-recording-rules.png` });
});

test('Record in the guide schedules the airing and marks the cell', async ({ page }) => {
  const cell = await selectUpcoming(page);

  const record = page.getByRole('button', { name: 'Record', exact: true });
  await expect(record).toBeVisible();
  await record.click();

  // The button flips to the undo action, and the grid cell gains a record dot.
  await expect(page.getByRole('button', { name: 'Cancel recording' })).toBeVisible();
  await expect(cell.locator('[aria-label="Recording scheduled"]')).toBeVisible();

  await settle(page, 400);
  await page.screenshot({ path: `${SHOTS}/23-guide-record.png` });

  // And it undoes cleanly rather than scheduling a second copy.
  await page.getByRole('button', { name: 'Cancel recording' }).click();
  await expect(page.getByRole('button', { name: 'Record', exact: true })).toBeVisible();
  await expect(cell.locator('[aria-label="Recording scheduled"]')).toHaveCount(0);
});

test('a scheduled airing from the guide appears in the recordings page', async ({ page }) => {
  await selectUpcoming(page);
  const title = await page.locator('h2').first().innerText();
  await page.getByRole('button', { name: 'Record', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Cancel recording' })).toBeVisible();

  await page.goto('/#/recordings');
  await page.getByRole('tab', { name: /Scheduled/ }).click();
  await expect(page.getByText(title, { exact: false }).first()).toBeVisible();
});

test('Remind me toggles, and Record series becomes the undo action', async ({ page }) => {
  await selectUpcoming(page);

  const remind = page.getByRole('button', { name: 'Remind me' });
  await remind.click();
  await expect(page.getByRole('button', { name: 'Reminder set' })).toBeVisible();
  await page.getByRole('button', { name: 'Reminder set' }).click();
  await expect(page.getByRole('button', { name: 'Remind me' })).toBeVisible();

  await page.getByRole('button', { name: 'Record series' }).click();
  await expect(page.getByRole('button', { name: 'Stop recording series' })).toBeVisible();

  // The new rule is on the rules screen.
  await page.goto('/#/recordings');
  await page.getByRole('tab', { name: /Series rules/ }).click();
  await expect(page.getByText('New episodes only').first()).toBeVisible();
});
