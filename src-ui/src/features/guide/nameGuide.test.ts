import { describe, expect, it } from 'vitest';
import { nameGuide } from './nameGuide';

// Every name here is one the real line carries.
describe('nameGuide', () => {
  it('reads a dated event out of brackets, with its cut-off closing parenthesis', () => {
    const g = nameGuide(
      'US ★ Flo 564 : [Hockey:2025 Des Moines Buccaneers vs Fargo Force Home] (2025-10-18 19:05:30',
    );
    expect(g.kind).toBe('event');
    if (g.kind !== 'event') return;
    expect(g.title).toBe('Hockey: Des Moines Buccaneers vs Fargo Force Home');
    expect(g.at).toBe(Math.floor(new Date(2025, 9, 18, 19, 5, 30).getTime() / 1000));
  });

  it('reads a game and its start time after a colon', () => {
    expect(nameGuide('US ★ NHL GAME 06: LIGHTNING @ BLUE JACKETS 7PM ET')).toEqual({
      kind: 'event', title: 'LIGHTNING @ BLUE JACKETS', at: null, detail: '7PM ET',
    });
  });

  it('says an empty event slot is empty', () => {
    expect(nameGuide('US ★ Flo 068 : ').kind).toBe('idle');
    expect(nameGuide('US ★ ESPN+ 201:').kind).toBe('idle');
  });

  it('names what a 24/7 loop plays', () => {
    expect(nameGuide('24/7 ★ The Kardashians')).toEqual({ kind: 'loop', title: 'The Kardashians' });
    expect(nameGuide('24/7 ★ KUNG FU PANDA MOVIES HD'))
      .toEqual({ kind: 'loop', title: 'KUNG FU PANDA MOVIES' });
  });

  it('falls back to the channel name without its country or quality, case untouched', () => {
    expect(nameGuide('CA ★ THE ASYLUM')).toEqual({ kind: 'name', title: 'THE ASYLUM' });
    expect(nameGuide('US ★ AMC PRESENTS')).toEqual({ kind: 'name', title: 'AMC PRESENTS' });
    expect(nameGuide('Ghost Adventures FHD')).toEqual({ kind: 'name', title: 'Ghost Adventures' });
    expect(nameGuide("US ★ Hell's Kitchen en español"))
      .toEqual({ kind: 'name', title: "Hell's Kitchen en español" });
  });

  it('does not read a bracketed city as an event', () => {
    expect(nameGuide('US ★  CBS 7 HD [SPARTANBURG] (WSPA)').kind).toBe('name');
  });

  it("makes the provider's separators readable", () => {
    const g = nameGuide('US ★ Flo 726 : [FC:2025 Redlands vs Whittier _ Men`s] (2025-10-18 22:02:40');
    expect(g.title).toBe('FC: Redlands vs Whittier · Men’s');
  });

  it('does not mistake a channel called "X | Y" for an event', () => {
    expect(nameGuide('F1 ★ Liam Lawson | Racing Bulls | LAW').kind).toBe('name');
  });
});
