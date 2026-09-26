import { useCallback, useEffect, useRef, useState } from 'react';
import type { CommandArgs, CommandName, CommandResult } from '@shared/ipc';
import { invoke } from '@/ipc';
import { notify } from '@/lib/errors';

interface State<T> {
  data: T | null;
  loading: boolean;
  error: string | null;
}

/**
 * Fetch once (and on dependency change) from the host, with loading and error states.
 *
 * A failure is also reported through `lib/errors`, and that is not belt and braces —
 * it is the fix for a whole class of wrong screen. Two of the twenty-two call sites
 * read `error`; the rest destructure `{ data, loading }` and render their empty state
 * when `data` is null. So a host that refused `channels.list` produced "No channels —
 * add a provider in Settings", which is a confident answer to a question nobody could
 * answer, on a machine that already had a provider and forty thousand channels.
 *
 * Telling the viewer something failed is the hook's job precisely because remembering
 * to is not something twenty-two call sites will keep doing.
 */
export function useCommand<K extends CommandName>(
  name: K,
  args: CommandArgs<K>,
  deps: unknown[] = [],
  /**
   * Whether to ask at all. Hooks cannot be called conditionally, so a component that
   * needs an answer only sometimes — episodes, which exist for a series and not for a
   * film — would otherwise have to ask a question that makes no sense and then ignore
   * a failure it caused itself. `loading` is false while disabled, because nothing is.
   */
  enabled = true,
): State<CommandResult<K>> & { reload: () => void } {
  const [state, setState] = useState<State<CommandResult<K>>>({
    data: null, loading: enabled, error: null,
  });
  const [nonce, setNonce] = useState(0);
  const argsRef = useRef(args);
  argsRef.current = args;

  useEffect(() => {
    if (!enabled) {
      setState({ data: null, loading: false, error: null });
      return;
    }
    let live = true;
    setState((s) => ({ ...s, loading: true }));
    invoke(name, argsRef.current)
      .then((data) => live && setState({ data, loading: false, error: null }))
      .catch((e: unknown) => {
        if (!live) return;
        notify(`Could not load ${describe(name)}`, e);
        setState({
          data: null, loading: false,
          error: e instanceof Error ? e.message : String(e),
        });
      });
    return () => { live = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [name, nonce, enabled, ...deps]);

  const reload = useCallback(() => setNonce((n) => n + 1), []);
  return { ...state, reload };
}

/** `channels.list` → "the channel list", for a message a person reads. */
function describe(name: string): string {
  const [area, action] = name.split('.');
  const spaced = (action ?? '').replace(/([A-Z])/g, ' $1').toLowerCase().trim();
  return `the ${area} ${spaced}`.replace(/\s+/g, ' ').trim();
}
