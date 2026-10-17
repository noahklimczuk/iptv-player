/**
 * What each import phase is called on screen, in one place with one list of phases.
 *
 * There were two maps. `SetupWizard`'s was complete and correct, and annotated
 * `Record<IngestPhase, string>` — which is the whole mechanism, because that is
 * exhaustiveness-checked and so cannot drift. `RefreshButton`'s was annotated
 * `Record<string, string>`, which accepts any key and requires none, so `tsc --strict`
 * passed over it while it carried **two keys that are not phases** (`downloading`,
 * `importingEpg`) and was **missing four that are** (`authenticating`,
 * `fetchingPlaylist`, `fetchingEpg`, `matchingEpg`).
 *
 * The render fell through to the raw value, so a viewer pressing Refresh in Settings
 * watched the label read `authenticating`, then `fetchingPlaylist`, then Channels /
 * Movies / Series, then `fetchingEpg`, then `matchingEpg`. Four camelCase identifiers —
 * and on a real subscription those are not the quick ones: fetching a 165 MB guide and
 * matching it across 6,699 channels is the longest stretch of a 23.7-second refresh
 * (R-145).
 *
 * Two registers, because the two screens are doing different jobs: the wizard narrates a
 * first run in sentences, and the button's label sits inline beside a spinner. Both are
 * `Record<IngestPhase, string>`, so adding a phase to the contract breaks the build here
 * rather than printing an identifier at somebody.
 */
import type { IngestPhase } from '@shared/ipc';

/** Full sentences, for the first-run wizard. */
export const PHASE_SENTENCE: Record<IngestPhase, string> = {
  authenticating: 'Checking your subscription',
  fetchingPlaylist: 'Fetching your channel list',
  importingChannels: 'Importing channels',
  importingMovies: 'Importing movies',
  importingSeries: 'Importing series',
  fetchingEpg: 'Downloading the TV guide',
  matchingEpg: 'Matching the guide to your channels',
  indexing: 'Building the search index',
  done: 'Finished',
};

/** One or two words, for the inline progress on Settings → Refresh. */
export const PHASE_SHORT: Record<IngestPhase, string> = {
  authenticating: 'Signing in',
  fetchingPlaylist: 'Fetching list',
  importingChannels: 'Channels',
  importingMovies: 'Movies',
  importingSeries: 'Series',
  fetchingEpg: 'Guide',
  matchingEpg: 'Matching guide',
  indexing: 'Indexing',
  done: 'Done',
};
