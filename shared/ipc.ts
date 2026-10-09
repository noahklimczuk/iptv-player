/**
 * IPC contract — the single boundary between the React UI and the Rust host.
 *
 * README §3: "The IPC boundary is fully typed... The UI never touches the network or the
 * database directly." Commands are request/response; events are push. This file is the
 * hand-kept mirror of the Rust side; `pnpm typecheck` fails if the UI drifts from it.
 */

export type MediaKind = 'live' | 'movie' | 'episode';
export type Quality = '4K' | 'FHD' | 'HD' | 'SD';

export interface Channel {
  id: number;
  name: string;
  number: number | null;
  logo: string | null;
  group: string | null;
  epgChannelId: string | null;
  quality: Quality | null;
  hidden: boolean;
  isRadio: boolean;
  hasCatchup: boolean;
  /** Whether the profile the list was fetched for has this channel in Favourites.
   *  False when the request named no profile. */
  favorite: boolean;
  /** ISO 639-1 as the host worked it out, or null when the name never said. */
  lang?: string | null;
}

export interface Programme {
  id: number;
  channelId: string;
  /** Unix seconds, UTC. The UI formats into the viewer's local zone. */
  start: number;
  stop: number;
  title: string;
  subTitle: string | null;
  description: string | null;
  categories: string[];
  season: number | null;
  episode: number | null;
  rating: string | null;
  isNew: boolean;
  isLive: boolean;
  isPremiere: boolean;
}

export interface Movie {
  id: number;
  title: string;
  year: number | null;
  quality: Quality | null;
  poster: string | null;
  backdrop: string | null;
  logoArt: string | null;
  overview: string | null;
  runtimeMins: number | null;
  rating: number | null;
  certification: string | null;
  genres: string[];
  cast: string[];
  /** Percentage match computed locally from viewing history (README §8.4). */
  match?: number;
  addedAt: number | null;
  lang?: string | null;
  /**
   * The trailer's YouTube key, or null when enrichment has not found one.
   *
   * A key rather than a URL: the embed address is built from it and the iframe API is
   * addressed by it. Null for every title until a TMDB key is configured and the
   * metadata sweep has reached it, so nothing in the interface may assume one exists.
   */
  trailerKey: string | null;
}

export interface Series {
  id: number;
  title: string;
  year: number | null;
  quality: Quality | null;
  poster: string | null;
  backdrop: string | null;
  logoArt: string | null;
  overview: string | null;
  rating: number | null;
  certification: string | null;
  genres: string[];
  cast: string[];
  seasons: number[];
  match?: number;
  addedAt: number | null;
  lang?: string | null;
  /** See `Movie.trailerKey`. */
  trailerKey: string | null;
}

export interface Episode {
  id: number;
  seriesId: number;
  season: number;
  episode: number;
  title: string | null;
  overview: string | null;
  still: string | null;
  runtimeMins: number | null;
  airDate: number | null;
}

/** Where one episode of a show got to, for the episode list. */
export interface EpisodeProgress {
  episodeId: number;
  positionSecs: number;
  durationSecs: number;
  completed: boolean;
}

/** Which episode Play opens for a show, and where to start in it. */
export interface ResumePoint {
  episodeId: number;
  positionSecs: number;
}

/** README §9: intro / recap / credits regions the Skip button can jump. */
export type MarkerKind = 'intro' | 'recap' | 'credits';
/** Where a marker came from, so the UI can explain itself. */
export type MarkerSource = 'chapters' | 'user' | 'learned' | 'convention';

export interface SkipMarker {
  kind: MarkerKind;
  startSecs: number;
  endSecs: number;
  source: MarkerSource;
}

export interface SeriesPrefs {
  alwaysSkipIntro: boolean;
  alwaysSkipRecap: boolean;
  autoplayNext: boolean;
}

/** Everything the player needs to drive Skip and Up Next for one episode. */
export interface PlaybackAids {
  markers: SkipMarker[];
  /** Playhead position at which the Up Next card should appear. */
  upNextAtSecs: number | null;
  nextEpisode: Episode | null;
  prefs: SeriesPrefs;
}

export interface Progress {
  itemKind: 'movie' | 'episode' | 'channel' | 'recording';
  itemId: number;
  positionSecs: number;
  durationSecs: number;
  completed: boolean;
  updatedAt: number;
}

export type RailKind =
  | 'continueWatching' | 'myList' | 'recentlyAdded' | 'trending' | 'top10'
  | 'becauseYouWatched' | 'watchAgain' | 'genre' | 'acclaimed' | 'fourK'
  | 'newReleases' | 'hiddenGems' | 'providerCategory' | 'shortAndSweet'
  | 'collections' | 'upNext'
  /** Suggested by Gemini from what has been watched, resolved against this library. */
  | 'aiPicks';

export interface Rail {
  id: string;
  kind: RailKind;
  title: string;
  /** For "Because you watched X" — what drove the recommendation (README §11). */
  reason?: string;
  /**
   * How far into each item, keyed `movie:12` / `series:7`. Continue Watching only.
   *
   * On the rail rather than on the items because a `CatalogItem` is a library row:
   * the same film is the same row whether it sits in My List or half-watched, and
   * giving the row a position would make it mean different things in different places.
   */
  progress?: Record<string, { positionSecs: number; durationSecs: number }>;
  /**
   * Why each item is on this rail, keyed `movie:12` / `series:7`. Recommendations
   * only.
   *
   * Per item rather than per rail because that is the only version anybody believes:
   * "Because you watched Blade Runner" under one poster and "More Crime" under the
   * next is the recommender showing its working, where one heading for twenty titles
   * is a claim about all of them that is true of none.
   */
  reasons?: Record<string, string>;
  items: CatalogItem[];
}

/** How a browse list is ordered. */
export type BrowseSort = 'recentlyAdded' | 'title' | 'year' | 'rating';

/** One shelf the provider files titles under, and how many are on it. */
export interface Category {
  name: string;
  count: number;
}

/**
 * What a browse page needs before it can draw its filter bar.
 *
 * `categories` is the structure a real library actually has: genres come from TMDB
 * enrichment, which needs an API key a viewer may never set, so on most libraries
 * `genres` is empty while the panel has been filing everything under named shelves
 * the whole time.
 */
export interface BrowseFacets {
  categories: Category[];
  /** Genres for this kind only, commonest first, counted like the categories. */
  genres: Category[];
  /** The real total for the current filters, not the page size. */
  total: number;
}

