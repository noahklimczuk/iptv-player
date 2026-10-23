/**
 * `clockTime`, bound to the viewer's 12/24-hour preference.
 *
 * The preference existed in three halves and was joined up by none of them: `use24h` was
 * declared and initialised in the store, honoured by `clockTime` *if anyone passed it*,
 * and read by nothing — none of the twenty-odd call sites passed a second argument, and
 * there was no action to set it beside `setTheme` and `setDensity`. So the AM/PM branch
 * was unreachable and the setting was a field with no switch (R-136).
 *
 * A hook rather than a default inside `clockTime`, for two reasons. `lib/format.ts` is a
 * pure module that a node-environment unit test imports, and reaching into the store
 * from it drags `window` in. And a component that draws a time has to *redraw* when the
 * notation changes, which only a subscription does.
 */
import { clockTime, untilLabel } from '@/lib/format';
import { useUi } from '@/state/ui';

export function useClockTime(): (unix: number) => string {
  const use24h = useUi((s) => s.use24h);
  return (unix: number) => clockTime(unix, use24h);
}

/**
 * `untilLabel`, bound the same way.
 *
 * Its "Thursday at 21:30" branch is a clock time, and the scheduled recordings list is
 * where a viewer reads it (R-136).
 */
export function useUntilLabel(): (unix: number) => string {
  const use24h = useUi((s) => s.use24h);
  return (unix: number) => untilLabel(unix, undefined, use24h);
}
