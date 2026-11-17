/**
 * The rebuilt player controls (docs/PLAYER.md §3), one test per requirement that no
 * other journey covers. The C-numbers are the spec's.
 *
 * Fullscreen is read from the commands the page sends: the browser build answers
 * `window.fullscreen` itself, and what matters is that leaving the player asks for it
 * to end, every way the player can be left.
 */
import { expect, test, type Page } from '@playwright/test';

interface Call { name: string; args: Record<string, unknown> }
type PlayerState = { status: string; muted: boolean; volume: number; positionSecs: number };

const calls = (page: Page, name: string) => page.evaluate(
  (n) => (window as unknown as { __auroraCalls: () => Call[] }).__auroraCalls()
    .filter((c) => c.name === n).map((c) => c.args),
  name,
);
const clearCalls = (page: Page) => page.evaluate(
  () => (window as unknown as { __auroraClearCalls: () => void }).__auroraClearCalls(),
);
const state = (page: Page) => page.evaluate(() =>
  (window as unknown as { __auroraInvoke: (c: string) => Promise<PlayerState> }).__auroraInvoke('player.state'));
/** The last fullscreen request the page made, or undefined if it made none. */
const lastFullscreen = async (page: Page) => (await calls(page, 'window.fullscreen')).at(-1);

const overlay = (page: Page) => page.locator('[data-player-overlay]');
const pause = (page: Page) => page.getByRole('button', { name: 'Pause' });
const volume = (page: Page) => page.getByRole('slider', { name: 'Volume' });

async function playFilm(page: Page) {
  await page.goto('/#/movies');
  await expect(page.getByTestId('catalog-card').first()).toBeVisible();
  await page.getByTestId('catalog-card').first().getByRole('button').first().click();
  await page.getByRole('dialog').getByRole('button', { name: /^(Play|Resume)/ }).click();
  await expect(pause(page)).toBeVisible();
  // The film's detail panel fades out above the player; a click on the picture before
  // it has gone lands on the panel.
  await expect(page.getByRole('dialog')).toHaveCount(0);
}

async function watchLive(page: Page) {
  await page.goto('/#/live');
  await expect(page.getByTestId('channel-row').first()).toBeVisible();
  await page.getByTestId('channel-row').first().click();
  await expect(pause(page)).toBeVisible();
}

/** Wake the controls, the way a viewer would. */
async function wake(page: Page) {
  await page.mouse.move(640, 300);
  await page.mouse.move(646, 304);
}

/** Into fullscreen with the dock's button, and confirmed by what the page asked for. */
async function goFullscreen(page: Page) {
  await wake(page);
  await page.getByRole('button', { name: 'Fullscreen' }).click();
  await expect(page.getByRole('button', { name: 'Leave fullscreen' })).toBeVisible();
  // The browser enters fullscreen on its own time, and reports it with an event that
  // would otherwise land in the middle of the test.
  await expect.poll(() => page.evaluate(() => document.fullscreenElement !== null)).toBe(true);
  await clearCalls(page);
}

/* ── C15, C16: fullscreen is left with the player ──────────────────────── */

test('C15: the Back button leaves fullscreen as it leaves the player', async ({ page }) => {
  await playFilm(page);
  await goFullscreen(page);
  await page.getByRole('button', { name: 'Back', exact: true }).click();
  await expect(overlay(page)).toHaveCount(0);
  await expect.poll(() => lastFullscreen(page)).toEqual({ fullscreen: false });
});

test('C16: Escape in fullscreen leaves fullscreen first, and a second Escape leaves the player', async ({ page }) => {
  await playFilm(page);
  await goFullscreen(page);
  await page.keyboard.press('Escape');
  await expect.poll(() => lastFullscreen(page)).toEqual({ fullscreen: false });
  await expect(overlay(page), 'the first Escape only leaves fullscreen').toHaveCount(1);
  await page.keyboard.press('Escape');
  await expect(overlay(page)).toHaveCount(0);
});

test('C15: going to another page from fullscreen leaves fullscreen', async ({ page }) => {
  await watchLive(page);
  await goFullscreen(page);
  await page.keyboard.press('Alt+1');
  await expect(overlay(page)).toHaveCount(0);
  await expect.poll(() => lastFullscreen(page)).toEqual({ fullscreen: false });
});

test('C15: the guide from fullscreen returns to the guide un-fullscreened', async ({ page }) => {
  await watchLive(page);
  await goFullscreen(page);
  await page.keyboard.press('g');
  await expect(page.getByRole('heading', { name: 'TV Guide' })).toBeVisible();
  await expect.poll(() => lastFullscreen(page)).toEqual({ fullscreen: false });
});

test('C15: Open settings on an error leaves fullscreen', async ({ page }) => {
  await playFilm(page);
  await goFullscreen(page);
  await page.evaluate(() => (window as unknown as { __auroraPlayerError: (e: unknown) => void }).__auroraPlayerError({
    code: 'unreachable', message: 'The provider could not be reached.', cause: 'It did not answer.',
    actions: ['retry', 'openSettings'], retryable: true,
  }));
  await page.getByRole('alert').getByRole('button', { name: 'Open settings' }).click();
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
  await expect.poll(() => lastFullscreen(page)).toEqual({ fullscreen: false });
});