/** What `library.recommended` returns — the same shape a rail renders from. */
export interface Recommended {
  /** Named after the viewer's own taste where there is any: "More Sci-fi and Crime". */
  title: string;
  items: CatalogItem[];
  reasons: Record<string, string>;
  /**
   * False when there is no watch history yet and this is rating-led rather than
   * personal, so the screen can say so instead of implying it knows them.
   */
  personalised: boolean;
}

export type CatalogItem =
  | ({ kind: 'movie' } & Movie)
  | ({ kind: 'series' } & Series);

export interface SearchHit {
  kind: 'channel' | 'movie' | 'series' | 'episode' | 'programme' | 'person';
  refId: number;
  title: string;
  subtitle: string | null;
}

export interface SearchResults {
  channels: SearchHit[];
  onNow: SearchHit[];
  upcoming: SearchHit[];
  movies: SearchHit[];
  series: SearchHit[];
  people: SearchHit[];
}

/** Mirrors aurora-player's PlayerBackend state. */
export interface PlayerState {
  status: 'idle' | 'loading' | 'buffering' | 'playing' | 'paused' | 'error';
  /** What is loaded, for the OSD title. */
  title: string | null;
  subtitle: string | null;
  channelId: number | null;
  itemKind: MediaKind | null;
  itemId: number | null;
  positionSecs: number;
  durationSecs: number;
  /** Live streams have no duration; the OSD shows a live badge instead of a scrubber. */
  isLive: boolean;
  volume: number;
  muted: boolean;
  speed: number;
  audioTracks: Track[];
  subtitleTracks: Track[];
  activeAudioTrack: number | null;
  activeSubtitleTrack: number | null;
  aspect: 'auto' | '16:9' | '4:3' | '21:9' | 'stretch' | 'zoom';
  /**
   * The rewindable window of a live stream being buffered (README §7.6), or null when
   * nothing is being kept — which is also how the OSD decides between a live-edge
   * scrubber and a plain live badge.
   */
  timeshift: TimeshiftWindow | null;
  error: PlaybackError | null;
  stats: PlaybackStats | null;
}

/**
 * What the viewer can reach while timeshifting, in the stream's own timebase.
 *
 * Three timestamps and nothing derived: the delay behind live, the rewind left and the
 * scrub position are arithmetic, and one copy of that arithmetic is enough.
 */
export interface TimeshiftWindow {
  /** The oldest moment still held. */
  startSecs: number;
  positionSecs: number;
  /** The newest moment held — the live edge. */
  liveSecs: number;
}

/** The buffer's configuration, and what it is costing on disk (README §7.6, §15). */
export interface TimeshiftSettings {
  enabled: boolean;
  /** Rewind budget. Both caps are real and the tighter one decides the window. */
  bytes: number;
  secs: number;
  folder: string;
  bytesOnDisk: number;
}

export interface Track {
  id: number;
  kind: 'audio' | 'subtitle';
  title: string | null;
  language: string | null;
  codec: string | null;
  channels: string | null;
  default: boolean;
}

/** README §17: every error maps to a human sentence, a cause, and an action. */
export interface PlaybackError {
  code:
    | 'dns' | 'refused' | 'tls' | 'unauthorized' | 'forbidden' | 'notFound' | 'badRequest'
    | 'rateLimited'
    | 'serverError' | 'connectionLimit' | 'timeout' | 'unsupportedCodec'
    | 'drmProtected' | 'dropped' | 'unknown';
  message: string;
  cause: string;
  actions: ErrorAction[];
  retryable: boolean;
}

export type ErrorAction = 'retry' | 'tryAnotherSource' | 'reportBroken' | 'openSettings';

export interface PlaybackStats {
  resolution: string | null;
  videoCodec: string | null;
  audioCodec: string | null;
  fps: number | null;
  bitrateKbps: number | null;
  droppedFrames: number;
  bufferSecs: number;
  hwDecoder: string | null;
}

export interface GuideSlice {
  channels: Channel[];
  /** Keyed by EPG channel id. */
  programmes: Record<string, Programme[]>;
  from: number;
  to: number;
}

/**
 * A published build, as the host understood it.
 *
 * `version` is a `major.minor.patch` string: the host parses and compares, so nothing
 * here should try to order two of these itself — comparing the strings decides 0.9.0
 * beats 0.10.0.
 */
export interface UpdateRelease {
  version: string;
  tag: string;
  /** Release notes as published. Plain text; nothing renders it as markup. */
  notes: string;
  pageUrl: string;
  installerUrl: string | null;
  installerBytes: number | null;
  /**
   * The installer's SHA-256, as GitHub published it beside the asset. The host will
   * not download an installer without one, and checks the file against it before
   * anything is allowed to run it.
   */
  installerSha256: string | null;
  /** The portable zip, which is what an unzipped copy updates itself from. */
  portableUrl: string | null;
  portableBytes: number | null;
  portableSha256: string | null;
  publishedAt: string | null;
}

export type UpdateDownloadStatus = 'idle' | 'downloading' | 'ready' | 'failed';

/** The installer download (docs/DECISIONS.md D17). */
export interface UpdateDownload {
  status: UpdateDownloadStatus;
  /** What is being fetched, or what is waiting to be installed. */
  version: string | null;
  receivedBytes: number;
  totalBytes: number | null;
  /** Why it failed, in words worth showing. */
  message: string | null;
}

export interface UpdateStatus {
  /** The running build. */
  current: string;
  /** The newest published build, or null when none is published or tagged readably. */
  latest: UpdateRelease | null;
  /** Whether `latest` is actually newer. Decided by the host, never re-derived here. */
  available: boolean;
  /** Whether the check runs by itself on launch. */
  automatic: boolean;
  lastCheckedAt: number | null;
  /** Where "Get the update" goes. Fixed by the host, not chosen by the UI. */
  releasesUrl: string;
  download: UpdateDownload;
  /**
   * Whether this build can install an update over itself. False only off Windows,
   * where there is neither an installer to run nor a `.exe` to swap.
   */
  canInstall: boolean;
  /** Which kind of copy this is: a portable one replaces its own files and restarts,
   *  an installed one runs the installer. */
  kind: 'installer' | 'portable';
}

