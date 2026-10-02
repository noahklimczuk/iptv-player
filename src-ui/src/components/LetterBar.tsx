/**
 * A–Z down the side of a long list.
 *
 * **It filters rather than jumps, and that is not a shortcut.** The browse lists are paged
 * — 120 titles at a time out of a hundred thousand — so there is no "scroll to W" to
 * perform: W is not in the DOM, and getting it there by fetching every page up to it would
 * read the whole library to show one screen of it. Asking the host for the titles starting
 * with W is one query, pages normally from there, and lets the count beside the heading
 * stay true.
 *
 * `#` is everything that does not start with a letter, which on a real provider's library
 * is thousands of titles: `[4K] Dune`, `2001: A Space Odyssey`, `|UK| Match of the Day`.
 * Without it they would be reachable only by scrolling or searching.
 */
const LETTERS = ['#', ...'ABCDEFGHIJKLMNOPQRSTUVWXYZ'] as const;

export function LetterBar({
  selected, onSelect, disabled,
}: {
  /** The active letter, or undefined for the whole list. */
  selected: string | undefined;
  onSelect: (letter: string | undefined) => void;
  /**
   * Shown greyed when the list is not in alphabetical order, where picking a letter
   * would narrow to a set in an order that has nothing to do with the bar. Kept visible
   * rather than removed, with the reason in a tooltip, because a control that vanishes is
   * a control nobody learns.
   */
  disabled?: boolean;
}) {
  return (
    <div
      role="group"
      aria-label="Jump to letter"
      title={disabled ? 'Sort by A–Z to filter by letter' : undefined}
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 1,
        flexShrink: 0,
        opacity: disabled ? 0.35 : 1,
        pointerEvents: disabled ? 'none' : 'auto',
        position: 'sticky',
        top: 'var(--sp-4)',
        alignSelf: 'flex-start',
      }}
    >
      {LETTERS.map((letter) => {
        const active = selected === letter;
        return (
          <button
            key={letter}
            type="button"
            aria-pressed={active}
            aria-label={letter === '#' ? 'Titles starting with a number or symbol' : letter}
            // Pressing the active letter again clears it: the bar is a filter, and every
            // filter needs an off.
            onClick={() => onSelect(active ? undefined : letter)}
            style={{
              width: 20,
              height: 18,
              display: 'grid',
              placeItems: 'center',
              border: 'none',
              borderRadius: 3,
              cursor: 'pointer',
              fontSize: 10,
              fontWeight: active ? 800 : 600,
              lineHeight: 1,
              background: active ? 'var(--accent)' : 'transparent',
              color: active ? 'var(--accent-text)' : 'var(--text-faint)',
              transition: 'background var(--t-fast) var(--ease), color var(--t-fast) var(--ease)',
            }}
          >
            {letter}
          </button>
        );
      })}
    </div>
  );
}
