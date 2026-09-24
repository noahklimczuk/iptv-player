/**
 * The update check, and installing what it finds (README §23).
 *
 * Aurora installs from a GitHub release, so the only thing that tells a viewer a fix
 * exists is this panel — and now the only thing that acts on it. The mock reports one
 * patch ahead of the running build and fills the same progress bar a real download
 * would, in about a second rather than forty megabytes.
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

  // Both routes are offered: install it here, or go and read the page first.
  await expect(page.getByRole('button', { name: 'Download update' })).toBeEnabled();
  await expect(page.getByRole('button', { name: 'Open release page' })).toBeEnabled();

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

test('downloading an update fills a bar and then offers to install it', async ({ page }) => {
  await openUpdates(page);
  await page.getByRole('button', { name: 'Download update' }).click();

  // The size is shown as it goes: a bar with no numbers on a 40 MB download is a
  // spinner with extra steps.
  await expect(page.getByText(/Downloading 0\.1\.1/)).toBeVisible();
  await expect(page.getByText(/of 37\.0 MB|of 38\.8 MB|MB$/).first()).toBeVisible();

  const install = page.getByRole('button', { name: 'Install and restart' });
  await expect(install).toBeVisible({ timeout: 10_000 });
  // The two things a person should know before pressing it.
  await expect(page.getByText(/checksum GitHub published/i)).toBeVisible();
  await expect(page.getByText(/not signed, so Windows will ask/i)).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/39-update-ready.png` });

  await install.click();
  // The browser has no host to exit, so the mock says what would have happened.
  await expect(page.getByText(/installer would run here/i)).toBeVisible();
});

test('a portable copy is offered the release page, not an installer', async ({ page }) => {
  // An NSIS installer would install beside the portable folder rather than replacing
  // it, which is how someone ends up with two copies and updates neither.
  await page.goto('/?portable#/settings');
  await expect(page.getByRole('heading', { name: 'Updates' })).toBeVisible();

  await expect(page.getByRole('button', { name: 'Get the update' })).toBeEnabled();
  await expect(page.getByRole('button', { name: 'Download update' })).toBeHidden();
  await expect(page.getByText(/copy is portable/i)).toBeVisible();
});

test('the launch-time check can be turned off', async ({ page }) => {
  await openUpdates(page);

  const toggle = page.getByRole('switch', { name: 'Check for updates on launch' });
  await expect(toggle).toHaveAttribute('aria-checked', 'true');

  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-checked', 'false');
});
