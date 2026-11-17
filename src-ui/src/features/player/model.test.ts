/** The player's rules (docs/PLAYER.md §3), each on its own. */
import { describe, expect, it, vi } from 'vitest';
import type { PlayerState } from '@shared/ipc';

let surface = true;
vi.mock('@/ipc', () => ({ hasVideoSurface: () => surface }));

const m = await import('./model');

const base: PlayerState = {
  status: 'playing', hasPicture: true, title: 'x', subtitle: null, channelId: null, itemKind: 'movie',
  itemId: 1, positionSecs: 100, durationSecs: 1000, isLive: false, volume: 70, muted: false, speed: 1,
  audioTracks: [], subtitleTracks: [], activeAudioTrack: null, activeSubtitleTrack: null,
  aspect: 'auto', timeshift: null, error: null, stats: null, reconnecting: false,
};
const holds = (h: Partial<Parameters<typeof m.mayHide>[0]>) => ({
  idleMs: 0, overControls: false, pressed: false, readingMenu: false, paused: false, ...h,
});

describe('when the controls may hide (C1, C2, C5)', () => {
  it('hides after 3.2 s over the picture and 5 s over the controls', () => {
    expect(m.mayHide(holds({ idleMs: 3199 }))).toBe(false);
    expect(m.mayHide(holds({ idleMs: 3200 }))).toBe(true);
    expect(m.mayHide(holds({ idleMs: 4999, overControls: true }))).toBe(false);
    expect(m.mayHide(holds({ idleMs: 5000, overControls: true }))).toBe(true);
  });

  it('never hides while held, while a menu is read, or while paused', () => {
    for (const h of [{ pressed: true }, { readingMenu: true }, { paused: true }]) {
      expect(m.mayHide(holds({ idleMs: 60_000, ...h }))).toBe(false);
      expect(m.nextCheckMs(holds({ idleMs: 60_000, ...h }))).toBe(250);
    }
  });

  it('checks again exactly when the limit would be reached', () => {
    expect(m.nextCheckMs(holds({ idleMs: 1000 }))).toBe(2200);
    expect(m.nextCheckMs(holds({ idleMs: 1000, overControls: true }))).toBe(4000);
  });
});

describe('volume (C12)', () => {
  it('stays between 0 and 100, in whole steps', () => {
    expect(m.clampVolume(-5)).toBe(0);
    expect(m.clampVolume(150)).toBe(100);
    expect(m.clampVolume(42.6)).toBe(43);
    expect(m.clampVolume(Number.NaN)).toBe(0);
  });

  it('a notch of the wheel is a step, up is louder, and small deltas add up', () => {
    expect(m.wheelVolume(0, -50, 50)).toEqual({ carried: 0, volume: 55 });
    expect(m.wheelVolume(0, 100, 50)).toEqual({ carried: 0, volume: 40 });
    let state = { carried: 0, volume: 50 };
    for (let i = 0; i < 10; i += 1) state = m.wheelVolume(state.carried, 10, state.volume);
    expect(state.volume).toBe(40);
    expect(m.wheelVolume(0, -500, 98).volume).toBe(100);
  });

  it('reads lines and pages as pixels', () => {
    expect(m.wheelPixels(3, 1)).toBe(48);
    expect(m.wheelPixels(1, 2)).toBe(400);
    expect(m.wheelPixels(-120, 0)).toBe(-120);
  });
});

describe('timelines (C9)', () => {
  it('on demand: position within the duration, and what is downloaded ahead', () => {
    const t = m.timeline({ ...base, positionSecs: 1200, stats: { bufferSecs: 30 } as PlayerState['stats'] });
    expect(t).toEqual({ kind: 'vod', position: 1000, duration: 1000, buffered: 1000 });
    const u = m.timeline({ ...base, stats: { bufferSecs: 30 } as PlayerState['stats'] });
    expect(u).toMatchObject({ kind: 'vod', position: 100, buffered: 130 });
  });

  it('on demand with no duration has nothing to scrub', () => {
    expect(m.timeline({ ...base, durationSecs: 0 }).kind).toBe('unknown');
    expect(m.canSkip({ ...base, durationSecs: 0 })).toBe(false);
  });

  it('live with nothing held is just live; with a buffer it is a window', () => {
    const live = { ...base, isLive: true, durationSecs: 0 };
    expect(m.timeline(live).kind).toBe('live');
    expect(m.canSkip(live)).toBe(false);
    const held = { ...live, timeshift: { startSecs: 10, positionSecs: 40, liveSecs: 100 } };
    expect(m.timeline(held)).toEqual({ kind: 'timeshift', start: 10, live: 100, position: 40, behind: 60 });
    expect(m.canSkip(held)).toBe(true);
  });

  it('within five seconds of the edge is live, not behind it', () => {
    const at = (positionSecs: number) => m.behindLive({
      ...base, isLive: true, timeshift: { startSecs: 0, positionSecs, liveSecs: 100 },
    });
    expect(at(96)).toBe(0);
    expect(at(94)).toBe(6);
    expect(m.behindLive(base)).toBe(0);
  });
});

describe('the picture (C24, C27, C29)', () => {
  it('lets the window show through only once there is a frame', () => {
    surface = true;
    expect(m.showingPicture(base)).toBe(true);
    expect(m.showingPicture({ ...base, status: 'buffering' })).toBe(true);
    expect(m.showingPicture({ ...base, status: 'loading' })).toBe(false);
    expect(m.showingPicture({ ...base, hasPicture: false })).toBe(false);
    expect(m.showingPicture({ ...base, status: 'error' })).toBe(false);
    expect(m.showingPicture(null)).toBe(false);
    surface = false;
    expect(m.showingPicture(base)).toBe(false);
    surface = true;
  });

  it('waits while opening, refilling or reconnecting', () => {
    expect(m.isWaiting({ ...base, status: 'loading' })).toBe(true);
    expect(m.isWaiting({ ...base, status: 'buffering' })).toBe(true);
    expect(m.isWaiting({ ...base, reconnecting: true })).toBe(true);
    expect(m.isWaiting(base)).toBe(false);
  });
});

describe('menus (C13)', () => {
  it('offers 0.25x to 4x, as the engine accepts', () => {
    expect(m.SPEEDS[0]).toBe(0.25);
    expect(m.SPEEDS[m.SPEEDS.length - 1]).toBe(4);
    expect(m.speedLabel(1)).toBe('Normal');
    expect(m.speedLabel(1.5)).toBe('1.5×');
  });

  it('names the current subtitle and audio choice', () => {
    const p = {
      ...base,
      subtitleTracks: [{ id: 3, title: null, language: 'eng', codec: null, channels: null }],
      audioTracks: [{ id: 1, title: 'Stereo', language: 'eng', codec: 'aac', channels: '2.0' }],
      activeSubtitleTrack: 3,
      activeAudioTrack: 1,
    } as PlayerState;
    expect(m.currentChoices(p)).toEqual({ subtitles: 'eng', audio: 'Stereo' });
    expect(m.currentChoices(base)).toEqual({ subtitles: 'Off', audio: null });
  });
});
