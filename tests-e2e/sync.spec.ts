/**
 * The Sync panel in Settings, and the wizard that sets sync up
 * (docs/APPLE_PORT_AND_SYNC.md §3; `features/settings/SyncSetupWizard.tsx`).
 *
 * Against the mock, which keeps a household on its "server" already: home@example.com,
 * password hunter22, passphrase "correct horse". The account flow itself is tested in
 * Rust against an in-memory Firebase (`aurora-sync/tests/household.rs`); this is what the
 * panel and the wizard make of it.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

async function openSync(page: Page, query = '') {
  await page.goto(`/${query}#/settings`);
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
}

const wizard = (page: Page) => page.getByTestId('sync-setup');

/** Settings → Account and sync → Set up sync. */
async function startSetup(page: Page) {
  await page.getByTestId('sync-setup-start').click();
  await expect(page.getByRole('dialog', { name: 'Sync your devices' })).toBeVisible();
}

async function next(page: Page, name = 'Continue') {
  await wizard(page).getByRole('button', { name, exact: true }).click();
}

/** The second device's way in: up to pressing Sign in, so a refusal can be looked at. */
async function joinHousehold(page: Page, opts: { password?: string; passphrase?: string } = {}) {
  await startSetup(page);
  await page.getByTestId('sync-path-join').click();
  await page.getByTestId('sync-email').fill('home@example.com');
  await page.getByTestId('sync-password').fill(opts.password ?? 'hunter22');
  await next(page);
  await page.getByTestId('sync-passphrase').fill(opts.passphrase ?? 'correct horse');
  await next(page);
  await page.getByTestId('sync-device-name').fill('Bedroom PC');
  await next(page, 'Sign in');
}

/** Joined, and the wizard closed on its last step. */
async function signInHome(page: Page) {
  await joinHousehold(page);
  const done = page.getByTestId('sync-setup-done');
  await expect(done).toBeVisible();
  await done.getByRole('button', { name: 'Done' }).click();
  await expect(wizard(page)).toHaveCount(0);
}

/** The first device's way in, up to the last step before anything is sent. */
async function fillNewAccount(page: Page, email = 'new@example.com') {
  await startSetup(page);
  await page.getByTestId('sync-path-create').click();
  await page.getByTestId('sync-email').fill(email);
  await page.getByTestId('sync-password').fill('hunter2222');
  await page.getByTestId('sync-password-again').fill('hunter2222');
  await next(page);
  await page.getByTestId('sync-passphrase').fill('a long passphrase');
  await page.getByTestId('sync-confirm').fill('a long passphrase');
  await next(page);
  await page.getByTestId('sync-agree').check();
}

/** Create the account and get past the recovery code. */
async function createAccount(page: Page, email = 'new@example.com') {
  await fillNewAccount(page, email);
  await next(page, 'Create account');
  await expect(page.getByTestId('sync-recovery-code')).toBeVisible();
  await page.getByTestId('sync-recovery-saved').check();
  await next(page);
}

test('a second device joins the household and is told what it has spent today', async ({ page }) => {
  await openSync(page);
  await signInHome(page);

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
  await signInHome(page);

  await expect(page.getByTestId('sync-signed-in')).toBeVisible();
  await expect(page.getByTestId('sync-unreadable')).toContainText(
    '2 changes from your other devices need a newer version of Aurora',
  );
  await expect(page.getByTestId('sync-unreadable')).toContainText('once this device updates');
});

test('sharing sign-ins and keys is off until chosen, and says what it means', async ({ page }) => {
  await openSync(page);
  await signInHome(page);

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
  await signInHome(page);
  await expect(page.getByTestId('sync-signed-in')).toBeVisible();
  await expect(page.getByTestId('sync-unreadable')).toHaveCount(0);
});

test('the wizard starts with a choice, and says what sync is before asking for anything', async ({ page }) => {
  await openSync(page);
  await startSetup(page);
  const dialog = wizard(page);
  await expect(dialog).toContainText('Encrypted on this device before it is uploaded');
  await expect(page.getByTestId('sync-path-create')).toContainText('This is my first device');
  await expect(page.getByTestId('sync-path-join')).toContainText('I already use sync');
  await expect(page.getByTestId('sync-email')).toHaveCount(0);
  await page.screenshot({ path: `${SHOTS}/62-sync-setup-choose.png` });

  // Escape before anything is sent leaves nothing behind.
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(page.getByTestId('sync-signed-out')).toBeVisible();
});

test('a wrong passphrase reopens the passphrase step, and nothing is signed in', async ({ page }) => {
  await openSync(page);
  await joinHousehold(page, { passphrase: 'wrong horse' });

  await expect(page.getByRole('heading', { name: /Unlock your household/ })).toBeVisible();
  await expect(wizard(page).getByRole('alert')).toContainText(/passphrase/i);
  await expect(page.getByTestId('sync-passphrase')).toBeVisible();

  // Fixed where it went wrong, and on from there.
  await page.getByTestId('sync-passphrase').fill('correct horse');
  await next(page);
  await next(page, 'Sign in');
  await expect(page.getByTestId('sync-setup-done')).toContainText('from your other devices');
});

