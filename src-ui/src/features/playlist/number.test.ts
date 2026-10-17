/**
 * What the playlist editor does with what you type into the Number cell.
 *
 * The old expression was `Number(next) || 0`, and `0` is the sentinel that *clears* the
 * override — so every unparseable input meant "discard my renumbering". Measured in a
 * browser at the time: a channel renumbered to 5, mistyped while editing, went back to
 * the provider's 201 with nothing said (R-124).
 */
import { describe, expect, it } from 'vitest';
import { numberPatch } from './number';

describe('numberPatch', () => {
  it('takes a whole channel number', () => {
    expect(numberPatch('5')).toEqual({ number: 5 });
    expect(numberPatch('201')).toEqual({ number: 201 });
    // Spreadsheet habits: the surrounding spaces are not part of the number.
    expect(numberPatch('  42 ')).toEqual({ number: 42 });
  });

  it('treats an empty field as "use the provider\'s number"', () => {
    // 0 is the sentinel both sides agree on: `repo::playlist` turns it into NULL.
    expect(numberPatch('')).toEqual({ number: 0 });
    expect(numberPatch('   ')).toEqual({ number: 0 });
    expect(numberPatch('0')).toEqual({ number: 0 });
  });

  it('does nothing at all with a typo', () => {
    // Every one of these used to clear the override, because `Number(x) || 0` is 0 for
    // all of them.
    for (const typo of ['12abc', 'abc', '-3', '1.5', 'NaN', '12 34', '--']) {
      expect(numberPatch(typo), `${typo} should be refused`).toBeUndefined();
    }
  });

  it('refuses the shapes `Number` is too generous about', () => {
    // `Number('0x10')` is 16, `Number('1e3')` is 1000, and `Number('٣')` is 3. None of
    // them is something somebody typed into a channel-number box on purpose, and
    // accepting one silently puts a number in the cell that nobody entered.
    for (const odd of ['0x10', '1e3', 'Infinity', '٣', '+5', '5.0']) {
      expect(numberPatch(odd), `${odd} should be refused`).toBeUndefined();
    }
  });
});
