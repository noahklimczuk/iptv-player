/**
 * In-memory IPC transport used when the UI is not hosted by Tauri (docs/DECISIONS.md D4).
 * Mirrors the real host's behaviour closely enough to develop and screenshot every screen.
 */
import type {
  CatalogItem, Channel, CommandArgs, CommandName, CommandResult, DetectedSource,
  ArtworkCacheStatus, ArtworkPrefetchReport, CreditEntry, DvrStorage,
  MetadataReport, MetadataStatus, ParentalSettings, PinOutcome, Profile,
  Recording, RecordingConflict, RecordingRule, Reminder,
  Alternate, FilterCounts, LibraryFilters, PlaylistEntry, PlaylistKind, PlaylistShow,
  GuideSlice, IngestProgress, MarkerKind, Movie, PlaybackAids, PlayerState, Programme,
  Progress, Rail, SearchHit, SearchResults, Series, SeriesPrefs, SkipMarker, SyncReport,
  ProviderCredentials,
  UpdateStatus,
  ValidationResult,
} from '@shared/ipc';
import * as fx from './fixtures';

/**
 * A pretend published build, one patch ahead of whatever this bundle was built from.
 *
 * Mutable so `updates.setAutomatic` actually takes effect in a browser session; the
 * host keeps the real thing in its settings table.
 */
/** Edits made in a browser session, so the form round-trips without a host. */
const mockProviderEdits = new Map<number, { url: string; username: string }>();

const mockUpdates: UpdateStatus = {
  current: '0.1.0',
  latest: {
    version: '0.1.1',
    tag: 'v0.1.1',
    notes: 'Live TV rolls to the next source when a stream dies.\nAdd provider in Settings opens the wizard.',
    pageUrl: 'https://github.com/noahklimczuk/iptv-player/releases/tag/v0.1.1',
    installerUrl: 'https://github.com/noahklimczuk/iptv-player/releases/download/v0.1.1/Aurora-TV-0.1.1-x64-setup.exe',
    installerBytes: 38_767_916,
    publishedAt: '2026-09-23T03:13:28Z',
  },
  available: true,
  automatic: true,
  lastCheckedAt: null,
  releasesUrl: 'https://github.com/noahklimczuk/iptv-player/releases/latest',
};

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

/** Profile state, mirroring aurora-db's defaults. */
const mockProfiles: Profile[] = [
  {
    id: 1, name: 'Me', avatar: 'default', isKids: false, hasPin: false,
    maxAge: null, allowUnrated: true, dailyLimitMin: null,
  },
];
const mockPins = new Map<number, string | null>();
let mockMasterPin: string | null = null;
let mockParental: ParentalSettings = {
  hasMasterPin: false, hideAdult: true, lockSettings: false,
};

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

/* ── Playlist edits and library filters (README §7.3) ──────────────────────── */

let filters: LibraryFilters = { englishOnly: false, hideDuplicates: false };

/** One viewer edit. `null` means the override was cleared. */
interface Edit {
  name?: string | null;
  number?: number | null;
  group?: string | null;
  hidden?: boolean;
}
const edits = new Map<string, Edit>();
const editOf = (kind: PlaylistKind, id: number): Edit =>
  edits.get(`${kind}:${id}`) ?? {};

function putEdit(kind: PlaylistKind, id: number, patch: Edit) {
  edits.set(`${kind}:${id}`, { ...editOf(kind, id), ...patch });
}

/** Channels with the viewer's renames, renumbers, regroups and hides applied. */
function editedChannels(): Channel[] {
  return fx.channels.map((c) => {
    const e = editOf('live', c.id);
    return {
      ...c,
      name: e.name ?? c.name,
      number: e.number ?? c.number,
      group: e.group ?? c.group,
      hidden: e.hidden ?? c.hidden,
      favorite: favorites.has(c.id),
    };
  });
}

const editedMovies = (): Movie[] =>
  fx.movies.map((m) => ({ ...m, title: editOf('movies', m.id).name ?? m.title }));

const editedSeries = (): Series[] =>
  fx.series.map((s) => ({ ...s, title: editOf('series', s.id).name ?? s.title }));

const isHidden = (kind: PlaylistKind, id: number, fallback = false): boolean =>
  editOf(kind, id).hidden ?? fallback;

/**
 * Both filters, applied the way the host applies them
 * (`aurora_db::repo::filtering::LibraryFilter::where_sql`).
 *
 * Duplicate collapsing asks "is there a better copy of this?" rather than grouping, and
 * the better copy has to survive the other filters too — otherwise a Spanish 4K rip
 * would suppress the English HD one it is not allowed to replace.
 */
function applyFilters<T extends { id: number; quality: string | null; lang?: string | null }>(
  items: T[],
  kind: PlaylistKind,
  nameOf: (t: T) => string,
  yearOf: (t: T) => number | null,
): T[] {
  let out = items.filter((t) => !isHidden(kind, t.id, 'hidden' in t ? !!t.hidden : false));
  if (filters.englishOnly) out = out.filter((t) => !t.lang || t.lang === 'en');
  if (!filters.hideDuplicates) return out;

  const eligible = out;
  const better = (a: T, b: T) =>
    fx.qualityRank(a.quality) > fx.qualityRank(b.quality) ||
    (fx.qualityRank(a.quality) === fx.qualityRank(b.quality) && a.id < b.id);
  return out.filter(
    (t) =>
      !eligible.some(
        (o) =>
          o.id !== t.id &&
          fx.matchKey(nameOf(o)) === fx.matchKey(nameOf(t)) &&
          (yearOf(o) ?? 0) === (yearOf(t) ?? 0) &&
          better(o, t),
      ),
  );
}

