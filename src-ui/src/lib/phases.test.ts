/**
 * Every import phase the contract declares has a label, and nothing else does.
 *
 * `RefreshButton` carried its own map annotated `Record<string, string>` — which accepts
 * any key and requires none, so `tsc --strict` passed over two keys that are not phases
 * (`downloading`, `importingEpg`) and four that were missing (`authenticating`,
 * `fetchingPlaylist`, `fetchingEpg`, `matchingEpg`). The render fell through to the raw
 * value, so Settings → Refresh showed camelCase identifiers for four of the nine phases,
 * including the two longest ones on a real subscription (R-145).
 *
 * `Record<IngestPhase, string>` is what stops that, and both maps have it now — so this
 * file is really checking the other direction: that the union in `shared/ipc.ts` is the
 * list these maps were written against, read from the contract as text rather than
 * trusted. A phase added there without a label here is a compile error; a phase *removed*
 * there leaves a label behind that nothing would notice, and this is what notices.
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath, URL } from 'node:url';
import { describe, expect, it } from 'vitest';
import { PHASE_SENTENCE, PHASE_SHORT } from './phases';

/** The `IngestPhase` union's members, straight out of the contract. */
function declaredPhases(): string[] {
  const contract = readFileSync(
    fileURLToPath(new URL('../../../shared/ipc.ts', import.meta.url)),
    'utf8',
  );
  const start = contract.indexOf('export type IngestPhase =');
  expect(start, 'the contract no longer declares IngestPhase').toBeGreaterThan(-1);
  const decl = contract.slice(start, contract.indexOf(';', start));
  return [...decl.matchAll(/'([a-zA-Z]+)'/g)].map((m) => m[1]!);
}

describe('the import phase labels', () => {
  const phases = declaredPhases();

  it('reads nine phases out of the contract', () => {
    // A sanity check on the parse itself: if this ever reads zero, the two assertions
    // below would pass by covering nothing.
    expect(phases.length).toBe(9);
    expect(phases).toContain('matchingEpg');
  });

  for (const [name, map] of [
    ['sentences', PHASE_SENTENCE],
    ['short labels', PHASE_SHORT],
  ] as const) {
    it(`has ${name} for exactly the declared phases`, () => {
      expect(Object.keys(map).sort()).toEqual([...phases].sort());
      // And nothing is labelled with its own identifier, which is what the viewer saw.
      for (const [phase, label] of Object.entries(map)) {
        expect(label, `${phase} is labelled with its own name`).not.toBe(phase);
        expect(label, `${phase} has an empty label`).not.toBe('');
      }
    });
  }
});