/**
 * A provider's settings including its password, for the edit form.
 *
 * The one place a stored secret comes back to the UI. It is the viewer's own
 * subscription password on their own machine, and an account they cannot re-read is one
 * they cannot correct after a typo or a provider rotation. Never fetched to render a
 * list — only the edit form asks for it, and only when opened.
 */
export interface ProviderCredentials {
  id: number;
  name: string;
  kind: 'xtream' | 'm3u' | 'stalker';
  url: string;
  username: string;
  /** Absent when there never was one, or the store has lost it. */
  password: string | null;
  /** False when the store cannot keep secrets across a restart. */
  passwordIsPersistent: boolean;
}

export interface Provider {
  id: number;
  name: string;
  kind: 'xtream' | 'm3u' | 'stalker';
  enabled: boolean;
  maxConnections: number | null;
  activeConnections: number | null;
  expiresAt: number | null;
  lastRefreshAt: number | null;
  channelCount: number;
  movieCount: number;
  seriesCount: number;
}

export interface EpgCoverage {
  total: number;
  matched: number;
  unmatched: string[];
}

/* ── Profiles and parental controls (README §11) ──────────────────────────── */

export interface Profile {
  id: number;
  name: string;
  avatar: string | null;
  isKids: boolean;
  /** Whether a PIN is required. The hash itself never crosses this boundary. */
  hasPin: boolean;
  /** Highest age rating this profile may watch, if limited. */
  maxAge: number | null;
  allowUnrated: boolean;
  dailyLimitMin: number | null;
}

export interface ParentalSettings {
  hasMasterPin: boolean;
  hideAdult: boolean;
  lockSettings: boolean;
}

export type PinOutcome =
  | { ok: null }
  | 'ok'
  | 'notRequired'
  | { wrong: { remaining: number } }
  | { lockedOut: { until: number } };

/* ── Provider setup (README §4, §13) ──────────────────────────────────────── */

export type SourceKind = 'xtream' | 'm3u';

export interface DraftProvider {
  name: string;
  kind: SourceKind;
  url: string;
  username?: string | null;
  password?: string | null;
}

/** What paste-detection made of whatever the user dropped in the box. */
export interface DetectedSource {
  kind: SourceKind;
  url: string;
  username: string | null;
  password: string | null;
}

/** Result of checking credentials before anything is saved. */
export interface ValidationResult {
  ok: boolean;
  message: string;
  detail: string | null;
  expiresAt: number | null;
  daysUntilExpiry: number | null;
  maxConnections: number | null;
  activeConnections: number | null;
  isTrial: boolean;
  credentialsDetected: boolean;
  /**
   * The same host under the other scheme, when the given one did not answer and that one
   * does. Null the rest of the time.
   *
   * Many panels are published on plain HTTP, which is indistinguishable from a dead host
   * when the address says `https` — the viewer gets a spinner and a red box either way.
   * The host probes without credentials, so finding this out never puts a password on an
   * unencrypted connection to somewhere that has not been established as the right place.
   */
  suggestedUrl: string | null;
  /**
   * The provider type `suggestedUrl` needs, when it is not the one being checked.
   *
   * `"m3u"` when an Xtream panel's API will not answer but its playlist will. `get.php`
   * is the other half of the same protocol and on plenty of panels it is the half that
   * works — most other players use it and never touch the API, which is why a
   * subscription can work everywhere else and fail here.
   */
  suggestedKind: SourceKind | null;
}

export type IngestPhase =
  | 'authenticating' | 'fetchingPlaylist' | 'importingChannels' | 'importingMovies'
  | 'importingSeries' | 'fetchingEpg' | 'matchingEpg' | 'indexing' | 'done';

export interface IngestProgress {
  phase: IngestPhase;
  done: number;
  /** Zero when the total is not knowable yet, e.g. while streaming an EPG. */
  total: number;
}

/** README §4.6: what actually changed in this refresh. */
export interface SyncReport {
  channels: number;
  movies: number;
  series: number;
  episodes: number;
  epgChannels: number;
  epgProgrammes: number;
  channelsMissing: number;
  epgMatched: number;
  epgUnmatched: string[];
  warnings: string[];
  /**
   * Everything the import saw and did not keep, and why.
   *
   * Zero everywhere is the claim that nothing was lost. Anything else names what went:
   * "your provider sent nothing" and "your provider sent twenty thousand channels and
   * the import threw them away" used to be the same screen.
   */
  dropped: {
    hiddenByRule: number;
    kindExcluded: number;
    noStreamId: number;
    noEpisodeMarker: number;
  };
}

export interface LibraryStats {
  channels: number;
  movies: number;
  series: number;
  episodes: number;
  programmes: number;
  epgCoverage: EpgCoverage;
}


/** README §7.7: what a scheduled or finished recording looks like to the UI. */
export type RecordingState = 'scheduled' | 'recording' | 'completed' | 'failed' | 'skipped';

export interface Recording {
  id: number;
  channelId: number;
  ruleId: number | null;
  title: string;
  subTitle: string | null;
  season: number | null;
  episode: number | null;
  /** Airtime as the guide gave it, without padding. */
  airStart: number;
  airStop: number;
  /** What the recorder opens and closes on: airtime plus padding. */
  start: number;
  stop: number;
  state: RecordingState;
  /** Why it failed, or why a completed recording is short. */
  reason: string | null;
  priority: number;
  filePath: string | null;
  bytes: number;
  durationSecs: number;
  keep: boolean;
  watched: boolean;
  /** Bytes on disk right now, for a recording still in flight. */
  liveBytes?: number | null;
}

/** A window where the schedule wants more streams than the subscription allows. */
export interface RecordingConflict {
  start: number;
  stop: number;
  slotIds: number[];
  overBy: number;
}

export interface RecordingRule {
  id: number;
  title: string;
  channelId: number | null;
  newOnly: boolean;
  /** 0 = Monday. Null means any day. */
  weekdays: number[] | null;
  aroundLocalMinute: number | null;
  prePaddingSecs: number;
  postPaddingSecs: number;
  keepEpisodes: number | null;
  priority: number;
  enabled: boolean;
  scheduled: number;
}

export interface Reminder {
  id: number;
  channelId: number;
  title: string;
  start: number;
  leadSecs: number;
}

export interface DvrStorage {
  folder: string;
  usedBytes: number;
  /** Zero means no limit. */
  quotaBytes: number;
  prunable: number[];
  maxConcurrent: number;
}


