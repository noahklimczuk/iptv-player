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
  await page.getByRole('tab', { name: 'Sign in' }).click();
  await page.getByTestId('sync-email').fill('home@example.com');
  await page.getByTestId('sync-password').fill('hunter22');
  await page.getByTestId('sync-passphrase').fill('correct horse');
  await page.getByTestId('sync-device-name').fill('Bedroom PC');
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();

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
  await page.getByRole('tab', { name: 'Sign in' }).click();
  await page.getByTestId('sync-email').fill('home@example.com');
  await page.getByTestId('sync-password').fill('hunter22');
  await page.getByTestId('sync-passphrase').fill('correct horse');
  await page.getByTestId('sync-device-name').fill('Bedroom PC');
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();

  await expect(page.getByTestId('sync-signed-in')).toBeVisible();
  await expect(page.getByTestId('sync-unreadable')).toContainText(
    '2 changes from your other devices need a newer version of Aurora',
  );
  await expect(page.getByTestId('sync-unreadable')).toContainText('once this device updates');
});

test('sharing sign-ins and keys is off until chosen, and says what it means', async ({ page }) => {
  await openSync(page);
  await page.getByRole('tab', { name: 'Sign in' }).click();
  await page.getByTestId('sync-email').fill('home@example.com');
  await page.getByTestId('sync-password').fill('hunter22');
  await page.getByTestId('sync-passphrase').fill('correct horse');
  await page.getByTestId('sync-device-name').fill('Bedroom PC');
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();

  const share = page.getByTestId('sync-share-secrets');
  await expect(share).toHaveAttribute('aria-pressed', 'false');
  await expect(page.getByText(/each device keeps its own playlist passwords/)).toBeVisible();

  await share.click();
  await expect(share).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByText(/will go to your other devices from the next sync/)).toBeVisible();
  await expect(page.getByText(/stays in the household.s encrypted sync until you reset it/)).toBeVisible();

  await share.click();
  await expect(share).toHaveAttribute('aria-pressed', 'false');
  await expect(page.getByText('Sign-ins and keys are no longer sent.')).toBeVisible();
});

test('with nothing unreadable, the panel says nothing about it', async ({ page }) => {
  await openSync(page);
  await page.getByRole('tab', { name: 'Sign in' }).click();
  await page.getByTestId('sync-email').fill('home@example.com');
  await page.getByTestId('sync-password').fill('hunter22');
  await page.getByTestId('sync-passphrase').fill('correct horse');
  await page.getByTestId('sync-device-name').fill('Bedroom PC');
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await expect(page.getByTestId('sync-signed-in')).toBeVisible();
  await expect(page.getByTestId('sync-unreadable')).toHaveCount(0);
});

test('a wrong passphrase is called that, and nothing is signed in', async ({ page }) => {
  await openSync(page);
  await page.getByRole('tab', { name: 'Sign in' }).click();
  await page.getByTestId('sync-email').fill('home@example.com');
  await page.getByTestId('sync-password').fill('hunter22');
  await page.getByTestId('sync-passphrase').fill('wrong horse');
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();

  await expect(page.getByText(/passphrase/i).last()).toBeVisible();
  await expect(page.getByTestId('sync-signed-in')).toHaveCount(0);
});

async function fillNewAccount(page: Page, email = 'new@example.com') {
  await page.getByTestId('sync-email').fill(email);
  await page.getByTestId('sync-password').fill('hunter2222');
  await page.getByTestId('sync-password-again').fill('hunter2222');
  await page.getByTestId('sync-passphrase').fill('a long passphrase');
  await page.getByTestId('sync-confirm').fill('a long passphrase');
  await page.getByTestId('sync-agree').check();
}

async function signInHome(page: Page) {
  await page.getByRole('tab', { name: 'Sign in' }).click();
  await page.getByTestId('sync-email').fill('home@example.com');
  await page.getByTestId('sync-password').fill('hunter22');
  await page.getByTestId('sync-passphrase').fill('correct horse');
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
}

