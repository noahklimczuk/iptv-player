/**
 * The three buttons in the player that did nothing.
 *
 * Picture-in-picture, Record and Playback settings were all drawn, all labelled, and all
 * without a handler. Two of them had a host behind them the whole time: `dvr.schedule`
 * with its own scheduler and tick, and `player.setSpeed` / `player.setAspect`, which no
 * screen in the app had ever called.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

/** Open the player on a live channel, with the OSD up. */
async function watchLive(page: Page) {
  await page.goto('/#/live');
  await expect(page.getByTestId('channel-row').first()).toBeVisible();
  await page.getByTestId('channel-row').first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await page.mouse.move(640, 360);
}

test('playback settings opens speed and aspect, which nothing could reach before', async ({
  page,
}) => {
  // A film, so the speed control is there: live TV arrives at the rate it arrives.
  await page.goto('/#/movies');
  await expect(page.getByTestId('catalog-card').first()).toBeVisible();
  await page.getByTestId('catalog-card').first().getByRole('button').first().click();
  await page.getByRole('dialog').getByRole('button', { name: /^(Play|Resume)/ }).click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await page.mouse.move(640, 360);

  await page.getByRole('button', { name: 'Playback settings' }).click();
  const panel = page.getByTestId('playback-panel');
  await expect(panel).toBeVisible();

  // Speed goes through to the host and comes back in the player state.
  await panel.getByRole('button', { name: '1.5×' }).click();
  await expect(panel.getByRole('button', { name: '1.5×' })).toHaveAttribute(
    'aria-pressed',
    'true',
  );

  // So does the aspect ratio.
  await panel.getByRole('button', { name: '4:3' }).click();
  await expect.poll(async () =>
    panel.getByRole('button', { name: '4:3' }).getAttribute('aria-pressed')).toBe(null);
  await page.screenshot({ path: `${SHOTS}/52-playback-settings.png` });
});

test('recording what is on schedules the programme by name', async ({ page }) => {
  await watchLive(page);

  await page.getByRole('button', { name: "Record what's on" }).click();
  // The notice names the programme, which is what makes it findable in Recordings
  // afterwards rather than being a block of time.
  const notice = page.getByText(/^Recording /).first();
  await expect(notice).toBeVisible();
  // The notice says how much of the programme is going to disk when some of it has
  // already gone — "Recording the last 38 minutes of X" — because the recording starts
  // now and not at the programme's start (R-109). The title is what this test is after.
  const announced = (await notice.innerText())
    .replace(/^Recording (?:the last \d+ minutes of )?/, '')
    .trim();

  // And it is really scheduled, not just announced. Reached by navigating *within* the
  // app: the mock's library lives in the page, so a `goto` would reload it and throw away
  // the very thing being checked.
  await page.keyboard.press('Escape');
  await page.getByRole('navigation', { name: 'Main' })
    .getByRole('link', { name: 'Recordings' }).click();
  await expect(page.getByRole('heading', { name: 'Recordings' })).toBeVisible();
  await expect(page.getByRole('tab', { name: 'Scheduled' })).toBeVisible();
  // By the name it was announced under, so this cannot pass on a fixture's own rows.
  await expect(page.getByText(announced).first()).toBeVisible({ timeout: 10_000 });
});

test('a film offers no Record button, because there is nothing going past', async ({ page }) => {
  await page.goto('/#/movies');
  await expect(page.getByTestId('catalog-card').first()).toBeVisible();
  await page.getByTestId('catalog-card').first().getByRole('button').first().click();
  await page.getByRole('dialog').getByRole('button', { name: /^(Play|Resume)/ }).click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await page.mouse.move(640, 360);

  await expect(page.getByRole('button', { name: "Record what's on" })).toHaveCount(0);
  // And the button that never did anything is gone rather than inert.
  await expect(page.getByRole('button', { name: 'Picture-in-picture' })).toHaveCount(0);
});

/**
 * A remote has no pointer, so a key has to bring the OSD back.
 *
 * `bump` — which shows the controls and restarts the 3,200 ms hide timer — was wired to
 * `onMouseMove` and `onClick` and nothing else. Grepped: it appeared five times in
 * `PlayerOverlay.tsx` and nowhere else in `src-ui`, while keyboard and remote input
 * arrives at a global listener in `useHotkeys`.
 *
 * So 3.2 seconds after the last mouse movement, volume-up changed the volume with no
 * indicator, seeking moved the playhead with no scrub bar, and pause paused with no sign
 * of it. The action happened; the feedback did not. With a mouse that is an annoyance —
 * one twitch restores everything. On a television driven by a remote, which README §1
 * names as the target, the OSD is unreachable: there is no pointer to move (R-74).
 */
test('a key brings the OSD back after it has hidden itself', async ({ page }) => {
  await page.goto('/#/?video');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await page.getByRole('button', { name: 'Play' }).first().click();

  // `Pause` is unique to the OSD while something is playing — the hero behind it offers
  // `Play` and `More Info` — and the controls are unmounted when the OSD hides, not
  // merely faded, so "hidden" here means gone.
  const osd = page.getByRole('button', { name: 'Pause' });
  await expect(osd).toBeVisible();

  // Let it hide itself. The mouse must not move, which is the state a remote is always
  // in — Playwright's pointer stays where it was put.
  await expect(osd).toBeHidden({ timeout: 8_000 });

  // A key the player handles. Against the old code the volume changes and nothing on
  // screen says so.
  await page.keyboard.press('ArrowUp');
  await expect(osd, 'the OSD stayed hidden through a keypress').toBeVisible();

  // And it hides again afterwards, so this did not simply pin it open.
  await expect(osd).toBeHidden({ timeout: 8_000 });
});
