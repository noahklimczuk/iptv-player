/**
 * The setup wizard, and the gate that could lock somebody out of their own app.
 *
 * `Continue` was `disabled={!validation?.ok}`, so a provider whose panel was slow,
 * overloaded, blocking this machine's User-Agent or simply down for ten minutes could not
 * be added at all. The check is worth running and worth reporting; it is not worth being
 * the only way in.
 */
import { expect, test } from '@playwright/test';

const SHOTS = 'screenshots';

/** `?setup` forces the wizard, which is otherwise only shown on a fresh library. */
test('a provider that fails its check can still be added', async ({ page }) => {
  await page.goto('/?setup#/');
  await expect(page.getByRole('heading', { name: /Welcome to Aurora/i })).toBeVisible();

  // A panel address that will not answer.
  await page.getByRole('textbox').first().fill('https://unreachable.invalid');
  await page.getByRole('button', { name: 'Xtream / panel login' }).click();
  await page.getByLabel('Username').fill('someone');
  await page.getByLabel('Password').fill('secret');

  const proceed = page.getByRole('button', { name: /^(Continue|Add it anyway)$/ });
  // Filled in, so the form is complete whether or not the panel answers.
  await expect(proceed).toBeEnabled();

  await page.getByRole('button', { name: 'Check connection' }).click();
  await expect(page.getByRole('button', { name: 'Add it anyway' })).toBeEnabled({
    timeout: 20_000,
  });
  await page.screenshot({ path: `${SHOTS}/53-setup-failed-check.png` });
});

test('a panel login with no credentials cannot continue', async ({ page }) => {
  await page.goto('/?setup#/');
  await expect(page.getByRole('heading', { name: /Welcome to Aurora/i })).toBeVisible();

  await page.getByRole('textbox').first().fill('https://panel.example.com');
  await page.getByRole('button', { name: 'Xtream / panel login' }).click();

  // Not a provider that might work — a form that is not filled in.
  await expect(page.getByRole('button', { name: /^(Continue|Add it anyway)$/ })).toBeDisabled();
  await page.getByLabel('Username').fill('someone');
  await expect(page.getByRole('button', { name: /^(Continue|Add it anyway)$/ })).toBeDisabled();
  await page.getByLabel('Password').fill('secret');
  await expect(page.getByRole('button', { name: /^(Continue|Add it anyway)$/ })).toBeEnabled();
});

test('a panel that only answers on http is found and offered', async ({ page }) => {
  await page.goto('/?setup#/');
  await expect(page.getByRole('heading', { name: /Welcome to Aurora/i })).toBeVisible();

  // An https address that will not answer — the shape of the report that prompted this.
  await page.getByRole('textbox').first().fill('https://panel.invalid');
  await page.getByRole('button', { name: 'Xtream / panel login' }).click();
  await page.getByLabel('Username').fill('someone');
  await page.getByLabel('Password').fill('secret');
  await page.getByRole('button', { name: 'Check connection' }).click();

  // The host probed the other scheme and found it answering there.
  await expect(page.getByText('http://panel.invalid')).toBeVisible({ timeout: 20_000 });
  // Said plainly, because it is a downgrade and the viewer is the one making it.
  await expect(page.getByText(/unencrypted address/)).toBeVisible();

  // Taking the offer rewrites the address and checks it again.
  await page.getByTestId('use-suggested-url').click();
  await expect(page.getByLabel('Provider address')).toHaveValue('http://panel.invalid');
});
