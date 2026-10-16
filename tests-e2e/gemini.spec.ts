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

/**
 * Opens Settings and hands back the Recommendations panel.
 *
 * Scoped for the same reason the metadata journeys are: this panel's key field, Save and
 * status all have near-twins in the panel above it, and a page-wide locator picks
 * whichever shipped first.
 */
async function settings(page: Page, query = '') {
  await page.goto(`/${query}#/settings`);
  const panel = page.getByRole('region', { name: 'Recommendations' });
  await expect(panel).toBeVisible();
  return panel;
}

test('without a key there is no rail, and Settings says where to get one', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await page.waitForTimeout(900);
  // No heading, no skeleton, no empty state — it simply is not there.
  await expect(page.getByRole('region', { name: RAIL })).toHaveCount(0);

  const panel = await settings(page);
  await expect(panel.getByText(/No key yet/)).toBeVisible();
  await expect(panel.getByText(/aistudio\.google\.com/)).toBeVisible();
});

test('the panel says what leaves this computer, before anything is sent', async ({ page }) => {
  const panel = await settings(page);
  // The privacy claim is on the screen where the key is entered, which is the only moment
  // somebody is deciding whether to turn this on.
  await expect(panel.getByText(/What leaves this computer/)).toBeVisible();
  await expect(panel.getByText(/nothing about your provider, your sign-in/)).toBeVisible();
});

test('with a key, the rail appears and every card says why', async ({ page }) => {
  const panel = await settings(page);
  await panel.getByLabel('Gemini API key').fill('test-key');
  await panel.getByRole('button', { name: 'Use this key' }).click();

  // Settings can ask for a set directly, and reports what came back.
  await panel.getByRole('button', { name: 'Get recommendations now' }).click();
  await expect(panel.getByRole('status')).toContainText(/are in your library/, {
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

/**
 * The switch, which is the only thing that stops anything being sent in a release build.
 *
 * `removing the key takes the rail away again`, below, passes in a browser because the
 * mock has no built-in key — and in an official build removing the key falls back to the
 * one compiled in, so the rail stays and the history keeps going to Google.
 * `docs/PRIVACY.md` told viewers to clear the key for exactly this purpose.
 *
 * So this journey turns the feature *off* with a key still saved, which is the state the
 * old instruction could never reach.
 */
test('turning suggestions off takes the rail away with the key still saved', async ({ page }) => {
  const panel = await settings(page);
  await panel.getByLabel('Gemini API key').fill('test-key');
  await panel.getByRole('button', { name: 'Use this key' }).click();
  await expect(panel.getByRole('button', { name: 'Get recommendations now' })).toBeVisible();

  const toggle = panel.getByTestId('gemini-enabled');
  await expect(toggle).toHaveAttribute('aria-checked', 'true');
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-checked', 'false');
  await expect(panel.getByText(/Nothing is sent to Google/)).toBeVisible();

  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await page.waitForTimeout(900);
  await expect(page.getByRole('region', { name: RAIL })).toHaveCount(0);

  // And back on, with the key that was never removed.
  const again = await settings(page);
  await again.getByTestId('gemini-enabled').click();
  await expect(again.getByTestId('gemini-enabled')).toHaveAttribute('aria-checked', 'true');
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: RAIL })).toBeVisible({ timeout: 20_000 });
});

test('removing the key takes the rail away again', async ({ page }) => {
  const panel = await settings(page);
  await panel.getByLabel('Gemini API key').fill('test-key');
  await panel.getByRole('button', { name: 'Use this key' }).click();
  await expect(panel.getByRole('button', { name: 'Get recommendations now' })).toBeVisible();

  await panel.getByRole('button', { name: 'Remove key' }).click();
  await expect(panel.getByText(/No key yet/)).toBeVisible();

  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await page.waitForTimeout(900);
  await expect(page.getByRole('region', { name: RAIL })).toHaveCount(0);
});

/**
 * What "Remove key" does in the build people actually run.
 *
 * `BUILT_IN_KEY` is `option_env!("AURORA_GEMINI_KEY")` — resolved by `rustc`, so a
 * release is compiled with a key and `resolve_key` falls back to it the moment the
 * viewer's own is cleared. The mock had a mutable string and `keyIsBuiltIn: false`
 * hardcoded, so the one state that matters for privacy could not be reached from a
 * browser at all, and the journey above it ("removing the key takes the rail away
 * again") passed only because the mock is kinder than the host.
 *
 * `docs/PRIVACY.md` told viewers to clear the key for exactly this purpose, and in an
 * official build that did nothing. `?builtInKey` is how a compile-time fact gets
 * modelled — it cannot be set at runtime, so it is a query flag, read once (R-159).
 */
test('with a key compiled in, removing your own does not stop the rail', async ({ page }) => {
  const panel = await settings(page, '?builtInKey');

  // No key of their own, and the panel says where the one in play came from.
  await expect(panel.getByText(/Using the key built into this build/)).toBeVisible();

  // The rail is there on the strength of the compiled-in key alone.
  await page.goto('/?builtInKey#/');
  await expect(page.getByRole('region', { name: RAIL })).toBeVisible({ timeout: 20_000 });

  // Entering a key of their own replaces it...
  const mine = await settings(page, '?builtInKey');
  await mine.getByLabel('Gemini API key').fill('my-own-key');
  await mine.getByRole('button', { name: 'Use this key' }).click();
  await expect(mine.getByText(/Using the key built into this build/)).toHaveCount(0);

  // ...and removing it falls back to the compiled one rather than turning anything off.
  // This is the step `docs/PRIVACY.md` used to describe as the way to stop sending.
  await mine.getByRole('button', { name: 'Remove key' }).click();
  await expect(mine.getByText(/Using the key built into this build/)).toBeVisible();

  await page.goto('/?builtInKey#/');
  await expect(
    page.getByRole('region', { name: RAIL }),
    'removing the key stopped the rail, which a release build cannot do',
  ).toBeVisible({ timeout: 20_000 });

  // The switch is the only thing that does stop it, which is what R-141 added and what
  // the privacy note says now.
  const off = await settings(page, '?builtInKey');
  await off.getByTestId('gemini-enabled').click();
  await expect(off.getByTestId('gemini-enabled')).toHaveAttribute('aria-checked', 'false');
  await page.goto('/?builtInKey#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await page.waitForTimeout(900);
  await expect(page.getByRole('region', { name: RAIL })).toHaveCount(0);
});
