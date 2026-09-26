/**
 * The playlist editor and the two library filters (README §7.3), driven through the
 * real UI against the production bundle.
 *
 * The fixtures carry what these exist for: six tagged foreign channels, three channels
 * the provider lists twice at different qualities, and the same in the film and show
 * libraries. The mock mirrors the host's filtering exactly — see
 * `aurora_db::repo::filtering` and the Rust tests beside it.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

async function openEditor(page: Page, tab: 'Live TV' | 'Movies' | 'Series' = 'Live TV') {
  await page.goto('/#/playlist');
  await expect(page.getByRole('heading', { name: 'Playlist' })).toBeVisible();
  if (tab !== 'Live TV') await page.getByRole('tab', { name: tab }).click();
  await expect(page.getByRole('switch', { name: /^Show / }).first()).toBeVisible();
}

/** Turn one of the library filters on or off in settings. */
/** Which field of `LibraryFilters` each switch in Settings writes. */
const FILTER_FIELD: Record<string, 'englishOnly' | 'hideDuplicates'> = {
  'Show English content only': 'englishOnly',
  'Collapse duplicates': 'hideDuplicates',
};

/** What the host currently believes about one filter. */
async function hostFilter(page: Page, field: string): Promise<boolean> {
  return page.evaluate(async (key) => {
    const w = window as unknown as {
      __auroraInvoke?: (c: string, a: unknown) => Promise<unknown>;
    };
    const filters = (await w.__auroraInvoke!('library.filters', undefined)) as Record<
      string,
      boolean
    >;
    return filters[key]!;
  }, field);
}

/**
 * Turn a library filter on or off, and do not return until the *host* agrees.
 *
 * Two races, both of which made this flaky once Live TV started reading the list
 * lazily. The switch renders its default before its own fetch resolves, so deciding
 * whether to click from `aria-checked` alone could skip a click that was needed; and
 * the command carrying the change to the host is still in flight when the switch
 * flips, so navigating straight to Live TV read a list filtered by what the host
 * still believed. Driving off the host removes both.
 */
async function setFilter(page: Page, name: string, on: boolean) {
  const field = FILTER_FIELD[name]!;
  await page.goto('/#/settings');
  const toggle = page.getByRole('switch', { name });
  await expect(toggle).toBeVisible();

  // Wait for the panel to have read the host, so the switch means something.
  await expect
    .poll(async () => (await toggle.getAttribute('aria-checked')) === String(await hostFilter(page, field)))
    .toBe(true);

  if ((await hostFilter(page, field)) !== on) {
    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-checked', String(on));
  }
  await expect.poll(() => hostFilter(page, field)).toBe(on);
}

/** Channel names as Live TV lists them. */
async function liveChannelNames(page: Page): Promise<string[]> {
  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();
  await page.waitForTimeout(500);

  // The list is virtualised, so the DOM only ever holds the rows on screen: reading
  // it once would answer "what is visible", not "what does Live TV list". Scroll a
  // screenful at a time and accumulate. The rows are `div role="button"` because a
  // heart lives inside each one and a button inside a button is invalid markup.
  const rows = page.getByTestId('channel-row');
  await expect(rows.first()).toBeVisible();
  const scroller = page.getByTestId('channel-scroller');
  const seen = new Set<string>();

  // Keep going until the set stops growing, rather than until `scrollTop` stops
  // moving. The virtualiser measures rows as it renders them, so `scrollHeight` is
  // still settling on the first few passes: stopping when the scroll appears to have
  // reached the bottom truncated the list and made this flaky.
  let idle = 0;
  for (let guard = 0; guard < 200 && idle < 3; guard += 1) {
    const before = seen.size;
    for (const name of await rows.evaluateAll((els) =>
      els.map((el) => (el.getAttribute('aria-label') ?? '').replace(/^Watch /, '')),
    )) {
      seen.add(name);
    }
    idle = seen.size > before ? 0 : idle + 1;
    await scroller.evaluate((el) => {
      el.scrollTop += Math.max(el.clientHeight - 80, 100);
    });
    await page.waitForTimeout(120);
  }
  return [...seen];
}

