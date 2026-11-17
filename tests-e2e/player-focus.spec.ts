/**
 * "Test the lag issues": every Space pressed while a film was opening started it again.
 *
 * Driven on the real installed app with the mouse: Play from the title dialog, then
 * Space three times while it loaded, logged four `player.play` and no pause. Each one
 * restarted the stream on a line that allows one connection, so the start went from
 * about 3 s to 11–45 s and pause did nothing. The dialog closes on Play and hands focus
 * back to the poster that opened it, the poster plays on Space, and the player takes no
 * focus of its own, so the poster kept it behind the picture.
 */
import { expect, test, type Page } from '@playwright/test';

interface MockCall { name: string; args: Record<string, unknown>; bytes: number }

const calls = (page: Page, name: string) => page.evaluate(
  (n) => (window as unknown as { __auroraCalls: () => MockCall[] }).__auroraCalls()
    .filter((c) => c.name === n).map((c) => c.args),
  name,
);
const clearCalls = (page: Page) => page.evaluate(
  () => (window as unknown as { __auroraClearCalls: () => void }).__auroraClearCalls(),
);
const focused = (page: Page) => page.evaluate(() => {
  const el = document.activeElement;
  return el === document.body || el == null ? 'body' : (el.getAttribute('aria-label') ?? el.tagName);
});

async function playFromDialog(page: Page) {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  const card = page.locator('[role="button"]').first();
  await card.click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.getByRole('button', { name: /^(Play|Resume)/ }).first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  return card;
}

test('Space after Play from the title dialog pauses, and does not play it again', async ({ page }) => {
  await playFromDialog(page);
  expect(await focused(page), 'the poster behind the picture kept the keyboard').toBe('body');
  await clearCalls(page);
  await page.keyboard.press(' ');
  await expect.poll(() => calls(page, 'player.pause')).toHaveLength(1);
  await page.keyboard.press(' ');
  await expect.poll(() => calls(page, 'player.resume')).toHaveLength(1);
  expect(await calls(page, 'player.play'), 'Space started the film over').toHaveLength(0);
});

test('leaving the player gives the keyboard back to the poster it was opened from', async ({ page }) => {
  const card = await playFromDialog(page);
  await page.keyboard.press('Escape');
  await expect(page.getByRole('button', { name: 'Pause' })).toBeHidden();
  await expect(card).toBeFocused();
});
