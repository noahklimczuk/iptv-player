/**
 * Media keys, a headset's buttons and the Windows media overlay (`media_keys.rs`).
 *
 * Only Windows can press them for real, so these press them the way the host hands them
 * over — a `media.button` event — and check the page does what its own keys do.
 */
import { expect, test, type Page } from '@playwright/test';

type MediaButton =
  | { button: 'play' | 'pause' | 'next' | 'previous' }
  | { button: 'seek'; positionSecs: number };
type PlayerState = {
  status: string;
  positionSecs: number;
  durationSecs: number;
  channelId: number | null;
};

const press = (page: Page, b: MediaButton) => page.evaluate(
  (button) => (window as unknown as { __auroraMediaKey: (b: MediaButton) => void })
    .__auroraMediaKey(button),
  b,
);
const state = (page: Page) => page.evaluate(() =>
  (window as unknown as { __auroraInvoke: (c: string) => Promise<PlayerState> })
    .__auroraInvoke('player.state'));
const status = async (page: Page) => (await state(page)).status;
const channel = async (page: Page) => (await state(page)).channelId;

async function watchLive(page: Page) {
  await page.goto('/#/live');
  await expect(page.getByTestId('channel-row').first()).toBeVisible();
  await page.getByTestId('channel-row').first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
}

async function playFilm(page: Page) {
  await page.goto('/#/movies');
  await expect(page.getByTestId('catalog-card').first()).toBeVisible();
  await page.getByTestId('catalog-card').first().getByRole('button').first().click();
  await page.getByRole('dialog').getByRole('button', { name: /^(Play|Resume)/ }).click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await expect(page.getByRole('dialog')).toHaveCount(0);
}

test('pause and play from a media key, each only when it changes something', async ({ page }) => {
  await watchLive(page);
  await press(page, { button: 'pause' });
  await expect.poll(() => status(page)).toBe('paused');
  // A second pause, sent before Windows heard about the first, does not resume it.
  await press(page, { button: 'pause' });
  await page.waitForTimeout(300);
  expect(await status(page)).toBe('paused');

  await press(page, { button: 'play' });
  await expect.poll(() => status(page)).toBe('playing');
  await press(page, { button: 'play' });
  await page.waitForTimeout(300);
  expect(await status(page)).toBe('playing');
});

test('next and previous change channel on live TV, as PageUp and PageDown do', async ({ page }) => {
  await watchLive(page);
  const first = await channel(page);
  await press(page, { button: 'previous' });
  await expect.poll(() => channel(page)).not.toBe(first);
  await press(page, { button: 'next' });
  await expect.poll(() => channel(page)).toBe(first);
});

test('on a film, next does nothing and the overlay bar seeks', async ({ page }) => {
  await playFilm(page);
  await expect.poll(async () => (await state(page)).durationSecs).toBeGreaterThan(0);
  const before = await state(page);
  await press(page, { button: 'next' });
  await page.waitForTimeout(300);
  expect((await state(page)).status).toBe('playing');
  expect((await state(page)).channelId).toBe(before.channelId);

  await press(page, { button: 'seek', positionSecs: 600 });
  await expect.poll(async () => (await state(page)).positionSecs).toBeGreaterThanOrEqual(600);
});

test('with nothing playing, a media key does nothing', async ({ page }) => {
  await page.goto('/#/live');
  await expect(page.getByTestId('channel-row').first()).toBeVisible();
  await press(page, { button: 'play' });
  await press(page, { button: 'next' });
  await page.waitForTimeout(300);
  expect(await status(page)).toBe('idle');
  await expect(page.locator('[data-player-overlay]')).toHaveCount(0);
});
