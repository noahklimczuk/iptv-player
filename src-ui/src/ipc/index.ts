/**
 * Transport selection (docs/DECISIONS.md D4): the real Tauri bridge when the host is
 * present, an in-memory mock otherwise, so the UI is developable and screenshot-able
 * without Windows.
 */
import type {
  CommandArgs, CommandName, CommandResult, Events, IngestProgress, PlayerState,
} from '@shared/ipc';
import { subscribe } from './subscribe';
import {
  invokeMock,
  onUpdateDownload as onMockUpdateDownload,
  onDvrTick as onMockDvrTick,
  onArtworkProgress as onMockArtworkProgress,
  onMetadataProgress as onMockMetadataProgress,
  onIngestProgress as onMockIngestProgress,
  onPlayerState as onMockPlayerState,
} from './mock';

interface TauriInternals {
  invoke: (cmd: string, args: unknown) => Promise<unknown>;
}

function tauri(): TauriInternals | null {
  const w = window as unknown as { __TAURI_INTERNALS__?: TauriInternals };
  return w.__TAURI_INTERNALS__ ?? null;
}

export const isNativeHost = (): boolean => tauri() !== null;

/** Call a host command. Rejects with a readable message; callers surface it per README §17. */
export async function invoke<K extends CommandName>(
  name: K,
  args: CommandArgs<K> = undefined as CommandArgs<K>,
): Promise<CommandResult<K>> {
  const host = tauri();
  if (!host) return invokeMock(name, args);
  // Command names cross the bridge as snake_case module_action pairs.
  const cmd = name.replace(/\./g, '_').replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`);
  // Arguments go *under* `args`, not spread across the payload. Every host command
  // takes them as one `args: SomeArgs` parameter, and Tauri keys the payload by the
  // parameter's name — `tauri::ipc::command` errors outright when the key is missing,
  // with no fallback to the payload as a whole. Spreading them made all sixty-one
  // commands that take arguments fail to deserialize on Windows, which the mock
  // transport could never show because it reads the flat object.
  return host.invoke(cmd, args === undefined ? {} : { args }) as Promise<CommandResult<K>>;
}

export function onPlayerState(fn: (s: PlayerState) => void): () => void {
  if (!isNativeHost()) return onMockPlayerState(fn);
  return subscribe<PlayerState>('player.state', fn);
}

/** How far the update installer has got (docs/DECISIONS.md D17). */
export function onUpdateDownload(fn: (d: Events['update.download']) => void): () => void {
  if (!isNativeHost()) return onMockUpdateDownload(fn);
  return subscribe<Events['update.download']>('update.download', fn);
}

/** Refresh progress, so a long import is never a frozen spinner (README C8). */
export function onIngestProgress(fn: (p: IngestProgress) => void): () => void {
  if (!isNativeHost()) return onMockIngestProgress(fn);
  return subscribe<IngestProgress>('ingest.progress', fn);
}

/** What one DVR tick changed, so the recordings page stays live without polling. */
export function onDvrTick(fn: (t: Events['dvr.tick']) => void): () => void {
  if (!isNativeHost()) return onMockDvrTick(fn);
  return subscribe<Events['dvr.tick']>('dvr.tick', fn);
}

/** Enrichment progress, so a long metadata pass is never a frozen spinner. */
export function onMetadataProgress(fn: (p: Events['metadata.progress']) => void): () => void {
  if (!isNativeHost()) return onMockMetadataProgress(fn);
  return subscribe<Events['metadata.progress']>('metadata.progress', fn);
}

/** Artwork download progress, for the cache panel in settings. */
export function onArtworkProgress(fn: (p: Events['artwork.progress']) => void): () => void {
  if (!isNativeHost()) return onMockArtworkProgress(fn);
  return subscribe<Events['artwork.progress']>('artwork.progress', fn);
}

export * from './mock';
export { subscribe } from './subscribe';