const visibleChannels = (): Channel[] =>
  applyFilters(editedChannels(), 'live', (c) => c.name, () => null);
const visibleMovies = (): Movie[] =>
  applyFilters(editedMovies(), 'movies', (m) => m.title, (m) => m.year);
const visibleSeries = (): Series[] =>
  applyFilters(editedSeries(), 'series', (s) => s.title, (s) => s.year);

/** Every row of one list, hidden ones included: the editor's own view. */
function allRows(kind: PlaylistKind): PlaylistEntry[] {
  if (kind === 'live') {
    const list = editedChannels();
    return list.map((c) => {
      const original = fx.channels.find((f) => f.id === c.id)!;
      const e = editOf('live', c.id);
      return {
        id: c.id,
        name: c.name,
        providerName: original.name,
        number: c.number,
        group: c.group,
        quality: c.quality,
        lang: c.lang ?? null,
        hidden: c.hidden,
        edited: e.name != null || e.number != null || e.group != null,
        duplicates: list.filter(
          (o) => o.id !== c.id && fx.matchKey(o.name) === fx.matchKey(c.name),
        ).length,
        provider: fx.providers[0]?.name ?? null,
      };
    });
  }
  const list = kind === 'movies' ? editedMovies() : editedSeries();
  const source = kind === 'movies' ? fx.movies : fx.series;
  return list.map((item) => {
    const original = source.find((f) => f.id === item.id)!;
    const e = editOf(kind, item.id);
    return {
      id: item.id,
      name: item.title,
      providerName: original.title,
      number: null,
      group: item.genres[0] ?? null,
      quality: item.quality,
      lang: item.lang ?? null,
      hidden: isHidden(kind, item.id),
      edited: e.name != null,
      duplicates: list.filter(
        (o) =>
          o.id !== item.id &&
          fx.matchKey(o.title) === fx.matchKey(item.title) &&
          (o.year ?? 0) === (item.year ?? 0),
      ).length,
      provider: fx.providers[0]?.name ?? null,
    };
  });
}

interface RowQuery {
  kind: PlaylistKind;
  text?: string;
  group?: string;
  show?: PlaylistShow;
  duplicatesOnly?: boolean;
}

function matchingRows(q: RowQuery): PlaylistEntry[] {
  const text = q.text?.trim().toLowerCase();
  return allRows(q.kind).filter((r) => {
    if (q.show === 'visible' && r.hidden) return false;
    if (q.show === 'hidden' && !r.hidden) return false;
    if (q.group && r.group !== q.group) return false;
    if (q.duplicatesOnly && r.duplicates === 0) return false;
    if (text) {
      const hit =
        r.name.toLowerCase().includes(text) || r.providerName.toLowerCase().includes(text);
      if (!hit) return false;
    }
    return true;
  });
}

function filterCounts(kind: PlaylistKind): FilterCounts {
  const rows = allRows(kind).filter((r) => !r.hidden);
  const before = filters;
  filters = { englishOnly: false, hideDuplicates: true };
  const kept =
    kind === 'live'
      ? visibleChannels().length
      : kind === 'movies'
        ? visibleMovies().length
        : visibleSeries().length;
  filters = before;
  return {
    total: rows.length,
    nonEnglish: rows.filter((r) => r.lang && r.lang !== 'en').length,
    untagged: rows.filter((r) => !r.lang).length,
    duplicates: rows.length - kept,
  };
}

/* ── Rails (README §8.2) ───────────────────────────────────────────────────── */

const asMovie = (m: Movie): CatalogItem => ({ kind: 'movie', ...m });
const asSeries = (s: Series): CatalogItem => ({ kind: 'series', ...s });