/** README §4.5: how far metadata enrichment has got for one content type. */
export interface EnrichmentCoverage {
  total: number;
  matched: number;
  /** Searched, nothing confident enough. Not a failure — some titles aren't listed. */
  noMatch: number;
  /** The attempt itself failed; these are retried after a cooldown. */
  failed: number;
}

export interface MetadataStatus {
  /** Whether a key is available at all, stored or built in. Never the key itself. */
  hasKey: boolean;
  /**
   * True when the only key is the one compiled into this build, so Settings can say
   * there is nothing to do rather than showing an empty field that looks unfinished.
   */
  keyIsBuiltIn: boolean;
  /** False when the key would be lost on restart, so the UI can say so. */
  keyIsPersistent: boolean;
  movies: EnrichmentCoverage;
  series: EnrichmentCoverage;
  /**
   * How many titles are looked up at once, with the range the control allows.
   *
   * The throughput ceiling is not this number: the client paces request starts 25ms
   * apart however many threads are asking. Concurrency hides the latency of each
   * lookup, which is what made a pass take hours.
   */
  concurrency: number;
  concurrencyMin: number;
  concurrencyMax: number;
}

export interface MetadataReport {
  matched: number;
  noMatch: number;
  failed: number;
}

export interface ArtworkCacheStatus {
  folder: string;
  files: number;
  usedBytes: number;
  /** Eviction brings the cache back under this. */
  maxBytes: number;
}

export interface ArtworkPrefetchReport {
  downloaded: number;
  /** Already on disk, so nothing was requested. */
  cached: number;
  failed: number;
  evicted: number;
}

export interface CreditEntry {
  personId: number;
  name: string;
  profilePath: string | null;
  /** Character for cast, job for crew. */
  role: string | null;
  isCast: boolean;
}

/* ── Playlist editing and library filters (README §7.3) ────────────────────── */

/** Which of the three lists an edit or a filter is about. */
export type PlaylistKind = 'live' | 'movies' | 'series';

export interface LibraryFilters {
  /**
   * Hide what is positively tagged as another language. Anything the provider did not
   * tag is kept: treating unknown as foreign empties most real libraries.
   */
  englishOnly: boolean;
  /** Show one entry per title — the highest-quality copy of it. */
  hideDuplicates: boolean;
  /**
   * Also hide what could not be identified, which is what "only English" usually means.
   * Only has an effect with `englishOnly` on, and what it costs is `FilterCounts.untagged`
   * — on a playlist that tags nothing, that is everything.
   */
  hideUntagged: boolean;
}

/** What each filter would hide, so the settings screen can say before it is turned on. */
export interface FilterCounts {
  total: number;
  nonEnglish: number;
  untagged: number;
  duplicates: number;
}

/** One row of the playlist editor, the same shape for channels, films and shows. */
export interface PlaylistEntry {
  id: number;
  /** What it is called now — the viewer's name for it if they gave it one. */
  name: string;
  /** What the provider calls it, shown when the two differ. */
  providerName: string;
  /** Channels only; null for films and shows. */
  number: number | null;
  group: string | null;
  quality: Quality | null;
  lang: string | null;
  hidden: boolean;
  /** Any override is set, so the row can offer a reset. */
  edited: boolean;
  /** How many other copies of this title exist. */
  duplicates: number;
  provider: string | null;
}

export interface PlaylistPage {
  rows: PlaylistEntry[];
  /** Matches in total, not on this page. */
  total: number;
}

/** Which rows the editor is looking at. */
export type PlaylistShow = 'all' | 'visible' | 'hidden';

export interface PlaylistQuery {
  kind: PlaylistKind;
  text?: string;
  group?: string;
  show?: PlaylistShow;
  duplicatesOnly?: boolean;
}

/** An edit. An absent field is left alone; an empty string or a 0 clears an override. */
export interface PlaylistPatch {
  name?: string;
  number?: number;
  group?: string;
  hidden?: boolean;
}

/** Another copy of the same title — the source picker behind a collapsed entry. */
export interface Alternate {
  id: number;
  name: string;
  quality: Quality | null;
  provider: string | null;
}

/** The typed command surface. Every UI data need goes through exactly one of these. */
/* ── Multi-view (README §7.4) ─────────────────────────────────────────────── */

/** The arrangements a mosaic can take. The names match `aurora_core::mosaic::Layout`. */
export type MosaicLayout = 'grid2x2' | 'grid3x3' | 'onePlusThree' | 'onePlusFive';

/** How many tiles each layout has, and so how many connections it needs. */
export const MOSAIC_TILES: Record<MosaicLayout, number> = {
  grid2x2: 4,
  onePlusThree: 4,
  onePlusFive: 6,
  grid3x3: 9,
};

/**
 * Where a tile goes, in physical pixels of the window's client area.
 *
 * The host computes these rather than the UI, because the same numbers position the
 * real video surfaces: a layout the UI drew differently from where mpv put the picture
 * would be chrome that does not line up with its own video.
 */
export interface MosaicRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface MosaicTile {
  index: number;
  rect: MosaicRect;
  channelId: number | null;
  name: string | null;
  /** The one tile with audio. Exactly one, whenever any tile has a picture. */
  focused: boolean;
  status: PlayerState['status'];
  /**
   * Why this tile has no picture. A tile failing is ordinary — it is one stream of
   * several — so it is reported here rather than failing the whole mosaic.
   */
  error: string | null;
}

export interface MosaicView {
  open: boolean;
  layout: MosaicLayout | null;
  tiles: MosaicTile[];
  focused: number;
}

/**
 * Whether the provider's line can carry a layout.
 *
 * `unknown` is the common case rather than the odd one: an M3U playlist declares no
 * `max_connections` at all, so the honest answer is that opening six streams may work
 * and may get the line cut. Only `exceeds` blocks an attempt.
 */
export type MosaicBudget =
  | { verdict: 'fits'; needed: number; limit: number }
  | { verdict: 'exceeds'; needed: number; limit: number; over: number; recordings: number }
  | { verdict: 'unknown'; needed: number };

export type MosaicCheck = MosaicBudget & {
  layout: MosaicLayout;
  tiles: number;
  /** The largest layout that would fit right now, or `null` when none does. */
  largestFitting: MosaicLayout | null;
  /** Recordings in flight, so the UI can say why the budget is tighter than the line. */
  recordings: number;
};

