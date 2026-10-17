/**
 * No journey may assert only that something is absent.
 *
 * `toHaveCount(0)` cannot tell "this was correctly suppressed" from "nothing happened at
 * all", and for a long time one of these was the second: `trailers.spec.ts` asserted that
 * turning animations off stopped trailers starting, and it passed because with animations
 * off the card **did not expand at all** — no Play, no My List, no thumbs-up, for every
 * viewer with reduced motion enabled in Windows. The test read as proof about trailers
 * while the feature under it was dead (R-115, found by hand; R-159 is the pattern).
 *
 * So this is the pattern made checkable: a test that says something is gone must also say
 * what is still there. It reads the specs as text — the same approach `contract.rs` takes
 * to `shared/ipc.ts`, and for the same reason: the thing worth pinning is a property of
 * the source, and parsing it is cheaper than running it.
 */
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath, URL } from 'node:url';
import { describe, expect, it } from 'vitest';

const DIR = fileURLToPath(new URL('../../../tests-e2e', import.meta.url));

/** Assertions that say something is *not* there. */
const NEGATIVE = [
  /\.toHaveCount\(0\)/,
  /\.toBeHidden\(\)/,
  /\.not\.to[A-Z]/,
  /\.toBeNull\(\)/,
  /\.toEqual\(\[\]\)/,
  /\.toBe\(null\)/,
];

/** Assertions that say something *is*. */
const POSITIVE = [
  /\.toBeVisible\(\)/,
  /\.toBeFocused\(\)/,
  /\.toBeDisabled\(\)/,
  /\.toBeEnabled\(\)/,
  /\.toBeChecked\(\)/,
  /\.toHaveText\(/,
  /\.toContainText\(/,
  /\.toHaveValue\(/,
  /\.toHaveAttribute\(/,
  /\.toHaveJSProperty\(/,
  /\.toHaveURL\(/,
  /\.toMatch\(/,
  /\.toBeGreaterThan/,
  /\.toBeLessThan/,
  /\.toBeTruthy\(\)/,
  // `toHaveCount(n)` and `toBe(x)` for anything but zero and null.
  /\.toHaveCount\((?!0\))/,
  /\.toBe\((?!null\))/,
  /\.toEqual\((?!\[\]\))/,
  /\.toMatchObject\(/,
  /\.toContain\(/,
  /\.toContainEqual\(/,
  /\.toHaveLength\((?!0\))/,
  /\.toBeDefined\(\)/,
];

/** Every `test('…', …)` body in a spec file, by name. */
function testsIn(source: string): { name: string; body: string }[] {
  const out: { name: string; body: string }[] = [];
  const opener = /\btest\(\s*(['"`])((?:\\.|(?!\1).)*)\1/g;
  let match: RegExpExecArray | null;
  while ((match = opener.exec(source)) !== null) {
    // From the opening paren of `test(` to its matching close.
    let depth = 0;
    let i = source.indexOf('(', match.index);
    const start = i;
    for (; i < source.length; i += 1) {
      if (source[i] === '(') depth += 1;
      else if (source[i] === ')') {
        depth -= 1;
        if (depth === 0) break;
      }
    }
    out.push({ name: match[2]!, body: source.slice(start, i) });
  }
  return out;
}

const hits = (body: string, patterns: RegExp[]) =>
  patterns.filter((p) => p.test(body)).length;

describe('the end-to-end journeys', () => {
  const files = readdirSync(DIR).filter((f) => f.endsWith('.spec.ts'));

  it('has specs to read', () => {
    // If this ever reads zero the two checks below would pass by covering nothing.
    expect(files.length).toBeGreaterThan(20);
  });

  it('never asserts only that something is absent', () => {
    const blind: string[] = [];
    for (const file of files) {
      const source = readFileSync(`${DIR}/${file}`, 'utf8');
      for (const { name, body } of testsIn(source)) {
        const negative = hits(body, NEGATIVE);
        const positive = hits(body, POSITIVE);
        if (negative > 0 && positive === 0) blind.push(`${file} › ${name}`);
      }
    }
    expect(
      blind,
      'these say something is gone and never say what is still there, so they would '
        + 'pass against a blank screen:\n  ' + blind.join('\n  '),
    ).toEqual([]);
  });

  it('asserts something in every test', () => {
    const silent: string[] = [];
    for (const file of files) {
      const source = readFileSync(`${DIR}/${file}`, 'utf8');
      for (const { name, body } of testsIn(source)) {
        // `expect(` or `expect` on its own line before `.poll(` — the latter is how a
        // journey waits for a value to settle, and matching only the first form
        // called it silent.
        if (!/\bexpect\s*[(.]/.test(body)) silent.push(`${file} › ${name}`);
      }
    }
    expect(silent, `these assert nothing at all:\n  ${silent.join('\n  ')}`).toEqual([]);
  });
});
