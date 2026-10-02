/**
 * Live TV — channel list, groups, favorites (README §7.3).
 * The set-top-box behaviours (banner, digit entry, last-channel) live in
 * features/player/ChannelBanner.tsx and hooks/useZapper.ts so they work from any screen.
 *
 * **The groups were a strip that scrolled sideways.** That is fine for the eight a
 * fixture has. A real subscription has several hundred — "UK | ENTERTAINMENT", "US|
 * SPORTS HD", "DE Kinder", one per country per genre per quality — and a sideways strip
 * of those is a list you cannot scan, cannot see the end of, and cannot get back to the
 * start of. The group you had selected scrolled out of sight as soon as you moved down
 * the channels. They are a sidebar now, with their counts and a filter box.
 *
 * **And there was no way to search within one.** Ctrl-K searches everything, which
 * answers a different question: "find me this" rather than "narrow what I am looking
 * at". A group of 482 channels needed one.
 */
import { useCallback, useDeferredValue, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { Channel } from '@shared/ipc';
import { Badge, Button, EmptyState, Skeleton, TextField } from '@/components/Primitives';
import { GroupSidebar } from '@/components/GroupSidebar';
import { Icon } from '@/components/Icon';
import { LetterBar } from '@/components/LetterBar';
import { useCommand } from '@/hooks/useCommand';
import { invoke } from '@/ipc';
import { report } from '@/lib/errors';
import { clockTime, progressPct } from '@/lib/format';
import { activeProfileId } from '@/state/profile';
import { type NowNext, useNowNext } from './useNowNext';

/** Row height and grid tile height, which the virtualiser needs up front. */
const ROW_H = 72;
const TILE_H = 150;
/** Tiles per row in the grid view, at the 150px minimum the template sets. */
const TILE_MIN_W = 150;

export function LivePage({ onTune }: { onTune: (c: Channel) => void }) {
  const [group, setGroup] = useState<string | undefined>(undefined);
  const [favoritesOnly, setFavoritesOnly] = useState(false);
  const [view, setView] = useState<'list' | 'grid'>('list');
  const [typed, setTyped] = useState('');
  const [letter, setLetter] = useState<string | undefined>();
  /**
   * Channel order. A provider's own numbering is what a set-top box uses and what digit
   * entry addresses, so it stays the default — but it makes the A–Z bar meaningless,
   * which is why the bar is inert until this says otherwise.
   */
  const [order, setOrder] = useState<'number' | 'name'>('number');

  const profileId = activeProfileId();
  const { data: groups, loading: groupsLoading } = useCommand('channels.groups', undefined, []);
  // Only so the empty state can say something true. "Add a provider in Settings" on a
  // machine that already has one, with twenty thousand films in the library, is the
  // F-27 mistake again: a confident answer to a question nobody asked.
  const { data: providers } = useCommand('providers.list', undefined, []);
  // A nonce rather than a full reload, so toggling a heart repaints the list without
  // the screen blanking back to skeletons.
  const [favNonce, setFavNonce] = useState(0);
  const { data: channels, loading } = useCommand(
    'channels.list',
    { group, favoritesOnly, profileId },
    [group, favoritesOnly, profileId, favNonce],
  );

  const toggleFavorite = useCallback(
    (channel: Channel) => {
      invoke('favorites.toggle', { profileId, channelId: channel.id })
        .then(() => setFavNonce((n) => n + 1))
        .catch(report(`Could not change favourites for ${channel.name}`));
    },
    [profileId],
  );

  /**
   * Narrowing, sorting and lettering, in that order, on the rows already in memory.
   *
   * Client-side on purpose: `channels.list` is not paged — it answers with every channel
   * in the group, which on a real library is 6.3 MB of JSON — so asking the host again
   * per keystroke would re-send the whole group to filter it. The browse pages do the
   * opposite for the opposite reason: they are paged, so the host must do it.
   */
  const lazyTyped = useDeferredValue(typed);
  const visible = useMemo(() => {
    let rows = channels ?? [];
    const needle = lazyTyped.trim().toLowerCase();
    if (needle) rows = rows.filter((c) => c.name.toLowerCase().includes(needle));
    if (letter) {
      const first = (c: Channel) => c.name.trim().charAt(0).toUpperCase();
      rows = letter === '#'
        ? rows.filter((c) => first(c) < 'A' || first(c) > 'Z')
        : rows.filter((c) => first(c) === letter);
    }
    if (order === 'name') {
      rows = [...rows].sort((a, b) => a.name.localeCompare(b.name));
    }
    return rows;
  }, [channels, lazyTyped, letter, order]);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [width, setWidth] = useState(0);
  // Measured rather than assumed: the grid template is `auto-fill` over a 150px
  // minimum, and the virtualiser has to agree with it about how many fit.
  const measure = useCallback((el: HTMLDivElement | null) => {
    scrollRef.current = el;
    if (el) setWidth(el.clientWidth);
  }, []);
  const perRow = view === 'grid' ? Math.max(1, Math.floor((width || 1200) / TILE_MIN_W)) : 1;
  const rowCount = view === 'grid' ? Math.ceil(visible.length / perRow) : visible.length;

  const virt = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => (view === 'grid' ? TILE_H : ROW_H),
    overscan: 8,
  });

  // Only the rows on screen get a guide lookup, and they get it in one call.
  const visibleIds = useMemo(() => {
    const ids: number[] = [];
    for (const row of virt.getVirtualItems()) {
      const from = view === 'grid' ? row.index * perRow : row.index;
      const to = view === 'grid' ? from + perRow : from + 1;
      for (let i = from; i < to && i < visible.length; i += 1) ids.push(visible[i]!.id);
    }
    return ids;
    // `getVirtualItems` is recomputed on scroll; depending on its output directly is
    // what keeps the window and the request in step.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [virt.getVirtualItems(), visible, perRow, view]);

  const guide = useNowNext(visibleIds);

  return (
    <div
      style={{
        padding: 'var(--sp-5) var(--sp-6)',
        display: 'flex',
        gap: 'var(--sp-5)',
        alignItems: 'flex-start',
      }}
    >
      <GroupSidebar
        label="Channel groups"
        groups={groups ?? []}
        selected={group}
        onSelect={(next) => { setGroup(next); setFavoritesOnly(false); }}
        loading={groupsLoading}
        pinned={[
          {
            label: 'All channels',
            value: undefined,
            icon: 'tv',
            active: !group && !favoritesOnly,
            onSelect: () => { setGroup(undefined); setFavoritesOnly(false); },
          },
          {
            label: 'Favourites',
            value: undefined,
            icon: 'heart',
            active: favoritesOnly,
            // Favourites cut across groups, so choosing them clears the group rather
            // than intersecting with it — "my favourites, within UK Sports" is a
            // question nobody asks of a list of fourteen channels.
            onSelect: () => { setFavoritesOnly(true); setGroup(undefined); },
          },
        ]}
      />

      <div style={{ flex: 1, minWidth: 0 }}>
      <div
        style={{
          display: 'flex', alignItems: 'center', gap: 'var(--sp-3)',
          marginBottom: 'var(--sp-4)', flexWrap: 'wrap',
        }}
      >
        <h1 style={{ margin: 0, fontSize: 'var(--fs-2xl)', fontWeight: 800 }}>
          {/* Which list this is. "Live TV" over one group of 482 left nothing on screen
              saying which group, once the strip had scrolled away. */}
          {favoritesOnly ? 'Favourites' : group ?? 'Live TV'}
        </h1>
        <span
          data-testid="channel-count"
          style={{ color: 'var(--text-faint)', fontVariantNumeric: 'tabular-nums' }}
        >
          {loading ? '' : NUMBER.format(visible.length)}
        </span>

        <TextField
          icon="search"
          clearable
          onClear={() => setTyped('')}
          value={typed}
          onChange={(e) => setTyped(e.target.value)}
          placeholder="Search channels…"
          aria-label="Search channels"
          data-testid="channel-search"
          style={{ width: 200 }}
        />

        <div style={{ display: 'flex', gap: 4 }}>
          <Button
            size="sm"
            variant={order === 'number' ? 'primary' : 'ghost'}
            onClick={() => setOrder('number')}
          >
            Number
          </Button>
          <Button
            size="sm"
            variant={order === 'name' ? 'primary' : 'ghost'}
            onClick={() => setOrder('name')}
          >
            A–Z
          </Button>
        </div>

        <div style={{ marginLeft: 'auto', display: 'flex', gap: 4 }}>
          <Button
            size="sm" icon="stack"
            variant={view === 'list' ? 'primary' : 'ghost'}
            onClick={() => setView('list')}
          >
            List
          </Button>
          <Button
            size="sm" icon="grid"
            variant={view === 'grid' ? 'primary' : 'ghost'}
            onClick={() => setView('grid')}
          >
            Grid
          </Button>
        </div>
      </div>

      {loading && (
        <div style={{ display: 'grid', gap: 8 }}>
          {Array.from({ length: 8 }, (_, i) => <Skeleton key={i} h={68} />)}
        </div>
      )}

      {!loading && (channels ?? []).length === 0 && (
        <EmptyState
          icon="tv"
          // British, like the aria-labels on every heart in this list and the sidebar
          // entry that got you here. The screen had both spellings in it.
          title={favoritesOnly ? 'No favourite channels yet' : 'No channels'}
          body={
            favoritesOnly
              ? 'Press F while watching, or use the heart on any channel, to add it here.'
              : emptyReason(providers ?? null)
          }
        />
      )}

      {/* Channels exist, but not after this search or this letter. A different
          sentence from "No channels", because the answer is different: clear the box. */}
      {!loading && (channels ?? []).length > 0 && visible.length === 0 && (
        <EmptyState
          icon="search"
          title="No channel matches"
          body={
            letter && typed.trim()
              ? `Nothing here matches “${typed.trim()}” and starts with ${letter}.`
              : letter
                ? `No channel in this group starts with ${letter === '#' ? 'a number or symbol' : letter}.`
                : `Nothing here matches “${typed.trim()}”.`
          }
          action={(
            <Button onClick={() => { setTyped(''); setLetter(undefined); }}>
              Clear search
            </Button>
          )}
        />
      )}

      {!loading && visible.length > 0 && (
        <div style={{ display: 'flex', gap: 'var(--sp-3)', alignItems: 'flex-start' }}>
        <div
          ref={measure}
          data-testid="channel-scroller"
          // The list is virtualised because a real subscription has twenty-two
          // thousand channels, and `.map()` over that is twenty-two thousand DOM
          // nodes. The Guide and the playlist editor were already virtualised; this
          // screen was the one that was missed.
          style={{
            height: 'calc(100vh - 220px)', overflowY: 'auto', overflowX: 'hidden',
            // It is a flex item now, beside the A–Z bar. Without this it sizes to its
            // content — and its content is absolutely positioned, so it has none: the
            // scroller collapsed to 26px, every row with it, and the rows became a
            // 26px-wide column of slivers that could not be clicked.
            flex: 1,
            minWidth: 0,
          }}
        >
          <div style={{ height: virt.getTotalSize(), position: 'relative' }}>
            {virt.getVirtualItems().map((row) => {
              const items = view === 'list'
                ? [visible[row.index]!]
                : visible.slice(row.index * perRow, row.index * perRow + perRow);
              return (
                <div
                  key={row.key}
                  data-index={row.index}
                  style={{
                    position: 'absolute', top: 0, left: 0, width: '100%',
                    transform: `translateY(${row.start}px)`,
                    ...(view === 'grid'
                      ? {
                        display: 'grid', gap: 'var(--sp-3)',
                        gridTemplateColumns: `repeat(${perRow}, minmax(0, 1fr))`,
                        paddingBottom: 'var(--sp-3)',
                      }
                      : { paddingBottom: 4 }),
                  }}
                >
                  {items.map((c) => (view === 'list' ? (
                    <ChannelRow
                      key={c.id}
                      channel={c}
                      guide={guide.get(c.id) ?? null}
                      onTune={onTune}
                      onToggleFavorite={toggleFavorite}
                    />
                  ) : (
                    <ChannelTile key={c.id} channel={c} guide={guide.get(c.id) ?? null} onTune={onTune} />
                  )))}
                </div>
              );
            })}
          </div>
        </div>
        <LetterBar selected={letter} onSelect={setLetter} disabled={order !== 'name'} />
        </div>
      )}
      </div>
    </div>
  );
}

/**
 * Thousands separators without depending on the container's locale — a WebView with no
 * locale configured groups by nothing, and a channel count runs to five figures.
 */
const NUMBER = new Intl.NumberFormat('en-US');

function ChannelRow({
  channel, guide, onTune, onToggleFavorite,
}: {
  channel: Channel;
  /** Now and next, fetched for the visible window rather than by this row. */
  guide: NowNext | null;
  onTune: (c: Channel) => void;
  onToggleFavorite: (c: Channel) => void;
}) {
  const data = guide;
  const now = Math.floor(Date.now() / 1000);
  const pct = data?.now
    ? progressPct(now - data.now.start, data.now.stop - data.now.start)
    : 0;

  return (
    // A div with a button role rather than a <button>: the heart is a control of its
    // own and a button inside a button is invalid markup that browsers resolve by
    // dropping one of them.
    <div
      role="button"
      tabIndex={0}
      data-testid="channel-row"
      aria-label={`Watch ${channel.name}`}
      onClick={() => onTune(channel)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onTune(channel); }
      }}
      style={{
        display: 'grid', gridTemplateColumns: '44px 44px 1fr auto', gap: 'var(--sp-3)',
        alignItems: 'center', padding: 'var(--sp-3)', textAlign: 'left',
        background: 'var(--bg-elevated)', border: '1px solid var(--border)',
        borderRadius: 'var(--r-md)', cursor: 'pointer', color: 'inherit',
        // The height the virtualiser was told, not the height the content wants. Rows are
        // absolutely positioned at multiples of `ROW_H`, so one that grows taller than
        // that is drawn *over* the row below — and the row below, being later in the DOM,
        // then swallows its clicks. Which is what happened the moment the group sidebar
        // took 250px off this column and the longer channel names began to wrap.
        height: ROW_H - 4,
        boxSizing: 'border-box',
        overflow: 'hidden',
      }}
    >
      <span
        style={{
          fontSize: 'var(--fs-md)', fontWeight: 800, color: 'var(--text-faint)',
          fontVariantNumeric: 'tabular-nums', textAlign: 'right',
        }}
      >
        {channel.number}
      </span>
      {channel.logo ? (
        <img
          src={channel.logo} alt=""
          style={{ width: 40, height: 40, borderRadius: 'var(--r-sm)', objectFit: 'cover' }}
        />
      ) : <div style={{ width: 40 }} />}

      <div style={{ minWidth: 0 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 2, minWidth: 0 }}>
          {/* One line. A provider's channel names run to "UK: SKY SPORTS MAIN EVENT FHD
              (1080p)", which wrapped as soon as this column narrowed. */}
          <strong
            style={{
              fontSize: 'var(--fs-md)', whiteSpace: 'nowrap', overflow: 'hidden',
              textOverflow: 'ellipsis', minWidth: 0,
            }}
          >
            {channel.name}
          </strong>
          {channel.quality && (
            <span style={{ flexShrink: 0 }}>
              <Badge tone={channel.quality === '4K' ? 'accent' : 'neutral'}>
                {channel.quality}
              </Badge>
            </span>
          )}
        </div>
        <div
          style={{
            fontSize: 'var(--fs-sm)', color: 'var(--text-muted)',
            whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
          }}
        >
          {data?.now
            ? `${clockTime(data.now.start)} ${data.now.title}`
            : 'No guide data'}
          {data?.next && (
            <span style={{ color: 'var(--text-faint)' }}>
              {'  ·  Next: '}{data.next.title}
            </span>
          )}
        </div>
        {pct > 0 && (
          <div
            style={{
              marginTop: 5, height: 2, background: 'var(--border)',
              borderRadius: 'var(--r-full)', overflow: 'hidden', maxWidth: 260,
            }}
          >
            <div style={{ width: `${pct}%`, height: '100%', background: 'var(--live)' }} />
          </div>
        )}
      </div>

      <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
        {channel.hasCatchup && <Badge tone="outline">Catch-up</Badge>}
        <button
          type="button"
          aria-label={
            channel.favorite
              ? `Remove ${channel.name} from favourites`
              : `Add ${channel.name} to favourites`
          }
          aria-pressed={channel.favorite}
          data-testid={`favorite-${channel.id}`}
          onClick={(e) => { e.stopPropagation(); onToggleFavorite(channel); }}
          style={{
            display: 'grid', placeItems: 'center', width: 30, height: 30,
            background: 'transparent', border: 'none', borderRadius: 'var(--r-full)',
            cursor: 'pointer',
            color: channel.favorite ? 'var(--accent)' : 'var(--text-faint)',
          }}
        >
          <Icon name="heart" size={16} filled={channel.favorite} />
        </button>
        <Icon name="play" size={18} filled style={{ color: 'var(--text-faint)' }} />
      </div>
    </div>
  );
}