export interface SavedMosaicLayout {
  id: number;
  name: string;
  layout: MosaicLayout;
  /** Channel id per tile, in tile order. `null` is an empty tile. */
  channels: (number | null)[];
  createdAt: number;
}

/* ── The assistant (README §11) ──────────────────────────────────────────── */

/**
 * Something the assistant offered, which the viewer can play.
 *
 * Always a real library row: the host resolves every id before it becomes one of these,
 * so a title the model invented appears in the prose and never as a card.
 */
export interface ChatItem {
  kind: 'movie' | 'series' | 'live';
  id: number;
  title: string;
  year: number | null;
  poster: string | null;
  /** The model's one line on why this one, for this viewer. */
  note: string | null;
}

export interface ChatMessage {
  role: 'user' | 'assistant';
  text: string;
  items: ChatItem[];
  at: number;
}

export interface AssistantReply {
  message: ChatMessage;
  /**
   * The lookups it ran, in order — "searched your library for “heist” — 11 found".
   *
   * Shown rather than hidden: it turns a multi-second pause into visible thinking, and it
   * is how somebody notices the assistant went looking for the wrong thing.
   */
  steps: string[];
}

/* ── Picture-in-picture (README §6.2) ─────────────────────────────────────── */

export type PipCorner = 'topLeft' | 'topRight' | 'bottomLeft' | 'bottomRight';

export interface PipView {
  enabled: boolean;
  corner: PipCorner;
  /**
   * Where the small picture is, in **physical** pixels of the client area.
   *
   * The host computes it because the same numbers position the real video surface, and
   * it is reported even while PiP is off — that is where the tile *would* go, which is
   * what lets the UI frame it without a round-trip.
   */
  rect: MosaicRect;
  /**
   * Where a page has asked for the picture to be inlaid, if anywhere.
   *
   * The guide's preview panel. Echoed back rather than just remembered by the caller
   * because the host clamps it to the window: a box measured mid-animation or
   * mid-scroll can be partly outside, and a surface placed outside its parent is not
   * drawn at all. This is where the picture actually went.
   */
  inlay: MosaicRect | null;
}

export interface Commands {
  'library.rails': (args: { profileId: number }) => Rail[];
  'library.recommended': (args: { profileId: number; limit?: number }) => Recommended;
  'library.movies': (args: {
    sort: 'recentlyAdded' | 'title' | 'year' | 'rating';
    limit: number;
    offset: number;
    genre?: string;
    category?: string;
    query?: string;
    /**
     * Only titles starting with this one character, or `#` for the ones starting with
     * anything that is not a letter.
     *
     * The A–Z bar filters rather than scrolls, because the list is paged: W is not in the
     * DOM to scroll to, and fetching every page up to it would read the whole library to
     * show one screen. Anything but a single letter or `#` is ignored by the host.
     */
    letter?: string;
  }) => Movie[];
  'library.series': (args: {
    limit: number; offset: number; genre?: string;
    category?: string; query?: string; letter?: string; sort?: BrowseSort;
  }) => Series[];
  /**
   * What a browse page is showing, before it has fetched any of it.
   *
   * One call for the shelves, the genres and the real total — three questions the
   * page has to answer before the first poster arrives, and three round trips if they
   * are asked separately.
   */
  /**
   * The shelves, the genres, and the real total for the current filters.
   *
   * Takes `letter` as well, because the total is printed beside the heading: without it
   * the count said 117,508 while the grid showed the 4,312 films beginning with S.
   */
  'library.browseFacets': (args: {
    kind: 'movies' | 'series'; genre?: string; category?: string; query?: string;
    letter?: string;
  }) => BrowseFacets;
  /**
   * Recommendations from Gemini, resolved against this library.
   *
   * The model is asked for titles *by name* — what it is good at, and what the local
   * recommender cannot do: no amount of genre overlap says that somebody who watched
   * Arrival and Primer would like Coherence. Every answer is then looked up here, and
   * anything this subscription does not carry is dropped, so a card on the rail is always
   * something there is a stream for.
   *
   * Cached for six hours and keyed on how much has been watched, because a generation
   * costs seconds and money and taste does not move hourly. `refresh` forces a new one.
   *
   * Only the watched titles, their years and genres, how much of each was watched and
   * whether it was liked ever leave the machine. Needs a key, which is the opt-in.
   */
  'gemini.recommendations': (args: { profileId: number; refresh?: boolean }) => {
    items: CatalogItem[];
    /** `kind:id` to the model's own sentence about why this viewer would like it. */
    reasons: Record<string, string>;
    generatedAt: number;
    /** How many it offered that this library does not carry. */
    notInLibrary: number;
  };
  'gemini.status': (args: { profileId: number }) => {
    hasKey: boolean;
    keyIsBuiltIn: boolean;
    keyIsPersistent: boolean;
    hasHistory: boolean;
  };
  /** Empty clears the stored key. */
  'gemini.setKey': (args: { key: string }) => void;

  /* ── The assistant ──────────────────────────────────────────────────────── */

  'assistant.status': (args: { profileId: number }) => {
    hasKey: boolean;
    model: string;
    messages: number;
  };
  'assistant.history': (args: { profileId: number }) => ChatMessage[];
  /**
   * Ask it something. One call is a whole turn: the model may look through the library
   * several times before answering, so this can take seconds and reports what it did.
   */
  'assistant.send': (args: { profileId: number; text: string }) => AssistantReply;
  'assistant.clear': (args: { profileId: number }) => void;

  'library.episodes': (args: { seriesId: number; season?: number }) => Episode[];
  /**
   * One film or show by id, for opening something that was found rather than browsed to.
   *
   * Ignores the library filter on purpose: search can surface a title that "English only"
   * or duplicate collapsing keeps off the browse pages, and refusing to open what was
   * just found would be the same bug somewhere else.
   */
  'library.item': (args: { kind: 'movie' | 'series'; id: number }) => CatalogItem | null;
  'library.stats': () => LibraryStats;
  'library.playbackAids': (args: {
    profileId: number;
    episodeId: number;
    durationSecs: number;
  }) => PlaybackAids;
  /**
   * Derive skip markers from the loaded file's chapter list and store them. Returns how
   * many were written.
   *
   * Called before `library.playbackAids` for an episode, because that command only
   * reports what is already stored — and this is the only thing that puts
   * chapter-derived markers there. The host has answered it since markers were built;
   * it was never declared here, so nothing ever asked, and `skip_markers` stayed empty
   * on every real library. Skip Intro therefore never appeared.
   */
  'library.syncChapters': (args: {
    profileId: number;
    episodeId: number;
    durationSecs: number;
  }) => number;
  'library.recordSkip': (args: {
    episodeId: number;
    kind: MarkerKind;
    startSecs: number;
    endSecs: number;
  }) => void;
  'library.seriesPrefs': (args: { profileId: number; seriesId: number }) => SeriesPrefs;
  'library.setSeriesPrefs': (args: {
    profileId: number;
    seriesId: number;
    prefs: SeriesPrefs;
  }) => void;
  'library.genres': () => string[];
  'library.filters': () => LibraryFilters;
  'library.setFilters': (args: LibraryFilters) => LibraryFilters;
  'library.filterCounts': (args: { kind: PlaylistKind }) => FilterCounts;
  'library.alternates': (args: { kind: PlaylistKind; id: number }) => Alternate[];

