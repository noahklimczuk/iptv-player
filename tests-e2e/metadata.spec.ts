/**
 * Metadata enrichment journeys (README §4.5): the key field, coverage, running a pass,
 * and cast reaching the detail modal. Runs against the mock transport, which mirrors
 * the host's batching, no-match recording and write-only key.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

async function settle(page: Page, ms = 600) {
  await page.waitForLoadState('networkidle').catch(() => {});
  await page.waitForTimeout(ms);
}

async function openMetadata(page: Page) {
  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Artwork and metadata' })).toBeVisible();
}

test('the metadata panel shows coverage and hides the key', async ({ page }) => {
  await openMetadata(page);

  // The field never shows a stored key back — only that one exists.
  const field = page.getByLabel('API key');
  await expect(field).toHaveAttribute('type', 'password');
  await expect(field).toHaveAttribute('placeholder', /a key is saved/i);
  await expect(field).toHaveValue('');

  await expect(page.getByText(/Movies/).first()).toBeVisible();
  await expect(page.getByRole('progressbar').first()).toBeVisible();

  await settle(page);
  await page.screenshot({ path: `${SHOTS}/30-metadata-settings.png` });
});

test('running a pass reports what matched and what did not', async ({ page }) => {
  await openMetadata(page);

  const run = page.getByRole('button', { name: 'Fetch metadata now' });
  await expect(run).toBeEnabled();
  await run.click();

  // A long pass is never a frozen spinner: the button says what it is doing.
  await expect(page.getByRole('button', { name: 'Fetching…' })).toBeVisible();

  const status = page.getByRole('status');
  await expect(status).toBeVisible({ timeout: 20_000 });
  await expect(status).toContainText(/matched/);
  // Not-found is reported as a settled answer, not an error.
  await expect(status).toContainText(/not found/);

  await settle(page, 300);
  await page.screenshot({ path: `${SHOTS}/31-metadata-run.png` });

  // Everything is answered now, so there is nothing left to ask about.
  await expect(page.getByText('Nothing left to look up.')).toBeVisible();
  await expect(run).toBeDisabled();
});

test('removing the key disables the fetch button and says why', async ({ page }) => {
  await openMetadata(page);

  await page.getByRole('button', { name: 'Remove' }).click();
  await expect(page.getByRole('status')).toContainText('Key removed');

  await expect(page.getByRole('button', { name: 'Fetch metadata now' })).toBeDisabled();
  await expect(page.getByText('Add a key to enable this.')).toBeVisible();
  // And the placeholder stops claiming a key is saved.
  await expect(page.getByLabel('API key')).toHaveAttribute('placeholder', /Paste your TMDB/);
});

test('saving a key re-enables enrichment', async ({ page }) => {
  await openMetadata(page);
  await page.getByRole('button', { name: 'Remove' }).click();
  await expect(page.getByRole('button', { name: 'Fetch metadata now' })).toBeDisabled();

  // Save is inert until something is typed, so an empty submit cannot clear the key.
  await expect(page.getByRole('button', { name: 'Save' })).toBeDisabled();

  await page.getByLabel('API key').fill('a-new-key');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('status')).toContainText('Key saved');
  // The field clears rather than holding the secret on screen.
  await expect(page.getByLabel('API key')).toHaveValue('');
  await expect(page.getByRole('button', { name: 'Fetch metadata now' })).toBeEnabled();
});

test('the detail modal shows enriched cast and director', async ({ page }) => {
  await page.goto('/#/');
  await settle(page);
  // The hero billboard's own button, same as the smoke test uses.
  await page.getByRole('button', { name: 'More Info' }).click();

  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();

  // Cast comes from enrichment now, not from the playlist's flat string list, and the
  // director row only exists because crew is stored alongside it.
  await expect(dialog.getByText('Cast:', { exact: false })).toBeVisible();
  await expect(dialog.getByText('Director:', { exact: false })).toBeVisible();

  await settle(page, 400);
  await page.screenshot({ path: `${SHOTS}/32-detail-credits.png` });
});

test('the artwork cache reports its size and can be filled and emptied', async ({ page }) => {
  await openMetadata(page);

  await expect(page.getByText('Artwork cache')).toBeVisible();
  await expect(page.getByText(/\d+ images ·/)).toBeVisible();

  const download = page.getByRole('button', { name: 'Download artwork' });
  await download.click();
  await expect(page.getByRole('status').last()).toContainText(/downloaded|already downloaded/, {
    timeout: 20_000,
  });

  await settle(page, 300);
  await page.screenshot({ path: `${SHOTS}/33-artwork-cache.png` });

  // Clearing needs no confirmation: the library keeps the remote URLs, so the only
  // cost is the next download.
  const clear = page.getByRole('button', { name: 'Clear cache' });
  await clear.click();
  await expect(page.getByRole('status').last()).toContainText(/images removed|image removed/);
  await expect(page.getByText('0 images · 0 MB')).toBeVisible();
  // Nothing left to clear, so the button stands down.
  await expect(clear).toBeDisabled();
});
