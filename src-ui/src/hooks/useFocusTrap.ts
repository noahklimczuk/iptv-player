/**
 * Make a dialog's `aria-modal="true"` true.
 *
 * Three dialogs asserted it — the detail modal, the command palette and the mosaic
 * picker — and none of them held focus. No trap, no initial focus, no focus restoration,
 * and nothing hiding the background. Measured in Chromium, opening the detail modal from
 * a rail card:
 *
 * ```text
 * focus right after opening: DIV[The Silent Tide, 2019] inDialog=false
 * tab trail from the open modal:
 *   BUTTON[Play The Silent Tide]            inDialog=false
 *   BUTTON[Add The Silent Tide to My List]  inDialog=false
 *   ...
 * of 12 tabs, landed outside the dialog: 12
 * focus after Escape: BUTTON[Add [SPANISH] La Casa del Lago to My List]
 * ```
 *
 * Every one of twelve Tab presses landed on a control *behind the scrim*, including Play
 * buttons on other titles — reachable by keyboard, and they start playback with a modal
 * over them. Nothing inside the dialog was reachable by Tab at all. After Escape, focus
 * was left wherever the walk ended rather than on the card that opened it, so a keyboard
 * user loses their place in the rail.
 *
 * `aria-modal="true"` is a promise to assistive technology that the rest of the document
 * is unavailable. It was false, and a screen-reader user was told the background was
 * inert while the keyboard walked straight into it (R-102).
 */
import { useEffect, type RefObject } from 'react';

/** What counts as reachable by Tab. `:not([disabled])` because a disabled control is not. */
const FOCUSABLE = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

function focusable(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE))
    // `offsetParent` is null for anything `display: none`, which is how a collapsed
    // section's buttons stay out of the cycle.
    .filter((el) => el.offsetParent !== null || el === document.activeElement);
}

/**
 * Hold focus inside `ref` while `open`, and give it back to the opener afterwards.
 *
 * `initial` names what to focus on open; without it the dialog itself is focused, which
 * is what a screen reader reads out as the dialog's label. The container needs
 * `tabIndex={-1}` for that to work, which is the one thing a caller has to remember.
 */
export function useFocusTrap(
  ref: RefObject<HTMLElement | null>,
  open: boolean,
  initial?: RefObject<HTMLElement | null>,
) {
  useEffect(() => {
    if (!open) return;
    const root = ref.current;
    if (!root) return;

    // Where focus came from, so it can go back. `closest` because the opener is often a
    // span inside the button that was clicked.
    const opener = document.activeElement as HTMLElement | null;

    (initial?.current ?? root).focus({ preventScroll: true });

    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Tab') return;
      const stops = focusable(root);
      if (stops.length === 0) {
        // Nothing to move to: keep focus on the dialog rather than letting it escape.
        e.preventDefault();
        root.focus({ preventScroll: true });
        return;
      }
      const first = stops[0]!;
      const last = stops[stops.length - 1]!;
      const active = document.activeElement as HTMLElement | null;

      // Wrapping is the trap. Out-of-dialog focus — which is where it starts, since the
      // opener is behind the scrim — comes back to the first stop.
      if (!active || !root.contains(active)) {
        e.preventDefault();
        first.focus();
        return;
      }
      if (e.shiftKey && active === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    };

    document.addEventListener('keydown', onKey, true);
    return () => {
      document.removeEventListener('keydown', onKey, true);
      // Back where it came from, if that is still on the page — a refresh can remove the
      // card that opened the dialog while it is open.
      if (opener && document.contains(opener)) opener.focus({ preventScroll: true });
    };
  }, [ref, open, initial]);
}
