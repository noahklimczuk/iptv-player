/**
 * The shell must not paint over the video.
 *
 * On Windows libmpv renders into a child window *behind* the WebView2, at
 * `HWND_BOTTOM`, and the page floats over it. That only works if the page is actually
 * transparent where the picture is. It was not: `body` was transparent, and then the
 * React root repainted `var(--bg)` across the whole window, the sidebar painted
 * itself, and the page the viewer pressed play from stayed mounted underneath. The
 * result on a real machine was audio, a working OSD, and no picture — a poster grid
 * sitting exactly where the film should have been.
 *
 * Nothing caught it, because every journey here runs against the mock transport where
 * there is no surface behind the page, so an opaque shell looks perfectly correct.
 * `?video` is the fix for that blind spot: it says "compose as though libmpv were
 * behind you" without claiming a Tauri bridge exists, so the mock keeps answering
 * commands while the shell arranges itself the way it does on Windows.
 *
 * These assertions are about *computed* style, not about what the source says, because
 * the bug was three separate paints stacking up rather than any one of them.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

/** Start playing something, so the overlay is up. */
async function play(page: Page) {
  await page.getByTestId('channel-row').first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
}

/** The computed background of an element, as the compositor would see it. */
async function background(page: Page, testId: string) {
  return page.getByTestId(testId).evaluate((el) => getComputedStyle(el).backgroundColor);
}

/** Transparent in the only sense that matters here: the compositor sees through it. */
function isSeeThrough(color: string) {
  return color === 'rgba(0, 0, 0, 0)' || color === 'transparent';
}

test('with a surface behind it, nothing in the shell paints over the video', async ({
  page,
}) => {
  await page.goto('/?video#/live');
  await play(page);

  const shell = await background(page, 'app-shell');
  expect(isSeeThrough(shell), `the app shell painted ${shell} over the video`).toBe(true);

  const body = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  expect(isSeeThrough(body), `body painted ${body} over the video`).toBe(true);

  const html = await page.evaluate(
    () => getComputedStyle(document.documentElement).backgroundColor,
  );
  expect(isSeeThrough(html), `html painted ${html} over the video`).toBe(true);

  // The sidebar and the page are opaque by design, so they have to be out of the way
  // rather than merely behind — the overlay is transparent, so "behind" is still on
  // screen.
  await expect(page.getByRole('navigation', { name: 'Main' })).toBeHidden();
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeHidden();

  await page.screenshot({ path: `${SHOTS}/44-video-surface-clear.png` });
});

test('the OSD is still usable with the chrome out of the way', async ({ page }) => {
  await page.goto('/?video#/live');
  await play(page);

  // Hiding the shell must not take the controls with it: this is the whole interface
  // while something is playing.
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await page.getByRole('button', { name: 'Pause' }).click();
  // Exact: "Playback stats" and "Playback settings" both contain "Play".
  await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeVisible();
});

test('closing the player gives the shell back', async ({ page }) => {
  await page.goto('/?video#/live');
  await play(page);
  await expect(page.getByRole('navigation', { name: 'Main' })).toBeHidden();

  await page.keyboard.press('Escape');

  await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible();
  const shell = await background(page, 'app-shell');
  expect(isSeeThrough(shell), 'the shell stayed transparent after playback').toBe(false);
});

test('without a surface behind it the shell still paints, so a browser is not a hole', async ({
  page,
}) => {
  // The same journey with no `?video`: there is nothing behind the page, so the shell
  // must keep its own background rather than showing the desktop through it.
  await page.goto('/#/live');
  await play(page);

  const shell = await background(page, 'app-shell');
  expect(isSeeThrough(shell), 'the shell went transparent with nothing behind it').toBe(
    false,
  );
  await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible();
});
