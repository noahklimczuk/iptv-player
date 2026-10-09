/**
 * The guide's preview: where it is, how big, and what two clicks do.
 *
 * What a browser can and cannot show here is worth being precise about, because the
 * interesting half is invisible from in here. There is no video surface in a browser, so
 * `preview.place` records the rectangle and nothing moves — which means these assert the
 * *contract*: that the page asks for the right rectangle, that it asks only once there
 * is something to show, that it gives it back, and that the shell stops painting over it
 * while it has it. Whether mpv actually lands there is a fact about a window tree, and
 * `tests-host/scenarios/guide_preview.py` is where that is measured.
 *
 * The two-click behaviour is fully testable here, and is the part a person notices.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

const preview = (page: Page) => page.getByTestId('guide-preview');
const channelCells = (page: Page) => page.locator('[data-testid="guide-channel"]');

async function openGuide(page: Page) {
  await page.goto('/#/guide');
  await expect(page.getByRole('heading', { name: 'TV Guide' })).toBeVisible();
  await expect(preview(page)).toBeVisible();
  // The grid settles after its first paint; measuring mid-layout proves nothing.
  await page.waitForTimeout(700);
}

/** What the page last asked the host for, in physical pixels. */
async function placedRect(page: Page) {
  return page.evaluate(async () => {
    const w = window as unknown as {
      __auroraInvoke: (c: string, a?: unknown) => Promise<unknown>;
    };
    const view = (await w.__auroraInvoke('pip.state')) as {
      inlay: { x: number; y: number; width: number; height: number } | null;
    };
    return view.inlay;
  });
}

test('the preview is in the top right, and big', async ({ page }) => {
  await openGuide(page);

  const box = (await preview(page).boundingBox())!;
  const viewport = page.viewportSize()!;

  // Top right: past the middle horizontally, and near the top.
  expect(box.x).toBeGreaterThan(viewport.width / 2);
  expect(box.x + box.width).toBeGreaterThan(viewport.width - 40);
  expect(box.y).toBeLessThan(viewport.height / 3);

  // Big enough to be a picture rather than a thumbnail. The panel it replaced was 340
  // wide, so this is the assertion that it actually got bigger.
  expect(box.width).toBeGreaterThan(400);
  // And 16:9, because it is standing in for a video surface.
  expect(box.width / box.height).toBeCloseTo(16 / 9, 1);

  await page.screenshot({ path: `${SHOTS}/guide-preview.png` });
});

test('nothing is asked of the host until a channel is playing', async ({ page }) => {
  await openGuide(page);
  // A hole punched before there is a picture is a rectangle of desktop showing through.
  expect(await placedRect(page)).toBeNull();
  await expect(preview(page)).toHaveAttribute('data-live', 'false');
});

test('the first click previews, the second goes full screen', async ({ page }) => {
  await openGuide(page);
  const first = channelCells(page).first();
  const name = (await first.getAttribute('data-channel-name'))!;

  // One click: playing, in the box, with the shell still up.
  await first.click();
  await expect(preview(page)).toHaveAttribute('data-live', 'true');
  await expect(page.getByTestId('nav-pill')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeHidden();
  // The name is in the bar *under* the box, not in it: nothing is drawn over the
  // picture, which is the point of the layout.
  await expect(page.getByTestId('guide-preview-fullscreen')).toBeVisible();
  await expect(page.getByRole('complementary').getByText(name).first()).toBeVisible();

  // And the host has been handed the box.
  const rect = await placedRect(page);
  expect(rect).not.toBeNull();
  const box = (await preview(page).boundingBox())!;
  const dpr = await page.evaluate(() => window.devicePixelRatio || 1);
  expect(rect!.width).toBeCloseTo(box.width * dpr, 0);
  expect(rect!.x).toBeCloseTo(box.x * dpr, 0);

  // Second click on the *same* channel: full screen.
  await first.click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/guide-preview-fullscreen.png` });
});

test('a different channel previews rather than jumping to full screen', async ({ page }) => {
  await openGuide(page);
  const cells = channelCells(page);

  await cells.nth(0).click();
  await expect(preview(page)).toHaveAttribute('data-live', 'true');

  // The second channel has never been previewed, so its first click must preview it —
  // otherwise moving down the guide and pressing OK would take over the screen.
  await cells.nth(1).click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeHidden();
  await expect(preview(page)).toHaveAttribute('data-live', 'true');

  // And now *its* second click is the one that goes full screen.
  await cells.nth(1).click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
});

test('selecting a programme highlights its channel without tuning it', async ({ page }) => {
  await openGuide(page);
  // Clicking a programme is how you read what is on; it must not start a stream, and it
  // must not arm the second click either.
  await page.locator('[data-testid="programme"]').first().click();
  expect(await placedRect(page)).toBeNull();
  await expect(preview(page)).toHaveAttribute('data-live', 'false');

  // So the channel's first click still previews.
  await channelCells(page).first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeHidden();
  await expect(preview(page)).toHaveAttribute('data-live', 'true');
});

test('leaving the guide gives the picture back', async ({ page }) => {
  await openGuide(page);
  await channelCells(page).first().click();
  expect(await placedRect(page)).not.toBeNull();

  await page.getByRole('navigation', { name: 'Main' })
    .getByRole('link', { name: 'Home' })
    .click();
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();

  // Otherwise the surface stays pinned to a box on a page that is no longer on screen.
  expect(await placedRect(page)).toBeNull();
});

test('the shell stops painting over the picture, and starts again after', async ({ page }) => {
  await openGuide(page);
  const shellBackground = () =>
    page.evaluate(() => {
      const el = document.querySelector('[data-testid="app-shell"]') as HTMLElement;
      return getComputedStyle(el).backgroundColor;
    });

  const opaque = await shellBackground();
  expect(opaque).not.toBe('rgba(0, 0, 0, 0)');

  await channelCells(page).first().click();
  // The whole mechanism: the picture is behind the page, so every layer over that
  // rectangle has to stop painting. A transparent box inside an opaque shell shows the
  // shell, not the video.
  await expect.poll(shellBackground).toBe('rgba(0, 0, 0, 0)');
  // And the background is drawn around it instead.
  await expect(page.getByTestId('surface-hole')).toBeAttached();

  await page.getByRole('navigation', { name: 'Main' })
    .getByRole('link', { name: 'Home' })
    .click();
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await expect.poll(shellBackground).toBe(opaque);
});