test('the editor lists every entry, hidden ones included', async ({ page }) => {
  await openEditor(page);

  await expect(page.getByText(/\d+ entries/)).toBeVisible();
  await expect(page.getByRole('button', { name: 'Name for Meridian News' })).toBeVisible();
  // A channel the provider lists more than once says so on the row itself.
  await expect(page.getByTitle(/other copies of this entry/).first()).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/28-playlist-editor.png` });
});

test('renaming a channel keeps the provider name underneath and shows up in Live TV', async ({
  page,
}) => {
  await openEditor(page);

  await page.getByRole('button', { name: 'Name for Meridian News' }).click();
  const input = page.getByRole('textbox', { name: 'Name for Meridian News' });
  await input.fill('My News Channel');
  await input.press('Enter');

  await expect(page.getByRole('button', { name: 'Name for My News Channel' })).toBeVisible();
  // The provider's own name stays visible, which is how a rename is undone later.
  await expect(page.getByText('Meridian News', { exact: true })).toBeVisible();

  const names = await liveChannelNames(page);
  expect(names.join(' ')).toContain('My News Channel');
});

test('hiding a channel removes it from Live TV but not from the editor', async ({ page }) => {
  await openEditor(page);

  const row = page.getByRole('switch', { name: 'Show Northwind 24' });
  await expect(row).toHaveAttribute('aria-checked', 'true');
  await row.click();
  await expect(row).toHaveAttribute('aria-checked', 'false');

  const names = await liveChannelNames(page);
  expect(names.join(' ')).not.toContain('Northwind 24');

  // Still in the editor: hiding has to be reversible.
  await openEditor(page);
  await expect(page.getByRole('switch', { name: 'Show Northwind 24' })).toBeVisible();
  // The filters are popovers now, not native selects: a `<select>` renders as a blank
  // rectangle in the WebView this ships inside, and 202 categories is not a list
  // anybody scrolls.
  await page.getByRole('button', { name: 'Show' }).click();
  await page.getByRole('option', { name: 'Hidden only' }).click();
  await expect(page.getByRole('button', { name: 'Name for Northwind 24' })).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/29-playlist-hidden.png` });
});

test('a selection can be hidden and reset in one go', async ({ page }) => {
  await openEditor(page);

  await page.getByRole('checkbox', { name: 'Select Civic Report' }).check();
  await page.getByRole('checkbox', { name: 'Select Continental News' }).check();
  await expect(page.getByText('2 selected')).toBeVisible();

  await page.getByRole('button', { name: 'Hide', exact: true }).click();
  await expect(page.getByRole('switch', { name: 'Show Civic Report' })).toHaveAttribute(
    'aria-checked',
    'false',
  );
  await expect(page.getByRole('switch', { name: 'Show Continental News' })).toHaveAttribute(
    'aria-checked',
    'false',
  );

  await page.getByRole('button', { name: 'Reset' }).first().click();
  await expect(page.getByRole('switch', { name: 'Show Civic Report' })).toHaveAttribute(
    'aria-checked',
    'true',
  );
});

test('everything a search matches can be hidden without paging through it', async ({ page }) => {
  await openEditor(page);

  await page.getByRole('textbox', { name: 'Search this list' }).fill('Apex');
  await expect(page.getByText(/^[1-9]\d* entries$/)).toBeVisible();

  await page.getByRole('checkbox', { name: 'Select everything listed' }).check();
  await page.getByRole('button', { name: 'Hide', exact: true }).click();

  const names = await liveChannelNames(page);
  expect(names.join(' ')).not.toContain('Apex Sports');
});

test('the duplicates filter finds exactly the entries listed more than once', async ({ page }) => {
  await openEditor(page);

  await page.getByRole('button', { name: 'Duplicates only' }).click();
  const rows = page.getByTitle(/other copies of this entry/);
  await expect(rows.first()).toBeVisible();
  // Every row left is one with another copy.
  const badges = await rows.count();
  const names = await page.getByRole('button', { name: /^Name for / }).count();
  expect(badges).toBe(names);
});

test('English only hides tagged foreign content across all three lists', async ({ page }) => {
  // Before: the foreign channels are there.
  const before = (await liveChannelNames(page)).join(' ');
  expect(before).toContain('TF1');

  await setFilter(page, 'Show English content only', true);

  const after = (await liveChannelNames(page)).join(' ');
  expect(after).not.toContain('TF1');
  expect(after).not.toContain('MBC 1');
  // Untagged channels stay: the whole point of the rule.
  expect(after).toContain('Meridian News');

  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  await page.waitForTimeout(600);
  await expect(page.getByText('[SPANISH] La Casa del Lago')).toBeHidden();

  await page.goto('/#/series');
  await expect(page.getByRole('heading', { name: 'Series' })).toBeVisible();
  await page.waitForTimeout(600);
  await expect(page.getByText('[SPANISH] La Casa de Papel del Norte')).toBeHidden();

  await setFilter(page, 'Show English content only', false);
  expect((await liveChannelNames(page)).join(' ')).toContain('TF1');
});

test('collapsing duplicates leaves the best copy of each channel', async ({ page }) => {
  const before = (await liveChannelNames(page)).join(' ');
  expect(before).toContain('Meridian News SD');

  await setFilter(page, 'Collapse duplicates', true);

  const after = (await liveChannelNames(page)).join(' ');
  // One Meridian News survives, and it is not the SD copy.
  expect(after).not.toContain('Meridian News SD');
  expect(after).not.toContain('Meridian News HD');
  expect(after).toContain('Meridian News');

  await setFilter(page, 'Collapse duplicates', false);
  expect((await liveChannelNames(page)).join(' ')).toContain('Meridian News SD');
});