test('C15: the player going to a corner leaves fullscreen', async ({ page }) => {
  await playFilm(page);
  await goFullscreen(page);
  await page.keyboard.press('p');
  await expect(page.getByTestId('pip-tile')).toBeVisible();
  await expect.poll(() => lastFullscreen(page)).toEqual({ fullscreen: false });
});

test('C14: F and a double click on the picture switch fullscreen; the double click does not pause', async ({ page }) => {
  await playFilm(page);
  await clearCalls(page);
  await page.keyboard.press('f');
  await expect.poll(() => lastFullscreen(page)).toEqual({});
  await clearCalls(page);
  await page.mouse.dblclick(640, 330);
  await expect.poll(() => lastFullscreen(page)).toEqual({});
  await page.waitForTimeout(500);
  expect(await calls(page, 'player.pause'), 'two clicks of a double click are not a pause').toHaveLength(0);
});

test('Escape with a menu open closes the menu and nothing else', async ({ page }) => {
  await playFilm(page);
  await wake(page);
  await page.getByRole('button', { name: 'Subtitles' }).click();
  const sheet = page.getByRole('group', { name: 'Watch settings' });
  await expect(sheet).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(sheet).toHaveCount(0);
  await expect(overlay(page), 'Escape closed the menu, not the player').toHaveCount(1);
});

/* ── C1–C5: showing and hiding ─────────────────────────────────────────── */

test('C3: the cursor hides with the controls and comes back on a move', async ({ page }) => {
  await playFilm(page);
  await wake(page);
  await expect(volume(page)).toBeHidden({ timeout: 6_000 });
  await expect(overlay(page)).toHaveCSS('cursor', 'none');
  await page.mouse.move(700, 320);
  await expect(volume(page)).toBeVisible();
  await expect(overlay(page)).toHaveCSS('cursor', 'default');
});

test('C4: a key that wakes the controls also does what the key does', async ({ page }) => {
  await playFilm(page);
  await wake(page);
  await expect(volume(page)).toBeHidden({ timeout: 6_000 });
  await page.keyboard.press('m');
  await expect(volume(page)).toBeVisible();
  await expect.poll(async () => (await state(page)).muted).toBe(true);
});

test('C5: paused content keeps the controls up', async ({ page }) => {
  await playFilm(page);
  await pause(page).click();
  await expect.poll(async () => (await state(page)).status).toBe('paused');
  await page.waitForTimeout(5_500);
  await expect(volume(page)).toBeVisible();
  // And playing again lets them go.
  await page.getByRole('button', { name: 'Play', exact: true }).click();
  await expect(volume(page)).toBeHidden({ timeout: 6_000 });
});

/* ── C6–C13 ─────────────────────────────────────────────────────────────── */

test('C6: a click on the picture plays and pauses', async ({ page }) => {
  await playFilm(page);
  await page.mouse.click(640, 330);
  await expect.poll(async () => (await state(page)).status).toBe('paused');
  await page.mouse.click(640, 330);
  await expect.poll(async () => (await state(page)).status).toBe('playing');
});

test('C7: J and L skip ten seconds, and Shift makes it thirty', async ({ page }) => {
  await playFilm(page);
  await clearCalls(page);
  await page.keyboard.press('l');
  await page.keyboard.press('Shift+J');
  await expect.poll(() => calls(page, 'player.seek')).toEqual([
    { positionSecs: 10, relative: true },
    { positionSecs: -30, relative: true },
  ]);
});

test('C9: a film shows time elapsed and its length', async ({ page }) => {
  await playFilm(page);
  await wake(page);
  const s = await state(page);
  const total = (await page.evaluate(() =>
    (window as unknown as { __auroraInvoke: (c: string) => Promise<{ durationSecs: number }> })
      .__auroraInvoke('player.state'))).durationSecs;
  const dock = page.locator('.pl-dock');
  await expect(dock.getByText(/^\d+:\d{2}(:\d{2})?$/).first()).toBeVisible();
  const hours = Math.floor(total / 3600);
  const mins = Math.floor((total % 3600) / 60);
  const length = hours ? `${hours}:${String(mins).padStart(2, '0')}:00` : `${mins}:00`;
  await expect(dock.getByText(length, { exact: true })).toBeVisible();
  expect(s.positionSecs).toBeLessThan(total);
});

test('C12: the volume goes no higher than 100, and ↑ ↓ step by five', async ({ page }) => {
  await playFilm(page);
  await wake(page);
  await expect(volume(page)).toHaveAttribute('max', '100');
  const before = (await state(page)).volume;
  await page.keyboard.press('ArrowUp');
  await expect.poll(async () => (await state(page)).volume).toBe(Math.min(100, before + 5));
  for (let i = 0; i < 25; i += 1) await page.keyboard.press('ArrowUp');
  await expect.poll(async () => (await state(page)).volume).toBe(100);
  await page.keyboard.press('ArrowDown');
  await expect.poll(async () => (await state(page)).volume).toBe(95);
});

