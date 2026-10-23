/**
 * Every "Settings → X" the app says out loud names a section that is really there.
 *
 * The headings are addressable regions precisely so things can be found — `Section`'s own
 * doc comment says so — and three of the four pointers in the app named something else:
 * "Metadata and recommendations" (two headings merged into a name belonging to neither),
 * "Assistant" (no such section), and "Filters" when the heading reads "Filtering". Two of
 * those were the worst possible copies to be wrong. `AssistantPage`'s is the empty state
 * shown *when the assistant will not work*, so it is read by exactly the people hunting
 * for the right screen; `PRIVACY.md`'s was the instruction for turning off what gets sent
 * to Google (R-153).
 *
 * A property of the source, like `journeys.test.ts` and `command_args.rs`: the text and
 * the headings are both in the tree, so nothing has to be rendered to know they disagree.
 *
 * Scoped to `src-ui/src` on purpose. The docs also say "Settings → API → Create" and
 * "Settings → Secrets and variables", which are GitHub's settings and a provider panel's
 * — not this app's, and not ours to check.
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath, URL } from 'node:url';
import { describe, expect, it } from 'vitest';

const SRC = fileURLToPath(new URL('../..', import.meta.url));
const SETTINGS = join(SRC, 'features', 'settings', 'SettingsPage.tsx');
const POINTER = `Settings → `;

/** Blanked comments, lines preserved. Also in `state/subscriptions.test.ts`. */
function withoutComments(source: string): string {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
    .replace(/\/\/[^\n]*/g, (m) => m.replace(/[^\n]/g, ' '));
}

/** CRLF on Windows, LF in CI. Also in `state/subscriptions.test.ts`, which says why. */
function read(path: string): string {
  return readFileSync(path, 'utf8').replace(/\r\n/g, '\n');
}

function sources(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) sources(path, out);
    else if (/\.tsx?$/.test(name) && !name.includes('.test.')) out.push(path);
  }
  return out;
}

/** The headings `SettingsPage` renders, in the order it renders them. */
function sections(): string[] {
  const source = read(SETTINGS);
  return [...source.matchAll(/<Section title="([^"]+)"/g)].map((m) => m[1]!);
}

describe('Settings pointers', () => {
  it('finds the sections, so the check below has something to check against', () => {
    const titles = sections();
    expect(titles).toContain('Appearance');
    expect(titles).toContain('Recommendations');
    expect(titles).toContain('Filtering');
    expect(titles.length).toBeGreaterThan(8);
  });

  it('each names a heading that exists', () => {
    const titles = sections();
    const wrong: string[] = [];
    for (const path of sources(SRC)) {
      const lines = withoutComments(read(path)).split('\n');
      lines.forEach((line, i) => {
        let at = line.indexOf(POINTER);
        while (at !== -1) {
          // What follows the arrow, as the viewer reads it — the heading plus whatever
          // sentence it is embedded in, so a real heading has to be the start of it.
          const rest = line.slice(at + POINTER.length);
          if (!titles.some((t) => rest.startsWith(t))) {
            wrong.push(`${relative(SRC, path)}:${i + 1}: "${rest.slice(0, 40)}"`);
          }
          at = line.indexOf(POINTER, at + 1);
        }
      });
    }
    expect(wrong, `sections are: ${sections().join(' · ')}`).toEqual([]);
  });
});
