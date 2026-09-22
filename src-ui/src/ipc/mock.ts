/**
 * In-memory IPC transport used when the UI is not hosted by Tauri (docs/DECISIONS.md D4).
 * Mirrors the real host's behaviour closely enough to develop and screenshot every screen.
 */
import type {
  CatalogItem, Channel, CommandArgs, CommandName, CommandResult, GuideSlice, Movie,
  PlayerState, Programme, Progress, Rail, SearchHit, SearchResults, Series,
} from '@shared/ipc';
import * as fx from './fixtures';

const myList = new Set<string>(['movie:2', 'series:1', 'movie:9', 'series:5', 'movie:14']);
const favorites = new Set<number>(
  fx.channels.filter((c) => c.favorite).map((c) => c.id),
);
const progress = new Map<string, Progress>();

for (const p of fx.seedProgress) {
  const duration = p.kind === 'movie'
    ? (fx.movies.find((m) => m.id === p.id)?.runtimeMins ?? 100) * 60
    : (fx.episodes.find((e) => e.id === p.id)?.runtimeMins ?? 45) * 60;
  progress.set(`${p.kind}:${p.id}`, {
    itemKind: p.kind,
    itemId: p.id,
    positionSecs: Math.floor(duration * p.pct),
    durationSecs: duration,
    completed: false,
    updatedAt: Math.floor(Date.now() / 1000) - fx.seedProgress.indexOf(p) * 7200,
  });
}

let player: PlayerState = {
  status: 'idle',
  title: null, subtitle: null, channelId: null, itemKind: null, itemId: null,
  positionSecs: 0, durationSecs: 0, isLive: false,
  volume: 70, muted: false, speed: 1,
  audioTracks: [
    { id: 1, kind: 'audio', title: 'Main', language: 'eng', codec: 'E-AC-3', channels: '5.1', default: true },
    { id: 2, kind: 'audio', title: 'Descriptive', language: 'eng', codec: 'AAC', channels: '2.0', default: false },
    { id: 3, kind: 'audio', title: null, language: 'fra', codec: 'AAC', channels: '2.0', default: false },
  ],
  subtitleTracks: [
    { id: 1, kind: 'subtitle', title: 'English', language: 'eng', codec: 'subrip', channels: null, default: false },
    { id: 2, kind: 'subtitle', title: 'English SDH', language: 'eng', codec: 'ass', channels: null, default: false },
  ],
  activeAudioTrack: 1, activeSubtitleTrack: null,
  aspect: 'auto', error: null,
  stats: {
    resolution: '1920x1080', videoCodec: 'h264', audioCodec: 'eac3', fps: 50,
    bitrateKbps: 6200, droppedFrames: 0, bufferSecs: 8.4, hwDecoder: 'd3d11va',
  },
};

const listeners = new Set<(s: PlayerState) => void>();
function setPlayer(patch: Partial<PlayerState>) {
  player = { ...player, ...patch };
  listeners.forEach((l) => l(player));
  return player;
}
export function onPlayerState(fn: (s: PlayerState) => void) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

/* ── Rails (README §8.2) ───────────────────────────────────────────────────── */

const asMovie = (m: Movie): CatalogItem => ({ kind: 'movie', ...m });
const asSeries = (s: Series): CatalogItem => ({ kind: 'series', ...s });

