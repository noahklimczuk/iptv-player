/**
 * First-run wizard (README §13): from nothing to a populated library.
 * `?setup` forces the wizard so the journey is testable without clearing state.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

async function openWizard(page: Page) {
  await page.goto('/?setup#/');
  await expect(page.getByRole('heading', { name: 'Welcome to Aurora TV' })).toBeVisible();
}

test('pasting an Xtream URL splits out the credentials', async ({ page }) => {
  await openWizard(page);

  await page
    .getByLabel('Provider address')
    .fill('http://panel.example.com:8080/get.php?username=alice&password=hunter2');

  await expect(page.getByText(/Recognised an Xtream address/)).toBeVisible();
  await expect(page.getByLabel('Username')).toHaveValue('alice');
  await expect(page.getByLabel('Password')).toHaveValue('hunter2');
  // The provider is named after its host so the user does not have to.
  await expect(page.getByLabel('Provider name')).toHaveValue('panel.example.com');

  await page.screenshot({ path: `${SHOTS}/18-wizard-detect.png` });
});

test('a plain playlist URL does not ask for credentials', async ({ page }) => {
  await openWizard(page);
  await page.getByLabel('Provider address').fill('http://example.com/list.m3u');

  await expect(page.getByText(/Recognised an Xtream address/)).toBeHidden();
  await expect(page.getByLabel('Username')).toBeHidden();
});

test('validation reports account status before anything is saved', async ({ page }) => {
  await openWizard(page);
  await page
    .getByLabel('Provider address')
    .fill('http://panel.example.com/get.php?username=alice&password=hunter2');

  // Continue stays disabled until the connection is proven.
  await expect(page.getByRole('button', { name: 'Continue' })).toBeDisabled();

  await page.getByRole('button', { name: 'Check connection' }).click();
  await expect(page.getByRole('status')).toContainText('Connected');
  await expect(page.getByText('41 days left')).toBeVisible();
  await expect(page.getByText('1/2 connections')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Continue' })).toBeEnabled();
});

test('a bad address is refused with a readable reason', async ({ page }) => {
  await openWizard(page);
  await page.getByLabel('Provider address').fill('not-a-url');
  await page.getByRole('button', { name: 'Check connection' }).click();

  await expect(page.getByRole('status')).toContainText('does not look like a URL');
  await expect(page.getByRole('button', { name: 'Continue' })).toBeDisabled();
});

test('the full journey ends in a populated library', async ({ page }) => {
  await openWizard(page);
  await page
    .getByLabel('Provider address')
    .fill('http://panel.example.com/get.php?username=alice&password=hunter2');
  await page.getByRole('button', { name: 'Check connection' }).click();
  await page.getByRole('button', { name: 'Continue' }).click();

  // Content selection.
  await expect(page.getByRole('heading', { name: /What should Aurora import/ })).toBeVisible();
  await expect(page.getByRole('switch', { name: 'Live TV' })).toHaveAttribute(
    'aria-checked',
    'true',
  );
  await page.screenshot({ path: `${SHOTS}/19-wizard-content.png` });

  await page.getByRole('button', { name: 'Import library' }).click();

  // Progress is reported by phase, not a bare spinner.
  await expect(page.getByRole('status')).toContainText(/Importing|Fetching|Matching|Building/);
  await page.screenshot({ path: `${SHOTS}/20-wizard-importing.png` });

  // ...and finishes with a real summary.
  await expect(page.getByRole('heading', { name: 'Your library is ready' })).toBeVisible({
    timeout: 15_000,
  });
  await expect(page.getByText(/Guide data matched/)).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/21-wizard-done.png` });

  await page.getByRole('button', { name: 'Start watching' }).click();
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
});

test('deselecting every content type blocks the import', async ({ page }) => {
  await openWizard(page);
  await page
    .getByLabel('Provider address')
    .fill('http://panel.example.com/get.php?username=a&password=b');
  await page.getByRole('button', { name: 'Check connection' }).click();
  await page.getByRole('button', { name: 'Continue' }).click();

  for (const name of ['Live TV', 'Movies', 'Series']) {
    await page.getByRole('switch', { name, exact: true }).click();
  }
  await expect(page.getByRole('button', { name: 'Import library' })).toBeDisabled();
});

test('the wizard can be skipped', async ({ page }) => {
  await openWizard(page);
  await page.getByRole('button', { name: 'Skip for now' }).click();
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
});

test('a bare panel host offers the credential fields', async ({ page }) => {
  await openWizard(page);

  // The shape on a provider's credentials card: a host on one line, the username and
  // password on the next two. Nothing in the URL to detect — which is exactly why this
  // journey used to dead-end, with the fields never appearing.
  await page.getByLabel('Provider address').fill('http://12345678.panel-host.example');

  // The banner must say what actually happened. Claiming the credentials were "filled
  // in for you" when the fields are empty is a lie the user can see.
  await expect(page.getByText(/Looks like a panel login/)).toBeVisible();
  await expect(page.getByText(/filled in for you/)).toBeHidden();

  await expect(page.getByLabel('Username')).toBeVisible();
  await expect(page.getByLabel('Password')).toBeVisible();
  // Nothing was invented to fill them with.
  await expect(page.getByLabel('Username')).toHaveValue('');

  await page.getByLabel('Username').fill('AB12CD34');
  await page.getByLabel('Password').fill('not-a-real-password');
  await expect(page.getByLabel('Username')).toHaveValue('AB12CD34');

  await page.screenshot({ path: `${SHOTS}/19-wizard-bare-host.png` });
});

test('the provider type can be chosen when detection guesses wrong', async ({ page }) => {
  await openWizard(page);
  await page.getByLabel('Provider address').fill('http://example.com/list.m3u');

  // Detected as a playlist, and no credentials asked for.
  await expect(page.getByLabel('Username')).toBeHidden();

  // But a panel that happens to serve its playlist at a path is still reachable:
  // the choice is always offered, never inferred away.
  await page.getByRole('button', { name: 'Xtream / panel login' }).click();
  await expect(page.getByLabel('Username')).toBeVisible();

  await page.getByRole('button', { name: 'M3U playlist URL' }).click();
  await expect(page.getByLabel('Username')).toBeHidden();
});
