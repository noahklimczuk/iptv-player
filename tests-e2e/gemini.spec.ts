/**
 * Recommendations from Gemini.
 *
 * The rail is silent in every failure mode — no key, nothing watched, a refusal, or none
 * of the suggestions being in this library. Each of those is a reason for it not to exist
 * rather than a reason to apologise on somebody's home screen, so these journeys check
 * both halves: that it stays away when it should, and that Settings says why.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

const RAIL = 'Because of what you watch';

async function settings(page: Page) {
  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
}

test('without a key there is no rail, and Settings says where to get one', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await page.waitForTimeout(900);
  // No heading, no skeleton, no empty state — it simply is not there.
  await expect(page.getByRole('region', { name: RAIL })).toHaveCount(0);

  await settings(page);
  await expect(page.getByText(/No key yet/)).toBeVisible();
  await expect(page.getByText(/aistudio\.google\.com/)).toBeVisible();
});

test('the panel says what leaves this computer, before anything is sent', async ({ page }) => {
  await settings(page);
  // The privacy claim is on the screen where the key is entered, which is the only moment
  // somebody is deciding whether to turn this on.
  await expect(page.getByText(/What leaves this computer/)).toBeVisible();
  await expect(page.getByText(/nothing about your provider, your sign-in/)).toBeVisible();
});

test('with a key, the rail appears and every card says why', async ({ page }) => {
  await settings(page);
  await page.getByLabel('Gemini API key').fill('test-key');
  await page.getByRole('button', { name: 'Use this key' }).click();

  // Settings can ask for a set directly, and reports what came back.
  await page.getByRole('button', { name: 'Get recommendations now' }).click();
  await expect(page.getByRole('status')).toContainText(/are in your library/, {
    timeout: 20_000,
  });

  await page.goto('/#/');
  const rail = page.getByRole('region', { name: RAIL });
  await expect(rail).toBeVisible({ timeout: 20_000 });
  expect(await rail.getByTestId('catalog-card').count()).toBeGreaterThan(0);

  // The model's own sentence, under each poster. A rail that cannot say why is one
  // nobody trusts, which is the whole argument for carrying the reason through.
  await expect(rail.getByTestId('card-reason').first()).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/54-gemini-rail.png` });
});

test('removing the key takes the rail away again', async ({ page }) => {
  await settings(page);
  await page.getByLabel('Gemini API key').fill('test-key');
  await page.getByRole('button', { name: 'Use this key' }).click();
  await expect(page.getByRole('button', { name: 'Get recommendations now' })).toBeVisible();

  await page.getByRole('button', { name: 'Remove key' }).click();
  await expect(page.getByText(/No key yet/)).toBeVisible();

  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await page.waitForTimeout(900);
  await expect(page.getByRole('region', { name: RAIL })).toHaveCount(0);
});