  'playlist.list': (args: PlaylistQuery & { limit: number; offset: number }) => PlaylistPage;
  'playlist.groups': (args: { kind: PlaylistKind }) => { name: string; count: number }[];
  'playlist.update': (args: {
    kind: PlaylistKind;
    id: number;
    patch: PlaylistPatch;
  }) => void;
  'playlist.setHidden': (args: {
    kind: PlaylistKind;
    ids: number[];
    hidden: boolean;
  }) => number;
  /** Hide everything the query matches, however many pages of it there are. */
  'playlist.hideMatching': (args: PlaylistQuery & { hidden: boolean }) => number;
  'playlist.reset': (args: { kind: PlaylistKind; ids: number[] }) => number;

  'channels.list': (
    args: { group?: string; favoritesOnly?: boolean; profileId?: number },
  ) => Channel[];
  'channels.groups': () => { name: string; count: number }[];
  'channels.byNumber': (args: { number: number }) => Channel | null;

  'epg.gridSlice': (args: { from: number; to: number; channelIds: number[] }) => GuideSlice;
  'epg.nowNext': (args: { channelId: number }) => {
    now: Programme | null;
    next: Programme | null;
  };
  /** Now and next for a screenful of channels at once, keyed by channel id.
   *  Channels with nothing in the guide are left out rather than sent as nulls. */
  'epg.nowNextMany': (args: { channelIds: number[] }) => Record<
    number,
    { now: Programme | null; next: Programme | null }
  >;

  /** Copy the current and previous log beside the library, for a support message. */
  'logs.export': () => { files: string[]; folder: string };
  /** Version, licence, and the third-party notices this build is obliged to carry. */
  /**
   * Put the window in or out of fullscreen; omit the argument to toggle. Returns the
   * state it ended up in.
   *
   * The *window*, not the document. mpv draws into a child window behind the WebView, so
   * `requestFullscreen()` would fill the screen with UI and leave the video letterboxed
   * into the old window behind it.
   */
  'window.fullscreen': (args?: { fullscreen?: boolean }) => boolean;
  /**
   * Show the window, now that the UI has painted something.
   *
   * The window is created hidden because it is also transparent — mpv composites behind
   * the WebView2 — and a transparent window with nothing painted in it is a hole through
   * to the desktop. Called as early as possible, from the first frame rather than once
   * data has arrived, because `index.html` has already painted a boot screen by then.
   *
   * Idempotent, and nothing waits on it: the host reveals the window by itself after a
   * few seconds, so a UI that fails to boot still leaves a window somebody can close.
   */
  'window.ready': () => void;
  'app.about': () => {
    version: string;
    license: string;
    /** The full notices text, or null when the file is not beside the executable. */
    notices: string | null;
    noticesPath: string;
    releasesUrl: string;
  };
  /** What this launch found: where the data lives, and whether anything was lost. */
  'app.diagnostics': () => {
    dataDir: string;
    /** Path the unreadable library was moved to, when startup had to replace it. */
    libraryWasReplaced: string | null;
    credentialsPersist: boolean;
    /** What is decoding video. `rendersVideo: false` is the whole explanation for an
     *  empty window — this build was never going to produce a picture. */
    videoEngine: { name: string; version: string | null; rendersVideo: boolean };
  };

  'search.query': (args: { text: string }) => SearchResults;

  'player.play': (args: {
    kind: MediaKind;
    id: number;
    positionSecs?: number;
  }) => PlayerState;
  /**
   * Play a past programme from its start (README §7.5). Addressed by channel and
   * airtime rather than an item id, because catch-up has no library row of its own.
   */
  'player.playCatchup': (args: {
    channelId: number;
    start: number;
    stop: number;
  }) => PlayerState;
  'player.pause': () => PlayerState;
  'player.resume': () => PlayerState;
  'player.stop': () => PlayerState;
  'player.seek': (args: { positionSecs: number; relative?: boolean }) => PlayerState;
  /**
   * Give up the timeshift delay and rejoin the live edge (README §7.6). Resumes if the
   * viewer had paused, and is harmless on a stream that was never buffered.
   */
  'player.backToLive': () => PlayerState;
  'player.setVolume': (args: { volume: number }) => PlayerState;
  'player.setMuted': (args: { muted: boolean }) => PlayerState;
  'player.setSpeed': (args: { speed: number }) => PlayerState;
  'player.setAudioTrack': (args: { trackId: number }) => PlayerState;
  'player.setSubtitleTrack': (args: { trackId: number | null }) => PlayerState;
  'player.setAspect': (args: { aspect: PlayerState['aspect'] }) => PlayerState;
  'player.state': () => PlayerState;

  /* ── Multi-view (README §7.4) ───────────────────────────────────────────── */

  /** What the picker should say about a layout, before any channel is chosen. */
  'mosaic.check': (args: { layout: MosaicLayout }) => MosaicCheck;
  /**
   * Open a mosaic. `channelIds` is positional: index 2 is tile 2, and `null` leaves
   * that tile empty. Refused when the streams it would open exceed a limit the
   * provider actually declared.
   */
  'mosaic.open': (args: {
    layout: MosaicLayout;
    channelIds: (number | null)[];
  }) => MosaicView;
  'mosaic.close': () => MosaicView;
  'mosaic.state': () => MosaicView;
  /** Move audio to a tile — `1`–`9`, or a click. */
  'mosaic.focus': (args: { index: number }) => MosaicView;
  /** Put a different channel in one tile, or `null` to empty it. */
  'mosaic.setTile': (args: { index: number; channelId: number | null }) => MosaicView;
  /** Promote a tile to the main player, closing the mosaic. */
  'mosaic.promote': (args: { index: number }) => PlayerState;
  /** Save the mosaic as it stands. Saving over an existing name replaces it. */
  'mosaic.save': (args: { name: string }) => number;
  'mosaic.layouts': () => SavedMosaicLayout[];
  'mosaic.openSaved': (args: { id: number }) => MosaicView;
  'mosaic.deleteLayout': (args: { id: number }) => boolean;