test('creating an account shows the recovery code once, then the signed-in card', async ({ page }) => {
  await openSync(page);
  await expect(page.getByRole('tab', { name: 'Create account' })).toHaveAttribute('aria-selected', 'true');
  await fillNewAccount(page);
  await page.getByRole('button', { name: 'Create account', exact: true }).click();

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

test('creating an account needs the password twice and agreement to the privacy policy', async ({ page }) => {
  await openSync(page);
  const create = page.getByRole('button', { name: 'Create account', exact: true });
  await fillNewAccount(page);
  await expect(create).toBeEnabled();

  await page.getByTestId('sync-agree').uncheck();
  await expect(create).toBeDisabled();
  await page.getByTestId('sync-agree').check();

  await page.getByTestId('sync-password-again').fill('something else');
  await expect(page.getByText('The passwords differ.')).toBeVisible();
  await expect(create).toBeDisabled();

  await page.getByTestId('sync-password').fill('short');
  await expect(page.getByText(/needs at least 8 characters/)).toBeVisible();

  // The passphrase must not be the password: one is sent to the service, the other never.
  await page.getByTestId('sync-password').fill('a long passphrase');
  await page.getByTestId('sync-password-again').fill('a long passphrase');
  await expect(page.getByText(/not the account password/)).toBeVisible();
  await expect(create).toBeDisabled();

  await page.getByTestId('sync-email').scrollIntoViewIfNeeded();
  await page.screenshot({ path: `${SHOTS}/61-sync-create-account.png` });
});

test('the account has its own entry in the Settings index', async ({ page }) => {
  await openSync(page);
  await page.getByRole('link', { name: 'Account and sync' }).or(
    page.getByRole('button', { name: 'Account and sync' }),
  ).first().click();
  await expect(page.getByRole('tab', { name: 'Create account' })).toBeInViewport();
});

test('a forgotten password can be reset from the sign-in form', async ({ page }) => {
  await openSync(page);
  await page.getByRole('tab', { name: 'Sign in' }).click();
  const forgot = page.getByTestId('sync-forgot-password');
  // Needs an address to send to.
  await expect(forgot).toBeDisabled();
  await page.getByTestId('sync-email').fill('home@example.com');
  await forgot.click();
  await expect(page.getByText(/If there is an Aurora account for home@example.com/)).toBeVisible();
});

test('a new account is asked to confirm its email, and can have the link sent again', async ({ page }) => {
  await openSync(page);
  await fillNewAccount(page, 'fresh@example.com');
  await page.getByRole('button', { name: 'Create account', exact: true }).click();
  await page.getByRole('button', { name: 'I have written it down' }).click();

  const verify = page.getByTestId('sync-verify');
  await expect(verify).toContainText('Confirm your email address');
  await expect(verify).toContainText('fresh@example.com');
  await verify.getByRole('button', { name: 'Send the email again' }).click();
  await expect(verify).toContainText('Sent.');
  await verify.getByRole('button', { name: 'I have confirmed it' }).click();
  await expect(verify).toContainText('Not confirmed yet');
});

test('a verified account is not nagged about its email', async ({ page }) => {
  await openSync(page);
  await signInHome(page);
  await expect(page.getByTestId('sync-signed-in')).toBeVisible();
  await expect(page.getByTestId('sync-verify')).toHaveCount(0);
});

test('the account can be deleted from the app, behind its password', async ({ page }) => {
  await openSync(page);
  await signInHome(page);
  await expect(page.getByTestId('sync-signed-in')).toBeVisible();

  await page.getByTestId('sync-delete-account').click();
  const form = page.getByTestId('sync-delete-form');
  await expect(form).toContainText('This cannot be undone');
  const confirm = form.getByRole('button', { name: 'Delete account permanently' });
  await expect(confirm).toBeDisabled();

  await page.getByTestId('sync-delete-password').fill('not it');
  await confirm.click();
  await expect(page.getByText(/Could not delete the account/)).toBeVisible();
  await expect(page.getByTestId('sync-signed-in')).toBeVisible();

  await page.getByTestId('sync-delete-account').click();
  await page.getByTestId('sync-delete-password').fill('hunter22');
  await page.getByTestId('sync-delete-form')
    .getByRole('button', { name: 'Delete account permanently' }).click();
  await expect(page.getByText(/have been deleted/)).toBeVisible();
  await expect(page.getByTestId('sync-signed-in')).toHaveCount(0);

  // The address no longer signs in.
  await signInHome(page);
  await expect(page.getByText('That email and password do not match a sync account.')).toBeVisible();
});
