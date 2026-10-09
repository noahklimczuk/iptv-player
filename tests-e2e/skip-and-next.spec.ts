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

/**
 * Move the playhead with the OSD scrubber, the way a person does: drag, then let go.
 *
 * The release is the half that matters. The scrubber seeks on `pointerup` rather than
 * on every value it passes through, because seeking on `change` sent a seek for every
 * pixel of a drag and let the handle snap back to the host's last reported position in
 * between. A test that only dispatches `change` is describing the bug, not the bar.
 */
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
    input.dispatchEvent(new PointerEvent('pointerup', { bubbles: true }));
  }, seconds);
}

/**
 * Open a series that ships **no** chapters and start its first episode.
 *
 * The fixtures give odd-numbered series chapters and even-numbered ones none, so this is
 * the state an ordinary library is in: no chapters anywhere, nothing ever skipped. The
 * card is found by asking the mock which series is even rather than by its position on
 * the page, because the grid's order is a sort and not an id.
 */
async function playEpisodeWithoutChapters(page: Page) {
  await page.goto('/#/series');
  await expect(page.getByRole('heading', { name: 'Series' })).toBeVisible();

  const title = await page.evaluate(async () => {
    const w = window as unknown as {
      __auroraInvoke?: (c: string, a?: unknown) => Promise<unknown>;
    };
    const rows = (await w.__auroraInvoke!('library.series', {
      sort: 'title',
      limit: 200,
      offset: 0,
    })) as { id: number; title: string }[];
    return rows.find((r) => r.id % 2 === 0)!.title;
  });

  await page.getByRole('button', { name: new RegExp(title.slice(0, 18), 'i') }).first().click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.getByRole('tab', { name: 'episodes' }).click();
  await dialog.locator('button', { hasText: /^\d/ }).first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
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

test('per-show playback preferences persist', async ({ page }) => {
  await page.goto('/#/series');
  await page.locator('[role="button"]').first().click();

  const dialog = page.getByRole('dialog');
  await dialog.getByRole('tab', { name: 'episodes' }).click();

  const alwaysSkip = dialog.getByRole('switch', { name: 'Always skip intros' });
  await expect(alwaysSkip).toHaveAttribute('aria-checked', 'false');
  await alwaysSkip.click();
  await expect(alwaysSkip).toHaveAttribute('aria-checked', 'true');
  await page.screenshot({ path: `${SHOTS}/17-series-prefs.png` });

  // Reopening the show reflects the stored value rather than resetting.
  await page.keyboard.press('Escape');
  await page.locator('[role="button"]').first().click();
  await dialog.getByRole('tab', { name: 'episodes' }).click();
  await expect(
    dialog.getByRole('switch', { name: 'Always skip intros' }),
  ).toHaveAttribute('aria-checked', 'true');
});

test('auto-skip jumps the intro without being pressed', async ({ page }) => {
  await page.goto('/#/series');
  await page.locator('[role="button"]').first().click();
  const dialog = page.getByRole('dialog');
  await dialog.getByRole('tab', { name: 'episodes' }).click();
  await dialog.getByRole('switch', { name: 'Always skip intros' }).click();

  await dialog.locator('button', { hasText: /^\d/ }).first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();

  await seekTo(page, 45);

  // No button press: the playhead should move itself past the intro.
  await expect
    .poll(async () =>
      page
        .getByRole('slider', { name: 'Seek' })
        .evaluate((el) => Number((el as HTMLInputElement).value)),
    )
    .toBeGreaterThanOrEqual(92);
  await expect(page.getByRole('button', { name: 'Skip Intro' })).toBeHidden();
});

/**
 * The point of the conventional tier: a library with no chapters and a viewer who has
 * never skipped anything still gets Skip Credits — and therefore Up Next, and therefore
 * autoplay. Before this there was nothing at all.
 */
test('an episode with no chapters still gets Skip Credits from its runtime', async ({
  page,
}) => {
  await playEpisodeWithoutChapters(page);

  // Nothing has said where an intro is, so there is no Skip Intro. A clock cannot find
  // one, and guessing would mean cutting a cold open.
  await seekTo(page, 60);
  await expect(page.getByRole('button', { name: 'Skip Intro' })).toBeHidden();

  // The credits, though, are defined by being at the end — so ask the host how long
  // this episode is rather than assuming every fixture episode is the same length.
  const duration = await page.evaluate(async () => {
    const w = window as unknown as {
      __auroraInvoke?: (c: string, a?: unknown) => Promise<unknown>;
    };
    const state = (await w.__auroraInvoke!('player.state')) as { durationSecs: number };
    return state.durationSecs;
  });
  expect(duration).toBeGreaterThan(600);
  await seekTo(page, duration - 30);
  await expect(page.getByRole('button', { name: 'Skip Credits' })).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/skip-credits-from-convention.png` });
});

test('Up Next appears on an episode nobody has taught anything', async ({ page }) => {
  await playEpisodeWithoutChapters(page);
  const duration = await page.evaluate(async () => {
    const w = window as unknown as {
      __auroraInvoke?: (c: string, a?: unknown) => Promise<unknown>;
    };
    const state = (await w.__auroraInvoke!('player.state')) as { durationSecs: number };
    return state.durationSecs;
  });
  await seekTo(page, duration - 30);
  await expect(page.getByText(/Up next/i).first()).toBeVisible();
});

/**
 * The case an ordinary library is actually in: a stream that never says how long it is.
 *
 * mpv reads a duration out of the container, and a provider's VOD stream frequently
 * gives it nothing to read — sometimes until the whole file is buffered, sometimes never.
 * Both the host and `upNextVisible` were taught to cope, by falling back to the episode's
 * stored runtime, and it made no difference: `useEpisodeAids` would not *ask* until the
 * player reported a duration, so `aids` stayed null for the entire episode and Skip
 * Credits, Skip Intro and autoplay were dead together.
 *
 * Every other test here plays a fixture with a known runtime, which is why none of them
 * saw it.
 */
test('the aids work on a stream that never reports a duration', async ({ page }) => {
  await page.goto('/#/');
  await page.evaluate(() => {
    (window as unknown as { __auroraSilentDuration: (on: boolean) => void })
      .__auroraSilentDuration(true);
  });

  await playEpisodeWithoutChapters(page);

  // The premise: the player genuinely does not know.
  const state = await page.evaluate(async () => {
    const w = window as unknown as {
      __auroraInvoke: (c: string, a?: unknown) => Promise<unknown>;
    };
    return (await w.__auroraInvoke('player.state')) as {
      durationSecs: number;
      itemId: number;
    };
  });
  expect(state.durationSecs).toBe(0);

  // The host does, from the episode's own runtime — and that is the number the card and
  // the button have to come from.
  const upNextAt = await page.evaluate(async (episodeId) => {
    const w = window as unknown as {
      __auroraInvoke: (c: string, a?: unknown) => Promise<unknown>;
    };
    const aids = (await w.__auroraInvoke('library.playbackAids', {
      profileId: 1,
      episodeId,
      durationSecs: 0,
    })) as { upNextAtSecs: number | null };
    return aids.upNextAtSecs;
  }, state.itemId);
  expect(upNextAt, 'the host should place Up Next from the stored runtime').not.toBeNull();

  // Seeking through the command surface rather than the slider, whose range comes from
  // the duration the player does not have.
  await page.evaluate(async (secs) => {
    const w = window as unknown as {
      __auroraInvoke: (c: string, a?: unknown) => Promise<unknown>;
    };
    await w.__auroraInvoke('player.seek', { positionSecs: secs });
  }, upNextAt! + 5);

  await expect(page.getByRole('button', { name: 'Skip Credits' })).toBeVisible();
  await expect(page.getByText(/Up next/i).first()).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/aids-without-a-duration.png` });
});