  /* ── Picture-in-picture ─────────────────────────────────────────────────── */

  'pip.state': () => PipView;
  /** `P`. Refused while a mosaic is open — both want the same surfaces. */
  'pip.toggle': () => PipView;
  'pip.setEnabled': (args: { enabled: boolean }) => PipView;
  /** `null` moves clockwise, which is what the button on the tile does. */
  'pip.setCorner': (args: { corner: PipCorner | null }) => PipView;

  /* ── The inlaid picture ─────────────────────────────────────────────────── */

  /**
   * Put the picture inside a rectangle this page has measured — the guide's preview.
   *
   * **Physical** pixels of the client area, so the caller multiplies its CSS box by
   * `devicePixelRatio`. The host places real video surfaces in those coordinates and
   * only the page knows its own ratio.
   *
   * The rectangle comes back clamped, which is what the caller should draw its frame
   * around. Refused while a mosaic is open: the tiles have the surfaces.
   */
  'preview.place': (args: {
    x: number;
    y: number;
    width: number;
    height: number;
  }) => PipView;
  /**
   * Give the whole window back.
   *
   * Never refused — it is how a page cleans up, and a cleanup that can fail leaves the
   * picture stuck in a rectangle on a screen nobody is looking at.
   */
  'preview.clear': () => PipView;

  'progress.save': (args: {
    profileId: number;
    kind: 'movie' | 'episode' | 'channel';
    id: number;
    positionSecs: number;
    durationSecs: number;
  }) => void;
  'progress.get': (args: {
    profileId: number;
    kind: 'movie' | 'episode';
    id: number;
  }) => Progress | null;
  /**
   * Take something off Continue Watching. Returns whether anything was there to remove.
   *
   * Addressed by what the *card* is, which is not what the progress is stored against: a
   * show's position lives on its episodes, and removing it clears all of them. Deleting
   * only the episode on the card would promote the next one and the card would appear to
   * come back.
   */
  'progress.forget': (args: {
    profileId: number;
    kind: 'movie' | 'series';
    id: number;
  }) => boolean;

  /**
   * Every position this profile holds for one show, for the episode list.
   *
   * One call rather than one per episode: a season of a long-running show is forty rows.
   */
  'progress.forSeries': (args: {
    profileId: number;
    seriesId: number;
  }) => EpisodeProgress[];
  /**
   * Which episode Play should open for a show, and where to start in it.
   *
   * Null when the show has never been touched or is watched to the end; the caller then
   * opens the first episode.
   */
  'progress.resumePoint': (args: {
    profileId: number;
    seriesId: number;
  }) => ResumePoint | null;
  /** Mark one thing watched, or put it back to unwatched. Any position is left alone. */
  'progress.setWatched': (args: {
    profileId: number;
    kind: 'movie' | 'episode';
    id: number;
    watched: boolean;
  }) => void;

  'mylist.toggle': (args: {
    profileId: number;
    kind: 'movie' | 'series';
    id: number;
  }) => boolean;
  /**
   * Like a film or show, or stop. Returns whether it is liked afterwards.
   *
   * Not the same as My List: one is "watch this later", the other is "more like this".
   * Liking is what the recommender's `FAVOURITE_BOOST` reads — a flag it has always asked
   * about and which nothing in the app has ever set for a title.
   */
  'likes.toggle': (args: {
    profileId: number;
    kind: 'movie' | 'series';
    id: number;
  }) => boolean;
  /**
   * What this profile has marked, as `kind:id` keys.
   *
   * One request for both sets rather than a question per poster: a grid draws 120 cards
   * and each needs to know whether its plus is a tick. Small enough — tens of titles — for
   * the interface to hold them and answer from memory.
   */
  'lists.marks': (args: { profileId: number }) => { myList: string[]; liked: string[] };
  'favorites.toggle': (args: { profileId: number; channelId: number }) => boolean;

  'profiles.list': () => Profile[];
  'profiles.create': (args: {
    name: string;
    avatar?: string | null;
    isKids: boolean;
    maxAge?: number | null;
    dailyLimitMin?: number | null;
  }) => number;
  'profiles.delete': (args: { profileId: number }) => boolean;
  'profiles.rename': (args: { profileId: number; name: string }) => void;
  'profiles.setLimits': (args: {
    profileId: number;
    maxAge: number | null;
    allowUnrated: boolean;
    dailyLimitMin: number | null;
  }) => void;
  'profiles.setPin': (args: { profileId: number; pin: string | null }) => void;
  'profiles.verifyPin': (args: { profileId?: number; pin: string }) => PinOutcome;
  'profiles.parental': () => ParentalSettings;
  'profiles.setParental': (args: {
    hideAdult: boolean;
    lockSettings: boolean;
    masterPin?: string | null;
  }) => void;
  'profiles.watchedToday': (args: { profileId: number }) => number;

  /** Returns null when this airing was already on the schedule. */
  'dvr.schedule': (args: {
    channelId: number;
    title: string;
    subTitle?: string | null;
    season?: number | null;
    episode?: number | null;
    airStart: number;
    airStop: number;
    prePaddingSecs?: number;
    postPaddingSecs?: number;
  }) => number | null;
  'dvr.list': (args: { state?: RecordingState }) => Recording[];
  'dvr.cancel': (args: { id: number }) => boolean;
  'dvr.delete': (args: { id: number }) => boolean;
  'dvr.setKeep': (args: { id: number; value: boolean }) => void;
  'dvr.setWatched': (args: { id: number; value: boolean }) => void;
  'dvr.conflicts': () => RecordingConflict[];
  'dvr.rules': () => RecordingRule[];
  'dvr.createRule': (args: {
    title: string;
    channelId?: number | null;
    newOnly: boolean;
    weekdays?: number[] | null;
    aroundLocalMinute?: number | null;
    prePaddingSecs?: number | null;
    postPaddingSecs?: number | null;
    keepEpisodes?: number | null;
  }) => number;
  'dvr.deleteRule': (args: { id: number }) => boolean;
  'dvr.setRuleEnabled': (args: { id: number; value: boolean }) => void;
  'dvr.reminders': () => Reminder[];
  'dvr.addReminder': (args: {
    channelId: number;
    title: string;
    start: number;
    leadSecs?: number;
  }) => number | null;
  'dvr.removeReminder': (args: { id: number }) => boolean;
  'dvr.storage': () => DvrStorage;

