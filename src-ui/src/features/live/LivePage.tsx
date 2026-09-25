/**
 * Live TV — channel list, categories, favorites (README §7.3).
 * The set-top-box behaviours (banner, digit entry, last-channel) live in
 * features/player/ChannelBanner.tsx and hooks/useZapper.ts so they work from any screen.
 */
import { useCallback, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { Channel } from '@shared/ipc';
import { Badge, Button, EmptyState, Skeleton } from '@/components/Primitives';
import { Icon } from '@/components/Icon';
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

  const profileId = activeProfileId();
  const { data: groups } = useCommand('channels.groups', undefined, []);
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

  const categories = useMemo(
    () => [{ name: 'All', count: 0 }, ...(groups ?? [])],
    [groups],
  );

  const visible = useMemo(() => channels ?? [], [channels]);
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
    <div style={{ padding: 'var(--sp-5) var(--sp-6)' }}>
      <div
        style={{
          display: 'flex', alignItems: 'center', gap: 'var(--sp-3)',
          marginBottom: 'var(--sp-4)', flexWrap: 'wrap',
        }}
      >
        <h1 style={{ margin: 0, fontSize: 'var(--fs-2xl)', fontWeight: 800 }}>Live TV</h1>
        <Button
          size="sm"
          variant={favoritesOnly ? 'primary' : 'secondary'}
          icon="heart"
          iconFilled={favoritesOnly}
          onClick={() => setFavoritesOnly((f) => !f)}
        >
          Favorites
        </Button>
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

      <div
        className="no-scrollbar"
        style={{
          display: 'flex', gap: 6, overflowX: 'auto', paddingBottom: 'var(--sp-3)',
          marginBottom: 'var(--sp-4)',
        }}
      >
        {categories.map((c) => {
          const value = c.name === 'All' ? undefined : c.name;
          const active = group === value;
          return (
            <button
              key={c.name}
              onClick={() => setGroup(value)}
              style={{
                padding: '7px 15px', borderRadius: 'var(--r-full)', cursor: 'pointer',
                whiteSpace: 'nowrap', fontSize: 'var(--fs-sm)', fontWeight: 600,
                border: `1px solid ${active ? 'transparent' : 'var(--border)'}`,
                background: active ? 'var(--text)' : 'var(--surface)',
                color: active ? 'var(--text-invert)' : 'var(--text-muted)',
              }}
            >
              {c.name}{c.count ? ` (${c.count})` : ''}
            </button>
          );
        })}
      </div>

      {loading && (
        <div style={{ display: 'grid', gap: 8 }}>
          {Array.from({ length: 8 }, (_, i) => <Skeleton key={i} h={68} />)}
        </div>
      )}

      {!loading && (channels ?? []).length === 0 && (
        <EmptyState
          icon="tv"
          title={favoritesOnly ? 'No favorite channels yet' : 'No channels'}
          body={
            favoritesOnly
              ? 'Press F while watching, or use the heart on any channel, to add it here.'
              : 'Add a provider in Settings to populate your channel list.'
          }
        />
      )}

      {!loading && (channels ?? []).length > 0 && (
        <div
          ref={measure}
          data-testid="channel-scroller"
          // The list is virtualised because a real subscription has twenty-two
          // thousand channels, and `.map()` over that is twenty-two thousand DOM
          // nodes. The Guide and the playlist editor were already virtualised; this
          // screen was the one that was missed.
          style={{ height: 'calc(100vh - 220px)', overflowY: 'auto', overflowX: 'hidden' }}
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
      )}
    </div>
  );
}

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
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 2 }}>
          <strong style={{ fontSize: 'var(--fs-md)' }}>{channel.name}</strong>
          {channel.quality && <Badge tone={channel.quality === '4K' ? 'accent' : 'neutral'}>{channel.quality}</Badge>}
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
