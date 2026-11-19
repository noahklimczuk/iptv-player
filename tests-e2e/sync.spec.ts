/**
 * The Sync panel in Settings (docs/APPLE_PORT_AND_SYNC.md §3).
 *
 * Against the mock, which keeps a household on its "server" already: home@example.com,
 * password hunter22, passphrase "correct horse". The account flow itself is tested in
 * Rust against an in-memory Firebase (`aurora-sync/tests/household.rs`); this is what the
 * panel makes of it.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

async function openSync(page: Page, query = '') {
  await page.goto(`/${query}#/settings`);
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
}

test('a second device joins the household and is told what it has spent today', async ({ page }) => {
  await openSync(page);
  await page.getByRole('tab', { name: 'Join my household' }).click();
  await page.getByTestId('sync-email').fill('home@example.com');
  await page.getByTestId('sync-password').fill('hunter22');
  await page.getByTestId('sync-passphrase').fill('correct horse');
  await page.getByTestId('sync-device-name').fill('Bedroom PC');
  await page.getByRole('button', { name: 'Join', exact: true }).click();

  const card = page.getByTestId('sync-signed-in');
  await expect(card).toContainText('home@example.com');
  await expect(card).toContainText('on Bedroom PC');
  await expect(page.getByTestId('sync-state')).toContainText('Up to date');
  await expect(page.getByTestId('sync-usage')).toContainText('of the free plan');
  await expect(page.getByTestId('sync-thrifty')).toHaveCount(0);

  await card.scrollIntoViewIfNeeded();
  await page.screenshot({ path: `${SHOTS}/60-sync-signed-in.png` });
});

test('changes only a newer Aurora can read are counted, and say what will fix them', async ({ page }) => {
  await openSync(page, '?syncUnreadable');
  await page.getByRole('tab', { name: 'Join my household' }).click();
  await page.getByTestId('sync-email').fill('home@example.com');
  await page.getByTestId('sync-password').fill('hunter22');
  await page.getByTestId('sync-passphrase').fill('correct horse');
  await page.getByTestId('sync-device-name').fill('Bedroom PC');
  await page.getByRole('button', { name: 'Join', exact: true }).click();

  await expect(page.getByTestId('sync-signed-in')).toBeVisible();
  await expect(page.getByTestId('sync-unreadable')).toContainText(
    '2 changes from your other devices need a newer version of Aurora',
  );
  await expect(page.getByTestId('sync-unreadable')).toContainText('once this device updates');
});

test('with nothing unreadable, the panel says nothing about it', async ({ page }) => {
  await openSync(page);
  await page.getByRole('tab', { name: 'Join my household' }).click();
  await page.getByTestId('sync-email').fill('home@example.com');
  await page.getByTestId('sync-password').fill('hunter22');
  await page.getByTestId('sync-passphrase').fill('correct horse');
  await page.getByTestId('sync-device-name').fill('Bedroom PC');
  await page.getByRole('button', { name: 'Join', exact: true }).click();
  await expect(page.getByTestId('sync-signed-in')).toBeVisible();
  await expect(page.getByTestId('sync-unreadable')).toHaveCount(0);
});

test('a wrong passphrase is called that, and nothing is signed in', async ({ page }) => {
  await openSync(page);
  await page.getByRole('tab', { name: 'Join my household' }).click();
  await page.getByTestId('sync-email').fill('home@example.com');
  await page.getByTestId('sync-password').fill('hunter22');
  await page.getByTestId('sync-passphrase').fill('wrong horse');
  await page.getByRole('button', { name: 'Join', exact: true }).click();

  await expect(page.getByText(/passphrase/i).last()).toBeVisible();
  await expect(page.getByTestId('sync-signed-in')).toHaveCount(0);
});

test('setting sync up shows the recovery code once, then the signed-in card', async ({ page }) => {
  await openSync(page);
  await page.getByTestId('sync-email').fill('new@example.com');
  await page.getByTestId('sync-password').fill('hunter22');
  await page.getByTestId('sync-passphrase').fill('a long passphrase');
  await page.getByTestId('sync-confirm').fill('a long passphrase');
  await page.getByRole('button', { name: 'Set up sync', exact: true }).last().click();

  const recovery = page.getByTestId('sync-recovery');
  await expect(recovery).toContainText(/[0-9A-Z]{5}(-[0-9A-Z]{5}){10}/);
  await page.getByRole('button', { name: 'I have written it down' }).click();
  await expect(recovery).toHaveCount(0);
  await expect(page.getByTestId('sync-signed-in')).toContainText('new@example.com');
});

test('a build with no sync project says so instead of offering a form', async ({ page }) => {
  await openSync(page, '?noSync');
  await expect(page.getByTestId('sync-unavailable')).toBeVisible();
  await expect(page.getByTestId('sync-email')).toHaveCount(0);
});