  'timeshift.settings': () => TimeshiftSettings;
  /**
   * Change the buffer. Returns what was actually stored, which is not always what was
   * asked for: a budget outside the documented range is clamped into it.
   *
   * Applies to the next channel tuned, not to the stream already playing — the cache is
   * sized when a stream is loaded, and re-loading to apply a setting would black out
   * whatever is on.
   */
  'timeshift.setSettings': (args: {
    enabled?: boolean;
    bytes?: number;
    secs?: number;
    /** An empty string restores the default folder beside the library. */
    folder?: string;
  }) => TimeshiftSettings;
  /** Empty the buffer, returning the bytes reclaimed. */
  'timeshift.clear': () => number;

  'metadata.status': () => MetadataStatus;
  /** Null clears the stored key. The key is never read back. */
  'metadata.setKey': (args: { key: string | null }) => void;
  /** Returns what was stored, which is the value clamped to the allowed range. */
  'metadata.setConcurrency': (args: { concurrency: number }) => number;
  /** Enrich one batch and report what happened. Call again to continue. */
  'metadata.run': (args: {
    batch?: number;
    movies?: boolean;
    series?: boolean;
  }) => MetadataReport;
  'metadata.credits': (args: { kind: 'movie' | 'series'; id: number }) => CreditEntry[];
  /** Forget a title's match so the next pass looks again. */
  'metadata.rematch': (args: { kind: 'movie' | 'series'; id: number }) => void;

  'artwork.status': () => ArtworkCacheStatus;
  /**
   * Which of these remote image URLs are already in the local cache, as URLs the
   * WebView can load — `null` for any that are not, meaning "use the remote one".
   * Answered for a whole grid at once; a page paints a hundred posters.
   */
  'artwork.local': (args: { urls: string[] }) => (string | null)[];
  /** Download the library's artwork into the local cache, then evict to the budget. */
  'artwork.prefetch': (args: { limit?: number }) => ArtworkPrefetchReport;
  /** Empty the cache. Always safe — the library keeps the remote URLs. */
  'artwork.clear': () => number;

  'providers.list': () => Provider[];
  'providers.detect': (args: { text: string }) => DetectedSource;
  'providers.validate': (args: { draft: DraftProvider }) => ValidationResult;
  'providers.save': (args: { draft: DraftProvider }) => { id: number };
  'providers.refresh': (args: { providerId: number }) => SyncReport;
  /** A provider's settings, password included, for editing. */
  'providers.credentials': (args: { providerId: number }) => ProviderCredentials;
  /**
   * Save edits. A `password` of `undefined` leaves the stored one alone, `''` clears
   * it — without that distinction, changing only the name would wipe the password.
   */
  'providers.update': (args: { providerId: number; draft: DraftProvider }) => boolean;
  /** Remove a provider, its credential, and everything it imported. */
  'providers.delete': (args: { providerId: number }) => boolean;

  /**
   * What the newest published build is.
   *
   * Answers from the last check unless `force` is set, so opening Settings costs no
   * network; "Check now" forces it.
   */
  'updates.check': (args?: { force?: boolean }) => UpdateStatus;
  /** Turn the launch-time check on or off. */
  'updates.setAutomatic': (args: { enabled: boolean }) => boolean;
  /**
   * Open the releases page in the viewer's browser.
   *
   * Takes no URL on purpose: the host holds a constant, so there is no way for
   * anything here to make the app open something else.
   */
  'updates.openReleases': () => void;
  /**
   * Fetch the published installer and verify it against the digest GitHub published
   * beside it. Takes no URL — the host uses the one from the release it just checked,
   * for the same reason `openReleases` takes none.
   *
   * Returns as soon as the download starts; `update.download` reports the rest.
   */
  'updates.download': () => UpdateDownload;
  /**
   * Run the downloaded installer and quit so it can replace the files. Refuses unless
   * a verified download is waiting, and refuses while a recording is in progress.
   */
  'updates.install': () => void;
}

export type CommandName = keyof Commands;
export type CommandArgs<K extends CommandName> = Parameters<Commands[K]>[0];
export type CommandResult<K extends CommandName> = ReturnType<Commands[K]>;

/** Push events from the host. */
export interface Events {
  'player.state': PlayerState;
  /**
   * The mosaic, on the same heartbeat as the player.
   *
   * Pushed rather than polled for the same reason `player.state` is: a tile's stream
   * dying is something the host notices and the UI cannot ask about often enough.
   */
  'mosaic.state': MosaicView;
  /** One lookup the assistant just ran, so a long turn shows its working. */
  'assistant.step': { step: string };
  /** Emitted shortly after launch when a newer build turns out to be published. */
  'update.available': {
    current: string;
    latest: UpdateRelease | null;
    available: boolean;
  };
  /** How far the update installer has got, so the bar moves without polling. */
  'update.download': UpdateDownload;
  'ingest.progress': IngestProgress;
  'library.refreshed': { added: number; removed: number; updated: number };
  'toast': { level: 'info' | 'success' | 'warning' | 'error'; message: string };
  /** Enrichment progress, so a long metadata pass is never a frozen spinner. */
  'metadata.progress': { done: number; total: number };
  /**
   * How far the episode-listing sweep has got.
   *
   * Seasons are counted from stored episodes, and an import writes shows without them —
   * so this is what turns "0 seasons" on every card into a real number. One request per
   * show is the only way: the panel's series list carries no season information.
   */
  'series.listings': { done: number; total: number };
  'series.listingsDone': {
    asked: number; listed: number; episodes: number; failed: number;
  };
  'metadata.done': MetadataReport;
  'artwork.progress': { done: number; total: number };
  /** What one DVR tick changed, so the recordings page stays live (README §7.7). */
  'dvr.tick': {
    started: number[];
    completed: number[];
    failed: [number, string][];
    stalled: number[];
  };
}
export type EventName = keyof Events;
