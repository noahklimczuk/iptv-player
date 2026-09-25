/**
 * Pause live TV (README §7.6).
 *
 * The feature is not the pause button — that already existed and froze the picture. It
 * is that the stream carries on being kept while nobody is watching it, so these
 * journeys spend real seconds waiting: the buffer growing, the delay opening up, and the
 * way back to the live edge. The mock transport models the same window the host reports
 * (`aurora_core::timeshift`), so what is asserted here is what the OSD does with it.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

/** Long enough to be behind live: the edge tolerance is five seconds either side. */
const BEHIND_MS = 7_000;

async function tuneLive(page: Page) {
  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();
  await page.getByRole('button', { name: 'Watch Meridian News' }).first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
}

/** The OSD hides itself after a few idle seconds; a mouse move brings it back. */
async function showOsd(page: Page) {
  await page.mouse.move(800, 500);
}

const timeshiftBar = (page: Page) => page.getByRole('slider', { name: 'Timeshift' });
const position = (page: Page) => page.getByLabel('Position relative to live');

test('a live channel is scrubbable through what has been buffered', async ({ page }) => {
  await tuneLive(page);
  await showOsd(page);

  // A live stream has no duration, so the VOD scrubber is not what is on screen.
  await expect(page.getByRole('slider', { name: 'Seek' })).toBeHidden();
  const bar = timeshiftBar(page);
  await expect(bar).toBeVisible();
  await expect(position(page)).toHaveText('Live');

  // The window grows with the stream: what is buffered after five seconds is five
  // seconds, whatever the budget allows.
  await page.waitForTimeout(5_000);
  await showOsd(page);
  await expect(page.getByText(/buffered$/)).toBeVisible();
  const max = Number(await bar.getAttribute('max'));
  expect(max).toBeGreaterThanOrEqual(4);
  await page.screenshot({ path: `${SHOTS}/36-timeshift-live.png` });
});

test('pausing live TV falls behind it, and Back to live catches up', async ({ page }) => {
  await tuneLive(page);
  await showOsd(page);
  await page.getByRole('button', { name: 'Pause' }).click();

  // Live TV carries on without the viewer. That is the whole feature.
  await page.waitForTimeout(BEHIND_MS);
  await showOsd(page);

  await expect(position(page)).toHaveText(/^−\d/);
  await expect(page.getByText(/behind live$/)).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/37-timeshift-behind.png` });

  // Resuming keeps the delay — this is the buffer playing now, not the live edge.
  await page.getByRole('button', { name: 'Play', exact: true }).click();
  await page.waitForTimeout(2_000);
  await showOsd(page);
  await expect(position(page)).toHaveText(/^−\d/);

  const back = page.getByRole('button', { name: 'Back to live' });
  await expect(back).toBeVisible();
  await back.click();
  await expect(position(page)).toHaveText('Live');
  await expect(back).toBeHidden();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
});

test('rewinding stops at the oldest moment held rather than before it', async ({ page }) => {
  await tuneLive(page);
  await page.waitForTimeout(6_000);
  await showOsd(page);

  const bar = timeshiftBar(page);
  // Twenty seconds of rewind asked of a six-second buffer.
  await page.getByRole('button', { name: 'Back 10 seconds' }).click();
  await page.getByRole('button', { name: 'Back 10 seconds' }).click();

  await expect
    .poll(async () => {
      const value = Number(await bar.inputValue());
      const min = Number(await bar.getAttribute('min'));
      return value - min;
    })
    .toBe(0);
});

test('T pauses live TV into the buffer, and again to rejoin it', async ({ page }) => {
  await tuneLive(page);

  // README §14.1 binds T to timeshift; the keymap in Settings says the same.
  await page.keyboard.press('t');
  await showOsd(page);
  await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeVisible();

  await page.waitForTimeout(BEHIND_MS);
  await page.keyboard.press('t');
  await showOsd(page);
  await expect(position(page)).toHaveText('Live');
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
});

test('turning the buffer off takes the scrub bar away and leaves a live badge', async ({
  page,
}) => {
  await page.goto('/#/settings');
  const toggle = page.getByRole('button', { name: 'Turn pause live TV off' });
  await expect(toggle).toBeVisible();
  // The panel is well down a long page; the screenshot is only useful looking at it.
  await toggle.scrollIntoViewIfNeeded();
  await page.screenshot({ path: `${SHOTS}/38-timeshift-settings.png` });
  await toggle.click();
  await expect(page.getByRole('button', { name: 'Turn pause live TV on' })).toBeVisible();

  await tuneLive(page);
  await showOsd(page);
  // Nothing is being kept, so there is nothing to scrub and the OSD says so plainly.
  await expect(timeshiftBar(page)).toBeHidden();
  await expect(page.getByText(/^Live · \d{2}:\d{2}/)).toBeVisible();
});
