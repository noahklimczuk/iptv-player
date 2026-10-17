/**
 * What a typed channel number means, with a typo meaning nothing at all.
 *
 * This was `{ number: Number(next) || 0 }`, and `0` is not a neutral value here -- it is
 * the sentinel that **clears** the override, which both sides agree on deliberately
 * (`repo::playlist` turns 0 into `None`, and the mock mirrors it). So `||` mapped every
 * unparseable input onto "discard my renumbering": `Number('12abc')` is `NaN`, and
 * `NaN || 0` is `0`. Measured in a browser against this editor -- a channel renumbered
 * to 5, then mistyped while editing, went back to the provider's 201 with nothing said
 * and nothing in the console (R-124).
 *
 * Returning `undefined` for a typo rather than a patch, so nothing is sent at all; the
 * empty string keeps the `0` sentinel, because clearing the field is a real request and
 * the one thing a viewer can mean by it.
 */
export function numberPatch(input: string): { number: number } | undefined {
  const text = input.trim();
  // An empty field is a real request, and the one thing a viewer can mean by it: hand
  // the channel back to the provider's own numbering. `0` is how both sides spell that
  // -- `repo::playlist` turns it into NULL and the mock mirrors it -- so an explicitly
  // typed 0 means the same thing.
  if (text === '') return { number: 0 };
  // Digits and nothing else, rather than asking `Number`, which is far too generous
  // here: `Number('0x10')` is 16 and `Number('1e3')` is 1000, and neither is something
  // somebody typed into a channel-number box on purpose. `\d` is ASCII-only in
  // JavaScript, so Eastern Arabic numerals are refused too -- they are not a channel
  // number the host would accept either.
  if (!/^[0-9]+$/.test(text)) return undefined;
  const n = Number(text);
  if (!Number.isSafeInteger(n)) return undefined;
  return { number: n };
}
