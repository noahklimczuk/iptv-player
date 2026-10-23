/**
 * Nothing may subscribe to the whole UI store.
 *
 * `useUi()` with no selector subscribes to every field in it, and the host's player
 * heartbeat writes `setPlayer` four times a second (`PLAYER_TICK` is 250 ms). So any
 * component or hook that takes the store whole re-renders at 4 Hz for as long as
 * anything is playing, whether or not it reads a single thing the player touches.
 *
 * This is pinned as a property of the source rather than as a render count, for the same
 * reason `journeys.test.ts` reads the specs as text: the mistake is a shape in the code,
 * one character wide, and it has now been made three times. R-134 converted `App` to six
 * named selectors and left `useZapper` and `SettingsPage` whole-store — and `useZapper`
 * is called *by* `App`, so the shell went on re-rendering at the heartbeat rate anyway
 * and the careful fix above it bought nothing (R-90). A render-count test would need
 * jsdom and a React test renderer, which `vitest.config.ts` argues against for exactly
 * this kind of check, and would still only cover the components somebody remembered to
 * render.
 *
 * `useUi.getState()` is deliberately fine: it reads without subscribing, which is what
 * the hotkey handlers in `App` use so the memo does not have to be rebuilt on a tick.
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath, URL } from 'node:url';
import { describe, expect, it } from 'vitest';

const SRC = fileURLToPath(new URL('..', import.meta.url));

/** Every `.ts`/`.tsx` under `src-ui/src`, tests excluded. */
function sources(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      sources(path, out);
    } else if (/\.tsx?$/.test(name) && !name.includes('.test.')) {
      out.push(path);
    }
  }
  return out;
}

/**
 * Comments blanked, line structure kept.
 *
 * Three of the doc comments in this repository quote `useUi()` while explaining why not
 * to write it — including the one directly above the fixed call site in `App` — so a
 * scanner that did not do this would report the explanations as the defect.
 */
function withoutComments(source: string): string {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
    .replace(/\/\/[^\n]*/g, (m) => m.replace(/[^\n]/g, ' '));
}

describe('UI store subscriptions', () => {
  it('always names the fields it wants', () => {
    const offenders: string[] = [];
    for (const path of sources(SRC)) {
      const lines = withoutComments(readFileSync(path, 'utf8')).split('\n');
      lines.forEach((line, i) => {
        // `useUi()` or `useUi( )` — a call with no selector argument.
        if (/\buseUi\(\s*\)/.test(line)) {
          offenders.push(`${relative(SRC, path)}:${i + 1}`);
        }
      });
    }
    expect(offenders, 'subscribe to named fields: useUi((s) => s.x)').toEqual([]);
  });

  /**
   * The selector form is in use, so the test above cannot be passing because nobody
   * touches the store. Guards the case where somebody "fixes" a failure by deleting the
   * subscription instead of narrowing it (R-159: a test must say what is still there).
   */
  it('is not passing because nothing subscribes at all', () => {
    const selectors = sources(SRC)
      .flatMap((path) => withoutComments(readFileSync(path, 'utf8')).split('\n'))
      .filter((line) => /\buseUi\(\s*\(/.test(line)).length;
    expect(selectors).toBeGreaterThan(10);
  });
});
