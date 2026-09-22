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
  favorite?: boolean;
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

/** README §9: intro / recap / credits regions the Skip button can jump. */
export type MarkerKind = 'intro' | 'recap' | 'credits';
/** Where a marker came from, so the UI can explain itself. */
export type MarkerSource = 'chapters' | 'user' | 'learned';

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
  | 'collections' | 'upNext';

export interface Rail {
  id: string;
  kind: RailKind;
  title: string;
  /** For "Because you watched X" — what drove the recommendation (README §11). */
  reason?: string;
  items: CatalogItem[];
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
  error: PlaybackError | null;
  stats: PlaybackStats | null;
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
    | 'dns' | 'tls' | 'unauthorized' | 'forbidden' | 'notFound' | 'rateLimited'
    | 'serverError' | 'connectionLimit' | 'timeout' | 'unsupportedCodec'
    | 'drmProtected' | 'unknown';
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
  /** Whether a key is stored. Never the key itself. */
  hasKey: boolean;
  /** False when the key would be lost on restart, so the UI can say so. */
  keyIsPersistent: boolean;
  movies: EnrichmentCoverage;
  series: EnrichmentCoverage;
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

/** The typed command surface. Every UI data need goes through exactly one of these. */
export interface Commands {
  'library.rails': (args: { profileId: number }) => Rail[];
  'library.movies': (args: {
    sort: 'recentlyAdded' | 'title' | 'year' | 'rating';
    limit: number;
    offset: number;
    genre?: string;
  }) => Movie[];
  'library.series': (args: { limit: number; offset: number; genre?: string }) => Series[];
  'library.episodes': (args: { seriesId: number; season?: number }) => Episode[];
  'library.stats': () => LibraryStats;
  'library.playbackAids': (args: {
    profileId: number;
    episodeId: number;
    durationSecs: number;
  }) => PlaybackAids;
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

  'channels.list': (args: { group?: string; favoritesOnly?: boolean }) => Channel[];
  'channels.groups': () => { name: string; count: number }[];
  'channels.byNumber': (args: { number: number }) => Channel | null;

  'epg.gridSlice': (args: { from: number; to: number; channelIds: number[] }) => GuideSlice;
  'epg.nowNext': (args: { channelId: number }) => {
    now: Programme | null;
    next: Programme | null;
  };

  'search.query': (args: { text: string }) => SearchResults;

  'player.play': (args: {
    kind: MediaKind;
    id: number;
    positionSecs?: number;
  }) => PlayerState;
  'player.pause': () => PlayerState;
  'player.resume': () => PlayerState;
  'player.stop': () => PlayerState;
  'player.seek': (args: { positionSecs: number; relative?: boolean }) => PlayerState;
  'player.setVolume': (args: { volume: number }) => PlayerState;
  'player.setMuted': (args: { muted: boolean }) => PlayerState;
  'player.setSpeed': (args: { speed: number }) => PlayerState;
  'player.setAudioTrack': (args: { trackId: number }) => PlayerState;
  'player.setSubtitleTrack': (args: { trackId: number | null }) => PlayerState;
  'player.setAspect': (args: { aspect: PlayerState['aspect'] }) => PlayerState;
  'player.state': () => PlayerState;

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

  'mylist.toggle': (args: {
    profileId: number;
    kind: 'movie' | 'series';
    id: number;
  }) => boolean;
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

  'metadata.status': () => MetadataStatus;
  /** Null clears the stored key. The key is never read back. */
  'metadata.setKey': (args: { key: string | null }) => void;
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
  /** Download the library's artwork into the local cache, then evict to the budget. */
  'artwork.prefetch': (args: { limit?: number }) => ArtworkPrefetchReport;
  /** Empty the cache. Always safe — the library keeps the remote URLs. */
  'artwork.clear': () => number;

  'providers.list': () => Provider[];
  'providers.detect': (args: { text: string }) => DetectedSource;
  'providers.validate': (args: { draft: DraftProvider }) => ValidationResult;
  'providers.save': (args: { draft: DraftProvider }) => { id: number };
  'providers.refresh': (args: { providerId: number }) => SyncReport;
}

export type CommandName = keyof Commands;
export type CommandArgs<K extends CommandName> = Parameters<Commands[K]>[0];
export type CommandResult<K extends CommandName> = ReturnType<Commands[K]>;

/** Push events from the host. */
export interface Events {
  'player.state': PlayerState;
  'ingest.progress': IngestProgress;
  'library.refreshed': { added: number; removed: number; updated: number };
  'toast': { level: 'info' | 'success' | 'warning' | 'error'; message: string };
  /** Enrichment progress, so a long metadata pass is never a frozen spinner. */
  'metadata.progress': { done: number; total: number };
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
