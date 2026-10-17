/**
 * The player's error panel, and whether its buttons do anything.
 *
 * They did not. `error.actions.map` drew a `<button>` with no `onClick` at all, so all
 * four actions the host can offer were inert -- on the one screen a viewer reaches while
 * something is already wrong (R-110).
 *
 * No journey could have caught it before this file: `player.error` is filled in by the
 * host from `NetFailure`, so making a command refuse does not produce the panel. A
 * refusal is a rejected promise and goes to a notice instead. `__auroraPlayerError` is
 * the seam that puts the player into the state the panel is for.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

/** The shape the host sends for a dead stream on a one-connection line. */
const LIMIT = {
  code: 'connectionLimit',
  message: "You're already watching on another device",
  cause: 'This subscription allows a limited number of simultaneous streams.',
  actions: ['retry', 'tryAnotherSource', 'reportBroken', 'openSettings'],
  retryable: true,
};

async function playSomethingThenFail(page: Page) {
  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();
  await page.locator('[role="button"]').first().click();
  await page.waitForTimeout(600);

  await page.evaluate((error) => {
    const w = window as unknown as { __auroraPlayerError: (e: unknown) => void };
    w.__auroraPlayerError(error);
  }, LIMIT);

  const panel = page.getByRole('alert');
  await expect(panel).toBeVisible();
  return panel;
}

test('the error panel offers only actions it can carry out', async ({ page }) => {
  const panel = await playSomethingThenFail(page);

  await expect(panel.getByText(LIMIT.message)).toBeVisible();
  await expect(panel.getByRole('button', { name: 'Retry' })).toBeVisible();
  await expect(panel.getByRole('button', { name: 'Try another source' })).toBeVisible();
  await expect(panel.getByRole('button', { name: 'Open settings' })).toBeVisible();

  // `reportBroken` has no host command behind it, so it is not drawn. A button that
  // cannot act is the bug this file is about, in a new form.
  await expect(panel.getByRole('button', { name: /report/i })).toHaveCount(0);

  await page.screenshot({ path: `${SHOTS}/22-error-panel.png` });
});

test('Retry asks for the stream again and clears the error', async ({ page }) => {
  const panel = await playSomethingThenFail(page);

  await panel.getByRole('button', { name: 'Retry' }).click();

  // `player.play` answers with a state that has no error in it, so the panel goes. That
  // it goes is the proof the command was sent: nothing else clears it.
  await expect(page.getByRole('alert')).toHaveCount(0);
  const state = await page.evaluate(async () => {
    const w = window as unknown as {
      __auroraInvoke: (c: string, a?: unknown) => Promise<{
        status: string; error: unknown; itemKind: string | null;
      }>;
    };
    return w.__auroraInvoke('player.state');
  });
  expect(state.error).toBeNull();
  expect(state.status).toBe('playing');
  expect(state.itemKind).toBe('live');
});

test('Try another source re-tunes, which is what it means for live', async ({ page }) => {
  const panel = await playSomethingThenFail(page);

  await panel.getByRole('button', { name: 'Try another source' }).click();

  await expect(page.getByRole('alert')).toHaveCount(0);
  const status = await page.evaluate(async () => {
    const w = window as unknown as {
      __auroraInvoke: (c: string, a?: unknown) => Promise<{ status: string }>;
    };
    return (await w.__auroraInvoke('player.state')).status;
  });
  expect(status).toBe('playing');
});

test('Open settings goes to Settings, where a bad provider is fixed', async ({ page }) => {
  const panel = await playSomethingThenFail(page);

  await panel.getByRole('button', { name: 'Open settings' }).click();

  await expect(page).toHaveURL(/#\/settings$/);
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
});
