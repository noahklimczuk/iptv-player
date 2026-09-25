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
    | 'dns' | 'refused' | 'tls' | 'unauthorized' | 'forbidden' | 'notFound' | 'rateLimited'
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
   * Whether this build can install an update over itself: false for a portable copy,
   * which an installer would not replace, and false off Windows.
   */
  canInstall: boolean;
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
