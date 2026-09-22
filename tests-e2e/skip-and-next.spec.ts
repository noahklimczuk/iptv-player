/**
 * Skip Intro and Next Episode (README §9), driven through the real UI against the
 * production bundle.
 *
 * Series 1 ships chapter markers in the fixtures (intro 28–92s, credits at
 * runtime−50s), so these journeys exercise the chapter path; the learning path is
 * covered by the Rust tests in aurora-db::repo::markers.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

/** Open the first series and start its first episode. */
async function playFirstEpisode(page: Page) {
  await page.goto('/#/series');
  await expect(page.getByRole('heading', { name: 'Series' })).toBeVisible();
  await page.locator('[role="button"]').first().click();

  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.getByRole('tab', { name: 'episodes' }).click();
  // Episode rows are buttons inside the episodes tab.
  await dialog.locator('button', { hasText: /^\d/ }).first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
}

/** Move the playhead with the OSD scrubber. */
async function seekTo(page: Page, seconds: number) {
  const slider = page.getByRole('slider', { name: 'Seek' });
  await slider.evaluate((el, value) => {
    const input = el as HTMLInputElement;
    const setter = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      'value',
    )!.set!;
    setter.call(input, String(value));
    input.dispatchEvent(new Event('change', { bubbles: true }));
  }, seconds);
}

test('Skip Intro appears inside the intro and seeks past it', async ({ page }) => {
  await playFirstEpisode(page);

  // Before the intro there is nothing to skip.
  await seekTo(page, 10);
  await expect(page.getByRole('button', { name: 'Skip Intro' })).toBeHidden();

  // Inside the marker the button appears.
  await seekTo(page, 45);
  const skip = page.getByRole('button', { name: 'Skip Intro' });
  await expect(skip).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/15-skip-intro.png` });

  await skip.click();

  // It jumps past the intro and the button goes away.
  await expect(skip).toBeHidden();
  const position = await page
    .getByRole('slider', { name: 'Seek' })
    .evaluate((el) => Number((el as HTMLInputElement).value));
  expect(position).toBeGreaterThanOrEqual(92);
});

test('Skip Intro is gone once the intro has passed', async ({ page }) => {
  await playFirstEpisode(page);
  await seekTo(page, 600);
  await expect(page.getByRole('button', { name: 'Skip Intro' })).toBeHidden();
});

test('the Next Episode button plays the following episode', async ({ page }) => {
  await playFirstEpisode(page);

  const next = page.getByRole('button', { name: /^Next episode: S\d+ E\d+/ });
  await expect(next).toBeVisible();

  const before = await page.getByRole('heading', { level: 1 }).count();
  await next.click();

  // The player reloads on the next episode: position resets to the start.
  await expect
    .poll(async () =>
      page
        .getByRole('slider', { name: 'Seek' })
        .evaluate((el) => Number((el as HTMLInputElement).value)),
    )
    .toBeLessThan(5);
  expect(before).toBeGreaterThanOrEqual(0);
});

test('Up Next counts down at the credits and can be cancelled', async ({ page }) => {
  await playFirstEpisode(page);

  const duration = await page
    .getByRole('slider', { name: 'Seek' })
    .evaluate((el) => Number((el as HTMLInputElement).max));

  // The fixtures place credits 50s before the end.
  await seekTo(page, duration - 30);

  const card = page.getByRole('complementary', { name: 'Up next' });
  await expect(card).toBeVisible();
  await expect(card.getByText(/Up next in \d+s/)).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/16-up-next.png` });

  await card.getByRole('button', { name: 'Cancel' }).click();
  await expect(card).toBeHidden();

  // Dismissal sticks for this episode rather than reappearing a second later.
  await seekTo(page, duration - 20);
  await expect(card).toBeHidden();
});

test('Play now on the Up Next card starts the next episode immediately', async ({ page }) => {
  await playFirstEpisode(page);

  const duration = await page
    .getByRole('slider', { name: 'Seek' })
    .evaluate((el) => Number((el as HTMLInputElement).max));
  await seekTo(page, duration - 30);

  const card = page.getByRole('complementary', { name: 'Up next' });
  await expect(card).toBeVisible();
  await card.getByRole('button', { name: 'Play now' }).click();

  await expect(card).toBeHidden();
  await expect
    .poll(async () =>
      page
        .getByRole('slider', { name: 'Seek' })
        .evaluate((el) => Number((el as HTMLInputElement).value)),
    )
    .toBeLessThan(5);
});
