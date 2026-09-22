/**
 * Transport selection (docs/DECISIONS.md D4): the real Tauri bridge when the host is
 * present, an in-memory mock otherwise, so the UI is developable and screenshot-able
 * without Windows.
 */
import type {
  CommandArgs, CommandName, CommandResult, IngestProgress, PlayerState,
} from '@shared/ipc';
import {
  invokeMock,
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
  return host.invoke(cmd, args ?? {}) as Promise<CommandResult<K>>;
}

export function onPlayerState(fn: (s: PlayerState) => void): () => void {
  if (!isNativeHost()) return onMockPlayerState(fn);
  const w = window as unknown as {
    __TAURI__?: { event?: { listen: (e: string, cb: (p: { payload: PlayerState }) => void) => Promise<() => void> } };
  };
  let dispose: (() => void) | undefined;
  void w.__TAURI__?.event?.listen('player.state', (e) => fn(e.payload)).then((d) => {
    dispose = d;
  });
  return () => dispose?.();
}

/** Refresh progress, so a long import is never a frozen spinner (README C8). */
export function onIngestProgress(fn: (p: IngestProgress) => void): () => void {
  if (!isNativeHost()) return onMockIngestProgress(fn);
  const w = window as unknown as {
    __TAURI__?: {
      event?: {
        listen: (e: string, cb: (p: { payload: IngestProgress }) => void) => Promise<() => void>;
      };
    };
  };
  let dispose: (() => void) | undefined;
  void w.__TAURI__?.event?.listen('ingest.progress', (e) => fn(e.payload)).then((d) => {
    dispose = d;
  });
  return () => dispose?.();
}

export * from './mock';