test('C13: the menus mark the current choice, and switching tabs keeps the menu open', async ({ page }) => {
  await playFilm(page);
  await wake(page);
  await page.getByRole('button', { name: 'Subtitles' }).click();
  const sheet = page.getByRole('group', { name: 'Watch settings' });
  await expect(sheet.getByRole('button', { name: 'Off' })).toHaveAttribute('aria-pressed', 'true');
  await sheet.getByRole('tab', { name: 'Picture' }).click();
  await sheet.getByRole('tab', { name: 'Picture' }).click();
  await expect(sheet, 'pressing the open tab again does not close the menu').toBeVisible();
  await expect(sheet.getByRole('button', { name: 'Normal' })).toHaveAttribute('aria-pressed', 'true');
  await sheet.getByRole('button', { name: '0.25×' }).click();
  await expect(sheet.getByRole('button', { name: '0.25×' })).toHaveAttribute('aria-pressed', 'true');
});

/* ── C18, C24 ───────────────────────────────────────────────────────────── */

test('C18: I opens and closes the stats panel', async ({ page }) => {
  await playFilm(page);
  await page.keyboard.press('i');
  const stats = page.getByRole('group', { name: 'Playback statistics' });
  await expect(stats).toBeVisible();
  await expect(page.getByRole('button', { name: 'Playback stats' })).toHaveAttribute('aria-pressed', 'true');
  await page.keyboard.press('i');
  await expect(stats).toHaveCount(0);
});

test('C24: opening a stream shows a spinner and the title, not a black screen', async ({ page }) => {
  await playFilm(page);
  const title = await page.evaluate(() =>
    (window as unknown as { __auroraInvoke: (c: string) => Promise<{ title: string }> }).__auroraInvoke('player.state'))
    .then((s) => s.title);
  await page.evaluate(() => (window as unknown as { __auroraPlayerStatus: (p: unknown) => void })
    .__auroraPlayerStatus({ status: 'loading' }));
  const waiting = page.getByRole('status').filter({ hasText: 'Opening…' });
  await expect(waiting).toBeVisible();
  await expect(waiting).toContainText(title);
  await page.evaluate(() => (window as unknown as { __auroraPlayerStatus: (p: unknown) => void })
    .__auroraPlayerStatus({ status: 'playing' }));
  await expect(waiting).toHaveCount(0);
});

/* ── C32–C34: input ─────────────────────────────────────────────────────── */

test('C32: the guide\'s arrows never change the volume of its preview', async ({ page }) => {
  await page.goto('/#/guide');
  const first = page.getByTestId('guide-channel').first();
  await first.click();
  await expect(first).toHaveAttribute('data-live', 'true');
  await clearCalls(page);
  for (const key of ['ArrowDown', 'ArrowUp', 'ArrowLeft', 'ArrowRight']) await page.keyboard.press(key);
  await page.waitForTimeout(300);
  expect(await calls(page, 'player.setVolume')).toHaveLength(0);
  expect(await calls(page, 'player.seek')).toHaveLength(0);
});

test('C33: arrows move between the dock\'s buttons, with a focus ring, and do not seek', async ({ page }) => {
  await playFilm(page);
  await wake(page);
  await pause(page).focus();
  await clearCalls(page);
  await page.keyboard.press('ArrowRight');
  const back = page.getByRole('button', { name: 'Back 10 seconds' });
  await expect(back).toBeFocused();
  const ring = await back.evaluate((el) => getComputedStyle(el).outlineStyle);
  expect(ring).not.toBe('none');
  await page.keyboard.press('ArrowLeft');
  await expect(pause(page)).toBeFocused();
  expect(await calls(page, 'player.seek'), 'moving focus is not seeking').toHaveLength(0);
});

test('C33: every control in the dock has a name', async ({ page }) => {
  await playFilm(page);
  await wake(page);
  const unnamed = await page.locator('.pl-dock button, .pl-dock input, .pl-top button').evaluateAll((els) =>
    els.filter((el) => !(el.getAttribute('aria-label') ?? el.textContent ?? '').trim()).length);
  expect(unnamed).toBe(0);
});

test('C34: the key handler is not re-registered while something plays', async ({ page }) => {
  await page.addInitScript(() => {
    const w = window as unknown as { __keydownAdds: number };
    w.__keydownAdds = 0;
    const add = window.addEventListener.bind(window);
    window.addEventListener = ((type: string, ...rest: unknown[]) => {
      if (type === 'keydown') w.__keydownAdds += 1;
      return (add as (...a: unknown[]) => void)(type, ...rest);
    }) as typeof window.addEventListener;
  });
  await playFilm(page);
  await page.waitForTimeout(500);
  const settled = await page.evaluate(() => (window as unknown as { __keydownAdds: number }).__keydownAdds);
  // Several seconds of heartbeats, pausing and resuming: none of it adds a listener.
  await page.keyboard.press('k');
  await page.waitForTimeout(1_500);
  await page.keyboard.press('k');
  await page.waitForTimeout(1_500);
  const after = await page.evaluate(() => (window as unknown as { __keydownAdds: number }).__keydownAdds);
  expect(after).toBe(settled);
});
