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

export const useMarks = create<MarksState>((set, get) => ({
  myList: new Set(),
  liked: new Set(),
  profileId: null,
  loaded: false,

  load: async (profileId) => {
    try {
      const marks = await invoke('lists.marks', { profileId });
      // Still the profile that was asked about: switching profile mid-flight must not
      // apply one person's list to another's screen.
      if (get().profileId !== profileId) return;
      set({ myList: new Set(marks.myList), liked: new Set(marks.liked) });
    } catch (e) {
      // The buttons stay usable: a toggle writes to the host regardless, and the worst
      // case is a tick that starts in the wrong state until the next load.
      report('Could not read your lists')(e);
    } finally {
      set({ profileId, loaded: true });
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
