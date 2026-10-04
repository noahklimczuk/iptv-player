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
  // Recommendations. The heading names the viewer's own taste ("More Sci-fi and
  // Crime") or says it does not know them yet ("Worth a look") — it used to be
  // "Because you watched <the fourth film in the fixture>", which was true of nothing.
  // "Because you watched X" is now a line under each poster, where it can be true.
  await expect(
    page.getByRole('region', { name: /^(Worth a look|More |Recommended for you)/ }),
  ).toBeVisible();
  await expect(page.getByTestId('card-reason').first()).toBeVisible();

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
  // The preview itself is covered in `trailers.spec.ts`, against a rail whose titles are
  // known to have one — Recently Added is films, and only some films carry a trailer.
  await page.screenshot({ path: `${SHOTS}/02-hover-card.png` });
});

test('an expanded card pushes its neighbours aside rather than covering them', async ({
  page,
}) => {
  await page.goto('/#/');
  await settle(page, 600);

  const rail = page.getByRole('region', { name: 'Recently Added' });
  const cards = rail.getByTestId('catalog-card');
  await expect(cards.first()).toBeVisible();
  // The transform is on the inner element: the outer div is the flex item and never
  // moves, which is exactly why the rail has to own which card is expanded.
  const inner = (n: number) => cards.nth(n).getByRole('button').first();

  const before = await inner(3).boundingBox();
  await cards.nth(2).hover();
  await page.waitForTimeout(700);
  const after = await inner(3).boundingBox();

  expect(
    after!.x,
    'the card to the right of an expanded one should make room',
  ).toBeGreaterThan(before!.x + 10);
});

test('a card can be taken off Continue Watching, and stays off', async ({ page }) => {
  await page.goto('/#/');
  await settle(page, 600);

  const rail = page.getByRole('region', { name: 'Continue Watching' });
  const remove = rail.getByRole('button', { name: /^Remove .* from Continue Watching$/ });

  // The action lives in the expanded panel, so the card has to be hovered first.
  const first = rail.getByTestId('catalog-card').first();
  await first.hover();
  await expect(remove.first()).toBeVisible();

  // Which card, by name, so the assertion is about that one and not about the count.
  const label = await remove.first().getAttribute('aria-label');
  const title = label!.replace(/^Remove /, '').replace(/ from Continue Watching$/, '');
  const before = await rail.getByTestId('catalog-card').count();

  await remove.first().click();
  await expect(rail.getByRole('button', { name: new RegExp(`^${title},`) })).toHaveCount(0);
  expect(await rail.getByTestId('catalog-card').count()).toBe(before - 1);

  // And it is gone from the host, not just from the screen: leaving and coming back
  // rebuilds the rail from `library.rails`.
  await page.goto('/#/settings');
  await page.goto('/#/');
  await settle(page, 600);
  await expect(
    page
      .getByRole('region', { name: 'Continue Watching' })
      .getByRole('button', { name: new RegExp(`^${title},`) }),
  ).toHaveCount(0);
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

test('the channel banner goes away while the player keeps ticking', async ({ page }) => {
  await page.goto('/#/live');
  await settle(page, 600);
  await page.getByRole('button', { name: /^Watch / }).first().click();

  // The banner is the thing that carries the channel number and the now/next line.
  const banner = page.getByTestId('channel-banner');
  await expect(banner).toBeVisible();

  // It is supposed to fade after five seconds. The player's state arrives four times a
  // second the whole time, and that is the point: the auto-hide used to depend on the
  // whole UI store, so every heartbeat cancelled the pending timeout and started a new
  // five seconds. The banner then stayed up for the entire programme.
  await expect(banner).toBeHidden({ timeout: 15_000 });
});

test('player OSD exposes transport, tracks and stats', async ({ page }) => {
  await page.goto('/#/live');
  await settle(page, 600);
  await page.getByRole('button', { name: 'Watch Meridian News' }).first().click();

  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await page.getByRole('button', { name: 'Subtitles' }).click();
  await expect(page.getByText('English SDH')).toBeVisible();
  await page.getByRole('button', { name: 'Playback stats' }).click();
  await expect(page.getByText('Hardware decoder')).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/09-player-osd.png` });
});

test('the fullscreen button in the player actually does something', async ({ page }) => {
  // It had no onClick at all — a button that has always been decorative. The mock
  // answers `window.fullscreen` by toggling and reporting the state it ended in, so
  // what this checks is that pressing it changes that state and the label follows.
  await page.goto('/#/?video');
  await settle(page, 600);
  await page.getByRole('button', { name: 'Play' }).first().click();
  await settle(page, 600);

  // The OSD hides itself, so wake it before looking for anything on it.
  await page.mouse.move(640, 360);
  const button = page.getByRole('button', { name: 'Fullscreen' });
  await expect(button).toBeVisible();
  await button.click();
  await settle(page, 300);

  await page.mouse.move(640, 361);
  await expect(page.getByRole('button', { name: 'Leave fullscreen' })).toBeVisible();
});

test('command palette searches across every content kind', async ({ page }) => {
  await page.goto('/#/');
  await settle(page, 600);
  await page.keyboard.press('Control+k');

  const dialog = page.getByRole('dialog', { name: 'Search' });
  await expect(dialog).toBeVisible();
  await dialog.getByRole('textbox').fill('the');
  await page.waitForTimeout(400);

  // By the group's own test id rather than by its heading text. `getByText('Movies')`
  // also matches "Show 8 more movies", and — more to the point — Playwright calls an
  // element inside a scroller visible whether or not it is scrolled into view, so this
  // assertion passed the whole time the library was being pushed below the fold.
  const movies = dialog.getByTestId('palette-group-movies');
  await expect(movies).toBeVisible();
  await expect(movies.getByRole('button')).not.toHaveCount(0);

  // Before anything is unfolded, so the shot shows what a search actually looks like.
  await page.screenshot({ path: `${SHOTS}/10-search.png` });

  // Capped per group, so the guide's matches cannot bury the library. The rest are a
  // click away rather than thrown away.
  const onNow = dialog.getByTestId('palette-group-onNow');
  const more = onNow.getByRole('button', { name: /^Show \d+ more on now$/ });
  if (await more.count()) {
    const before = await onNow.getByRole('button').count();
    await more.click();
    await expect(more).toHaveCount(0);
    expect(await onNow.getByRole('button').count()).toBeGreaterThan(before);
  }
});

test('picking a film in the palette opens that film', async ({ page }) => {
  // The palette found the right title and then navigated to the Movies page, throwing
  // `hit.refId` away — so the answer to "open this" was a page of everything, which
  // reads as the title not being there at all.
  await page.goto('/#/');
  await settle(page, 600);
  await page.keyboard.press('Control+k');

  const dialog = page.getByRole('dialog', { name: 'Search' });
  await expect(dialog).toBeVisible();
  await dialog.getByRole('textbox').fill('the');
  await page.waitForTimeout(400);

  // A film specifically, and what it is called, so the sheet that opens can be checked
  // against it rather than against "something opened". Scoped to the Movies group: the
  // first button in the dialog belongs to whichever group comes first, usually a channel.
  const movies = dialog.locator('[data-testid="palette-group-movies"]');
  await expect(movies).toBeVisible();
  const hit = movies.getByRole('button').first();
  const picked = (await hit.innerText()).split('\n')[0]!.trim();
  await hit.click();

  const sheet = page.getByRole('dialog', { name: picked });
  await expect(sheet).toBeVisible();
  await expect(sheet.getByRole('button', { name: /^(Play|Resume from)/ })).toBeVisible();
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