function buildRails(): Rail[] {
  const byRating = [...fx.movies].sort((a, b) => (b.rating ?? 0) - (a.rating ?? 0));
  const byAdded = [...fx.movies].sort((a, b) => (b.addedAt ?? 0) - (a.addedAt ?? 0));
  const continueItems = [...progress.values()]
    .filter((p) => !p.completed && p.positionSecs > 60)
    .sort((a, b) => b.updatedAt - a.updatedAt)
    .map((p) => {
      if (p.itemKind === 'movie') {
        const m = fx.movies.find((x) => x.id === p.itemId);
        return m ? asMovie(m) : null;
      }
      const ep = fx.episodes.find((x) => x.id === p.itemId);
      const s = ep && fx.series.find((x) => x.id === ep.seriesId);
      return s ? asSeries(s) : null;
    })
    .filter((x): x is CatalogItem => x !== null);

  const genreRail = (g: string): Rail => ({
    id: `genre-${g}`,
    kind: 'genre',
    title: g,
    items: fx.movies.filter((m) => m.genres.includes(g)).slice(0, 18).map(asMovie),
  });

  const rails: Rail[] = [
    { id: 'continue', kind: 'continueWatching', title: 'Continue Watching', items: continueItems },
    {
      id: 'upnext', kind: 'upNext', title: 'Up Next',
      items: fx.series.slice(6, 20).map(asSeries),
    },
    {
      id: 'top10', kind: 'top10', title: 'Top 10 Movies Today',
      items: byRating.slice(0, 10).map(asMovie),
    },
    { id: 'recent', kind: 'recentlyAdded', title: 'Recently Added', items: byAdded.slice(0, 20).map(asMovie) },
    {
      id: 'because', kind: 'becauseYouWatched',
      title: `Because you watched ${fx.movies[3]!.title}`,
      reason: fx.movies[3]!.title,
      items: fx.movies.slice(60, 80).map(asMovie),
    },
    {
      id: 'mylist', kind: 'myList', title: 'My List',
      items: [...myList].flatMap((key): CatalogItem[] => {
        const [kind, rawId] = key.split(':');
        const id = Number(rawId);
        if (kind === 'movie') {
          const m = fx.movies.find((x) => x.id === id);
          return m ? [asMovie(m)] : [];
        }
        const s = fx.series.find((x) => x.id === id);
        return s ? [asSeries(s)] : [];
      }),
    },
    { id: 'series', kind: 'trending', title: 'Trending Series', items: fx.series.slice(0, 18).map(asSeries) },
    {
      id: '4k', kind: 'fourK', title: '4K & HDR',
      items: fx.movies.filter((m) => m.quality === '4K').slice(0, 18).map(asMovie),
    },
    {
      id: 'acclaimed', kind: 'acclaimed', title: 'Critically Acclaimed',
      items: byRating.slice(10, 28).map(asMovie),
    },
    genreRail('Sci-Fi'),
    genreRail('Thriller'),
    {
      id: 'short', kind: 'shortAndSweet', title: 'Short & Sweet',
      items: fx.movies.filter((m) => (m.runtimeMins ?? 999) < 95).slice(0, 18).map(asMovie),
    },
    genreRail('Documentary'),
  ];
  return rails.filter((r) => r.items.length > 0);
}

/* ── Guide ─────────────────────────────────────────────────────────────────── */

const progCache = new Map<string, Programme[]>();
function programmes(ch: Channel, from: number, to: number): Programme[] {
  const key = `${ch.id}:${from}:${to}`;
  let v = progCache.get(key);
  if (!v) {
    v = fx.programmesFor(ch, from, to);
    progCache.set(key, v);
  }
  return v;
}

function nowNext(ch: Channel, now: number) {
  const day = 86400;
  const list = programmes(ch, now - day, now + day);
  return {
    now: list.find((p) => p.start <= now && p.stop > now) ?? null,
    next: list.find((p) => p.start > now) ?? null,
  };
}

/* ── Search ────────────────────────────────────────────────────────────────── */

