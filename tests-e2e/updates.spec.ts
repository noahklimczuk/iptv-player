/**
 * The update check (README §23).
 *
 * Aurora installs from a GitHub release, so the only thing that tells a viewer a fix
 * exists is this panel. The mock reports one patch ahead of the running build, which
 * is the state worth designing for — "you are up to date" is the easy half.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

async function openUpdates(page: Page) {
  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Updates' })).toBeVisible();
}

test('an available update says which version, and what changed', async ({ page }) => {
  await openUpdates(page);

  await expect(page.getByText('Update available')).toBeVisible();
  await expect(page.getByText('0.1.1', { exact: true }).first()).toBeVisible();

  // The notes are the reason to care, so they have to actually be on screen.
  await expect(page.getByText(/rolls to the next source/)).toBeVisible();

  // And it must be honest that it will not install anything itself.
  await expect(page.getByText(/does not install it for you/i)).toBeVisible();
  await expect(page.getByRole('button', { name: 'Get the update' })).toBeEnabled();

  await page.screenshot({ path: `${SHOTS}/34-updates.png` });
});

test('the running version is shown whether or not anything is available', async ({ page }) => {
  await openUpdates(page);
  // Which build this is, is the question you ask before reporting a bug.
  await expect(page.getByText('This build')).toBeVisible();
  await expect(page.getByText('0.1.0', { exact: true }).first()).toBeVisible();
});

test('checking again is possible without restarting', async ({ page }) => {
  await openUpdates(page);
  const check = page.getByRole('button', { name: 'Check now' });
  await expect(check).toBeEnabled();
  await check.click();
  // Still there afterwards rather than collapsing into a spinner that never returns.
  await expect(page.getByRole('button', { name: 'Check now' })).toBeEnabled();
  await expect(page.getByText('Update available')).toBeVisible();
});

test('the launch-time check can be turned off', async ({ page }) => {
  await openUpdates(page);

  const toggle = page.getByRole('switch', { name: 'Check for updates on launch' });
  await expect(toggle).toHaveAttribute('aria-checked', 'true');

  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-checked', 'false');
});
