import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  type AppNotice,
  clearNotices,
  dismissNotice,
  installGlobalErrorHandlers,
  messageOf,
  notify,
  report,
  subscribeToNotices,
} from './errors';

afterEach(() => clearNotices());

describe('messageOf', () => {
  it('reads every shape a rejection arrives in', () => {
    expect(messageOf(new Error('boom'))).toBe('boom');
    // Tauri rejects with a bare string: its command errors are serialised through
    // `impl Serialize for AppError`, which writes `to_string()` and nothing else.
    expect(messageOf('channel 42 has no stream URL')).toBe('channel 42 has no stream URL');
    expect(messageOf({ message: 'from an object' })).toBe('from an object');
    expect(messageOf(undefined)).toBe('undefined');
  });
});

describe('notify', () => {
  it('keeps what the viewer was doing beside what the host said', () => {
    const notice = notify('Could not tune BBC One', 'channel 1 has no stream URL');
    expect(notice).not.toBeNull();
    expect(notice!.title).toBe('Could not tune BBC One');
    expect(notice!.detail).toBe('channel 1 has no stream URL');
  });

  /**
   * Zapping fast is a normal thing to do, and the host answers the tune you replaced
   * with `AppError::Superseded`. Showing that would mean a toast for every double
   * press of Ch+.
   */
  it('stays quiet about a tune the viewer replaced', () => {
    expect(notify('Could not tune BBC One', 'superseded by a newer request')).toBeNull();
    const seen: AppNotice[][] = [];
    const stop = subscribeToNotices((n) => seen.push(n));
    expect(seen.at(-1)).toEqual([]);
    stop();
  });

  it('bounds the stack and collapses repeats of the same intent', () => {
    // A provider that is down refuses every request on a screen at once.
    for (let i = 0; i < 10; i += 1) notify(`Could not load rail ${i}`);
    let latest: AppNotice[] = [];
    const stop = subscribeToNotices((n) => { latest = n; });
    expect(latest).toHaveLength(3);
    // Newest first.
    expect(latest[0]!.title).toBe('Could not load rail 9');

    notify('Could not load rail 9');
    expect(latest.filter((n) => n.title === 'Could not load rail 9')).toHaveLength(1);
    stop();
  });

  it('tells a subscriber immediately and again on every change', () => {
    const seen: AppNotice[][] = [];
    const stop = subscribeToNotices((n) => seen.push(n));
    expect(seen).toHaveLength(1);

    const notice = notify('Search failed', new Error('the index is rebuilding'));
    expect(seen).toHaveLength(2);

    dismissNotice(notice!.id);
    expect(seen.at(-1)).toEqual([]);

    stop();
    notify('Nobody is listening');
    expect(seen).toHaveLength(3);
  });
});

describe('report', () => {
  it('is the shape a .catch wants', async () => {
    await Promise.reject(new Error('command not found')).catch(
      report('Could not open Settings'),
    );
    let latest: AppNotice[] = [];
    const stop = subscribeToNotices((n) => { latest = n; });
    expect(latest[0]!.title).toBe('Could not open Settings');
    expect(latest[0]!.detail).toBe('command not found');
    stop();
  });
});

describe('installGlobalErrorHandlers', () => {
  it('catches what no call site handled, and unhooks cleanly', () => {
    const handlers = new Map<string, (e: unknown) => void>();
    const fakeWindow = {
      addEventListener: (name: string, fn: (e: unknown) => void) => handlers.set(name, fn),
      removeEventListener: (name: string) => handlers.delete(name),
    };
    vi.stubGlobal('window', fakeWindow);

    const uninstall = installGlobalErrorHandlers();
    expect([...handlers.keys()].sort()).toEqual(['error', 'unhandledrejection']);

    handlers.get('unhandledrejection')!({ reason: new Error('nobody caught this') });
    let latest: AppNotice[] = [];
    const stop = subscribeToNotices((n) => { latest = n; });
    expect(latest[0]!.detail).toBe('nobody caught this');
    stop();

    uninstall();
    expect(handlers.size).toBe(0);
    vi.unstubAllGlobals();
  });
});