test('settings says what each filter would hide before it is turned on', async ({ page }) => {
  await page.goto('/#/settings');
  const panel = page.locator('section', { hasText: 'FILTERING' });
  await expect(panel.getByText(/\d+ not English/).first()).toBeVisible();
  await expect(panel.getByText(/\d+ untagged/).first()).toBeVisible();
  await expect(panel.getByText(/\d+ duplicates/).first()).toBeVisible();
  await page.getByRole('switch', { name: 'Show English content only' }).scrollIntoViewIfNeeded();
  await page.screenshot({ path: `${SHOTS}/30-filter-settings.png` });
});

test('the copies collapsing hid are still reachable as other sources', async ({ page }) => {
  await setFilter(page, 'Collapse duplicates', true);

  // A channel the provider carries three times: the guide offers the other two.
  await page.goto('/#/guide');
  await page.waitForTimeout(700);
  await page.locator('button[title*="–"]').first().click();
  const aside = page.locator('aside');
  await expect(aside.getByText('Also in')).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/31-channel-sources.png` });

  await setFilter(page, 'Collapse duplicates', false);
});

test('a collapsed film keeps its other copy on the card', async ({ page }) => {
  await setFilter(page, 'Collapse duplicates', true);

  await page.goto('/#/movies');
  await expect(page.getByRole('heading', { name: 'Movies' })).toBeVisible();
  await page.waitForTimeout(600);

  // The fixtures carry one film twice; find the card that has another source.
  const cards = page.locator('[role="button"]');
  const count = Math.min(await cards.count(), 12);
  let found = false;
  for (let i = 0; i < count; i += 1) {
    await cards.nth(i).click();
    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    if (await dialog.getByText('Also available as').isVisible().catch(() => false)) {
      found = true;
      await page.screenshot({ path: `${SHOTS}/32-movie-sources.png` });
      break;
    }
    await page.keyboard.press('Escape');
  }
  expect(found, 'no collapsed film offered its other copy').toBe(true);

  await page.keyboard.press('Escape');
  await setFilter(page, 'Collapse duplicates', false);
});

test('the home page and search follow the filters too', async ({ page }) => {
  await setFilter(page, 'Show English content only', true);

  await page.goto('/#/');
  await page.waitForTimeout(800);
  await expect(page.getByText('[SPANISH] La Casa del Lago')).toBeHidden();

  await page.keyboard.press('Control+k');
  const dialog = page.getByRole('dialog', { name: 'Search' });
  await dialog.getByRole('textbox').fill('TF1');
  // The palette says so rather than showing a channel the filter is hiding.
  await expect(dialog.getByText(/Nothing matches/)).toBeVisible();

  await page.keyboard.press('Escape');
  await setFilter(page, 'Show English content only', false);

  await page.keyboard.press('Control+k');
  await dialog.getByRole('textbox').fill('TF1');
  await expect(dialog.getByText(/Nothing matches/)).toBeHidden();
  await expect(dialog.getByText('Live Channels')).toBeVisible();
});

test('films and shows are editable in the same screen as channels', async ({ page }) => {
  await openEditor(page, 'Movies');
  // No channel numbers on a film.
  await expect(page.getByRole('button', { name: /^Number for / })).toHaveCount(0);

  const first = page.getByRole('button', { name: /^Name for / }).first();
  const label = (await first.getAttribute('aria-label'))!.replace('Name for ', '');
  await first.click();
  const input = page.getByRole('textbox', { name: `Name for ${label}` });
  await input.fill('Renamed Film');
  await input.press('Enter');
  await expect(page.getByRole('button', { name: 'Name for Renamed Film' })).toBeVisible();

  await openEditor(page, 'Series');
  await expect(page.getByRole('button', { name: /^Name for / }).first()).toBeVisible();
});

/**
 * Live TV rendered `(channels ?? []).map(...)` with no virtualiser, and each row
 * mounted its own `epg.nowNext`. On the subscription in docs/ROADMAP.md that is
 * 22,121 buttons and 22,121 IPC round trips to paint one screen — each of those
 * round trips itself scanning the whole channel table to find one row.
 *
 * The Guide and this playlist editor were already virtualised; Live TV was missed.
 */
test('live TV renders a large channel list without mounting every row', async ({ page }) => {
  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();

  const total = await page.evaluate(async () => {
    const w = window as unknown as {
      __auroraInvoke?: (c: string, a: unknown) => Promise<unknown>;
    };
    const all = (await w.__auroraInvoke!('channels.list', {})) as unknown[];
    return all.length;
  });
  expect(total).toBeGreaterThan(20);

  const rows = page.getByTestId('channel-row');
  await expect(rows.first()).toBeVisible();
  const mounted = await rows.count();
  expect(mounted).toBeLessThan(total);

  // The rows past the fold are reachable, they are simply not in the DOM yet.
  const firstName = await rows.first().getAttribute('aria-label');
  await page.getByTestId('channel-scroller').evaluate((el) => {
    el.scrollTop = el.scrollHeight;
  });
  await page.waitForTimeout(400);
  await expect(rows.first()).not.toHaveAttribute('aria-label', firstName!);
});
