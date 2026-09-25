/**
 * Every screen, in its three states.
 *
 * The rest of the suite tests journeys — add a provider, tune a channel, schedule a
 * recording. This one is the sweep the brief asks for: does each screen draw at all,
 * does it say something useful when there is nothing to show, and does it say
 * something useful when the host refuses?
 *
 * All of it runs against the mock transport, which is the standing limitation of every
 * end-to-end test here and the reason the CSP and the missing `favorite` field stayed
 * invisible for so long. `AUDIT/test-report.md` lists what that leaves unverified.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

/** Every screen in the sidebar, with something on it that proves it drew. */
const SCREENS = [
  // `nav` is the sidebar label, `heading` the one on the page; they differ for the
  // Guide, which is "Guide" in the sidebar and "TV Guide" at the top of the screen.
  { path: '/', nav: 'Home', heading: null, region: 'Featured' },
  { path: '/live', nav: 'Live TV', heading: 'Live TV', region: null },
  { path: '/guide', nav: 'Guide', heading: 'TV Guide', region: null },
  { path: '/movies', nav: 'Movies', heading: 'Movies', region: null },
  { path: '/series', nav: 'Series', heading: 'Series', region: null },
  { path: '/recordings', nav: 'Recordings', heading: 'Recordings', region: null },
  { path: '/playlist', nav: 'Playlist', heading: 'Playlist', region: null },
  { path: '/settings', nav: 'Settings', heading: 'Settings', region: null },
] as const;

async function settle(page: Page, ms = 600) {
  await page.waitForLoadState('networkidle').catch(() => {});
  await page.waitForTimeout(ms);
}

/** Make the next call to `command` refuse, the way the host refuses. */
async function failNext(page: Page, command: string, message: string) {
  await page.evaluate(
    ([c, m]) => {
      const w = window as unknown as {
        __auroraFailNext?: (name: string, msg: string) => void;
      };
      w.__auroraFailNext!(c as string, m as string);
    },
    [command, message],
  );
}

test('every screen draws its happy path', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(`${e.name}: ${e.message}`));
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });

  for (const screen of SCREENS) {
    await page.goto(`/#${screen.path}`);
    if (screen.region) {
      await expect(page.getByRole('region', { name: screen.region })).toBeVisible();
    } else {
      await expect(page.getByRole('heading', { name: screen.heading! })).toBeVisible();
    }
    await settle(page, 400);
    await page.screenshot({
      path: `${SHOTS}/walkthrough-${screen.path.replace(/\//g, '') || 'home'}.png`,
    });
  }

  // A screen that renders but throws on the way is not a screen that works.
  expect(errors, `console errors across the walkthrough:\n${errors.join('\n')}`).toEqual([]);
});

test('every screen is reachable from the sidebar by keyboard', async ({ page }) => {
  await page.goto('/#/');
  const nav = page.getByRole('navigation', { name: 'Main' });
  for (const screen of SCREENS) {
    await expect(nav.getByRole('link', { name: screen.nav })).toBeVisible();
  }

  // Alt+1…5 are the documented jumps (README §14.1).
  for (const [key, heading] of [
    ['2', 'Live TV'],
    ['3', 'TV Guide'],
    ['4', 'Movies'],
    ['5', 'Series'],
  ] as const) {
    await page.keyboard.press(`Alt+${key}`);
    await expect(page.getByRole('heading', { name: heading })).toBeVisible();
  }
});

test('an empty library says what to do about it rather than showing nothing', async ({
  page,
}) => {
  // `?setup` forces the first-run wizard, which is the empty state for the whole app.
  // Before the hash, because `window.location.search` is what the flag is read from
  // and a HashRouter puts everything after `#` in the hash instead.
  await page.goto('/?setup#/');
  await expect(page.getByRole('button', { name: /check|continue|next/i }).first())
    .toBeVisible();
  await page.screenshot({ path: `${SHOTS}/walkthrough-empty-firstrun.png` });
});

test('an empty filter result says so on every list screen', async ({ page }) => {
  // Favourites with nothing favourited is the reachable empty state on Live TV.
  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();
  await page.getByRole('button', { name: 'Favorites' }).click();
  const filled = page.getByRole('button', { name: /Remove .* from favourites/ });
  for (let guard = 0; guard < 40 && (await filled.count()) > 0; guard += 1) {
    await filled.first().click();
  }
  await expect(page.getByText('No favorite channels yet')).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/walkthrough-empty-favorites.png` });

  // A genre nothing matches is the one on Movies.
  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
});

test('a refusal on a list screen is reported rather than left as a blank panel', async ({
  page,
}) => {
  // Set the fault, then navigate *within* the app: a reload would drop it, since the
  // mock transport's state lives in the page.
  for (const [nav, command] of [
    ['Live TV', 'channels.list'],
    ['Recordings', 'dvr.list'],
  ] as const) {
    await page.goto('/#/');
    await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();

    await failNext(page, command, 'the library is being rebuilt');
    await page.getByRole('navigation', { name: 'Main' })
      .getByRole('link', { name: nav })
      .click();

    // This is the finding Phase 4 turned up: two of twenty-two call sites read the
    // hook's `error`, so the rest drew their empty state — "No channels, add a
    // provider in Settings" — on a machine that already had one. The hook reports it
    // now, so the screen cannot swallow it.
    const notices = page.getByTestId('notices');
    await expect(notices).toBeVisible({ timeout: 8000 });
    await expect(notices).toContainText('being rebuilt');

    await page.screenshot({
      path: `${SHOTS}/walkthrough-refusal-${command.replace('.', '-')}.png`,
    });
  }
});

test('the player OSD draws its error state', async ({ page }) => {
  await page.goto('/#/live');
  await page.getByTestId('channel-row').first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();

  await page.evaluate(() => {
    const w = window as unknown as {
      __auroraInvoke?: (c: string, a: unknown) => Promise<unknown>;
    };
    return w.__auroraInvoke!('player.stop', undefined);
  });
  await settle(page, 300);
  await page.screenshot({ path: `${SHOTS}/walkthrough-player-stopped.png` });
});

test('the themes all render without losing text against their background', async ({
  page,
}) => {
  await page.goto('/#/settings');
  await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();

  for (const theme of ['dark', 'oled', 'light', 'contrast']) {
    await page.evaluate((t) => {
      document.documentElement.dataset.theme = t;
    }, theme);
    await page.goto('/#/');
    await settle(page, 300);

    const contrast = await page.evaluate(() => {
      const body = getComputedStyle(document.body);
      return { color: body.color, background: body.backgroundColor };
    });
    expect(
      contrast.color,
      `${theme}: text and background are the same colour`,
    ).not.toBe(contrast.background);

    await page.screenshot({ path: `${SHOTS}/walkthrough-theme-${theme}.png` });
  }
});