test('a wrong password reopens the account step', async ({ page }) => {
  await openSync(page);
  await joinHousehold(page, { password: 'not it' });
  await expect(page.getByRole('heading', { name: 'Sign in to your household' })).toBeVisible();
  await expect(wizard(page).getByRole('alert'))
    .toContainText('That email and password do not match a sync account.');
});

test('a forgotten passphrase can be replaced by the recovery code when joining', async ({ page }) => {
  await openSync(page);
  await startSetup(page);
  await page.getByTestId('sync-path-join').click();
  await page.getByTestId('sync-email').fill('home@example.com');
  await page.getByTestId('sync-password').fill('hunter22');
  await next(page);
  await wizard(page).getByRole('button', { name: /Use the recovery code/ }).click();
  await expect(page.getByTestId('sync-code')).toBeVisible();
  await expect(page.getByTestId('sync-passphrase')).toHaveCount(0);
});

test('creating an account keeps the two secrets apart, a step each', async ({ page }) => {
  await openSync(page);
  await startSetup(page);
  await page.getByTestId('sync-path-create').click();

  // Step one: the account. Nothing about a passphrase yet.
  await expect(page.getByRole('list', { name: 'Step 1 of 5' })).toBeVisible();
  await expect(page.getByTestId('sync-passphrase')).toHaveCount(0);
  const go = wizard(page).getByRole('button', { name: 'Continue' });
  await page.getByTestId('sync-email').fill('new@example.com');
  await page.getByTestId('sync-password').fill('short');
  await expect(page.getByText(/needs at least 8 characters/)).toBeVisible();
  await page.getByTestId('sync-password').fill('hunter2222');
  await page.getByTestId('sync-password-again').fill('something else');
  await expect(page.getByText('The passwords differ.')).toBeVisible();
  await expect(go).toBeDisabled();
  await page.getByTestId('sync-password-again').fill('hunter2222');
  await go.click();

  // Step two: the passphrase, with what it is for.
  await expect(page.getByRole('list', { name: 'Step 2 of 5' })).toBeVisible();
  await expect(wizard(page)).toContainText('never leaves your devices');
  await page.getByTestId('sync-passphrase').fill('hunter2222');
  await page.getByTestId('sync-confirm').fill('hunter2222');
  await expect(page.getByText(/not the account password/)).toBeVisible();
  await expect(go).toBeDisabled();

  await page.getByTestId('sync-passphrase').fill('short');
  await expect(page.getByTestId('sync-strength')).toContainText('Too short');
  await page.getByTestId('sync-passphrase').fill('orange kettle window seven');
  await page.getByTestId('sync-confirm').fill('orange kettle window seven');
  await expect(page.getByTestId('sync-strength')).toContainText('Strong');
  await page.getByTestId('sync-reveal').check();
  await expect(page.getByTestId('sync-passphrase')).toHaveAttribute('type', 'text');
  await page.screenshot({ path: `${SHOTS}/63-sync-setup-passphrase.png` });
  await go.click();

  // Step three: the device, and the privacy policy before anything is sent.
  const create = wizard(page).getByRole('button', { name: 'Create account', exact: true });
  await expect(page.getByTestId('sync-device-name')).toHaveValue('LIVING-ROOM-PC');
  await expect(create).toBeDisabled();
  await page.getByTestId('sync-agree').check();
  await expect(create).toBeEnabled();
  await page.screenshot({ path: `${SHOTS}/61-sync-create-account.png` });

  // Back keeps what was typed.
  await wizard(page).getByRole('button', { name: 'Back' }).click();
  await expect(page.getByTestId('sync-passphrase')).toHaveValue('orange kettle window seven');
});