function ChannelTile({
  channel, guide, onTune,
}: {
  channel: Channel;
  guide: NowNext | null;
  onTune: (c: Channel) => void;
}) {
  const data = guide;
  return (
    <button
      onClick={() => onTune(channel)}
      style={{
        background: 'var(--bg-elevated)', border: '1px solid var(--border)',
        borderRadius: 'var(--r-md)', padding: 'var(--sp-3)', cursor: 'pointer',
        textAlign: 'left', color: 'inherit', display: 'grid', gap: 'var(--sp-2)',
      }}
    >
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        {channel.logo && (
          <img
            src={channel.logo} alt=""
            style={{ width: 42, height: 42, borderRadius: 'var(--r-sm)', objectFit: 'cover' }}
          />
        )}
        <span style={{ color: 'var(--text-faint)', fontWeight: 800 }}>{channel.number}</span>
      </div>
      <strong style={{ fontSize: 'var(--fs-sm)' }}>{channel.name}</strong>
      <span
        style={{
          fontSize: 'var(--fs-xs)', color: 'var(--text-muted)', whiteSpace: 'nowrap',
          overflow: 'hidden', textOverflow: 'ellipsis',
        }}
      >
        {data?.now?.title ?? '—'}
      </span>
    </button>
  );
}

/**
 * Why the channel list is empty, for someone who has to act on the answer.
 *
 * Three different situations used to produce the same sentence — "Add a provider in
 * Settings to populate your channel list" — including the one where a provider is
 * already there with twenty thousand films behind it. That reading sends somebody off
 * to add a second copy of the provider they have, which fixes nothing and leaves them
 * with two.
 */
function emptyReason(
  providers: { name: string; channelCount: number }[] | null,
): string {
  if (providers === null) return 'Checking which providers are set up…';
  if (providers.length === 0) {
    return 'Add a provider in Settings to populate your channel list.';
  }

  const imported = providers.reduce((n, p) => n + p.channelCount, 0);
  if (imported === 0) {
    // The library has a provider and no channels from it. That is a fact about the
    // import, not about the filter, and it is the one case the old text actively
    // misdescribed.
    const names = providers.map((p) => p.name).join(', ');
    return `${names} imported no live channels. Some accounts carry films and series `
      + 'but no live streams; if yours should have them, press Refresh next to the '
      + 'provider in Settings and check the count it reports.';
  }

  // Channels exist and none are showing: something is filtering them out.
  return `Your library has ${imported} channels, but none are visible here. Check `
    + 'the group filter above, and Settings → Filters for English-only and '
    + 'hide-duplicates.';
}