function search(text: string): SearchResults {
  const q = text.trim().toLowerCase();
  const empty: SearchResults = {
    channels: [], onNow: [], upcoming: [], movies: [], series: [], people: [],
  };
  if (!q) return empty;
  const hit = (kind: SearchHit['kind'], refId: number, t: string, sub: string | null): SearchHit =>
    ({ kind, refId, title: t, subtitle: sub });
  const now = Math.floor(Date.now() / 1000);

  const onNow: SearchHit[] = [];
  const upcoming: SearchHit[] = [];
  for (const ch of fx.channels) {
    for (const p of programmes(ch, now - 3600, now + 86400)) {
      if (!p.title.toLowerCase().includes(q)) continue;
      const target = p.start <= now && p.stop > now ? onNow : upcoming;
      if (target.length < 8) target.push(hit('programme', p.id, p.title, ch.name));
    }
  }
  const people = new Set<string>();
  for (const m of fx.movies) {
    for (const c of m.cast) if (c.toLowerCase().includes(q)) people.add(c);
  }

  return {
    channels: fx.channels.filter((c) => c.name.toLowerCase().includes(q)).slice(0, 8)
      .map((c) => hit('channel', c.id, c.name, c.group)),
    onNow,
    upcoming: upcoming.slice(0, 8),
    movies: fx.movies.filter((m) => m.title.toLowerCase().includes(q)).slice(0, 12)
      .map((m) => hit('movie', m.id, m.title, m.year ? String(m.year) : null)),
    series: fx.series.filter((s) => s.title.toLowerCase().includes(q)).slice(0, 12)
      .map((s) => hit('series', s.id, s.title, s.year ? String(s.year) : null)),
    people: [...people].slice(0, 8).map((p, i) => hit('person', i, p, null)),
  };
}

/* ── Command dispatch ──────────────────────────────────────────────────────── */

