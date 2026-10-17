/**
 * Profiles and the PIN keypad (README §11).
 *
 * The picker is skipped when there is exactly one unlocked profile, so these tests
 * create a second one first — which is also the behaviour worth pinning down.
 */
import { expect, test, type Page } from '@playwright/test';

const SHOTS = 'screenshots';

/**
 * Add a profile through the same command surface the UI uses.
 *
 * Deliberately does not reload: the browser build keeps library state in memory for
 * the lifetime of the page, so a reload would discard the profile. Tests reach the
 * picker through the in-app switcher instead, which is what a user does anyway.
 */
async function addProfile(page: Page, name: string, opts: { kids?: boolean; pin?: string } = {}) {
  await page.evaluate(
    async ({ name, kids, pin }) => {
      const w = window as unknown as {
        __auroraInvoke?: (c: string, a: unknown) => Promise<unknown>;
      };
      const id = (await w.__auroraInvoke!('profiles.create', {
        name,
        isKids: !!kids,
        maxAge: kids ? 12 : null,
      })) as number;
      if (pin) await w.__auroraInvoke!('profiles.setPin', { profileId: id, pin });
    },
    { name, kids: opts.kids ?? false, pin: opts.pin ?? null },
  );
}

test('a single unlocked profile skips the picker entirely', async ({ page }) => {
  await page.goto('/#/');
  // Straight to the app: choosing between one option is not a choice.
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await expect(page.getByRole('heading', { name: "Who's watching?" })).toBeHidden();
});

test('the picker appears once there is more than one profile', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();

  await addProfile(page, 'Sam');
  await page.getByRole('button', { name: /Switch profile/ }).click();

  await expect(page.getByRole('heading', { name: "Who's watching?" })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Me' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Sam' })).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/22-profile-picker.png` });

  await page.getByRole('button', { name: 'Sam' }).click();
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  // The top bar shows who is active.
  await expect(page.getByRole('button', { name: /Switch profile \(currently Sam\)/ })).toBeVisible();
});

test('a kids profile is labelled in the picker', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await addProfile(page, 'Junior', { kids: true });
  await page.getByRole('button', { name: /Switch profile/ }).click();

  await expect(page.getByRole('button', { name: 'Junior' })).toContainText('KIDS');
});

test('a PIN-locked profile asks for the PIN and rejects the wrong one', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await addProfile(page, 'Locked', { pin: '1234' });
  await page.getByRole('button', { name: /Switch profile/ }).click();

  await page.getByRole('button', { name: /^Locked, PIN required/ }).click();
  await expect(page.getByRole('heading', { name: /Enter Locked's PIN/ })).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/23-pin-pad.png` });

  for (const d of ['9', '9', '9', '9']) {
    await page.getByRole('button', { name: d, exact: true }).click();
  }
  await expect(page.getByRole('alert')).toContainText(/Incorrect PIN/);
  // Still locked out of the app.
  await expect(page.getByRole('region', { name: 'Featured' })).toBeHidden();

  for (const d of ['1', '2', '3', '4']) {
    await page.getByRole('button', { name: d, exact: true }).click();
  }
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
});

test('the PIN pad can be backed out of', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await addProfile(page, 'Locked', { pin: '1234' });
  await page.getByRole('button', { name: /Switch profile/ }).click();

  await page.getByRole('button', { name: /^Locked, PIN required/ }).click();
  await page.getByRole('button', { name: 'Back' }).click();
  await expect(page.getByRole('heading', { name: "Who's watching?" })).toBeVisible();
});

test('switching profile from the top bar returns to the picker', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await addProfile(page, 'Sam');
  await page.getByRole('button', { name: /Switch profile/ }).click();
  await page.getByRole('button', { name: 'Sam' }).click();

  // The chip now names Sam, and clicking it goes back to the picker.
  await page.getByRole('button', { name: /Switch profile \(currently Sam\)/ }).click();
  await expect(page.getByRole('heading', { name: "Who's watching?" })).toBeVisible();
});

/**
 * The lockout after five wrong PINs, which left a dead pad and a frozen number.
 *
 * `isLocked` and `secondsLeft` were computed from `Date.now()` during render and nothing
 * scheduled another render. Every key is disabled while locked — 0–9 and backspace — so
 * there was nothing left to press that could cause one, and the only enabled control was
 * Back. The countdown therefore read "(60s)" for as long as the pad was open, and the
 * recovery path was to press Back and re-enter the profile, which resets the local state,
 * re-enables the keys, and has the host refuse again until the real minute is up.
 *
 * Unreachable from here until now for a second reason worth naming: the mock answered
 * `{ wrong: { remaining: 4 } }` to every wrong PIN, for ever, so no journey could get to
 * the locked-out state at all. It now counts failures and locks out on the fifth, with
 * the host's own `MAX_PIN_FAILURES` and `PIN_LOCKOUT_SECS` (R-152).
 */
test('the lockout counts down instead of freezing', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('region', { name: 'Featured' })).toBeVisible();
  await addProfile(page, 'Locked', { pin: '1234' });
  await page.getByRole('button', { name: /Switch profile/ }).click();
  await page.getByRole('button', { name: /^Locked, PIN required/ }).click();
  await expect(page.getByRole('heading', { name: /Enter Locked's PIN/ })).toBeVisible();

  // Five wrong attempts, which is what the host counts.
  for (let attempt = 0; attempt < 5; attempt += 1) {
    for (const d of ['9', '9', '9', '9']) {
      await page.getByRole('button', { name: d, exact: true }).click();
    }
    await expect(page.getByRole('alert')).toBeVisible();
  }

  await expect(page.getByRole('alert')).toContainText(/Too many attempts/);
  const countdown = page.getByTestId('pin-countdown');
  await expect(countdown).toBeVisible();

  // The keypad is unavailable, which is correct...
  await expect(page.getByRole('button', { name: '7', exact: true })).toBeDisabled();

  // ...and the number moves, which is the finding. Against the old code this stays at
  // whatever it was when the refusal arrived, because nothing re-renders.
  const first = Number((await countdown.innerText()).replace(/[^0-9]/g, ''));
  expect(first).toBeGreaterThan(50);
  await expect
    .poll(async () => Number((await countdown.innerText()).replace(/[^0-9]/g, '')), {
      timeout: 4000,
      message: 'the countdown never changed',
    })
    .toBeLessThan(first);
});
