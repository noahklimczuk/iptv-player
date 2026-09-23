/**
 * The shape commands actually take when they cross the bridge.
 *
 * This is the one thing the mock transport cannot check, and the omission cost a
 * working first run: every host command takes its arguments as a single
 * `args: SomeArgs` parameter, Tauri keys the payload by that parameter's *name*, and
 * the transport was spreading the fields instead. Sixty-one commands failed to
 * deserialize on Windows — `providers.detect` among them, which is why a pasted
 * playlist URL left "Check connection" greyed out with no explanation.
 *
 * So this runs the real bundle against a stub host and asserts on the payloads,
 * rather than on anything the UI does with them.
 */
import { expect, test } from '@playwright/test';

interface Call { cmd: string; payload: Record<string, unknown> }

test('arguments cross the bridge nested under `args`, never spread', async ({ page }) => {
  const calls: Call[] = [];
  await page.exposeFunction('__recordInvoke', (cmd: string, payload: Record<string, unknown>) => {
    calls.push({ cmd, payload });
  });

  // Installed before the bundle runs, so `isNativeHost()` is true and the real Tauri
  // branch of the transport is the one under test.
  await page.addInitScript(() => {
    (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
      invoke: (cmd: string, payload: Record<string, unknown>) => {
        (window as unknown as { __recordInvoke: (c: string, p: unknown) => void })
          .__recordInvoke(cmd, payload);
        // Enough of an answer to get past the profile picker and into the wizard,
        // which is where the commands that carry arguments live. Everything else can
        // fail: this test is about what went out, not what came back.
        if (cmd === 'profiles_list') {
          return Promise.resolve([{
            id: 1, name: 'Me', avatar: 'default', isKids: false, hasPin: false,
            maxAge: null, allowUnrated: true, dailyLimitMin: null,
          }]);
        }
        if (cmd === 'providers_list') return Promise.resolve([]);
        if (cmd === 'channels_list') return Promise.resolve([]);
        return Promise.reject(new Error('stub host'));
      },
    };
  });

  // A fresh profile opens the first-run wizard, which is fine — the boot commands
  // fire either way and those are what is being inspected.
  await page.goto('/');
  await expect(page.getByLabel('Provider address')).toBeVisible();
  await page.waitForTimeout(400);

  expect(calls.length, 'no commands were invoked at all').toBeGreaterThan(0);

  // Nothing may be spread: the only key a payload can carry is `args`.
  for (const { cmd, payload } of calls) {
    const keys = Object.keys(payload);
    expect(
      keys.filter((k) => k !== 'args'),
      `${cmd} sent ${JSON.stringify(payload)} — anything outside \`args\` is unreadable to the host`,
    ).toEqual([]);
  }

  // And at least one of them must actually carry arguments, or the assertion above
  // passed only because every command on this page happens to take none.
  const withArgs = calls.filter((c) => c.payload.args !== undefined);
  expect(withArgs.length, `no command carried arguments; saw ${calls.map((c) => c.cmd).join(', ')}`)
    .toBeGreaterThan(0);
});

test('a pasted provider address reaches the host as args.text', async ({ page }) => {
  // The exact call the greyed-out "Check connection" button turned on: the wizard
  // fills its draft from what `providers.detect` returns, so a payload the host
  // cannot read leaves the URL unset and the button disabled, with nothing on screen
  // to say why.
  const calls: Call[] = [];
  await page.exposeFunction('__recordInvoke', (cmd: string, payload: Record<string, unknown>) => {
    calls.push({ cmd, payload });
  });
  await page.addInitScript(() => {
    (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
      invoke: (cmd: string, payload: Record<string, unknown>) => {
        (window as unknown as { __recordInvoke: (c: string, p: unknown) => void })
          .__recordInvoke(cmd, payload);
        if (cmd === 'providers_detect') {
          return Promise.resolve({ kind: 'xtream', url: '', username: null, password: null });
        }
        if (cmd === 'profiles_list') {
          return Promise.resolve([{
            id: 1, name: 'Me', avatar: 'default', isKids: false, hasPin: false,
            maxAge: null, allowUnrated: true, dailyLimitMin: null,
          }]);
        }
        if (cmd === 'providers_list') return Promise.resolve([]);
        if (cmd === 'channels_list') return Promise.resolve([]);
        return Promise.reject(new Error('stub host'));
      },
    };
  });

  await page.goto('/');
  const address = page.getByLabel('Provider address');
  await expect(address).toBeVisible();
  await address.fill('http://panel.example.com');
  await page.waitForTimeout(300);

  const detect = calls.find((c) => c.cmd === 'providers_detect');
  expect(detect, 'the wizard never asked the host to detect anything').toBeTruthy();
  expect(detect!.payload).toEqual({ args: { text: 'http://panel.example.com' } });
});
