import { describe, expect, it } from 'vitest';
import type { Channel } from '@shared/ipc';
import { NO_FILTERS, activeCount, applyFilters, hostGroup, toggleGroup } from './guideFilters';

const ch = (id: number, over: Partial<Channel> = {}): Channel => ({
  id,
  name: `Channel ${id}`,
  number: id,
  logo: null,
  group: 'News',
  epgChannelId: null,
  tvgId: null,
  quality: 'HD',
  hidden: false,
  isRadio: false,
  hasCatchup: false,
  favorite: false,
  ...over,
} as Channel);

const list = [
  ch(1, { group: 'UK News', epgChannelId: 'bbc', quality: 'FHD', favorite: true }),
  ch(2, { group: 'UK Sport', hasCatchup: true, quality: '4K' }),
  ch(3, { group: 'US News', isRadio: true, quality: 'SD' }),
  ch(4, { group: 'Adult', quality: 'HD' }),
  ch(5, { group: null }),
];
const ids = (cs: Channel[]) => cs.map((c) => c.id);

describe('guide filters', () => {
  it('lets everything through with nothing chosen', () => {
    expect(ids(applyFilters(list, NO_FILTERS))).toEqual([1, 2, 3, 4, 5]);
  });

  it('shows only several chosen groups at once', () => {
    const f = { ...NO_FILTERS, groups: ['UK News', 'UK Sport'] };
    expect(ids(applyFilters(list, f))).toEqual([1, 2]);
  });

  it('hides the chosen groups instead, keeping channels with no group', () => {
    const f = { ...NO_FILTERS, groups: ['Adult', 'US News'], groupMode: 'hide' as const };
    expect(ids(applyFilters(list, f))).toEqual([1, 2, 5]);
  });

  it('narrows by what the channel is, on top of its group', () => {
    expect(ids(applyFilters(list, { ...NO_FILTERS, withListings: true }))).toEqual([1]);
    expect(ids(applyFilters(list, { ...NO_FILTERS, favoritesOnly: true }))).toEqual([1]);
    expect(ids(applyFilters(list, { ...NO_FILTERS, catchupOnly: true }))).toEqual([2]);
    expect(ids(applyFilters(list, { ...NO_FILTERS, hideRadio: true }))).toEqual([1, 2, 4, 5]);
    expect(ids(applyFilters(list, { ...NO_FILTERS, qualities: ['4K', 'FHD'] }))).toEqual([1, 2]);
  });

  it('asks the host for one group only when the filter is exactly that', () => {
    expect(hostGroup({ ...NO_FILTERS, groups: ['UK News'] })).toBe('UK News');
    expect(hostGroup({ ...NO_FILTERS, groups: ['UK News', 'UK Sport'] })).toBeUndefined();
    expect(hostGroup({ ...NO_FILTERS, groups: ['Adult'], groupMode: 'hide' })).toBeUndefined();
    expect(hostGroup(NO_FILTERS)).toBeUndefined();
  });

  it('toggles a group in and out, and counts what is on', () => {
    const one = toggleGroup(NO_FILTERS, 'UK News');
    expect(one.groups).toEqual(['UK News']);
    expect(toggleGroup(one, 'UK News').groups).toEqual([]);
    expect(activeCount(NO_FILTERS)).toBe(0);
    expect(activeCount({ ...one, catchupOnly: true, qualities: ['HD'] })).toBe(3);
  });
});
