/**
 * Managing a provider after it is added (README §15).
 *
 * Adding one was the only thing Settings could do: no way to fix a typo in an address,
 * re-read a password after the provider rotated it, or remove an account you no longer
 * have. The password is the delicate part — readable on request, never on sight.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

async function openEditor(page: Page) {
  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
  await page.getByRole('button', { name: 'Edit' }).first().click();
  await expect(page.getByLabel('Provider address')).toBeVisible();
}

test('a provider can be edited without its password being on display', async ({ page }) => {
  await openEditor(page);

  const password = page.getByLabel('Password');
  await expect(password).toHaveAttribute('type', 'password');
  // It is loaded — the form can save it back — but it is not readable yet.
  await expect(password).not.toHaveValue('');

  await page.getByRole('button', { name: 'Show' }).click();
  await expect(password).toHaveAttribute('type', 'text');

  await page.getByRole('button', { name: 'Hide' }).click();
  await expect(password).toHaveAttribute('type', 'password');

  await page.screenshot({ path: `${SHOTS}/35-provider-editor.png` });
});

test('an edited address is kept', async ({ page }) => {
  await openEditor(page);

  await page.getByLabel('Provider address').fill('http://moved.example.com');
  await page.getByRole('button', { name: 'Save changes' }).click();

  // The form closes back to the list rather than sitting there looking unsaved.
  await expect(page.getByLabel('Provider address')).toBeHidden();

  await page.getByRole('button', { name: 'Edit' }).first().click();
  await expect(page.getByLabel('Provider address')).toHaveValue('http://moved.example.com');
});

test('removing a provider asks first, and says what goes with it', async ({ page }) => {
  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
  // Counted before the editor opens: the row's own button reads "Close" once it has.
  const before = await page.getByRole('button', { name: 'Edit' }).count();
  expect(before).toBeGreaterThan(0);

  const row = page.getByRole('button', { name: 'Edit' }).first().locator('../..');
  // The row's first line is the provider's name; the rest is its counts.
  const name = (await row.innerText()).split('\n')[0]!.trim();

  await page.getByRole('button', { name: 'Edit' }).first().click();
  await expect(page.getByLabel('Provider address')).toBeVisible();

  await page.getByRole('button', { name: 'Remove provider' }).click();
  // Not a bare "are you sure": the count is the thing someone needs to see, because
  // the library goes too and there is no undo.
  await expect(page.getByText(/and its \d+ imported items\?/)).toBeVisible();

  // Backing out leaves everything alone. Asserted on the provider rather than on the
  // Edit button, which reads "Close" for as long as its editor is open.
  await page.getByRole('button', { name: 'Keep' }).click();
  await expect(page.getByText(/imported items\?/)).toBeHidden();
  await expect(page.getByText(name, { exact: true })).toBeVisible();

  await page.getByRole('button', { name: 'Remove provider' }).click();
  await page.getByRole('button', { name: 'Yes, remove' }).click();

  await expect(page.getByText(name, { exact: true })).toBeHidden();
  await expect(page.getByRole('button', { name: 'Edit' })).toHaveCount(before - 1);
});

test('the settings column is centred in the space it has', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();

  // Against its own container, not the viewport: the app shell's navigation takes a
  // slice off the left, and centring in the viewport would mean sitting off-centre in
  // the area the page actually occupies.
  const gutters = await page.evaluate(() => {
    const heading = document.querySelector('h1');
    const column = heading?.parentElement;
    const container = column?.parentElement;
    if (!column || !container) return null;
    const c = column.getBoundingClientRect();
    const p = container.getBoundingClientRect();
    return { left: c.left - p.left, right: p.right - c.right, width: c.width };
  });

  expect(gutters).not.toBeNull();
  // Narrower than its container, or "centred" would be vacuously true.
  expect(gutters!.width).toBeLessThan(1400);
  expect(Math.abs(gutters!.left - gutters!.right)).toBeLessThan(24);
});
