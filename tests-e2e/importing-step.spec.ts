/**
 * The import screen's two claims, both of which were untrue.
 *
 * "You can keep using Aurora while this runs", said by an opaque full-viewport modal
 * whose only control was "Skip for now" — the wording for abandoning setup, offered at
 * the one moment setup can no longer be abandoned, because a provider has been saved and
 * a refresh is already running. So for the duration of the import there was nothing to
 * do but watch (R-122).
 *
 * And `providers.detect` ran from the textarea's `onChange`, so typing an address by
 * hand was one IPC round trip per character — on the first field of the first screen a
 * new viewer sees, reaching the paste parser and its percent-decoder each time.
 */
import { expect, test } from '@playwright/test';

/** `?setup` forces the wizard, as the other wizard journeys do. */
const WIZARD = '/?setup#/';

test('the import screen offers a way out, and the sentence is true', async ({ page }) => {
  await page.goto(WIZARD);
  const address = page.getByLabel('Provider address');
  await expect(address).toBeVisible();

  // Hold the refresh open, so the importing step is observable at all: against the mock
  // it otherwise lands in milliseconds.
  await page.evaluate(() => {
    (window as unknown as { __auroraHang: (name: string) => void })
      .__auroraHang('providers.refresh');
  });

  await address.fill('http://panel.example.com/get.php?username=u&password=p');
  await page.getByRole('button', { name: /Check connection/i }).click();
  await expect(page.getByRole('button', { name: 'Continue' })).toBeEnabled({ timeout: 10_000 });
  await page.getByRole('button', { name: 'Continue' }).click();
  await page.getByRole('button', { name: 'Import library' }).click();

  // The import is running and has nowhere to go on its own.
  const out = page.getByRole('button', { name: 'Continue in the background' });
  await expect(out).toBeVisible();

  // "Skip for now" is no longer on offer, because by now it would describe something
  // that cannot happen.
  await expect(page.getByRole('button', { name: 'Skip for now' })).toBeHidden();

  // And it really leaves: the wizard goes and the app is there.
  await out.click();
  await expect(address).toHaveCount(0);
  await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible();
});

test('typing an address by hand is one detection, not one per character', async ({ page }) => {
  await page.goto(WIZARD);
  const address = page.getByLabel('Provider address');
  await expect(address).toBeVisible();

  await page.evaluate(() => {
    (window as unknown as { __auroraClearCalls: () => void }).__auroraClearCalls();
  });

  // Typed, not filled: `fill` is one change event and would pass whatever the code did.
  const typed = 'http://panel.example.com';
  await address.pressSequentially(typed, { delay: 15 });

  // `__auroraCalls` is a function returning the last 200 calls, each `{ name, args }`.
  const detections = async () =>
    page.evaluate(() => (window as unknown as { __auroraCalls: () => { name: string }[] })
      .__auroraCalls().filter((c) => c.name === 'providers.detect').length);

  // The answer still arrives.
  await expect.poll(detections, { message: 'nothing was ever detected' })
    .toBeGreaterThan(0);

  // But not once per keystroke. A couple is fine — the debounce can elapse mid-word on a
  // slow machine; twenty-four is the defect.
  const count = await detections();
  expect(count, `${typed.length} characters produced ${count} detections`)
    .toBeLessThan(typed.length / 2);

  // And the field itself still shows every character, which is the thing the delay must
  // not cost.
  await expect(address).toHaveValue(typed);
});
