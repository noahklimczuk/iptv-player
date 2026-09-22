/**
 * In-memory IPC transport used when the UI is not hosted by Tauri (docs/DECISIONS.md D4).
 * Mirrors the real host's behaviour closely enough to develop and screenshot every screen.
 */
import type {
  CatalogItem, Channel, CommandArgs, CommandName, CommandResult, DetectedSource,
  GuideSlice, IngestProgress, MarkerKind, Movie, PlaybackAids, PlayerState, Programme,
  Progress, Rail, SearchHit, SearchResults, Series, SeriesPrefs, SkipMarker, SyncReport,
  ValidationResult,
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

/**
 * Skip-marker state, mirroring aurora-core::markers and aurora-db::repo::markers so
 * the browser build behaves the same as the Windows one.
 */
const MIN_SAMPLES = 2;
const UP_NEXT_TAIL_SECS = 45;

/** Chapter-derived markers, keyed by episode id. */
const chapterMarkers = new Map<number, SkipMarker[]>();
/** The viewer's own skips, keyed by `seriesId:kind`. */
const userSkips = new Map<string, [number, number][]>();
const seriesPrefs = new Map<number, SeriesPrefs>();

// Episodes of odd-numbered series ship with chapters; even-numbered ones have none,
// so the "learn from the viewer" path is reachable in the demo too.
for (const ep of fx.episodes) {
  if (ep.seriesId % 2 !== 1) continue;
  const runtime = (ep.runtimeMins ?? 45) * 60;
  chapterMarkers.set(ep.id, [
    { kind: 'intro', startSecs: 28, endSecs: 92, source: 'chapters' },
    { kind: 'credits', startSecs: runtime - 50, endSecs: runtime, source: 'chapters' },
  ]);
}

const median = (xs: number[]) => {
  const v = [...xs].sort((a, b) => a - b);
  const mid = Math.floor(v.length / 2);
  return v.length % 2 === 0 ? (v[mid - 1]! + v[mid]!) / 2 : v[mid]!;
};

const plausible = (m: SkipMarker) =>
  m.startSecs >= 0 && m.endSecs - m.startSecs >= 5 && m.endSecs - m.startSecs <= 300;

function resolveMarkers(episodeId: number): SkipMarker[] {
  const ep = fx.episodes.find((e) => e.id === episodeId);
  const own = chapterMarkers.get(episodeId) ?? [];
  const out = own.filter(plausible);
  if (!ep) return out;

  for (const kind of ['intro', 'recap', 'credits'] as MarkerKind[]) {
    if (out.some((m) => m.kind === kind)) continue;
    // Learn from the viewer's own skips elsewhere in this series.
    const observations = userSkips.get(`${ep.seriesId}:${kind}`) ?? [];
    if (observations.length < MIN_SAMPLES) continue;
    const learned: SkipMarker = {
      kind,
      startSecs: median(observations.map(([start]) => start)),
      endSecs: median(observations.map(([, end]) => end)),
      source: 'learned',
    };
    if (plausible(learned)) out.push(learned);
  }
  return out.sort((a, b) => a.startSecs - b.startSecs);
}

function upNextAt(markers: SkipMarker[], durationSecs: number): number | null {
  if (durationSecs <= 0) return null;
  const credits = markers.find((m) => m.kind === 'credits');
  if (credits) return credits.startSecs;
  return Math.max(durationSecs - UP_NEXT_TAIL_SECS, durationSecs * 0.5);
}

function followingEpisode(episodeId: number) {
  const ep = fx.episodes.find((e) => e.id === episodeId);
  if (!ep) return null;
  return (
    fx.episodes
      .filter((e) => e.seriesId === ep.seriesId)
      .sort((a, b) => a.season - b.season || a.episode - b.episode)
      .find(
        (e) =>
          e.season > ep.season || (e.season === ep.season && e.episode > ep.episode),
      ) ?? null
  );
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

/**
 * Advance the playhead while playing. On Windows this comes from mpv's time-pos
 * property; here it has to be simulated, otherwise Skip Intro and Up Next could
 * never trigger in the browser build.
 */
let ticker: ReturnType<typeof setInterval> | undefined;
function ensureTicker() {
  if (ticker) return;
  ticker = setInterval(() => {
    if (player.status !== 'playing') return;
    const next = player.positionSecs + 1;
    if (!player.isLive && player.durationSecs > 0 && next >= player.durationSecs) {
      setPlayer({ positionSecs: player.durationSecs, status: 'paused' });
      return;
    }
    setPlayer({ positionSecs: next });
  }, 1000);
}
function setPlayer(patch: Partial<PlayerState>) {
  player = { ...player, ...patch };
  if (player.status === 'playing') ensureTicker();
  listeners.forEach((l) => l(player));
  return player;
}
export function onPlayerState(fn: (s: PlayerState) => void) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

const ingestListeners = new Set<(p: IngestProgress) => void>();
function emitIngest(p: IngestProgress) {
  ingestListeners.forEach((l) => l(p));
}
export function onIngestProgress(fn: (p: IngestProgress) => void) {
  ingestListeners.add(fn);
  return () => ingestListeners.delete(fn);
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

type Handler<K extends CommandName> = (
  a: CommandArgs<K>,
) => CommandResult<K> | Promise<CommandResult<K>>;

const handlers: { [K in CommandName]: Handler<K> } = {
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
  'library.playbackAids': ({ episodeId }): PlaybackAids => {
    const ep = fx.episodes.find((e) => e.id === episodeId);
    const duration = (ep?.runtimeMins ?? 45) * 60;
    const markers = resolveMarkers(episodeId);
    return {
      markers,
      upNextAtSecs: upNextAt(markers, duration),
      nextEpisode: followingEpisode(episodeId),
      prefs:
        (ep && seriesPrefs.get(ep.seriesId)) ?? {
          alwaysSkipIntro: false,
          alwaysSkipRecap: false,
          autoplayNext: true,
        },
    };
  },

  'library.recordSkip': ({ episodeId, kind, startSecs, endSecs }) => {
    const ep = fx.episodes.find((e) => e.id === episodeId);
    if (!ep) return;
    if (endSecs - startSecs < 5 || endSecs - startSecs > 300) return;
    const key = `${ep.seriesId}:${kind}`;
    const list = userSkips.get(key) ?? [];
    userSkips.set(key, [...list, [startSecs, endSecs]]);
  },

  'library.seriesPrefs': ({ seriesId }) =>
    seriesPrefs.get(seriesId) ?? {
      alwaysSkipIntro: false,
      alwaysSkipRecap: false,
      autoplayNext: true,
    },

  'library.setSeriesPrefs': ({ seriesId, prefs }) => {
    seriesPrefs.set(seriesId, prefs);
  },

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

  /* ── Provider setup. Mirrors aurora_ingest so the wizard behaves the same in a
        browser as it does on Windows, minus the actual network. ────────────── */

  'providers.detect': ({ text }): DetectedSource => {
    const trimmed = text.trim();
    const isHttp = /^https?:\/\//i.test(trimmed);
    const [before, query] = trimmed.split('?');
    if (isHttp && query) {
      const params = new URLSearchParams(query);
      const username = params.get('username') ?? params.get('user');
      const password = params.get('password') ?? params.get('pass');
      if (username && password) {
        const base = (before ?? '').replace(/\/[^/]*$/, '').replace(/\/+$/, '');
        return { kind: 'xtream', url: base, username, password };
      }
    }
    return { kind: 'm3u', url: trimmed, username: null, password: null };
  },

  'providers.validate': ({ draft }): ValidationResult => {
    const base: ValidationResult = {
      ok: false, message: '', detail: null, expiresAt: null, daysUntilExpiry: null,
      maxConnections: null, activeConnections: null, isTrial: false,
      credentialsDetected: false,
    };
    if (!/^https?:\/\//i.test(draft.url.trim())) {
      return { ...base, message: 'That does not look like a URL',
        detail: 'A provider address starts with http:// or https://' };
    }
    // The mock accepts anything well-formed; the shape of the answer is what the
    // wizard is being exercised against.
    if (draft.kind === 'xtream') {
      if (!draft.username || !draft.password) {
        return { ...base, message: 'Username and password are required',
          detail: 'An Xtream panel needs both.' };
      }
      return {
        ...base, ok: true, message: 'Connected',
        expiresAt: Math.floor(Date.now() / 1000) + 41 * 86400,
        daysUntilExpiry: 41, maxConnections: 2, activeConnections: 1,
      };
    }
    return { ...base, ok: true, message: `Found ${fx.channels.length} entries` };
  },

  'providers.save': () => ({ id: 1 }),

  'providers.refresh': async (): Promise<SyncReport> => {
    // Walk the same phases the host emits, and only resolve once they are done —
    // so the wizard's progress UI is genuinely exercised rather than skipped past.
    const phases: IngestProgress[] = [
      { phase: 'fetchingPlaylist', done: 0, total: 0 },
      { phase: 'importingChannels', done: fx.channels.length, total: fx.channels.length },
      { phase: 'importingMovies', done: fx.movies.length, total: fx.movies.length },
      { phase: 'importingSeries', done: fx.series.length, total: fx.series.length },
      { phase: 'fetchingEpg', done: 0, total: 0 },
      { phase: 'matchingEpg', done: 0, total: 0 },
      { phase: 'indexing', done: 0, total: 0 },
      { phase: 'done', done: 1, total: 1 },
    ];
    for (const p of phases) {
      emitIngest(p);
      await new Promise((r) => setTimeout(r, 120));
    }

    return {
      channels: fx.channels.length,
      movies: fx.movies.length,
      series: fx.series.length,
      episodes: fx.episodes.length,
      epgChannels: fx.channels.length,
      epgProgrammes: fx.channels.length * 48,
      channelsMissing: 0,
      epgMatched: fx.channels.length - 2,
      epgUnmatched: fx.channels.slice(-2).map((c) => c.name),
      warnings: [],
    };
  },
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
  const fn = handlers[name] as Handler<K> | undefined;
  if (!fn) throw new Error(`unknown command: ${name}`);
  return await fn(args);
}
