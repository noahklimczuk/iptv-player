/**
 * Loading My List, which is the half of this store with a race in it.
 *
 * `toggleMyList` and `toggleLiked` are covered by the Playwright journeys, which press
 * the buttons and read the ticks back. `load` is not: the journeys only ever see it after
 * it has run at least twice, and the bug was that the *first* call threw its answer away
 * (R-86). A cold start is what this file is for.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
vi.mock('@/ipc', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock('@/lib/errors', () => ({ report: () => () => {} }));
vi.mock('@/state/ui', () => ({ useUi: { getState: () => ({ bumpCatalog: () => {} }) } }));

const { useMarks } = await import('./marks');

const marksFor = (p: number) =>
  p === 1
    ? { myList: ['movie:10', 'movie:7'], liked: ['movie:10'] }
    : { myList: ['series:3'], liked: [] };

beforeEach(() => {
  invoke.mockReset();
  useMarks.setState({
    myList: new Set(),
    liked: new Set(),
    profileId: null,
    loaded: false,
  });
});

describe('load', () => {
  it('keeps what the first load returns', async () => {
    // The whole of the original bug: `profileId` starts null, the guard compared the
    // request against it, and `null !== 1` returned before anything was stored. The
    // app calls this once per profile change, so for the chosen profile this is the
    // only call -- every plus stayed a plus for titles that were on the list.
    invoke.mockResolvedValue(marksFor(1));

    await useMarks.getState().load(1);

    expect([...useMarks.getState().myList]).toEqual(['movie:10', 'movie:7']);
    expect([...useMarks.getState().liked]).toEqual(['movie:10']);
    expect(useMarks.getState().profileId).toBe(1);
    expect(useMarks.getState().loaded).toBe(true);
  });

  it('shows the new profile nothing rather than the old profile list', async () => {
    invoke.mockResolvedValue(marksFor(1));
    await useMarks.getState().load(1);

    // Switching, with the answer for profile 2 still in flight.
    let release: (v: unknown) => void = () => {};
    invoke.mockReturnValue(new Promise((r) => { release = r; }));
    const pending = useMarks.getState().load(2);

    expect(useMarks.getState().profileId).toBe(2);
    expect([...useMarks.getState().myList]).toEqual([]);

    release(marksFor(2));
    await pending;
    expect([...useMarks.getState().myList]).toEqual(['series:3']);
  });

  it('drops an answer for a profile that is no longer selected', async () => {
    // Profile 1's answer comes back after profile 2 has already been loaded, which is
    // the race the guard was there for and did not cover.
    let releaseOne: (v: unknown) => void = () => {};
    invoke.mockReturnValueOnce(new Promise((r) => { releaseOne = r; }));
    const first = useMarks.getState().load(1);

    invoke.mockResolvedValue(marksFor(2));
    await useMarks.getState().load(2);
    expect([...useMarks.getState().myList]).toEqual(['series:3']);

    releaseOne(marksFor(1));
    await first;

    expect([...useMarks.getState().myList]).toEqual(['series:3']);
    expect(useMarks.getState().profileId).toBe(2);
  });

  it('still lets the shell paint when the host refuses', async () => {
    // `loaded` gates the first paint, so a failure has to set it or the app never
    // appears.
    invoke.mockRejectedValue(new Error('no'));

    await useMarks.getState().load(1);

    expect(useMarks.getState().loaded).toBe(true);
    expect([...useMarks.getState().myList]).toEqual([]);
  });
});
