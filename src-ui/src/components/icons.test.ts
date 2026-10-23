/**
 * `IconName` stays a closed union.
 *
 * It was `keyof typeof ICON_PATHS | string`, which TypeScript widens to `string` — so the
 * `keyof` contributed nothing and every `<Icon name="…" />` and `icon:` prop in the app
 * was unchecked. `<Icon name="plya" />` compiled, on a type whose name and shape both
 * promised otherwise (R-146).
 *
 * Pinned as a property of the source, because the thing that went wrong is a type that
 * silently means something else: `tsc` cannot fail on a union that has quietly become
 * `string`, which is exactly why this went unnoticed. The same approach
 * `state/subscriptions.test.ts` and `features/settings/pointers.test.ts` take.
 *
 * Re-adding `| string` would be the only reason to allow a computed name, and the two
 * computed sites in the app are ternaries over literals — `playing ? 'pause' : 'play'`
 * and `muted ? 'volumeOff' : 'volume'` — which a closed union accepts.
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath, URL } from 'node:url';
import { describe, expect, it } from 'vitest';

import { ICON_PATHS } from './Icon';

const SOURCE = readFileSync(fileURLToPath(new URL('./Icon.tsx', import.meta.url)), 'utf8');

describe('IconName', () => {
  it('is the keys of the table and nothing wider', () => {
    const declaration = SOURCE.split('\n').find((l) => l.includes('export type IconName'));
    expect(declaration, 'the type has been renamed or removed').toBeTruthy();
    expect(declaration).toBe('export type IconName = keyof typeof ICON_PATHS;');
    // Stated separately so the failure says what is wrong rather than printing a diff:
    // `| string` is the specific mistake, and it is easy to re-add while "fixing" a
    // computed name somewhere else.
    expect(declaration, 'a union with `string` in it *is* `string`').not.toMatch(/\|\s*string/);
  });

  it('has the icons it is meant to have, so the check above is not vacuous', () => {
    const names = Object.keys(ICON_PATHS);
    expect(names.length).toBeGreaterThan(30);
    // A few the app would be visibly broken without, one per area that draws them.
    for (const required of ['play', 'pause', 'info', 'search', 'tv', 'film', 'person']) {
      expect(names, `${required} is missing`).toContain(required);
    }
    // Every path is a path, not an empty string left behind by an edit.
    for (const [name, d] of Object.entries(ICON_PATHS)) {
      expect(d.length, `${name} has no path`).toBeGreaterThan(5);
      expect(d, `${name} does not start with a move`).toMatch(/^M/);
    }
  });
});
