import { afterEach, describe, expect, it, vi } from 'vitest';

interface Registered {
  event: string;
  cb: (p: { payload: unknown }) => void;
}

const registered: Registered[] = [];
const disposed: string[] = [];
let resolveListen: (() => void) | undefined;

/**
 * Stand in for `@tauri-apps/api/event`, resolving only when the test says so — which
 * is the whole point of the first two cases: the leak lives in the window between
 * calling `listen()` and it coming back.
 */
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, cb: (p: { payload: unknown }) => void) => {
    registered.push({ event, cb });
    return new Promise<() => void>((resolve) => {
      resolveListen = () => resolve(() => disposed.push(event));
    });
  }),
}));

// After the mock, so `subscribe` picks it up.
const { subscribe, wireEventName } = await import('./subscribe');

/** Let the pending `listen()` resolve, then flush microtasks. */
async function settle() {
  resolveListen?.();
  await Promise.resolve();
  await Promise.resolve();
}

afterEach(() => {
  registered.length = 0;
  disposed.length = 0;
  resolveListen = undefined;
  vi.unstubAllGlobals();
});

describe('wireEventName', () => {
  /**
   * The bug that made every host event in the app dead. Tauri 2 rejects an event name
   * containing a `.` — "Event name must include only alphanumeric characters, `-`,
   * `/`, `:` and `_`" — and every event here is dotted. `listen` rejected the
   * subscription and the host's `emit` returned `Err(IllegalEventName)` into a
   * `let _ =`, so neither end complained and nothing ever arrived.
   *
   * Mirrored by `aurora_app::wire_event_name`, whose tests assert the same mapping.
   * The two have to agree or events go out under a name nothing listens for.
   */
  it('replaces the dots Tauri will not accept', () => {
    expect(wireEventName('player.state')).toBe('player:state');
    expect(wireEventName('update.download')).toBe('update:download');
    expect(wireEventName('a.b.c')).toBe('a:b:c');
    expect(wireEventName('plain')).toBe('plain');
  });

  it('produces names Tauri actually accepts', () => {
    const legal = /^[\p{L}\p{N}\-/:_]+$/u;
    for (const event of [
      'player.state', 'ingest.progress', 'update.available', 'update.download',
      'dvr.tick', 'metadata.progress', 'metadata.done', 'artwork.progress',
    ]) {
      expect(legal.test(event), `${event} was never a legal event name`).toBe(false);
      expect(legal.test(wireEventName(event)), `${event} is still illegal`).toBe(true);
    }
  });
});

describe('subscribe', () => {
  it('subscribes under the wire name, not the app name', async () => {
    subscribe('player.state', () => {});
    await settle();
    expect(registered[0]!.event).toBe('player:state');
  });

  /**
   * The leak. Every one of the six wrappers held the unsubscribe in a `let` that was
   * only assigned when the promise resolved, so a cleanup that ran first did nothing
   * and the listener was registered afterwards with nobody holding it.
   */
  it('disposes a subscription that finishes after the component is gone', async () => {
    const stop = subscribe('player.state', () => {});

    // Unmount before the round trip comes back.
    stop();
    await settle();

    expect(disposed).toEqual(['player:state']);
  });

  it('does not deliver a payload that arrives after teardown', async () => {
    const seen: unknown[] = [];
    const stop = subscribe('dvr.tick', (p) => seen.push(p));

    stop();
    registered[0]!.cb({ payload: { started: [1] } });
    await settle();

    expect(seen).toEqual([]);
  });

  it('delivers payloads while the subscription is live, and stops on teardown', async () => {
    const seen: unknown[] = [];
    const stop = subscribe('ingest.progress', (p) => seen.push(p));
    await settle();

    registered[0]!.cb({ payload: { phase: 'importingChannels' } });
    expect(seen).toEqual([{ phase: 'importingChannels' }]);

    stop();
    expect(disposed).toEqual(['ingest:progress']);
    registered[0]!.cb({ payload: { phase: 'done' } });
    expect(seen).toHaveLength(1);
  });

  it('is safe to tear down twice', async () => {
    const stop = subscribe('update.download', () => {});
    await settle();

    stop();
    stop();
    expect(disposed).toEqual(['update:download']);
  });
});
