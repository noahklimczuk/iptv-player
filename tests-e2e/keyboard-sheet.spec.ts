/**
 * The in-app keyboard reference, against the bindings the app actually interprets.
 *
 * Settings → Keyboard listed ten rows; `useHotkeys` implements eighteen bindings. The
 * six missing ones were `Escape`, `J`, `L`, `F`, `P` and `I` — and two of those had no
 * documentation anywhere else at all: `I` reaches the channel banner and the detail
 * panel, and `P` reaches 832 lines of picture-in-picture whose only discoverable trace
 * was the source. README §14.1 promises a sheet that `?` opens and a `docs/SHORTCUTS.md`;
 * neither exists, so this panel is the sheet (R-143).
 */
import { expect, test } from '@playwright/test';

/** Keys the hook binds, and the word that must appear beside each on the sheet. */
const BINDINGS: [string, RegExp][] = [
  ['Ctrl+K', /palette|search/i],
  ['Escape', /back|close/i],
  ['0–9', /channel/i],
  ['Backspace', /last channel/i],
  ['PgUp', /channel/i],
  ['G', /guide/i],
  ['Space', /play|pause/i],
  ['J / L', /seek/i],
  ['T', /pause live/i],
  ['F / F11', /fullscreen/i],
  ['P', /picture-in-picture/i],
  ['I', /info/i],
  ['Alt+1', /home/i],
];

test('the keyboard sheet lists every binding the app interprets', async ({ page }) => {
  await page.goto('/#/settings');
  const sheet = page.getByRole('region', { name: 'Keyboard' });
  await expect(sheet).toBeVisible();
  const text = await sheet.innerText();

  for (const [key, action] of BINDINGS) {
    expect(text, `${key} is not on the keyboard sheet`).toContain(key);
    // And the sheet says what the key does, not merely that it exists.
    expect(text, `${key} has no action beside it`).toMatch(action);
  }
});

test('F and F11 both go fullscreen, which is what the sheet now says', async ({ page }) => {
  // `f` was bound to fullscreen while README §14.1 assigned it to favouriting and gave
  // fullscreen to `F11`, which was bound to nothing. Both work now, and the OSD's own
  // label is what says so — the mock answers `window.fullscreen` by toggling and
  // reporting the state it ended in, so the label follows the real answer.
  await page.goto('/#/?video');
  await page.getByRole('button', { name: 'Play' }).first().click();
  await page.waitForTimeout(600);
  await page.mouse.move(640, 360);
  await expect(page.getByRole('button', { name: 'Fullscreen' })).toBeVisible();

  await page.keyboard.press('f');
  await page.mouse.move(640, 361);
  await expect(page.getByRole('button', { name: 'Leave fullscreen' })).toBeVisible();

  // `F11` is the key the table promised and nothing bound.
  await page.keyboard.press('F11');
  await page.mouse.move(640, 362);
  await expect(page.getByRole('button', { name: 'Fullscreen' })).toBeVisible();
});

test('the favourites empty state names a control that exists', async ({ page }) => {
  await page.goto('/#/live');
  await expect(page.getByTestId('channel-row').first()).toBeVisible();

  // The fixtures favourite about a fifth of the channels, so the empty state is
  // unreachable until they are cleared — which is worth doing rather than skipping,
  // because this sentence is only ever read by somebody who has none.
  await page.evaluate(async () => {
    const w = window as unknown as {
      __auroraInvoke: (c: string, a: unknown) => Promise<unknown>;
    };
    const favourites = (await w.__auroraInvoke('channels.list', {
      favoritesOnly: true, profileId: 1,
    })) as { id: number }[];
    for (const c of favourites) {
      await w.__auroraInvoke('favorites.toggle', { profileId: 1, channelId: c.id });
    }
  });

  // Favourites, with none set: the one sentence a viewer reads in exactly this
  // situation. It used to say "Press F while watching", and `F` goes fullscreen.
  await page
    .getByRole('navigation', { name: 'Channel groups' })
    .getByRole('button', { name: 'Favourites' })
    .click();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Favourites');
  const empty = page.getByText(/to add it here/);
  await expect(empty).toBeVisible();
  await expect(empty).not.toContainText('Press F');
  await expect(empty).toContainText('heart');
});