test('the recovery code has to be saved before going on, and cannot be closed over', async ({ page }) => {
  await openSync(page);
  await fillNewAccount(page);
  await next(page, 'Create account');

  const code = page.getByTestId('sync-recovery-code');
  await expect(code).toContainText(/[0-9A-Z]{5}(-[0-9A-Z]{5}){10}/);
  await expect(wizard(page).getByRole('button', { name: 'Close sync setup' })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(code).toBeVisible();

  const go = wizard(page).getByRole('button', { name: 'Continue' });
  await expect(go).toBeDisabled();
  await page.screenshot({ path: `${SHOTS}/64-sync-setup-recovery.png` });
  await page.getByTestId('sync-recovery-saved').check();
  await go.click();
  await expect(page.getByTestId('sync-setup-verify')).toBeVisible();
});

test('creating an account ends with how to add the next device', async ({ page }) => {
  await openSync(page);
  await createAccount(page);
  await wizard(page).getByRole('button', { name: 'Later' }).click();

  const done = page.getByTestId('sync-setup-done');
  await expect(done).toContainText('now in your household’s encrypted sync');
  await expect(page.getByTestId('sync-next-device')).toContainText('I already use sync on another device');
  await expect(page.getByTestId('sync-next-device')).toContainText('new@example.com');
  await page.screenshot({ path: `${SHOTS}/65-sync-setup-done.png` });

  await done.getByRole('button', { name: 'Done' }).click();
  await expect(page.getByTestId('sync-signed-in')).toContainText('new@example.com');
});

test('sharing sign-ins can be chosen while setting up', async ({ page }) => {
  await openSync(page);
  await fillNewAccount(page, 'sharer@example.com');
  await page.getByTestId('sync-setup-share').check();
  await next(page, 'Create account');
  await page.getByTestId('sync-recovery-saved').check();
  await next(page);
  await wizard(page).getByRole('button', { name: 'Later' }).click();
  await page.getByTestId('sync-setup-done').getByRole('button', { name: 'Done' }).click();
  await expect(page.getByTestId('sync-share-secrets')).toHaveAttribute('aria-pressed', 'true');
});

test('a build with no sync project says so instead of offering a form', async ({ page }) => {
  await openSync(page, '?noSync');
  await expect(page.getByTestId('sync-unavailable')).toBeVisible();
  await expect(page.getByTestId('sync-setup-start')).toHaveCount(0);
});

test('the account has its own entry in the Settings index', async ({ page }) => {
  await openSync(page);
  await page.getByRole('link', { name: 'Account and sync' }).or(
    page.getByRole('button', { name: 'Account and sync' }),
  ).first().click();
  await expect(page.getByTestId('sync-setup-start')).toBeInViewport();
});

test('a link can open the wizard straight away', async ({ page }) => {
  await page.goto(`/#/settings?section=${encodeURIComponent('Account and sync')}&setup=sync`);
  await expect(page.getByRole('dialog', { name: 'Sync your devices' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(wizard(page)).toHaveCount(0);
  await expect(page).not.toHaveURL(/setup=sync/);
});

test('a forgotten password can be reset from the sign-in step', async ({ page }) => {
  await openSync(page);
  await startSetup(page);
  await page.getByTestId('sync-path-join').click();
  const forgot = page.getByTestId('sync-forgot-password');
  // Needs an address to send to.
  await expect(forgot).toBeDisabled();
  await page.getByTestId('sync-email').fill('home@example.com');
  await forgot.click();
  await expect(page.getByText(/If there is an Aurora account for home@example.com/)).toBeVisible();
});

test('a new account is asked to confirm its email, and can have the link sent again', async ({ page }) => {
  await openSync(page);
  await createAccount(page, 'fresh@example.com');

  const verify = page.getByTestId('sync-setup-verify');
  await expect(verify).toContainText('fresh@example.com');
  await verify.getByRole('button', { name: 'Send the email again' }).click();
  await expect(verify).toContainText('Sent.');
  await verify.getByRole('button', { name: 'I have confirmed it' }).click();
  await expect(verify).toContainText('Not confirmed yet');

  // Left for later, Settings keeps asking.
  await verify.getByRole('button', { name: 'Later' }).click();
  await page.getByTestId('sync-setup-done').getByRole('button', { name: 'Done' }).click();
  await expect(page.getByTestId('sync-verify')).toContainText('Confirm your email address');
});

test('an account that wants its password again asks for just that, not the wizard', async ({ page }) => {
  await openSync(page, '?syncSignInAgain');
  await expect(page.getByText('The sync account needs its password again.')).toBeVisible();
  await expect(page.getByTestId('sync-setup-start')).toHaveCount(0);
  await expect(page.getByTestId('sync-email')).toHaveValue('home@example.com');
  await page.getByTestId('sync-password').fill('hunter22');
  await page.getByTestId('sync-passphrase').fill('correct horse');
  await page.getByRole('button', { name: 'Sign in again' }).click();
  await expect(page.getByTestId('sync-state')).toContainText('Up to date');
});

test('opening a film catches up with the other devices and offers their position', async ({ page }) => {
  await openSync(page, '?syncRemoteProgress');
  await signInHome(page);
  await expect(page.getByTestId('sync-signed-in')).toBeVisible();

  // Client-side, so the signed-in mock survives: the TV stopped this film at 41:00.
  await page.getByRole('link', { name: 'Movies', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  await page.locator('[role="button"]').first().click();
  const sheet = page.getByRole('dialog');
  await expect(sheet.getByRole('button', { name: 'Resume from 41:00' })).toBeVisible();
});

test('signed out, opening a film changes nothing', async ({ page }) => {
  await page.goto('/?syncRemoteProgress#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  await page.locator('[role="button"]').first().click();
  const sheet = page.getByRole('dialog');
  await expect(sheet.getByRole('button', { name: /^(Play|Resume from)/ })).toBeVisible();
  await expect(sheet.getByRole('button', { name: 'Resume from 41:00' })).toHaveCount(0);
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
  await joinHousehold(page);
  await expect(wizard(page).getByRole('alert'))
    .toContainText('That email and password do not match a sync account.');
});
