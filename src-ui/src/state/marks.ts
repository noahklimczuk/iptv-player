/**
 * What the viewer has marked: My List, and liked.
 *
 * Both buttons existed on three screens and did nothing. `<IconButton icon="plus"
 * label="Add to My List" onClick={(e) => e.stopPropagation()} />` — the whole handler was
 * stopping the click from reaching the card behind it. The host had `mylist.toggle`
 * implemented, registered and unit-tested the entire time; the thumbs-up had nothing
 * behind it at all, and the recommender's favourite boost was reading a flag for films
 * that nothing could set.
 *
 * Held here rather than fetched per card because a browse grid draws 120 posters and each
 * needs to know whether its plus should be a tick. One request per profile, answered from
 * memory afterwards, and both sets are small — tens of titles, not the library.
 *
 * Toggles are optimistic. The alternative is a button that does nothing for the length of
 * a round trip, which is what the whole of this file exists to stop.
 */
import { create } from 'zustand';
import type { CatalogItem } from '@shared/ipc';
import { invoke } from '@/ipc';
import { report } from '@/lib/errors';
import { useUi } from '@/state/ui';

/** `movie:12`. The same key the host sends back, and the one the rails use. */
export const markKey = (kind: 'movie' | 'series', id: number) => `${kind}:${id}`;

/** A catalog item's key, narrowing `kind` to the two a list can hold. */
export const keyOf = (item: CatalogItem) =>
  markKey(item.kind === 'series' ? 'series' : 'movie', item.id);

interface MarksState {
  myList: Set<string>;
  liked: Set<string>;
  /** The profile these belong to, so a switch reloads rather than showing the last one's. */
  profileId: number | null;
  /**
   * Whether the first load has finished, however it finished.
   *
   * The shell waits for this before it paints. Arriving afterwards meant every plus on
   * every card became a tick a moment after the grid appeared — and re-rendered a hundred
   * and twenty cards to do it, under whatever the pointer was already on.
   */
  loaded: boolean;
  load: (profileId: number) => Promise<void>;
  toggleMyList: (item: CatalogItem) => void;
  toggleLiked: (item: CatalogItem) => void;
}

/** Add or remove, without mutating the set the last render read. */
function flip(set: Set<string>, key: string): { next: Set<string>; added: boolean } {
  const next = new Set(set);
  const added = !next.has(key);
  if (added) next.add(key);
  else next.delete(key);
  return { next, added };
}

/**
 * Which `load` is the newest. Module-level, because it is about the sequence of calls
 * rather than about any one of them, and a value kept in the store would be read back
 * through the same `get()` that caused the bug it exists to fix.
 */
let requests = 0;

export const useMarks = create<MarksState>((set, get) => ({
  myList: new Set(),
  liked: new Set(),
  profileId: null,
  loaded: false,

  load: async (profileId) => {
    /*
     * The guard here used to be `get().profileId !== profileId`, and
     * `get().profileId` is the profile whose marks are *currently loaded* -- written in
     * the `finally` below, after the guard. So the request was compared against a value
     * it was itself about to write, and it was wrong both times it mattered:
     *
     *   - On the first load `profileId` is still `null`, so `null !== 1` returned early
     *     and the answer was thrown away. `App.tsx` calls `load` once per profile
     *     change, so for the chosen profile that is the only load -- every plus on
     *     every card rendered as a plus and every thumb as unset, for titles that were
     *     on the list. It came right on any second load, which is why it survived:
     *     invisible to anyone testing a flow rather than a cold start.
     *   - Switching profile returned early too, and the `finally` then moved
     *     `profileId` to the new person while leaving the old person's `myList` and
     *     `liked` on screen -- exactly what the comment said the guard prevented.
     *
     * A ticket instead, which is what `usePages` and `Playback::generation` already do
     * for this race (R-86).
     */
    const ticket = ++requests;
    // And the previous profile's ticks go now rather than when the answer arrives, so
    // there is no window in which one person's list is drawn on another's screen.
    if (get().profileId !== profileId) {
      set({ profileId, myList: new Set(), liked: new Set() });
    }
    try {
      const marks = await invoke('lists.marks', { profileId });
      // A late answer for a profile that is no longer the selected one is dropped.
      if (ticket !== requests) return;
      set({ myList: new Set(marks.myList), liked: new Set(marks.liked) });
    } catch (e) {
      // The buttons stay usable: a toggle writes to the host regardless, and the worst
      // case is a tick that starts in the wrong state until the next load.
      report('Could not read your lists')(e);
    } finally {
      // However it finished -- the shell waits on `loaded` before it paints, so this
      // must happen on failure too. Only for the newest request: an older one landing
      // late must not announce itself as the current state.
      if (ticket === requests) set({ profileId, loaded: true });
    }
  },

  toggleMyList: (item) => {
    const key = keyOf(item);
    const { next, added } = flip(get().myList, key);
    set({ myList: next });
    invoke('mylist.toggle', {
      profileId: get().profileId ?? 1,
      kind: item.kind === 'series' ? 'series' : 'movie',
      id: item.id,
    })
      .then((onList) => {
        // The host decides. Two fast clicks, or a row that was already there, can both
        // leave the optimistic guess one behind the truth.
        set((s) => {
          const fixed = new Set(s.myList);
          if (onList) fixed.add(key);
          else fixed.delete(key);
          return { myList: fixed };
        });
        // Home has a My List rail built by the host, and adding to a list you are looking
        // at and watching nothing happen is the complaint this whole change answers.
        // Liking does not do this: it changes what gets recommended rather than what is
        // on a shelf, and rebuilding nine rails under somebody's cursor to show no visible
        // difference is a worse trade.
        useUi.getState().bumpCatalog();
      })
      .catch((e: unknown) => {
        set((s) => {
          const back = new Set(s.myList);
          if (added) back.delete(key);
          else back.add(key);
          return { myList: back };
        });
        report(
          added
            ? `Could not add ${item.title} to My List`
            : `Could not remove ${item.title} from My List`,
        )(e);
      });
  },

  toggleLiked: (item) => {
    const key = keyOf(item);
    const { next, added } = flip(get().liked, key);
    set({ liked: next });
    invoke('likes.toggle', {
      profileId: get().profileId ?? 1,
      kind: item.kind === 'series' ? 'series' : 'movie',
      id: item.id,
    })
      .then((isLiked) => {
        set((s) => {
          const fixed = new Set(s.liked);
          if (isLiked) fixed.add(key);
          else fixed.delete(key);
          return { liked: fixed };
        });
      })
      .catch((e: unknown) => {
        set((s) => {
          const back = new Set(s.liked);
          if (added) back.delete(key);
          else back.add(key);
          return { liked: back };
        });
        report(`Could not record what you think of ${item.title}`)(e);
      });
  },
}));

/** Whether this item is on My List, as a hook so a card re-renders when it changes. */
export const useOnMyList = (item: CatalogItem) =>
  useMarks((s) => s.myList.has(keyOf(item)));

export const useIsLiked = (item: CatalogItem) => useMarks((s) => s.liked.has(keyOf(item)));
