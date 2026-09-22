/**
 * Synthetic library used by the mock IPC transport (docs/DECISIONS.md D4).
 *
 * README §24: no real playlist, provider, or credential may appear anywhere in this repo,
 * including fixtures. Every title here is invented, every host is example.com, and the
 * generator is seeded so screenshots and tests are reproducible.
 */
import type {
  Channel, Episode, Movie, Programme, Provider, Series,
} from '@shared/ipc';

/** Deterministic PRNG (mulberry32) — same library every run. */
function rng(seed: number) {
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rand = rng(20260922);
const pick = <T,>(xs: readonly T[]): T => xs[Math.floor(rand() * xs.length)]!;
const int = (lo: number, hi: number) => lo + Math.floor(rand() * (hi - lo + 1));

/* ── Invented title vocabulary ─────────────────────────────────────────────── */

const ADJ = ['Silent', 'Crimson', 'Hollow', 'Northern', 'Last', 'Broken', 'Golden',
  'Distant', 'Velvet', 'Iron', 'Pale', 'Second', 'Endless', 'Quiet', 'Burning',
  'Glass', 'Salt', 'Winter', 'Amber', 'Wandering'] as const;
const NOUN = ['Harbour', 'Signal', 'Orchard', 'Meridian', 'Lantern', 'Ledger', 'Tide',
  'Cartographer', 'Archive', 'Hour', 'Passage', 'Foundry', 'Almanac', 'Reckoning',
  'Cathedral', 'Circuit', 'Wilderness', 'Interval', 'Observatory', 'Undertow'] as const;
const SUFFIX = ['', '', '', '', ': Aftermath', ': Origins', ' II', ': The Long Road'] as const;

const GENRES = ['Action', 'Drama', 'Thriller', 'Sci-Fi', 'Comedy', 'Documentary',
  'Horror', 'Romance', 'Crime', 'Animation', 'Fantasy', 'Mystery'] as const;
const CERTS = ['U', 'PG', '12', '15', '18'] as const;
const FIRST = ['Mara', 'Idris', 'Noor', 'Theo', 'Lena', 'Kwame', 'Sora', 'Ines',
  'Rafa', 'Yuki', 'Amara', 'Dov', 'Petra', 'Cyrus', 'Nadia', 'Otto'] as const;
const LAST = ['Vance', 'Okonjo', 'Lindqvist', 'Marchetti', 'Haddad', 'Novak',
  'Ferreira', 'Aoki', 'Bello', 'Strand', 'Kovac', 'Reyes'] as const;

const title = () => `The ${pick(ADJ)} ${pick(NOUN)}${pick(SUFFIX)}`;
/** Article agreement, so generated synopses do not read "A action". */
const withArticle = (word: string) =>
  `${/^[aeiou]/i.test(word) ? 'An' : 'A'} ${word}`;
const person = () => `${pick(FIRST)} ${pick(LAST)}`;

/** Deterministic gradient artwork as a data URI — no network, no bundled images. */
function art(seed: string, w: number, h: number): string {
  let n = 0;
  for (let i = 0; i < seed.length; i++) n = (n * 31 + seed.charCodeAt(i)) >>> 0;
  const h1 = n % 360;
  const h2 = (h1 + 40 + (n % 80)) % 360;
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}">` +
    `<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">` +
    `<stop offset="0%" stop-color="hsl(${h1} 55% 24%)"/>` +
    `<stop offset="55%" stop-color="hsl(${h2} 48% 15%)"/>` +
    `<stop offset="100%" stop-color="hsl(${h1} 40% 8%)"/>` +
    `</linearGradient></defs>` +
    `<rect width="${w}" height="${h}" fill="url(#g)"/>` +
    `<circle cx="${w * 0.75}" cy="${h * 0.25}" r="${w * 0.35}" fill="hsl(${h2} 60% 30%)" opacity="0.28"/>` +
    `<circle cx="${w * 0.2}" cy="${h * 0.8}" r="${w * 0.28}" fill="hsl(${h1} 70% 40%)" opacity="0.18"/>` +
    `</svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

/* ── Channels ──────────────────────────────────────────────────────────────── */

const CHANNEL_GROUPS = [
  { name: 'News', base: 200, names: ['Meridian News', 'Northwind 24', 'Civic Report',
    'Continental News', 'The Brief', 'Harbour Report'] },
  { name: 'Sports', base: 400, names: ['Apex Sports 1', 'Apex Sports 2', 'Velocity',
    'Pitchside', 'Court & Field', 'Endurance', 'Apex Sports 3'] },
  { name: 'Movies', base: 300, names: ['Lantern Cinema', 'Grand Picture', 'Midnight Reel',
    'Classics Vault', 'Indie Frame'] },
  { name: 'Entertainment', base: 100, names: ['Aurora One', 'Aurora Two', 'Channel Ember',
    'Northlight', 'Studio 9', 'Parlour TV'] },
  { name: 'Kids', base: 600, names: ['Sprout', 'Tinker Town', 'Junior Reef'] },
  { name: 'Documentary', base: 500, names: ['Terra', 'Deep Field', 'The Record'] },
] as const;

/**
 * The parts of a real playlist the filters exist for (README §7.3): other languages,
 * and the same channel listed two or three times at different qualities.
 *
 * `lang` is given rather than derived. The host works it out with
 * `aurora_core::lang`; duplicating that here would be a second implementation to
 * disagree with, so the mock is simply told the answer a real import would reach.
 */
const FOREIGN_CHANNELS = [
  { name: 'FR | TF1', group: 'France', lang: 'fr', base: 700 },
  { name: 'FR | Canal+ Sport', group: 'France', lang: 'fr', base: 701 },
  { name: 'DE | RTL', group: 'Deutschland', lang: 'de', base: 702 },
  { name: 'ES | Antena 3', group: 'España', lang: 'es', base: 703 },
  { name: 'AR | MBC 1', group: 'Arabic', lang: 'ar', base: 704 },
  { name: 'IT | Rai 1', group: 'Italia', lang: 'it', base: 705 },
] as const;

/** Channels the provider lists more than once. The best copy is the one to keep. */
const QUALITY_VARIANTS = ['Meridian News', 'Apex Sports 1', 'Lantern Cinema'] as const;

export const channels: Channel[] = (() => {
  const out: Channel[] = [];
  let id = 1;
  for (const g of CHANNEL_GROUPS) {
    g.names.forEach((name, i) => {
      const q = pick(['4K', 'FHD', 'HD', 'HD', 'FHD'] as const);
      out.push({
        id: id++,
        name,
        number: g.base + i + 1,
        logo: art(name, 96, 96),
        group: g.name,
        epgChannelId: `${name.toLowerCase().replace(/[^a-z0-9]/g, '')}.example`,
        quality: q,
        hidden: false,
        isRadio: false,
        hasCatchup: rand() > 0.45,
        favorite: rand() > 0.8,
        lang: null,
      });
    });
  }
  // Lesser copies of channels already in the list, the way a panel repeats them.
  for (const name of QUALITY_VARIANTS) {
    const original = out.find((c) => c.name === name)!;
    for (const q of ['SD', 'HD'] as const) {
      if (original.quality === q) continue;
      out.push({
        ...original,
        id: id++,
        name: `${name} ${q}`,
        number: 900 + out.length,
        quality: q,
        favorite: false,
        // The same EPG channel: it is the same channel.
        epgChannelId: original.epgChannelId,
      });
    }
  }
  for (const f of FOREIGN_CHANNELS) {
    out.push({
      id: id++,
      name: f.name,
      number: f.base,
      logo: art(f.name, 96, 96),
      group: f.group,
      epgChannelId: `${f.name.toLowerCase().replace(/[^a-z0-9]/g, '')}.example`,
      quality: 'HD',
      hidden: false,
      isRadio: false,
      hasCatchup: false,
      favorite: false,
      lang: f.lang,
    });
  }
  return out;
})();

/**
 * What the host's duplicate collapsing groups by: a normalized title. Mirrors
 * `aurora_core::title::match_key` closely enough for the mock's purposes — the quality
 * suffix comes off, and so does a language prefix.
 */
export function matchKey(name: string): string {
  return name
    .replace(/^\s*[\[(]?[A-Za-z]{2,4}[\])]?\s*[:|\-–]\s*/, '')
    .toLowerCase()
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .replace(/\b(4k|uhd|fhd|hd|sd|1080p|720p|2160p)\b/g, '')
    .replace(/[^a-z0-9]/g, '');
}

/** Bigger is better; 0 is "the provider never said". */
export function qualityRank(quality: string | null | undefined): number {
  switch (quality) {
    case '4K': return 4;
    case 'FHD': return 3;
    case 'HD': return 2;
    case 'SD': return 1;
    default: return 0;
  }
}

/* ── EPG ───────────────────────────────────────────────────────────────────── */

const PROG_BY_GROUP: Record<string, readonly string[]> = {
  News: ['Morning Brief', 'The Hour', 'Newsroom Live', 'World Tonight', 'Dateline',
    'Market Watch', 'The Interview', 'Late Edition'],
  Sports: ['Matchday Live', 'Premier Highlights', 'Track & Field', 'Cycling: Stage 7',
    'Boxing Night', 'Post-Match Analysis', 'Grand Slam Tennis', 'Motorsport Weekly'],
  Movies: [],
  Entertainment: ['Quiz Night', 'The Late Show', 'Bake Off Rivals', 'Homestead',
    'Talent Search', 'Panel Games', 'Celebrity Antiques'],
  Kids: ['Tinker Tales', 'Reef Rangers', 'Sprout Songs', 'Puzzle Patrol', 'Story Corner'],
  Documentary: ['Ocean Deep', 'Cities From Above', 'The Ice Age', 'Engineering Giants',
    'Wild Continents', 'Hidden Histories'],
};

const HALF_HOUR = 1800;

/** Programmes for a channel across a window, on clean 30-minute boundaries. */
export function programmesFor(ch: Channel, from: number, to: number): Programme[] {
  const out: Programme[] = [];
  const pool = PROG_BY_GROUP[ch.group ?? 'Entertainment'] ?? [];
  // Seed per channel so the same channel always shows the same schedule.
  let seed = 0;
  for (const c of ch.epgChannelId ?? ch.name) seed = (seed * 31 + c.charCodeAt(0)) >>> 0;
  const r = rng(seed);

  let t = Math.floor(from / HALF_HOUR) * HALF_HOUR;
  let id = seed % 100000;
  while (t < to) {
    // Movie channels run films; everything else runs 30/60/90-minute slots.
    const isFilm = ch.group === 'Movies';
    const slots = isFilm ? int(3, 4) : (r() > 0.55 ? 2 : r() > 0.3 ? 1 : 3);
    const dur = slots * HALF_HOUR;
    const name = isFilm || pool.length === 0 ? title() : pool[Math.floor(r() * pool.length)]!;
    const isSeries = !isFilm && r() > 0.5;

    out.push({
      id: id++,
      channelId: ch.epgChannelId ?? String(ch.id),
      start: t,
      stop: t + dur,
      title: name,
      subTitle: isSeries ? `Episode ${int(1, 12)}` : null,
      description:
        `${name} — ${isFilm
          ? `${withArticle(pick(GENRES).toLowerCase())} feature starring ` +
            `${person()} and ${person()}.`
          : `${pick(['Coverage', 'Analysis', 'A new edition', 'Highlights'])} ` +
            `with ${person()}.`}`,
      categories: isFilm ? ['Movie', pick(GENRES)] : [ch.group ?? 'General'],
      season: isSeries ? int(1, 6) : null,
      episode: isSeries ? int(1, 12) : null,
      rating: pick(CERTS),
      isNew: r() > 0.75,
      isLive: ch.group === 'Sports' && r() > 0.5,
      isPremiere: r() > 0.94,
    });
    t += dur;
  }
  return out;
}

/* ── Movies & series ───────────────────────────────────────────────────────── */

function makeMovie(id: number): Movie {
  const t = title();
  const year = int(1978, 2026);
  return {
    id,
    title: t,
    year,
    quality: pick(['4K', 'FHD', 'FHD', 'HD'] as const),
    poster: art(t, 400, 600),
    backdrop: art(t + 'bd', 1280, 720),
    logoArt: null,
    overview:
      `After ${pick(['a long silence', 'an unexplained signal', 'the flood',
        'a stolen ledger', 'one last job'])}, ${person()} returns to ` +
      `${pick(['a town', 'a city', 'an island', 'a station'])} that has ` +
      `${pick(['moved on', 'forgotten them', 'been rebuilt', 'kept the secret'])}. ` +
      `${withArticle(pick(GENRES).toLowerCase())} about ${pick(['memory', 'debt',
        'loyalty', 'inheritance', 'distance'])}.`,
    runtimeMins: int(82, 172),
    rating: Math.round((5.2 + rand() * 4.6) * 10) / 10,
    certification: pick(CERTS),
    genres: [pick(GENRES), pick(GENRES)].filter((g, i, a) => a.indexOf(g) === i),
    cast: [person(), person(), person(), person()],
    match: int(72, 99),
    addedAt: Math.floor(Date.now() / 1000) - int(0, 90) * 86400,
    lang: null,
  };
}

function makeSeries(id: number): Series {
  const t = title();
  const seasonCount = int(1, 6);
  return {
    id,
    title: t,
    year: int(1998, 2026),
    quality: pick(['4K', 'FHD', 'FHD', 'HD'] as const),
    poster: art(t, 400, 600),
    backdrop: art(t + 'bd', 1280, 720),
    logoArt: null,
    overview:
      `${pick(['A detective', 'A cartographer', 'Two sisters', 'A translator',
        'A retired courier'])} in ${pick(['a coastal town', 'the northern reach',
        'a divided city', 'an orbital colony'])} uncovers ` +
      `${pick(['a decades-old cover-up', 'a pattern nobody else can see',
        'the truth about the flood', 'a debt that outlived its lender'])}.`,
    rating: Math.round((6.0 + rand() * 3.8) * 10) / 10,
    certification: pick(CERTS),
    genres: [pick(GENRES), pick(GENRES)].filter((g, i, a) => a.indexOf(g) === i),
    cast: [person(), person(), person()],
    seasons: Array.from({ length: seasonCount }, (_, i) => i + 1),
    match: int(70, 99),
    addedAt: Math.floor(Date.now() / 1000) - int(0, 120) * 86400,
    lang: null,
  };
}

export const movies: Movie[] = (() => {
  const out = Array.from({ length: 160 }, (_, i) => makeMovie(i + 1));
  // Foreign-language films, tagged the way a provider tags them.
  const foreign: [string, string][] = [
    ['[SPANISH] La Casa del Lago', 'es'],
    ['[FRENCH] Le Dernier Quai', 'fr'],
    ['[ARABIC] Bab El Shams', 'ar'],
  ];
  for (const [t, lang] of foreign) {
    const m = makeMovie(out.length + 1);
    out.push({ ...m, title: t, lang, quality: 'FHD' });
  }
  // The same film twice, once better than the other.
  const twice = out[0]!;
  out.push({ ...twice, id: out.length + 1, quality: 'SD', poster: twice.poster });
  return out;
})();

export const series: Series[] = (() => {
  const out = Array.from({ length: 70 }, (_, i) => makeSeries(i + 1));
  const s = makeSeries(out.length + 1);
  out.push({ ...s, title: '[SPANISH] La Casa de Papel del Norte', lang: 'es' });
  const twice = out[0]!;
  out.push({ ...twice, id: out.length + 1, quality: 'SD' });
  return out;
})();

export const episodes: Episode[] = (() => {
  const out: Episode[] = [];
  let id = 1;
  for (const s of series) {
    for (const season of s.seasons) {
      const n = int(6, 12);
      for (let e = 1; e <= n; e++) {
        out.push({
          id: id++,
          seriesId: s.id,
          season,
          episode: e,
          title: `${pick(ADJ)} ${pick(NOUN)}`,
          overview:
            `${person()} ${pick(['makes a choice', 'goes north', 'finds the file',
              'breaks the rule', 'tells the truth'])}; ` +
            `${person()} ${pick(['pays for it', 'disagrees', 'disappears',
              'covers the trail'])}.`,
          still: art(`${s.id}-${season}-${e}`, 480, 270),
          runtimeMins: int(38, 62),
          airDate: Math.floor(Date.now() / 1000) - int(30, 2000) * 86400,
        });
      }
    }
  }
  return out;
})();

export const providers: Provider[] = [
  {
    id: 1,
    name: 'Example Provider',
    kind: 'xtream',
    enabled: true,
    maxConnections: 2,
    activeConnections: 1,
    expiresAt: Math.floor(Date.now() / 1000) + 41 * 86400,
    lastRefreshAt: Math.floor(Date.now() / 1000) - 3600 * 4,
    channelCount: channels.length,
    movieCount: movies.length,
    seriesCount: series.length,
  },
];

/** Seeded watch progress so Continue Watching is populated on first paint. */
export const seedProgress = [
  { kind: 'movie' as const, id: movies[3]!.id, pct: 0.34 },
  { kind: 'movie' as const, id: movies[11]!.id, pct: 0.71 },
  { kind: 'episode' as const, id: episodes[2]!.id, pct: 0.55 },
  { kind: 'movie' as const, id: movies[27]!.id, pct: 0.12 },
  { kind: 'episode' as const, id: episodes[40]!.id, pct: 0.88 },
  { kind: 'movie' as const, id: movies[52]!.id, pct: 0.46 },
];