function buildRails(): Rail[] {
  // The home page is a view of the library, so it is the filtered library it views.
  const shownMovies = visibleMovies();
  const shownSeries = visibleSeries();
  const byRating = [...shownMovies].sort((a, b) => (b.rating ?? 0) - (a.rating ?? 0));
  const byAdded = [...shownMovies].sort((a, b) => (b.addedAt ?? 0) - (a.addedAt ?? 0));
  const continueItems = [...progress.values()]
    .filter((p) => !p.completed && p.positionSecs > 60)
    .sort((a, b) => b.updatedAt - a.updatedAt)
    .map((p) => {
      if (p.itemKind === 'movie') {
        const m = shownMovies.find((x) => x.id === p.itemId);
        return m ? asMovie(m) : null;
      }
      const ep = fx.episodes.find((x) => x.id === p.itemId);
      const s = ep && shownSeries.find((x) => x.id === ep.seriesId);
      return s ? asSeries(s) : null;
    })
    .filter((x): x is CatalogItem => x !== null);

  const genreRail = (g: string): Rail => ({
    id: `genre-${g}`,
    kind: 'genre',
    title: g,
    items: shownMovies.filter((m) => m.genres.includes(g)).slice(0, 18).map(asMovie),
  });

  const rails: Rail[] = [
    { id: 'continue', kind: 'continueWatching', title: 'Continue Watching', items: continueItems },
    {
      id: 'upnext', kind: 'upNext', title: 'Up Next',
      items: shownSeries.slice(6, 20).map(asSeries),
    },
    {
      id: 'top10', kind: 'top10', title: 'Top 10 Movies Today',
      items: byRating.slice(0, 10).map(asMovie),
    },
    { id: 'recent', kind: 'recentlyAdded', title: 'Recently Added', items: byAdded.slice(0, 20).map(asMovie) },
    {
      id: 'because', kind: 'becauseYouWatched',
      title: `Because you watched ${shownMovies[3]!.title}`,
      reason: shownMovies[3]!.title,
      items: shownMovies.slice(60, 80).map(asMovie),
    },
    {
      id: 'mylist', kind: 'myList', title: 'My List',
      items: [...myList].flatMap((key): CatalogItem[] => {
        const [kind, rawId] = key.split(':');
        const id = Number(rawId);
        if (kind === 'movie') {
          const m = shownMovies.find((x) => x.id === id);
          return m ? [asMovie(m)] : [];
        }
        const s = shownSeries.find((x) => x.id === id);
        return s ? [asSeries(s)] : [];
      }),
    },
    { id: 'series', kind: 'trending', title: 'Trending Series', items: shownSeries.slice(0, 18).map(asSeries) },
    {
      id: '4k', kind: 'fourK', title: '4K & HDR',
      items: shownMovies.filter((m) => m.quality === '4K').slice(0, 18).map(asMovie),
    },
    {
      id: 'acclaimed', kind: 'acclaimed', title: 'Critically Acclaimed',
      items: byRating.slice(10, 28).map(asMovie),
    },
    genreRail('Sci-Fi'),
    genreRail('Thriller'),
    {
      id: 'short', kind: 'shortAndSweet', title: 'Short & Sweet',
      items: shownMovies.filter((m) => (m.runtimeMins ?? 999) < 95).slice(0, 18).map(asMovie),
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

/**
 * How far back each channel's catch-up goes.
 *
 * The host reads this per channel from the provider; the IPC contract does not carry
 * it, because nothing in the UI can usefully act on it before the request is made. So
 * the mock keeps its own, the way a real provider offers different depths per channel,
 * and refuses the same way the host does.
 */
function catchupDays(channelId: number): number {
  return channelId % 4 === 0 ? 2 : 7;
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
  // Search is a way into the library, not a way around the filters.
  const shownChannels = visibleChannels();
  const shownMovies = visibleMovies();
  const shownSeries = visibleSeries();
  for (const ch of shownChannels) {
    for (const p of programmes(ch, now - 3600, now + 86400)) {
      if (!p.title.toLowerCase().includes(q)) continue;
      if (p.stop <= now) continue;
      const target = p.start <= now ? onNow : upcoming;
      // refId is the channel, matching the host: a programme hit is only useful if it
      // can be watched, and only the channel can be tuned.
      if (target.length < 8) target.push(hit('programme', ch.id, p.title, ch.name));
    }
  }
  const people = new Set<string>();
  for (const m of shownMovies) {
    for (const c of m.cast) if (c.toLowerCase().includes(q)) people.add(c);
  }

  return {
    channels: shownChannels.filter((c) => c.name.toLowerCase().includes(q)).slice(0, 8)
      .map((c) => hit('channel', c.id, c.name, c.group)),
    onNow,
    upcoming: upcoming.slice(0, 8),
    movies: shownMovies.filter((m) => m.title.toLowerCase().includes(q)).slice(0, 12)
      .map((m) => hit('movie', m.id, m.title, m.year ? String(m.year) : null)),
    series: shownSeries.filter((s) => s.title.toLowerCase().includes(q)).slice(0, 12)
      .map((s) => hit('series', s.id, s.title, s.year ? String(s.year) : null)),
    people: [...people].slice(0, 8).map((p, i) => hit('person', i, p, null)),
  };
}


/* ── DVR ────────────────────────────────────────────────────────────────────
   Mirrors aurora_core::dvr and aurora_db::repo::dvr: the same padding, the same
   duplicate rule, the same conflict arithmetic. The recorder is simulated on a
   timer so the recordings page is live in a browser too. ───────────────────── */

const PRE_PADDING = 60;
const POST_PADDING = 300;
const MAX_CONCURRENT = 2;
const DVR_QUOTA_BYTES = 200 * 1024 ** 3;
/** A plausible 8 Mb/s transport stream, for the simulated recorder. */
const BYTES_PER_SEC = 1024 * 1024;

let nextRecordingId = 1;
let nextRuleId = 1;
let nextReminderId = 1;
const recordings: Recording[] = [];
const dvrRules: RecordingRule[] = [];
const reminders: Reminder[] = [];

const dvrListeners = new Set<(t: { started: number[]; completed: number[]; failed: [number, string][]; stalled: number[] }) => void>();

export function onDvrTick(fn: (t: { started: number[]; completed: number[]; failed: [number, string][]; stalled: number[] }) => void) {
  dvrListeners.add(fn);
  return () => dvrListeners.delete(fn);
}

function withPadding(start: number, stop: number, pre: number, post: number): [number, number] {
  const paddedStart = Math.max(0, start - Math.max(0, pre));
  const paddedStop = stop + Math.max(0, post);
  return paddedStop <= paddedStart ? [start, Math.max(stop, start + 1)] : [paddedStart, paddedStop];
}

function scheduleRecording(a: {
  channelId: number;
  title: string;
  subTitle?: string | null;
  season?: number | null;
  episode?: number | null;
  airStart: number;
  airStop: number;
  prePaddingSecs?: number;
  postPaddingSecs?: number;
  priority?: number;
  ruleId?: number | null;
}): number | null {
  const [start, stop] = withPadding(
    a.airStart, a.airStop,
    a.prePaddingSecs ?? PRE_PADDING,
    a.postPaddingSecs ?? POST_PADDING,
  );
  // Same unique index the schema carries: (channelId, start, title).
  const clash = recordings.some(
    (r) => r.channelId === a.channelId && r.start === start && r.title === a.title,
  );
  if (clash) return null;

  const id = nextRecordingId++;
  recordings.push({
    id,
    channelId: a.channelId,
    ruleId: a.ruleId ?? null,
    title: a.title,
    subTitle: a.subTitle ?? null,
    season: a.season ?? null,
    episode: a.episode ?? null,
    airStart: a.airStart,
    airStop: a.airStop,
    start,
    stop,
    state: 'scheduled',
    reason: null,
    priority: a.priority ?? 0,
    filePath: null,
    bytes: 0,
    durationSecs: 0,
    keep: false,
    watched: false,
  });
  return id;
}

/** Seed a library that looks lived-in: things recorded, one in flight, some coming up. */
function seedDvr() {
  const now = Math.floor(Date.now() / 1000);
  const day = 86400;
  const finished: [string, number, number, number, boolean, string | null][] = [
    ['The Gilded Circuit', -2 * day, 3600, 2, true, null],
    ['Nightfall Sessions', -1 * day - 7200, 5400, 0, false, null],
    ['Harbour Lights', -1 * day, 1800, 0, true, null],
    ['Signal to Noise', -3 * day, 3600, 0, false, 'The provider closed the connection early'],
  ];
  for (const [title, offset, len, chIndex, watched, reason] of finished) {
    const airStart = now + offset;
    const id = scheduleRecording({
      channelId: fx.channels[chIndex]?.id ?? 1, title, airStart, airStop: airStart + len,
    });
    const rec = recordings.find((r) => r.id === id);
    if (!rec) continue;
    rec.state = 'completed';
    rec.reason = reason;
    rec.durationSecs = len + PRE_PADDING + POST_PADDING;
    rec.bytes = rec.durationSecs * BYTES_PER_SEC;
    rec.watched = watched;
    rec.filePath = `C:\\Users\\You\\Videos\\Aurora\\${title}.ts`;
  }

  // One failed, so the page has to say why rather than just showing nothing.
  const failedStart = now - 4 * day;
  const failedId = scheduleRecording({
    channelId: fx.channels[1]?.id ?? 1, title: 'Cross Harbour Derby',
    airStart: failedStart, airStop: failedStart + 7200,
  });
  const failed = recordings.find((r) => r.id === failedId);
  if (failed) {
    failed.state = 'failed';
    failed.reason = 'Aurora was not running when this was due';
  }

  // One in flight right now.
  const liveStart = now - 900;
  const liveId = scheduleRecording({
    channelId: fx.channels[0]?.id ?? 1, title: 'The Evening Report',
    airStart: liveStart, airStop: liveStart + 3600,
  });
  const live = recordings.find((r) => r.id === liveId);
  if (live) {
    live.state = 'recording';
    live.bytes = 900 * BYTES_PER_SEC;
  }

  // And a few upcoming, one of which is a conflict with two others.
  const tonight = now + 3 * 3600;
  scheduleRecording({
    channelId: fx.channels[0]?.id ?? 1, title: 'Late Kickoff',
    airStart: tonight, airStop: tonight + 7200, priority: 10,
  });
  scheduleRecording({
    channelId: fx.channels[1]?.id ?? 2, title: 'The Gilded Circuit',
    airStart: tonight + 600, airStop: tonight + 4200, season: 2, episode: 4,
  });
  scheduleRecording({
    channelId: fx.channels[2]?.id ?? 3, title: 'Midnight Movie',
    airStart: tonight + 1200, airStop: tonight + 8400,
  });

  dvrRules.push({
    id: nextRuleId++, title: 'The Gilded Circuit', channelId: null, newOnly: true,
    weekdays: null, aroundLocalMinute: null, prePaddingSecs: PRE_PADDING,
    postPaddingSecs: POST_PADDING, keepEpisodes: 5, priority: 0, enabled: true, scheduled: 2,
  });
  dvrRules.push({
    id: nextRuleId++, title: 'Nightfall Sessions', channelId: fx.channels[0]?.id ?? 1,
    newOnly: false, weekdays: [0, 1, 2, 3, 4], aroundLocalMinute: 22 * 60,
    prePaddingSecs: PRE_PADDING, postPaddingSecs: POST_PADDING, keepEpisodes: null,
    priority: 0, enabled: true, scheduled: 1,
  });

  reminders.push({
    id: nextReminderId++, channelId: fx.channels[3]?.id ?? 1,
    title: 'The Championship Final', start: now + 5 * 3600, leadSecs: 300,
  });
}
seedDvr();

/** Advance the simulated recorder, exactly as `Dvr::tick` advances the real one. */
function dvrTick() {
  const now = Math.floor(Date.now() / 1000);
  const started: number[] = [];
  const completed: number[] = [];
  const failed: [number, string][] = [];

  for (const r of recordings) {
    if (r.state === 'scheduled' && r.start <= now && r.stop > now) {
      const busy = recordings.filter((o) => o.state === 'recording').length;
      if (busy >= MAX_CONCURRENT) {
        r.state = 'skipped';
        r.reason = 'too many recordings at once for this subscription';
        failed.push([r.id, r.reason]);
        continue;
      }
      r.state = 'recording';
      started.push(r.id);
    } else if (r.state === 'scheduled' && r.stop <= now) {
      r.state = 'failed';
      r.reason = 'Aurora was not running when this was due';
      failed.push([r.id, r.reason]);
    } else if (r.state === 'recording') {
      r.bytes = Math.max(0, now - r.start) * BYTES_PER_SEC;
      if (r.stop <= now) {
        r.state = 'completed';
        r.durationSecs = r.stop - r.start;
        r.filePath = `C:\\Users\\You\\Videos\\Aurora\\${r.title}.ts`;
        completed.push(r.id);
      }
    }
  }

  if (started.length || completed.length || failed.length) {
    const report = { started, completed, failed, stalled: [] as number[] };
    for (const fn of dvrListeners) fn(report);
  }
}
setInterval(dvrTick, 5000);

function findConflicts(maxConcurrent: number): RecordingConflict[] {
  const now = Math.floor(Date.now() / 1000);
  const slots = recordings
    .filter((r) => (r.state === 'scheduled' || r.state === 'recording') && r.stop > now)
    .sort((a, b) => a.start - b.start);

  // Sweep the boundaries, same as aurora_core::dvr::find_conflicts.
  const edges = [...new Set(slots.flatMap((s) => [s.start, s.stop]))].sort((a, b) => a - b);
  const out: RecordingConflict[] = [];
  for (let i = 0; i < edges.length - 1; i += 1) {
    const from = edges[i]!;
    const to = edges[i + 1]!;
    const overlapping = slots.filter((s) => s.start < to && from < s.stop);
    if (overlapping.length <= maxConcurrent) continue;
    const last = out[out.length - 1];
    if (last && last.stop === from && last.overBy === overlapping.length - maxConcurrent) {
      last.stop = to;
      continue;
    }
    out.push({
      start: from,
      stop: to,
      slotIds: [...overlapping]
        .sort((a, b) => b.priority - a.priority || a.start - b.start)
        .map((s) => s.id),
      overBy: overlapping.length - maxConcurrent,
    });
  }
  return out;
}


/* ── Metadata enrichment ────────────────────────────────────────────────────
   Mirrors aurora_ingest::enrich: a bounded batch per call, a no-match recorded
   so the same title is never asked about twice, and a stored key the UI can see
   the presence of but never the value of. ─────────────────────────────────── */

let metadataKey: string | null = 'demo-key-not-a-real-one';
type EnrichState = 'matched' | 'nomatch' | 'failed';
const enriched = new Map<string, EnrichState>();
const mockCredits = new Map<string, CreditEntry[]>();

const CAST_POOL = [
  'Ines Lindqvist', 'Marcus Oyelaran', 'Priya Raghunathan', 'Tomas Berg',
  'Adaeze Nwosu', 'Jonah Whitfield', 'Elif Demirci', 'Rafael Monteiro',
];
const CREW_POOL: [string, string][] = [
  ['Dana Kovalenko', 'Director'],
  ['Sam Okonkwo', 'Screenplay'],
  ['Mira Haddad', 'Original Music Composer'],
];

/** Deterministic, so a screenshot of the same title is the same twice. */
function creditsFor(kind: 'movie' | 'series', id: number): CreditEntry[] {
  const key = `${kind}:${id}`;
  const existing = mockCredits.get(key);
  if (existing) return existing;

  const cast: CreditEntry[] = Array.from({ length: 5 }, (_, i) => {
    const name = CAST_POOL[(id + i) % CAST_POOL.length]!;
    return {
      personId: ((id + i) % CAST_POOL.length) + 1,
      name,
      profilePath: null,
      role: `${name.split(' ')[0]}'s character`,
      isCast: true,
    };
  });
  const crew: CreditEntry[] = CREW_POOL.map(([name, job], i) => ({
    personId: 100 + i,
    name,
    profilePath: null,
    role: job,
    isCast: false,
  }));
  const all = [...cast, ...crew];
  mockCredits.set(key, all);
  return all;
}

function enrichmentCoverage(kind: 'movie' | 'series') {
  const total = kind === 'movie' ? fx.movies.length : fx.series.length;
  let matched = 0;
  let noMatch = 0;
  let failed = 0;
  for (const [key, state] of enriched) {
    if (!key.startsWith(`${kind}:`)) continue;
    if (state === 'matched') matched += 1;
    else if (state === 'nomatch') noMatch += 1;
    else failed += 1;
  }
  return { total, matched, noMatch, failed };
}

// Most of the demo library is already enriched, with a few left so the button does
// something visible when pressed.
for (const m of fx.movies.slice(0, Math.max(0, fx.movies.length - 6))) {
  enriched.set(`movie:${m.id}`, 'matched');
}
for (const s of fx.series.slice(0, Math.max(0, fx.series.length - 3))) {
  enriched.set(`series:${s.id}`, 'matched');
}

const metadataListeners = new Set<(p: { done: number; total: number }) => void>();
export function onMetadataProgress(fn: (p: { done: number; total: number }) => void) {
  metadataListeners.add(fn);
  return () => metadataListeners.delete(fn);
}


/* ── Artwork cache ──────────────────────────────────────────────────────────
   There is no disk in a browser, so this models the counts the panel shows and
   nothing else. On the host the same commands drive a real content-addressed
   store under the data directory. ─────────────────────────────────────────── */

const ARTWORK_MAX_BYTES = 2 * 1024 ** 3;
/** A plausible average across w342 posters and w1280 backdrops. */
const ARTWORK_AVG_BYTES = 90 * 1024;
let artworkFiles = Math.round((fx.movies.length + fx.series.length) * 1.4);

const artworkListeners = new Set<(p: { done: number; total: number }) => void>();
export function onArtworkProgress(fn: (p: { done: number; total: number }) => void) {
  artworkListeners.add(fn);
  return () => artworkListeners.delete(fn);
}

/* ── Command dispatch ──────────────────────────────────────────────────────── */

type Handler<K extends CommandName> = (
  a: CommandArgs<K>,
) => CommandResult<K> | Promise<CommandResult<K>>;

const handlers: { [K in CommandName]: Handler<K> } = {
  'library.rails': () => buildRails(),
  'library.movies': ({ sort, limit, offset, genre }) => {
    const all = visibleMovies();
    let list = genre ? all.filter((m) => m.genres.includes(genre)) : [...all];
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
    const all = visibleSeries();
    const list = genre ? all.filter((s) => s.genres.includes(genre)) : all;
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
    [...new Set([...visibleMovies(), ...visibleSeries()].flatMap((m) => m.genres))].sort(),

  'channels.list': ({ group, favoritesOnly } = {}) =>
    visibleChannels().filter(
      (c) => (!group || c.group === group) && (!favoritesOnly || favorites.has(c.id)),
    ),
  'channels.groups': () => {
    const counts = new Map<string, number>();
    for (const c of visibleChannels()) {
      if (c.group) counts.set(c.group, (counts.get(c.group) ?? 0) + 1);
    }
    return [...counts].map(([name, count]) => ({ name, count }));
  },
  // Tuning by number is a lookup, not a list: a channel you can name is a channel you
  // can watch, whatever the filters hide from the list.
  'channels.byNumber': ({ number }) =>
    editedChannels().find((c) => c.number === number) ?? null,

  'epg.gridSlice': ({ from, to, channelIds }): GuideSlice => {
    const all = visibleChannels();
    const chans = channelIds.length ? all.filter((c) => channelIds.includes(c.id)) : all;
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

  'library.filters': () => filters,
  'library.setFilters': (next) => {
    filters = { ...next };
    return filters;
  },
  'library.filterCounts': ({ kind }) => filterCounts(kind),
  'library.alternates': ({ kind, id }): Alternate[] => {
    const rows = allRows(kind);
    const me = rows.find((r) => r.id === id);
    if (!me) return [];
    return rows
      .filter((r) => fx.matchKey(r.name) === fx.matchKey(me.name) && !r.hidden)
      .sort((a, b) => fx.qualityRank(b.quality) - fx.qualityRank(a.quality) || a.id - b.id)
      .map((r) => ({ id: r.id, name: r.name, quality: r.quality, provider: r.provider }));
  },

  'playlist.list': ({ limit, offset, ...q }) => {
    const rows = matchingRows(q);
    return { rows: rows.slice(offset, offset + limit), total: rows.length };
  },
  'playlist.groups': ({ kind }) => {
    const counts = new Map<string, number>();
    for (const r of allRows(kind)) {
      if (r.group) counts.set(r.group, (counts.get(r.group) ?? 0) + 1);
    }
    return [...counts].map(([name, count]) => ({ name, count })).sort((a, b) =>
      a.name.localeCompare(b.name));
  },
  'playlist.update': ({ kind, id, patch }) => {
    const edit: Edit = {};
    // An empty string or a zero clears the override, exactly as the host reads them.
    if (patch.name !== undefined) edit.name = patch.name.trim() || null;
    if (patch.number !== undefined) edit.number = patch.number === 0 ? null : patch.number;
    if (patch.group !== undefined) edit.group = patch.group.trim() || null;
    if (patch.hidden !== undefined) edit.hidden = patch.hidden;
    putEdit(kind, id, edit);
  },
  'playlist.setHidden': ({ kind, ids, hidden }) => {
    for (const id of ids) putEdit(kind, id, { hidden });
    return ids.length;
  },
  'playlist.hideMatching': ({ hidden, ...q }) => {
    const rows = matchingRows(q);
    for (const r of rows) putEdit(q.kind, r.id, { hidden });
    return rows.length;
  },
  'playlist.reset': ({ kind, ids }) => {
    for (const id of ids) edits.delete(`${kind}:${id}`);
    return ids.length;
  },

  'search.query': ({ text }) => search(text),

  'player.playCatchup': ({ channelId, start, stop }) => {
    const ch = fx.channels.find((c) => c.id === channelId);
    if (!ch) throw new Error(`unknown channel ${channelId}`);
    // Mirrors the host's refusals, which are three different problems and say so.
    if (!ch.hasCatchup) throw new Error(`${ch.name} does not offer catch-up`);
    const now = Math.floor(Date.now() / 1000);
    if (start > now) throw new Error(`${ch.name} has not aired yet`);
    const days = catchupDays(ch.id);
    if (start < now - days * 86400) {
      throw new Error(`That programme is outside ${ch.name}'s ${days}-day catch-up window`);
    }

    const programme = programmes(ch, start - 1, stop + 1).find((p) => p.start === start);
    return setPlayer({
      status: 'playing',
      // Catch-up is a recording served back: seekable, with a real duration, so the
      // OSD shows a scrubber rather than a live edge.
      isLive: false,
      channelId,
      itemKind: 'live',
      itemId: channelId,
      title: ch.name,
      subtitle: programme?.title ?? null,
      positionSecs: 0,
      durationSecs: Math.max(1, stop - start),
      error: null,
    });
  },

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

  /* ── Profiles (README §11) ────────────────────────────────────────────── */

  'profiles.list': () => mockProfiles,
  'profiles.create': ({ name, avatar, isKids, maxAge, dailyLimitMin }) => {
    const id = Math.max(0, ...mockProfiles.map((p) => p.id)) + 1;
    mockProfiles.push({
      id, name, avatar: avatar ?? null, isKids, hasPin: false,
      maxAge: maxAge ?? null,
      // A kids profile blocks unrated content by default, as the host does.
      allowUnrated: !isKids,
      dailyLimitMin: dailyLimitMin ?? null,
    });
    return id;
  },
  'profiles.delete': ({ profileId }) => {
    if (mockProfiles.length <= 1) return false;
    const i = mockProfiles.findIndex((p) => p.id === profileId);
    if (i < 0) return false;
    mockProfiles.splice(i, 1);
    return true;
  },
  'profiles.rename': ({ profileId, name }) => {
    const p = mockProfiles.find((x) => x.id === profileId);
    if (p) p.name = name;
  },
  'profiles.setLimits': ({ profileId, maxAge, allowUnrated, dailyLimitMin }) => {
    const p = mockProfiles.find((x) => x.id === profileId);
    if (p) Object.assign(p, { maxAge, allowUnrated, dailyLimitMin });
  },
  'profiles.setPin': ({ profileId, pin }) => {
    const p = mockProfiles.find((x) => x.id === profileId);
    if (p) { p.hasPin = pin !== null; mockPins.set(profileId, pin); }
  },
  'profiles.verifyPin': ({ profileId, pin }): PinOutcome => {
    const stored = profileId == null ? mockMasterPin : mockPins.get(profileId) ?? null;
    if (stored == null) return 'notRequired';
    return stored === pin ? 'ok' : { wrong: { remaining: 4 } };
  },
  'profiles.parental': (): ParentalSettings => mockParental,
  'profiles.setParental': ({ hideAdult, lockSettings, masterPin }) => {
    mockParental = {
      hideAdult, lockSettings,
      hasMasterPin: masterPin !== undefined ? masterPin !== null : mockParental.hasMasterPin,
    };
    if (masterPin !== undefined) mockMasterPin = masterPin;
  },
  'profiles.watchedToday': () => 42,

  /* ── DVR ────────────────────────────────────────────────────────────────── */

  'dvr.schedule': (a) => scheduleRecording({ ...a, priority: 10 }),
  'dvr.list': ({ state }) => {
    const rows = state ? recordings.filter((r) => r.state === state) : [...recordings];
    return rows
      .sort((a, b) => a.start - b.start)
      .map((r) => ({ ...r, liveBytes: r.state === 'recording' ? r.bytes : null }));
  },
  'dvr.cancel': ({ id }) => {
    const i = recordings.findIndex((r) => r.id === id);
    if (i < 0) return false;
    if (recordings[i]!.state === 'completed') {
      throw new Error('recording already finished — delete it instead');
    }
    recordings.splice(i, 1);
    return true;
  },
  'dvr.delete': ({ id }) => {
    const i = recordings.findIndex((r) => r.id === id);
    if (i < 0) return false;
    recordings.splice(i, 1);
    return true;
  },
  'dvr.setKeep': ({ id, value }) => {
    const r = recordings.find((x) => x.id === id);
    if (r) r.keep = value;
  },
  'dvr.setWatched': ({ id, value }) => {
    const r = recordings.find((x) => x.id === id);
    if (r) r.watched = value;
  },
  'dvr.conflicts': () => findConflicts(MAX_CONCURRENT),
  'dvr.rules': () => [...dvrRules].sort((a, b) => a.title.localeCompare(b.title)),
  'dvr.createRule': (a) => {
    const id = nextRuleId++;
    dvrRules.push({
      id,
      title: a.title,
      channelId: a.channelId ?? null,
      newOnly: a.newOnly,
      weekdays: a.weekdays && a.weekdays.length ? a.weekdays : null,
      aroundLocalMinute: a.aroundLocalMinute ?? null,
      prePaddingSecs: a.prePaddingSecs ?? PRE_PADDING,
      postPaddingSecs: a.postPaddingSecs ?? POST_PADDING,
      keepEpisodes: a.keepEpisodes ?? null,
      priority: 0,
      enabled: true,
      scheduled: 0,
    });
    return id;
  },
  'dvr.deleteRule': ({ id }) => {
    const i = dvrRules.findIndex((r) => r.id === id);
    if (i < 0) return false;
    dvrRules.splice(i, 1);
    // Episodes the rule already scheduled stay, as they do on the host.
    for (const r of recordings) if (r.ruleId === id) r.ruleId = null;
    return true;
  },
  'dvr.setRuleEnabled': ({ id, value }) => {
    const r = dvrRules.find((x) => x.id === id);
    if (r) r.enabled = value;
  },
  'dvr.reminders': () => [...reminders].sort((a, b) => a.start - b.start),
  'dvr.addReminder': ({ channelId, title, start, leadSecs }) => {
    if (reminders.some((r) => r.channelId === channelId && r.start === start && r.title === title)) {
      return null;
    }
    const id = nextReminderId++;
    reminders.push({ id, channelId, title, start, leadSecs: leadSecs ?? 120 });
    return id;
  },
  'dvr.removeReminder': ({ id }) => {
    const i = reminders.findIndex((r) => r.id === id);
    if (i < 0) return false;
    reminders.splice(i, 1);
    return true;
  },
  'dvr.storage': (): DvrStorage => {
    const done = recordings.filter((r) => r.state === 'completed');
    const used = done.reduce((n, r) => n + r.bytes, 0);
    let over = used - DVR_QUOTA_BYTES;
    const prunable: number[] = [];
    // Watched first, then oldest: the same order the host prunes in.
    for (const r of [...done].filter((r) => !r.keep)
      .sort((a, b) => Number(b.watched) - Number(a.watched) || a.start - b.start)) {
      if (over <= 0) break;
      over -= r.bytes;
      prunable.push(r.id);
    }
    return {
      folder: 'C:\\Users\\You\\Videos\\Aurora',
      usedBytes: used,
      quotaBytes: DVR_QUOTA_BYTES,
      prunable,
      maxConcurrent: MAX_CONCURRENT,
    };
  },

  /* ── Metadata enrichment ────────────────────────────────────────────────── */

  'metadata.status': (): MetadataStatus => ({
    hasKey: metadataKey !== null,
    keyIsBuiltIn: false,
    // The browser mock has nowhere durable to put a key, and says so rather than
    // pretending, exactly as the in-memory credential store does on the host.
    keyIsPersistent: false,
    movies: enrichmentCoverage('movie'),
    series: enrichmentCoverage('series'),
  }),

  'metadata.setKey': ({ key }) => {
    metadataKey = key && key.trim() ? key.trim() : null;
  },

  'metadata.run': async ({ batch, movies = true, series = true }): Promise<MetadataReport> => {
    if (!metadataKey) {
      throw new Error('No metadata API key is set. Add one in Settings to fetch artwork and cast.');
    }
    const limit = batch ?? 50;
    const work: [string, 'movie' | 'series'][] = [];
    if (movies) {
      for (const m of fx.movies) {
        if (!enriched.has(`movie:${m.id}`)) work.push([`movie:${m.id}`, 'movie']);
      }
    }
    if (series) {
      for (const s of fx.series) {
        if (!enriched.has(`series:${s.id}`)) work.push([`series:${s.id}`, 'series']);
      }
    }

    const slice = work.slice(0, limit);
    const report: MetadataReport = { matched: 0, noMatch: 0, failed: 0 };
    for (let i = 0; i < slice.length; i += 1) {
      for (const fn of metadataListeners) fn({ done: i, total: slice.length });
      await new Promise((r) => setTimeout(r, 60));
      const [key] = slice[i]!;
      // Every fifth title has no match, so the "not everything is found" case is
      // reachable in the demo rather than being a state nobody ever sees.
      const outcome: EnrichState = i % 5 === 4 ? 'nomatch' : 'matched';
      enriched.set(key, outcome);
      if (outcome === 'matched') report.matched += 1;
      else report.noMatch += 1;
    }
    for (const fn of metadataListeners) fn({ done: slice.length, total: slice.length });
    return report;
  },

  'metadata.credits': ({ kind, id }) => creditsFor(kind, id),

  'metadata.rematch': ({ kind, id }) => {
    enriched.delete(`${kind}:${id}`);
    mockCredits.delete(`${kind}:${id}`);
  },

  'artwork.status': (): ArtworkCacheStatus => ({
    folder: 'C:\\Users\\You\\AppData\\Local\\Aurora TV\\artwork',
    files: artworkFiles,
    usedBytes: artworkFiles * ARTWORK_AVG_BYTES,
    maxBytes: ARTWORK_MAX_BYTES,
  }),

  'artwork.prefetch': async ({ limit }): Promise<ArtworkPrefetchReport> => {
    const wanted = Math.round((fx.movies.length + fx.series.length) * 2.6);
    const missing = Math.max(0, Math.min(limit ?? 500, wanted - artworkFiles));
    for (let i = 0; i < missing; i += 1) {
      if (i % 10 === 0) {
        for (const fn of artworkListeners) fn({ done: i, total: missing });
        await new Promise((r) => setTimeout(r, 20));
      }
    }
    for (const fn of artworkListeners) fn({ done: missing, total: missing });
    // One URL in twenty is dead, so the "not everything downloads" case is reachable.
    const failed = Math.floor(missing / 20);
    artworkFiles += missing - failed;
    return { downloaded: missing - failed, cached: artworkFiles - missing, failed, evicted: 0 };
  },

  'artwork.clear': () => {
    const removed = artworkFiles;
    artworkFiles = 0;
    return removed;
  },

  /* ── Updates. The mock is deliberately "an update is available": the interesting
        state is the one with something in it, and a browser session is where that
        gets designed and screenshotted. ──────────────────────────────────────── */

  'updates.check': (args): UpdateStatus => {
    if (args?.force) mockUpdates.lastCheckedAt = Math.floor(Date.now() / 1000);
    return { ...mockUpdates };
  },

  'updates.setAutomatic': ({ enabled }) => {
    mockUpdates.automatic = enabled;
    return enabled;
  },

  'updates.openReleases': () => {
    // No host to ask, and a browser tab opening itself during a Playwright run would
    // be a nuisance rather than a feature.
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
    // A bare host has no credentials to find, but its shape says it is a panel root —
    // mirrors aurora_ingest::source::looks_like_panel_root.
    const withoutSlash = trimmed.replace(/\/+$/, '');
    const afterScheme = withoutSlash.split('://')[1] ?? '';
    if (isHttp && !withoutSlash.includes('?') && afterScheme && !afterScheme.includes('/')) {
      return { kind: 'xtream', url: withoutSlash, username: null, password: null };
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

  'providers.credentials': ({ providerId }): ProviderCredentials => {
    const p = fx.providers.find((x) => x.id === providerId) ?? fx.providers[0]!;
    return {
      id: p.id,
      name: p.name,
      kind: p.kind,
      url: mockProviderEdits.get(p.id)?.url ?? 'http://panel.example.com',
      username: mockProviderEdits.get(p.id)?.username ?? 'example-user',
      // Invented, like everything else in the fixtures (README §24).
      password: 'example-password',
      passwordIsPersistent: true,
    };
  },

  'providers.update': ({ providerId, draft }) => {
    mockProviderEdits.set(providerId, { url: draft.url, username: draft.username ?? '' });
    const p = fx.providers.find((x) => x.id === providerId);
    if (p) p.name = draft.name;
    return true;
  },

  'providers.delete': ({ providerId }) => {
    const at = fx.providers.findIndex((x) => x.id === providerId);
    if (at < 0) return false;
    fx.providers.splice(at, 1);
    return true;
  },

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
