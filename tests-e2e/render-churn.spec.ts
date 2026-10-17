/**
 * What the app does to itself while something is playing.
 *
 * `useUi()` with no selector subscribes to the *entire* store, and `App` renders the
 * whole route tree — so a write to any field re-rendered the mounted page. The rate was
 * already measured and written down thirteen lines below the subscription, on the banner
 * effect: the host's heartbeat writes `setPlayer` **four times a second**.
 *
 * Two consequences, and this measures the one that can be measured from outside. The
 * `handlers` memo listed `ui` first in its dependency array, so it was rebuilt on every
 * heartbeat, and `useHotkeys` depends on it — so the global `keydown` listener was
 * removed and re-added roughly four times a second for as long as anything played.
 * Nothing was dropped (React runs the cleanup and setup back to back), but it is pure
 * churn on the hottest path in the app (R-134).
 */
import { expect, test } from '@playwright/test';

/** Counts how many times anything registers a global `keydown` listener. */
const countListeners = () => {
  const w = window as unknown as { __keydownAdds: number };
  w.__keydownAdds = 0;
  const add = window.addEventListener.bind(window);
  window.addEventListener = function patched(
    this: Window,
    type: string,
    ...rest: unknown[]
  ) {
    if (type === 'keydown') w.__keydownAdds += 1;
    return (add as unknown as (t: string, ...r: unknown[]) => void)(type, ...rest);
  } as typeof window.addEventListener;
};

test('playing something does not re-register the key handler over and over', async ({
  page,
}) => {
  await page.addInitScript(countListeners);
  await page.goto('/#/?video');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();

  const adds = async () =>
    page.evaluate(() => (window as unknown as { __keydownAdds: number }).__keydownAdds);

  // Start something. The mock's player ticks once a second; the host's heartbeat is four
  // times a second, so anything proportional to it shows up here too.
  await page.getByRole('button', { name: 'Play' }).first().click();
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible();

  const settled = await adds();
  await page.waitForTimeout(5_000);
  const after = await adds();

  // Five seconds of playback. Against the old code this grew by one per tick; a couple
  // of re-registrations from ordinary state changes are fine, dozens are the defect.
  expect(
    after - settled,
    `the keydown listener was re-registered ${after - settled} times in five seconds`,
  ).toBeLessThanOrEqual(2);

  // And it still works, which is the half a churn test must not forget: a listener that
  // is stable because it is stale is worse than one that is rebuilt.
  //
  // Asked of the host rather than of the OSD, which hides itself after a few seconds —
  // and whose Play button has a twin on the hero behind it.
  const status = () =>
    page.evaluate(async () => {
      const w = window as unknown as {
        __auroraInvoke: (c: string, a?: unknown) => Promise<{ status: string }>;
      };
      return (await w.__auroraInvoke('player.state')).status;
    });

  await page.keyboard.press('k');
  await expect.poll(status, { message: 'k did not pause' }).toBe('paused');
  await page.keyboard.press('k');
  await expect.poll(status, { message: 'k did not resume' }).toBe('playing');
});
