import { afterEach, describe, expect, it, vi } from 'vitest';
import { subscribe } from './subscribe';

interface Registered {
  event: string;
  cb: (p: { payload: unknown }) => void;
}

/**
 * A host whose `listen()` resolves when the test says so, which is the whole point:
 * the bug only exists in the window between calling it and it coming back.
 */
function fakeHost() {
  const registered: Registered[] = [];
  const disposed: string[] = [];
  let resolveListen: (() => void) | undefined;

  const listen = vi.fn((event: string, cb: (p: { payload: unknown }) => void) => {
    registered.push({ event, cb });
    return new Promise<() => void>((resolve) => {
      resolveListen = () => resolve(() => disposed.push(event));
    });
  });

  vi.stubGlobal('window', { __TAURI__: { event: { listen } } });
  return {
    registered,
    disposed,
    /** Let the pending `listen()` resolve, then flush microtasks. */
    settle: async () => {
      resolveListen?.();
      await Promise.resolve();
      await Promise.resolve();
    },
  };
}

afterEach(() => vi.unstubAllGlobals());

describe('subscribe', () => {
  /**
   * The leak. Every one of the six wrappers held the unsubscribe in a `let` that was
   * only assigned when the promise resolved, so a cleanup that ran first did nothing
   * and the listener was registered afterwards with nobody holding it.
   */
  it('disposes a subscription that finishes after the component is gone', async () => {
    const host = fakeHost();
    const stop = subscribe('player.state', () => {});

    // Unmount before the round trip comes back.
    stop();
    await host.settle();

    expect(host.disposed).toEqual(['player.state']);
  });

  it('does not deliver a payload that arrives after teardown', async () => {
    const host = fakeHost();
    const seen: unknown[] = [];
    const stop = subscribe('dvr.tick', (p) => seen.push(p));

    stop();
    host.registered[0]!.cb({ payload: { started: [1] } });
    await host.settle();

    expect(seen).toEqual([]);
  });

  it('delivers payloads while the subscription is live, and stops on teardown', async () => {
    const host = fakeHost();
    const seen: unknown[] = [];
    const stop = subscribe('ingest.progress', (p) => seen.push(p));
    await host.settle();

    host.registered[0]!.cb({ payload: { phase: 'importingChannels' } });
    expect(seen).toEqual([{ phase: 'importingChannels' }]);

    stop();
    expect(host.disposed).toEqual(['ingest.progress']);
    host.registered[0]!.cb({ payload: { phase: 'done' } });
    expect(seen).toHaveLength(1);
  });

  it('is safe to tear down twice', async () => {
    const host = fakeHost();
    const stop = subscribe('update.download', () => {});
    await host.settle();

    stop();
    stop();
    expect(host.disposed).toEqual(['update.download']);
  });

  it('is a no-op outside the host, so the mock transport is never shadowed', () => {
    vi.stubGlobal('window', {});
    expect(() => subscribe('player.state', () => {})()).not.toThrow();
  });
});
