/**
 * Settings → Privacy (README §15: "history retention, clear history").
 *
 * Neither existed: the only way to forget anything watched was one card at a time off
 * Continue Watching. These journeys check the two controls do what they say on the
 * screens the history actually shows up on, not just in the panel.
 */
import { expect, test, type Page } from '@playwright/test';

/**
 * Opens Settings and hands back the Privacy panel.
 *
 * `goto` only here, at the start: the mock's library lives in the page, and a reload
 * would bring back the history a journey just cleared.
 */
async function privacy(page: Page) {
  await page.goto('/#/settings');
  const panel = page.getByRole('region', { name: 'Privacy' });
  await expect(panel).toBeVisible();
  return panel;
}

async function home(page: Page) {
  await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Home' }).click();
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
}

test('clearing watch history empties Continue Watching, after asking once', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Continue Watching' })).toBeVisible();

  const panel = await privacy(page);
  await expect(panel.getByText(/^\d+ titles$/)).toBeVisible();

  // One press only asks; Keep backs out with nothing lost.
  await panel.getByRole('button', { name: 'Clear history' }).click();
  await panel.getByRole('button', { name: 'Keep' }).click();
  await expect(panel.getByRole('button', { name: 'Clear history' })).toBeEnabled();

  await panel.getByRole('button', { name: 'Clear history' }).click();
  await panel.getByRole('button', { name: 'Yes, clear it' }).click();
  await expect(panel.getByRole('status')).toContainText(/Forgot \d+ titles/);
  await expect(panel.getByText('0 titles')).toBeVisible();
  // Nothing left to clear, so nothing to press.
  await expect(panel.getByRole('button', { name: 'Clear history' })).toBeDisabled();

  await home(page);
  // Give the rails their round trip before claiming one is absent.
  await page.waitForTimeout(500);
  await expect(page.getByRole('region', { name: 'Continue Watching' })).toHaveCount(0);
});

test('the history window starts at forever and keeps what is chosen', async ({ page }) => {
  const panel = await privacy(page);
  const windows = panel.getByRole('group', { name: 'Keep watch history' });
  await expect(windows.getByRole('button', { name: 'Forever' })).toHaveAttribute('aria-pressed', 'true');

  // Everything seeded was watched within the day, so a 30-day window forgets none of it.
  await windows.getByRole('button', { name: '30 days' }).click();
  await expect(panel.getByRole('status')).toHaveText('Kept for 30 days.');
  await expect(windows.getByRole('button', { name: '30 days' })).toHaveAttribute('aria-pressed', 'true');
  await expect(windows.getByRole('button', { name: 'Forever' })).toHaveAttribute('aria-pressed', 'false');

  await home(page);
  await expect(page.getByRole('region', { name: 'Continue Watching' })).toBeVisible();

  await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Settings' }).click();
  const again = page.getByRole('region', { name: 'Privacy' });
  await expect(
    again.getByRole('group', { name: 'Keep watch history' }).getByRole('button', { name: '30 days' }),
  ).toHaveAttribute('aria-pressed', 'true');

  await again.getByRole('group', { name: 'Keep watch history' }).getByRole('button', { name: 'Forever' }).click();
  await expect(again.getByRole('status')).toHaveText('Watch history is kept for good.');
});

test('the panel says plainly that no usage data is sent', async ({ page }) => {
  const panel = await privacy(page);
  await expect(panel.getByText('None sent')).toBeVisible();
  await expect(panel.getByText(/no analytics or crash reporting/)).toBeVisible();
});
