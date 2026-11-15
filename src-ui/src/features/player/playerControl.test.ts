/**
 * "The player controls are very laggy": a press shows its result at once, a state
 * report read before the command landed cannot undo it, and a failed command puts the
 * truth back.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { PlayerState } from '@shared/ipc';

const invoke = vi.fn();
vi.mock('@/ipc', () => ({
  invoke: (...args: unknown[]) => invoke(...args),
  onPlayerState: () => () => {},
}));
vi.mock('@/lib/errors', () => ({ report: () => () => {} }));
// The store applies the appearance to the document as it loads; there is no DOM here.
vi.stubGlobal('window', {
  localStorage: { getItem: () => null, setItem: () => {} },
  matchMedia: () => ({ matches: false }),
});
vi.stubGlobal('document', {
  documentElement: { dataset: {}, style: { setProperty: () => {} } },
});

const { useUi } = await import('@/state/ui');
const control = await import('./playerControl');

const base: PlayerState = {
  status: 'playing', title: 'x', subtitle: null, channelId: 1, itemKind: null, itemId: null,
  positionSecs: 100, durationSecs: 1000, isLive: false, volume: 70, muted: false, speed: 1,
  audioTracks: [], subtitleTracks: [], activeAudioTrack: null, activeSubtitleTrack: null,
  aspect: 'auto', timeshift: null, error: null, stats: null,
};
const flush = () => new Promise((r) => setTimeout(r, 30));

beforeEach(() => {
  invoke.mockReset();
  useUi.getState().releasePlayer();
  useUi.setState({ player: base });
});

describe('player controls', () => {
  it('shows a pause at once, before the host answers', () => {
    invoke.mockReturnValue(new Promise(() => {}));
    control.togglePause();
    expect(useUi.getState().player!.status).toBe('paused');
    expect(invoke).toHaveBeenCalledWith('player.pause');
  });

  it('is not undone by a report read before the command landed', () => {
    invoke.mockReturnValue(new Promise(() => {}));
    control.toggleMute();
    // A heartbeat from before the mute reached the player.
    useUi.getState().setPlayer({ ...base, muted: false, positionSecs: 101 });
    expect(useUi.getState().player!.muted).toBe(true);
    // Everything else from that report still applies.
    expect(useUi.getState().player!.positionSecs).toBe(101);
  });

  it('lets go once the host agrees', () => {
    invoke.mockReturnValue(new Promise(() => {}));
    control.toggleMute();
    useUi.getState().setPlayer({ ...base, muted: true });
    // Later the viewer unmutes from elsewhere: nothing is still being held.
    useUi.getState().setPlayer({ ...base, muted: false });
    expect(useUi.getState().player!.muted).toBe(false);
  });

  it('adopts the state the command returns', async () => {
    invoke.mockResolvedValue({ ...base, status: 'paused', positionSecs: 100.4 });
    control.pause();
    await flush();
    expect(useUi.getState().player!.positionSecs).toBe(100.4);
  });

  it('puts the truth back when a command fails', async () => {
    invoke.mockImplementation((cmd: string) =>
      cmd === 'player.state' ? Promise.resolve(base) : Promise.reject(new Error('no')));
    control.pause();
    expect(useUi.getState().player!.status).toBe('paused');
    await flush();
    expect(useUi.getState().player!.status).toBe('playing');
  });

  it('sends a slider drag once a frame, with the latest value, shown at once', async () => {
    invoke.mockResolvedValue(base);
    for (const v of [71, 75, 80, 90, 120]) control.setVolume(v);
    expect(useUi.getState().player!.volume).toBe(120);
    await flush();
    const sent = invoke.mock.calls.filter((c) => c[0] === 'player.setVolume');
    expect(sent).toHaveLength(1);
    expect(sent[0]![1]).toEqual({ volume: 120 });
  });

  it('keeps the volume between 0 and 200', () => {
    invoke.mockReturnValue(new Promise(() => {}));
    useUi.setState({ player: { ...base, volume: 3 } });
    control.changeVolume(-5);
    expect(useUi.getState().player!.volume).toBe(0);
    control.setVolume(500);
    expect(useUi.getState().player!.volume).toBe(200);
  });
});
