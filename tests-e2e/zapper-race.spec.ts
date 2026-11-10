/**
 * Digit entry, overtaken.
 *
 * Every way to reach a channel is synchronous except typing its number: that one has to
 * ask the host what channel 401 *is* before it can tune it, and `channels.byNumber`
 * takes the single writer mutex like everything else. So there is a window — the whole
 * of that lookup — in which the viewer has already moved on.
 *
 * `Playback` guards its half with a generation counter, so the *stream* ends up being
 * whatever was asked for last. Nothing guarded this half, so the stale reply still
 * called `tune`, which sets `current`, raises the banner, and makes the channel it names
 * the Backspace target. The picture and everything naming it disagreed.
 */
import { expect, test } from '@playwright/test';

/** Longer than it takes to click a row, which is the point. */
const LOOKUP_MS = 2000;

test('a channel clicked during a digit lookup is not overridden by it', async ({ page }) => {
  await page.goto(`/?slow=channels.byNumber:${LOOKUP_MS}#/live`);
  const rows = page.getByTestId('channel-row');
  await expect(rows.first()).toBeVisible();

  // The number of the row that will be clicked, read before anything is typed.
  const clicked = (await rows.first().innerText()).split('\n')[0]!.trim();
  expect(clicked, 'the first row has no channel number to assert on').toMatch(/^\d+$/);

  // Type a different channel's number and let the 1.8s entry timeout commit it, so its
  // lookup is in flight and will not answer for `LOOKUP_MS`.
  for (const d of '401') await page.keyboard.press(d);
  // The overlay is `DigitEntry`, which names what has been typed so far; it goes when
  // the entry commits, which is the moment the lookup goes out.
  const entry = page.getByLabel('Channel 401');
  await expect(entry).toBeVisible();
  await expect(entry).toBeHidden({ timeout: 6_000 });

  // Then change your mind, the ordinary way.
  await rows.first().click();
  const banner = page.getByTestId('channel-banner');
  await expect(banner).toBeVisible();
  await expect(banner).toContainText(clicked);

  // Long enough for the abandoned lookup to come back and have its say.
  await page.waitForTimeout(LOOKUP_MS + 1_000);
  await expect(
    banner,
    'a lookup the viewer had already moved on from tuned over their choice',
  ).toContainText(clicked);
});