const handlers: { [K in CommandName]: (a: CommandArgs<K>) => CommandResult<K> } = {
  'library.rails': () => buildRails(),
  'library.movies': ({ sort, limit, offset, genre }) => {
    let list = genre ? fx.movies.filter((m) => m.genres.includes(genre)) : [...fx.movies];
    const cmp: Record<string, (a: Movie, b: Movie) => number> = {
      recentlyAdded: (a, b) => (b.addedAt ?? 0) - (a.addedAt ?? 0),
      title: (a, b) => a.title.localeCompare(b.title),
      year: (a, b) => (b.year ?? 0) - (a.year ?? 0),
      rating: (a, b) => (b.rating ?? 0) - (a.rating ?? 0),
    };
    list.sort(cmp[sort] ?? cmp.recentlyAdded!);
    return list.slice(offset, offset + limit);
  },
  'library.series': ({ limit, offset, genre }) => {
    const list = genre ? fx.series.filter((s) => s.genres.includes(genre)) : fx.series;
    return list.slice(offset, offset + limit);
  },
  'library.episodes': ({ seriesId, season }) =>
    fx.episodes.filter((e) => e.seriesId === seriesId && (season == null || e.season === season)),
  'library.stats': () => ({
    channels: fx.channels.length,
    movies: fx.movies.length,
    series: fx.series.length,
    episodes: fx.episodes.length,
    programmes: fx.channels.length * 48 * 7,
    epgCoverage: {
      total: fx.channels.length,
      matched: fx.channels.length - 2,
      unmatched: fx.channels.slice(-2).map((c) => c.name),
    },
  }),
  'library.genres': () =>
    [...new Set(fx.movies.flatMap((m) => m.genres))].sort(),

  'channels.list': ({ group, favoritesOnly } = {}) =>
    fx.channels.filter(
      (c) => (!group || c.group === group) && (!favoritesOnly || favorites.has(c.id)),
    ).map((c) => ({ ...c, favorite: favorites.has(c.id) })),
  'channels.groups': () => {
    const counts = new Map<string, number>();
    for (const c of fx.channels) {
      if (c.group) counts.set(c.group, (counts.get(c.group) ?? 0) + 1);
    }
    return [...counts].map(([name, count]) => ({ name, count }));
  },
  'channels.byNumber': ({ number }) => fx.channels.find((c) => c.number === number) ?? null,

  'epg.gridSlice': ({ from, to, channelIds }): GuideSlice => {
    const chans = channelIds.length
      ? fx.channels.filter((c) => channelIds.includes(c.id))
      : fx.channels;
    const map: Record<string, Programme[]> = {};
    for (const c of chans) {
      map[c.epgChannelId ?? String(c.id)] = programmes(c, from, to);
    }
    return { channels: chans, programmes: map, from, to };
  },
  'epg.nowNext': ({ channelId }) => {
    const ch = fx.channels.find((c) => c.id === channelId);
    if (!ch) return { now: null, next: null };
    return nowNext(ch, Math.floor(Date.now() / 1000));
  },

  'search.query': ({ text }) => search(text),

  'player.play': ({ kind, id, positionSecs }) => {
    if (kind === 'live') {
      const ch = fx.channels.find((c) => c.id === id);
      const nn = ch ? nowNext(ch, Math.floor(Date.now() / 1000)) : { now: null, next: null };
      return setPlayer({
        status: 'playing', isLive: true, channelId: id, itemKind: 'live', itemId: id,
        title: ch?.name ?? null, subtitle: nn.now?.title ?? null,
        positionSecs: 0, durationSecs: 0, error: null,
      });
    }
    const item = kind === 'movie'
      ? fx.movies.find((m) => m.id === id)
      : fx.episodes.find((e) => e.id === id);
    const duration = item && 'runtimeMins' in item ? (item.runtimeMins ?? 100) * 60 : 5400;
    const name = item && 'title' in item ? (item.title ?? 'Untitled') : 'Untitled';
    return setPlayer({
      status: 'playing', isLive: false, channelId: null, itemKind: kind, itemId: id,
      title: name, subtitle: null,
      positionSecs: positionSecs ?? 0, durationSecs: duration, error: null,
    });
  },
  'player.pause': () => setPlayer({ status: 'paused' }),
  'player.resume': () => setPlayer({ status: 'playing' }),
  'player.stop': () => setPlayer({ status: 'idle', title: null, itemId: null, channelId: null }),
  'player.seek': ({ positionSecs, relative }) =>
    setPlayer({
      positionSecs: Math.max(
        0,
        Math.min(
          player.durationSecs || Infinity,
          relative ? player.positionSecs + positionSecs : positionSecs,
        ),
      ),
    }),
  'player.setVolume': ({ volume }) => setPlayer({ volume: Math.max(0, Math.min(200, volume)) }),
  'player.setMuted': ({ muted }) => setPlayer({ muted }),
  'player.setSpeed': ({ speed }) => setPlayer({ speed }),
  'player.setAudioTrack': ({ trackId }) => setPlayer({ activeAudioTrack: trackId }),
  'player.setSubtitleTrack': ({ trackId }) => setPlayer({ activeSubtitleTrack: trackId }),
  'player.setAspect': ({ aspect }) => setPlayer({ aspect }),
  'player.state': () => player,

  'progress.save': ({ kind, id, positionSecs, durationSecs }) => {
    const ratio = kind === 'episode' ? 0.95 : 0.92;
    progress.set(`${kind}:${id}`, {
      itemKind: kind, itemId: id, positionSecs, durationSecs,
      completed: durationSecs > 0 && positionSecs / durationSecs >= ratio,
      updatedAt: Math.floor(Date.now() / 1000),
    });
  },
  'progress.get': ({ kind, id }) => progress.get(`${kind}:${id}`) ?? null,

  'mylist.toggle': ({ kind, id }) => {
    const key = `${kind}:${id}`;
    if (myList.has(key)) { myList.delete(key); return false; }
    myList.add(key);
    return true;
  },
  'favorites.toggle': ({ channelId }) => {
    if (favorites.has(channelId)) { favorites.delete(channelId); return false; }
    favorites.add(channelId);
    return true;
  },

  'providers.list': () => fx.providers,
};

export function isInMyList(kind: 'movie' | 'series', id: number) {
  return myList.has(`${kind}:${id}`);
}
export function isFavorite(channelId: number) {
  return favorites.has(channelId);
}
export function getProgress(kind: 'movie' | 'episode', id: number) {
  return progress.get(`${kind}:${id}`) ?? null;
}

export async function invokeMock<K extends CommandName>(
  name: K,
  args: CommandArgs<K>,
): Promise<CommandResult<K>> {
  const fn = handlers[name] as (a: CommandArgs<K>) => CommandResult<K>;
  if (!fn) throw new Error(`unknown command: ${name}`);
  return fn(args);
}
