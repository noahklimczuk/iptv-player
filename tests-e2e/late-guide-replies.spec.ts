/**
 * A guide answer that arrives after the screen has moved on.
 *
 * `useNowNext` and `useGuideSlice` both batch: they ask about the rows the virtualiser
 * has mounted, and they remember which ids they have already asked about so a channel
 * with no guide is not re-requested on every scroll past it. The ids go into that set
 * when the request goes *out*, which is the only place it can go — the reply says
 * nothing about the channels it found nothing for.
 *
 * Both then discarded the reply if the visible rows had changed while it was in flight.
 * Those two halves together lose rows permanently: the ids are spent, the answer is
 * thrown away, and nothing asks twice. Scrolling on while a request is in flight is the
 * ordinary way to use a list, and is precisely what the 120ms settle produces — so on a
 * real panel, where `epg.nowNextMany` is two queries per channel under the single writer
 * mutex, the rows a fast scroll had passed sat on "No guide data" for the life of the
 * page.
 *
 * Needs `?slow=`: a late but *valid* answer is a third thing, which neither `?hang=`
 * (never answers) nor `__auroraFailNext` (answers with a refusal) can express.
 *
 * The interrupt is sequenced on the recorded calls rather than on the clock. `?slow`
 * records the call before it waits, so a call appearing means that request is in
 * flight — which is what makes "scroll while it is in flight" a fact here instead of a
 * race against however warm the browser's cache is.
 *
 * `run=` is not read by anything; it is there to make the second `goto` a different URL
 * from the first. Navigating to the URL already loaded is a same-document navigation —
 * the scripts do not run again — so without it the interrupted pass inherited the
 * control pass's filled rows and could not have failed whatever the hooks did.
 */
import { expect, test, type Page } from '@playwright/test';

/** Long enough that the scrolling below cannot lose the race to it. */
const LATE_MS = 2500;

/** How many of the top rows to judge. The virtualiser mounts more than this. */
const JUDGED = 6;

interface MockCall { name: string; args: { channelIds?: number[] } }

/** How many requests for `name` have gone out so far. */
async function sent(page: Page, name: string): Promise<number> {
  return (
    await page.evaluate(
      () => (window as unknown as { __auroraCalls: () => MockCall[] }).__auroraCalls(),
    )
  ).filter((c) => c.name === name).length;
}

/** How many of the first `JUDGED` channel rows show a programme rather than nothing. */
async function liveRowsWithGuide(page: Page): Promise<number> {
  const rows = page.getByTestId('channel-row');
  let n = 0;
  for (let i = 0; i < JUDGED; i += 1) {
    if (!(await rows.nth(i).innerText()).includes('No guide data')) n += 1;
  }
  return n;
}

test('a late now/next still reaches the rows that asked for it', async ({ page }) => {
  // The control: same screen, same fixtures, nothing interrupting the reply. Measured
  // rather than assumed, because how many fixture channels carry a guide at this moment
  // is not this test's business — only that interrupting must not cost any of them.
  await page.goto(`/?slow=epg.nowNextMany:${LATE_MS}&run=control#/live`);
  await expect(page.getByTestId('channel-row').first()).toBeVisible();
  await expect
    .poll(async () => liveRowsWithGuide(page), { timeout: 20_000 })
    .toBeGreaterThan(0);
  const undisturbed = await liveRowsWithGuide(page);
  expect(undisturbed, 'no fixture channel on screen has a guide to lose').toBeGreaterThan(0);

  // Again, this time scrolling while the top rows' answer is still in flight.
  await page.goto(`/?slow=epg.nowNextMany:${LATE_MS}&run=interrupted#/live`);
  await expect(page.getByTestId('channel-row').first()).toBeVisible();
  const scroller = page.getByTestId('channel-scroller');

  // The request covering the top rows is out, and is now waiting on a host that will
  // not answer for `LATE_MS`.
  await expect.poll(async () => sent(page, 'epg.nowNextMany')).toBeGreaterThan(0);

  // Away from those rows, far enough that the mounted set changes — which re-runs the
  // effect, and with it the cleanup that used to disown the reply still in flight.
  await scroller.evaluate((el) => { el.scrollTop = el.scrollHeight; });
  await expect.poll(async () => sent(page, 'epg.nowNextMany')).toBeGreaterThan(1);

  // And back. The top ids are spent by now, so nothing asks about them a second time
  // and the reply that was discarded was the only answer they were ever going to get.
  await scroller.evaluate((el) => { el.scrollTop = 0; });

  await expect
    .poll(async () => liveRowsWithGuide(page), { timeout: 20_000 })
    .toBe(undisturbed);
});

test('a late guide slice still reaches the rows that asked for it', async ({ page }) => {
  await page.goto(`/?slow=epg.gridSlice:${LATE_MS}&run=control#/guide`);
  await expect(page.getByTestId('guide-row').first()).toBeVisible();
  await expect
    .poll(async () => page.getByTestId('programme').count(), { timeout: 20_000 })
    .toBeGreaterThan(0);
  const undisturbed = await page.getByTestId('programme').count();
  expect(undisturbed, 'no programme is drawn even undisturbed').toBeGreaterThan(0);

  await page.goto(`/?slow=epg.gridSlice:${LATE_MS}&run=interrupted#/guide`);
  await expect(page.getByTestId('guide-row').first()).toBeVisible();
  await expect.poll(async () => sent(page, 'epg.gridSlice')).toBeGreaterThan(0);

  // The grid scrolls both ways in one container, so the wheel moves the rows rather
  // than the clock: changing `from` is the one thing that *should* discard a reply
  // (R-91), and this test is about the thing that should not.
  await page.getByTestId('guide-row').first().hover();
  await page.mouse.wheel(0, 2000);
  await expect.poll(async () => sent(page, 'epg.gridSlice')).toBeGreaterThan(1);
  await page.mouse.wheel(0, -2000);

  await expect
    .poll(async () => page.getByTestId('programme').count(), { timeout: 20_000 })
    .toBe(undisturbed);
});
