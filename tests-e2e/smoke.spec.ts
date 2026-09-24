/**
 * Critical-journey smoke tests (README §20) plus screenshot capture.
 * Runs against the production bundle with the mock IPC transport.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

/** Artwork is generated client-side, but rails and images still need a beat to settle. */
async function settle(page: Page, ms = 900) {
  await page.waitForLoadState('networkidle').catch(() => {});
  await page.waitForTimeout(ms);
}

test('home renders the hero billboard and Netflix-style rails', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();

  // Several named rails from README §8.2 must be present.
  await expect(page.getByRole('region', { name: 'Continue Watching' })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Top 10 Movies Today' })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Recently Added' })).toBeVisible();
  await expect(page.getByRole('region', { name: /^Because you watched/ })).toBeVisible();

  await settle(page);
  await page.screenshot({ path: `${SHOTS}/01-home.png` });
});

test('hovering a rail card expands it with quick actions', async ({ page }) => {
  await page.goto('/#/');
  await settle(page, 600);

  const rail = page.getByRole('region', { name: 'Recently Added' });
  const card = rail.getByRole('button').filter({ hasText: '' }).nth(2);
  await card.hover();
  // Preview dwell is 700ms.
  await page.waitForTimeout(1100);

  await expect(rail.getByRole('button', { name: /^Play / }).first()).toBeVisible();
  await expect(rail.getByText('Preview playing').first()).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/02-hover-card.png` });
});

test('detail modal opens with metadata and tabs', async ({ page }) => {
  await page.goto('/#/');
  await settle(page, 600);
  await page.getByRole('button', { name: 'More Info' }).click();

  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('tab', { name: 'More Like This' })).toBeVisible();
  await expect(dialog.getByRole('tab', { name: 'details' })).toBeVisible();

  await settle(page, 500);
  await page.screenshot({ path: `${SHOTS}/03-detail-modal.png` });
});

test('EPG guide renders a cable-style grid with a now line and preview', async ({ page }) => {
  await page.goto('/#/guide');
  await expect(page.getByRole('heading', { name: 'TV Guide' })).toBeVisible();

  // Channel column, time header, and programme blocks.
  await expect(page.getByText('CHANNEL', { exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Now' })).toBeVisible();
  await expect(page.getByRole('button', { name: /Tonight 8pm/ })).toBeVisible();

  await settle(page);
  await page.screenshot({ path: `${SHOTS}/04-guide.png` });
});

test('selecting a programme fills the info pane with actions', async ({ page }) => {
  await page.goto('/#/guide');
  await settle(page, 700);

  // Click a programme block (they carry a "title · HH:MM–HH:MM" tooltip).
  await page.locator('button[title*="–"]').first().click();
  await expect(page.getByRole('button', { name: 'Record series' })).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/05-guide-info.png` });
});

test('Search this title opens the palette already looking for the programme', async ({
  page,
}) => {
  await page.goto('/#/guide');
  await settle(page, 700);

  const cell = page.locator('button[title*="–"]').first();
  const tooltip = (await cell.getAttribute('title')) ?? '';
  const title = tooltip.split(' · ')[0]!;
  await cell.click();

  await page.getByRole('button', { name: 'Search this title' }).click();

  const dialog = page.getByRole('dialog', { name: 'Search' });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('textbox')).toHaveValue(title);
  // Seeded, not just focused: results are on screen without a keystroke.
  await expect(dialog.getByText(/On Now|Upcoming/).first()).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/27-guide-search.png` });
});

test('live TV lists channels with now/next from the EPG', async ({ page }) => {
  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();
  await expect(page.getByText('Next:').first()).toBeVisible();
  await settle(page);
  await page.screenshot({ path: `${SHOTS}/06-live.png` });
});

test('typing digits opens the channel-entry overlay and tunes', async ({ page }) => {
  await page.goto('/#/live');
  await settle(page, 600);

  await page.keyboard.press('2');
  await page.keyboard.press('0');
  await page.keyboard.press('3');
  // Overlay shows the typed digits before the commit timeout.
  await expect(page.getByRole('status', { name: 'Channel 203' })).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/07-digit-entry.png` });

  // After the timeout it tunes and the player + banner appear.
  await page.waitForTimeout(2200);
  await expect(page.getByText('Video surface (libmpv renders here on Windows)')).toBeVisible();
  await settle(page, 400);
  await page.screenshot({ path: `${SHOTS}/08-player-banner.png` });
});

test('player OSD exposes transport, tracks and stats', async ({ page }) => {
  await page.goto('/#/live');
  await settle(page, 600);
  await page.locator('button', { hasText: 'Meridian News' }).first().click();

  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await page.getByRole('button', { name: 'Subtitles' }).click();
  await expect(page.getByText('English SDH')).toBeVisible();
  await page.getByRole('button', { name: 'Playback stats' }).click();
  await expect(page.getByText('Hardware decoder')).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/09-player-osd.png` });
});

test('command palette searches across every content kind', async ({ page }) => {
  await page.goto('/#/');
  await settle(page, 600);
  await page.keyboard.press('Control+k');

  const dialog = page.getByRole('dialog', { name: 'Search' });
  await expect(dialog).toBeVisible();
  await dialog.getByRole('textbox').fill('the');
  await page.waitForTimeout(400);
  await expect(dialog.getByText('Movies')).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/10-search.png` });
});

test('movies browse grid filters and sorts', async ({ page }) => {
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  await page.getByRole('button', { name: 'Rating' }).click();
  await settle(page);
  await page.screenshot({ path: `${SHOTS}/11-movies.png` });
});

test('settings exposes providers, themes and library stats', async ({ page }) => {
  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
  await expect(page.getByText(/connections/)).toBeVisible();
  await expect(page.getByText('EPG coverage')).toBeVisible();
  await settle(page, 400);
  await page.screenshot({ path: `${SHOTS}/12-settings.png` });
});

test('theme and TV density switch at runtime', async ({ page }) => {
  await page.goto('/#/settings');
  await page.getByRole('button', { name: 'light' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await page.screenshot({ path: `${SHOTS}/13-light-theme.png` });

  await page.getByRole('button', { name: 'dark' }).click();
  await page.getByRole('button', { name: 'TV', exact: true }).click();
  await expect(page.locator('html')).toHaveAttribute('data-density', 'tv');
  await page.goto('/#/live');
  await settle(page, 600);
  await page.screenshot({ path: `${SHOTS}/14-tv-mode.png` });
});
