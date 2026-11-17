/**
 * The controls' commands: shown at once, not undone by a stale report, put right when
 * the host refuses, and sent no more often than they need to be.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { PlayerState } from '@shared/ipc';

const invoke = vi.fn();
vi.mock('@/ipc', () => ({
  invoke: (...args: unknown[]) => invoke(...args),
  hasVideoSurface: () => true,
  onPlayerState: () => () => {},
}));
vi.mock('@/lib/errors', () => ({ report: () => () => {} }));
vi.stubGlobal('window', {
  localStorage: { getItem: () => null, setItem: () => {} },
  matchMedia: () => ({ matches: false }),
});
vi.stubGlobal('document', { documentElement: { dataset: {}, style: { setProperty: () => {} } } });

const { useUi } = await import('@/state/ui');
const control = await import('./playerControl');

const base: PlayerState = {
  status: 'playing', hasPicture: true, title: 'x', subtitle: null, channelId: null, itemKind: 'movie',
  itemId: 7, positionSecs: 100, durationSecs: 1000, isLive: false, volume: 70, muted: false, speed: 1,
  audioTracks: [], subtitleTracks: [], activeAudioTrack: null, activeSubtitleTrack: null,
  aspect: 'auto', timeshift: null, error: null, stats: null, reconnecting: false,
};
const player = () => useUi.getState().player!;
const flush = () => new Promise((r) => setTimeout(r, 40));
const never = () => new Promise(() => {});

beforeEach(() => {
  invoke.mockReset();
  useUi.getState().releasePlayer();
  useUi.setState({ player: base });
});

describe('transport (C6, C7, C8)', () => {
  it('pauses at once and a stale report does not undo it', () => {
    invoke.mockReturnValue(never());
    control.togglePause();
    expect(player().status).toBe('paused');
    expect(invoke).toHaveBeenCalledWith('player.pause');
    useUi.getState().setPlayer({ ...base, positionSecs: 101 });
    expect(player().status).toBe('paused');
    expect(player().positionSecs).toBe(101);
  });

  it('a film still opening pauses, and the next press resumes it', () => {
    invoke.mockReturnValue(never());
    useUi.setState({ player: { ...base, status: 'loading', hasPicture: false, positionSecs: 0 } });
    control.togglePause();
    expect(invoke).toHaveBeenLastCalledWith('player.pause');
    expect(player().status).toBe('paused');
    useUi.getState().releasePlayer();
    useUi.setState({ player: { ...base, status: 'paused', hasPicture: false, positionSecs: 0 } });
    control.togglePause();
    expect(invoke).toHaveBeenLastCalledWith('player.resume');
  });

  it('a buffering stream pauses; one that is stopped or failed is left alone', () => {
    invoke.mockReturnValue(never());
    useUi.setState({ player: { ...base, status: 'buffering' } });
    control.togglePause();
    expect(invoke).toHaveBeenLastCalledWith('player.pause');
    invoke.mockClear();
    for (const status of ['idle', 'error'] as const) {
      useUi.getState().releasePlayer();
      useUi.setState({ player: { ...base, status } });
      control.togglePause();
    }
    expect(invoke).not.toHaveBeenCalled();
  });

  it('skips relative to wherever the player is, and shows it within the film', () => {
    invoke.mockReturnValue(never());
    control.seekBy(-300);
    expect(invoke).toHaveBeenCalledWith('player.seek', { positionSecs: -300, relative: true });
    expect(player().positionSecs).toBe(0);
  });

  it('a seek to nowhere is not sent', () => {
    control.seekTo(Number.NaN);
    expect(invoke).not.toHaveBeenCalled();
  });

  it('puts the truth back when a command fails', async () => {
    invoke.mockImplementation((name: string) => (name === 'player.state'
      ? Promise.resolve({ ...base, status: 'playing' })
      : Promise.reject(new Error('no'))));
    control.pause();
    expect(player().status).toBe('paused');
    await flush();
    expect(player().status).toBe('playing');
  });
});

describe('volume (C12)', () => {
  it('shows every value of a drag at once and sends only the latest, once a frame', async () => {
    invoke.mockReturnValue(never());
    control.setVolume(40);
    control.setVolume(45);
    control.setVolume(50);
    expect(player().volume).toBe(50);
    await flush();
    const sent = invoke.mock.calls.filter(([n]) => n === 'player.setVolume');
    expect(sent).toEqual([['player.setVolume', { volume: 50 }]]);
  });

  it('never asks for more than 100 or less than 0', async () => {
    invoke.mockReturnValue(never());
    control.changeVolume(500);
    expect(player().volume).toBe(100);
    await flush();
    control.changeVolume(-500);
    expect(player().volume).toBe(0);
    await flush();
    const sent = invoke.mock.calls.filter(([n]) => n === 'player.setVolume').map(([, a]) => a.volume);
    expect(sent).toEqual([100, 0]);
  });

  it('turning it up while muted unmutes; turning it down does not', () => {
    invoke.mockReturnValue(never());
    useUi.setState({ player: { ...base, muted: true } });
    control.setVolume(60);
    expect(player().muted).toBe(true);
    control.setVolume(80);
    expect(player().muted).toBe(false);
    expect(invoke).toHaveBeenCalledWith('player.setMuted', { muted: false });
  });
});

describe('errors (C25)', () => {
  it('Retry clears the error at once and asks for the same thing from where it died', async () => {
    let answer: (s: PlayerState) => void = () => {};
    invoke.mockReturnValue(new Promise((r) => { answer = r; }));
    useUi.setState({ player: { ...base, status: 'error', positionSecs: 612.7, error: {} as PlayerState['error'] } });
    const done = control.playAgain();
    expect(player().error).toBeNull();
    expect(player().status).toBe('loading');
    expect(invoke).toHaveBeenCalledWith('player.play', { kind: 'movie', id: 7, positionSecs: 612 });
    answer({ ...base, status: 'loading' });
    await done;
    expect(player().status).toBe('loading');
  });

  it('live is asked for at the live edge, not where it failed', () => {
    invoke.mockReturnValue(never());
    useUi.setState({ player: { ...base, isLive: true, itemKind: 'live', itemId: 3 } });
    void control.playAgain();
    expect(invoke).toHaveBeenCalledWith('player.play', { kind: 'live', id: 3, positionSecs: undefined });
  });
});
