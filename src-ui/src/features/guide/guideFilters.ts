/**
 * What the guide shows: which groups, and which kinds of channel.
 *
 * Asked for: "add comprehensive filters to the guide allowing me to only show certain
 * groups of channels". The guide used to take one category at a time, so a viewer who
 * wanted their country's channels -- spread across a dozen of a provider's groups --
 * could see one of them at once, and one who wanted everything *but* the adult and
 * foreign-sports groups could not say that at all. Now any number of groups can be
 * shown or hidden, on top of a few properties of the channel itself.
 *
 * Kept on this machine (`localStorage`), like the rest of the guide's view: a filter is
 * how somebody likes to browse, not part of the library, and it should be there next
 * time the guide is opened.
 */
import type { Channel, Quality } from '@shared/ipc';

export interface GuideFilters {
  /** The groups chosen. Empty means none chosen: every group is shown. */
  groups: string[];
  /** Whether the chosen groups are the only ones shown, or the ones left out. */
  groupMode: 'only' | 'hide';
  /** Only channels the guide has a schedule for. */
  withListings: boolean;
  favoritesOnly: boolean;
  catchupOnly: boolean;
  hideRadio: boolean;
  /** Empty means any quality. */
  qualities: Quality[];
}

export const NO_FILTERS: GuideFilters = {
  groups: [],
  groupMode: 'only',
  withListings: false,
  favoritesOnly: false,
  catchupOnly: false,
  hideRadio: false,
  qualities: [],
};

export const QUALITIES: Quality[] = ['4K', 'FHD', 'HD', 'SD'];

const KEY = 'aurora.guide.filters';
/** Where "With listings" lived before it became one filter of several. */
const OLD_LISTINGS_KEY = 'aurora.guide.withListings';

/** What was chosen last time. Checked field by field: the store is editable text. */
export function readFilters(): GuideFilters {
  let raw: string | null;
  let oldListings: boolean;
  try {
    raw = window.localStorage.getItem(KEY);
    oldListings = window.localStorage.getItem(OLD_LISTINGS_KEY) === '1';
  } catch {
    return NO_FILTERS;
  }
  if (!raw) return { ...NO_FILTERS, withListings: oldListings };
  try {
    const v = JSON.parse(raw) as Record<string, unknown>;
    const bool = (x: unknown) => x === true;
    return {
      groups: Array.isArray(v.groups) ? v.groups.filter((g): g is string => typeof g === 'string') : [],
      groupMode: v.groupMode === 'hide' ? 'hide' : 'only',
      withListings: bool(v.withListings),
      favoritesOnly: bool(v.favoritesOnly),
      catchupOnly: bool(v.catchupOnly),
      hideRadio: bool(v.hideRadio),
      qualities: Array.isArray(v.qualities)
        ? v.qualities.filter((q): q is Quality => QUALITIES.includes(q as Quality))
        : [],
    };
  } catch {
    return NO_FILTERS;
  }
}

export function writeFilters(f: GuideFilters) {
  try {
    window.localStorage.setItem(KEY, JSON.stringify(f));
  } catch {
    // Storage refused (a private window, a policy): the choice lasts this visit.
  }
}

/**
 * The one group to ask the host for, when the filter comes down to exactly that.
 *
 * The host narrows `channels.list` by a single group (R-130: choosing a category should
 * narrow what is fetched, not only what is drawn), so the common case -- one group
 * shown -- still crosses the bridge as one group. Anything else is fetched whole and
 * narrowed here, which is what "All channels" always did.
 */
export function hostGroup(f: GuideFilters): string | undefined {
  return f.groupMode === 'only' && f.groups.length === 1 ? f.groups[0] : undefined;
}

/** The channels the filters let through, in the order given. */
export function applyFilters(channels: Channel[], f: GuideFilters): Channel[] {
  const chosen = new Set(f.groups);
  const qualities = new Set(f.qualities);
  return channels.filter((c) => {
    if (chosen.size > 0) {
      const inChosen = chosen.has(c.group ?? '');
      if (f.groupMode === 'only' ? !inChosen : inChosen) return false;
    }
    if (f.withListings && c.epgChannelId == null) return false;
    if (f.favoritesOnly && !c.favorite) return false;
    if (f.catchupOnly && !c.hasCatchup) return false;
    if (f.hideRadio && c.isRadio) return false;
    if (qualities.size > 0 && !(c.quality && qualities.has(c.quality))) return false;
    return true;
  });
}

/** How many separate filters are on, for the badge on the Filters button. */
export function activeCount(f: GuideFilters): number {
  return (
    (f.groups.length > 0 ? 1 : 0) +
    (f.withListings ? 1 : 0) +
    (f.favoritesOnly ? 1 : 0) +
    (f.catchupOnly ? 1 : 0) +
    (f.hideRadio ? 1 : 0) +
    (f.qualities.length > 0 ? 1 : 0)
  );
}

/** Add a group to the choice, or take it out. */
export function toggleGroup(f: GuideFilters, group: string): GuideFilters {
  const has = f.groups.includes(group);
  return { ...f, groups: has ? f.groups.filter((g) => g !== group) : [...f.groups, group] };
}
