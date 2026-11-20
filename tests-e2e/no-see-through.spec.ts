/**
 * The window never shows the desktop through it.
 *
 * "It's still doing a transparent screen before it plays the content." The viewer's
 * screen recording showed it twice:
 *
 * - **Starting** (7.6-8.8 s): the host said `playing` the moment mpv had *opened* the
 *   file, a second or so before its first frame. The shell turned transparent on that,
 *   so for that second there was nothing behind it but the desktop.
 * - **Leaving** (71.4 s): `player.stop` was sent in the same tick the player was asked
 *   to close. The host took the picture away at once, while React was still drawing
 *   Home, and until it finished the screen was the transparent player over nothing.
 *
 * `?video` composes the shell as though mpv were behind it (see `video-surface.spec`).
 */
import { expect, test, type Page } from '@playwright/test';

const seeThrough = (c: string) => c === 'rgba(0, 0, 0, 0)' || c === 'transparent';
const shellColour = (page: Page) =>
  page.getByTestId('app-shell').evaluate((el) => getComputedStyle(el).backgroundColor);
const noPicture = async (page: Page, on: boolean) => {
  // The browser build's test hooks arrive with the mock, which is imported on demand
  // (`loadMock`) and can land after the page's load event.
  await page.waitForFunction(() => '__auroraNoPicture' in window);
  await page.evaluate((v) => (window as unknown as { __auroraNoPicture: (on: boolean) => void }).__auroraNoPicture(v), on);
};

test('the window stays opaque until there is a picture to show through it', async ({ page }) => {
  await page.goto('/?video#/live');
  await noPicture(page, true);
  await page.getByTestId('channel-row').first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();

  // Playing, by the host's account, but nothing drawn yet.
  const status = await page.evaluate(async () => {
    const w = window as unknown as { __auroraInvoke: (c: string) => Promise<{ status: string; hasPicture: boolean }> };
    return w.__auroraInvoke('player.state');
  });
  expect(status).toMatchObject({ status: 'playing', hasPicture: false });
  await page.waitForTimeout(300);
  expect(seeThrough(await shellColour(page)), 'see-through before the first frame').toBe(false);

  await noPicture(page, false);
  await expect.poll(async () => seeThrough(await shellColour(page))).toBe(true);
});

test('leaving the player paints the page before the picture is taken away', async ({ page }) => {
  await page.goto('/?video#/live');
  await page.getByTestId('channel-row').first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await expect.poll(async () => seeThrough(await shellColour(page))).toBe(true);
  await page.evaluate(() => (window as unknown as { __auroraClearCalls: () => void }).__auroraClearCalls());

  // The shell and the stop, frame by frame from the click until the stop is sent.
  const frames = await page.evaluate(async () => {
    const stopped = () => (window as unknown as { __auroraCalls: () => { name: string }[] }).__auroraCalls()
      .some((c) => c.name === 'player.stop');
    const opaque = () => {
      const shell = document.querySelector('[data-testid="app-shell"]');
      return !!shell && getComputedStyle(shell).backgroundColor !== 'rgba(0, 0, 0, 0)';
    };
    const seen: { opaque: boolean; stopped: boolean }[] = [];
    (document.querySelector('button[aria-label="Back"]') as HTMLButtonElement).click();
    for (let i = 0; i < 60 && !seen.some((f) => f.stopped); i++) {
      await new Promise((r) => requestAnimationFrame(r));
      seen.push({ opaque: opaque(), stopped: stopped() });
    }
    return seen;
  });
  const first = frames.findIndex((f) => f.stopped);
  expect(first, `never stopped: ${JSON.stringify(frames)}`).toBeGreaterThan(0);
  expect(frames[first - 1], 'a frame of the opaque page was painted before the stop was sent').toEqual({ opaque: true, stopped: false });
});
