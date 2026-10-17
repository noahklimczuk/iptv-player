/**
 * "Record what's on" stores the window it is actually going to record.
 *
 * It used to store the programme's own `airStart`, with a comment explaining that the
 * timeshift buffer would supply the part already shown. The buffer is never consulted:
 * `RecordRequest` carries a url, a destination, a window and two headers, and `dvr.rs`
 * fills the window with `(stop - now)` — a fresh HTTP stream from the moment recording
 * begins. So pressing Record fifty minutes into an hour-long programme produced a
 * ten-minute file filed against a one-hour window, drawn as `20:00–21:00` beside an
 * honest ten-minute duration, with nothing to explain the gap (R-109).
 *
 * The clock is pinned here, because the whole assertion is about which of two instants
 * ends up on the row.
 */
import { expect, test, type Page } from '@playwright/test';

/** 20:40 local on a fixed day — mid-programme, whatever the fixtures line up. */
const AT = new Date('2026-03-14T20:40:00');

function hhmm(d: Date): string {
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

async function watchFirstChannel(page: Page) {
  await page.goto('/#/live');
  await expect(page.getByRole('heading', { name: 'Live TV' })).toBeVisible();
  await page.getByTestId('channel-row').first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();
  await page.mouse.move(640, 360);
}

test('a recording starts from now, and says so', async ({ page }) => {
  await page.clock.install({ time: AT });
  await watchFirstChannel(page);

  // What is on, and when it started, straight from the host — so this test does not
  // have to know the fixture's schedule.
  const programme = await page.evaluate(async () => {
    const inv = (window as unknown as {
      __auroraInvoke: (c: string, a: unknown) => Promise<unknown>;
    }).__auroraInvoke;
    const state = (await inv('player.state', {})) as { channelId: number | null };
    if (state.channelId == null) return null;
    const guide = (await inv('epg.nowNext', { channelId: state.channelId })) as {
      now: { title: string; start: number; stop: number } | null;
    };
    return guide.now;
  });
  expect(programme, 'the channel has no guide data, so there is no window to compare')
    .not.toBeNull();
  const started = new Date(programme!.start * 1000);
  expect(
    AT.getTime() / 1000 - programme!.start,
    'the programme began at the moment the test presses Record, which proves nothing',
  ).toBeGreaterThan(60);

  await page.getByRole('button', { name: "Record what's on" }).click();

  // The notice says how much is going to disk rather than naming the programme as
  // though all of it were.
  const notice = page.getByText(/^Recording /).first();
  await expect(notice).toBeVisible();
  const text = await notice.innerText();
  expect(text).toContain(programme!.title);
  const remaining = Math.max(1, Math.round((programme!.stop - AT.getTime() / 1000) / 60));
  expect(text).toBe(`Recording the last ${remaining} minutes of ${programme!.title}`);

  // And the scheduled row's window starts now, not when the programme did.
  await page.keyboard.press('Escape');
  await page.getByRole('navigation', { name: 'Main' })
    .getByRole('link', { name: 'Recordings' }).click();
  await expect(page.getByRole('heading', { name: 'Recordings' })).toBeVisible();
  await page.getByRole('tab', { name: 'Scheduled' }).click();

  const row = page
    .getByTestId('scheduled-row')
    .filter({ hasText: programme!.title })
    .first();
  await expect(row).toBeVisible({ timeout: 10_000 });
  const window = await row.innerText();
  expect(window, `the row still claims the whole programme: ${window}`)
    .toContain(hhmm(AT));
  expect(window, `the row starts at the programme's start, not at the recording's`)
    .not.toContain(`${hhmm(started)}–`);
});
