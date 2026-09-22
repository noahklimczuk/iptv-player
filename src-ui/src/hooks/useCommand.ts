import { useCallback, useEffect, useRef, useState } from 'react';
import type { CommandArgs, CommandName, CommandResult } from '@shared/ipc';
import { invoke } from '@/ipc';

interface State<T> {
  data: T | null;
  loading: boolean;
  error: string | null;
}

/** Fetch once (and on dependency change) from the host, with loading and error states. */
export function useCommand<K extends CommandName>(
  name: K,
  args: CommandArgs<K>,
  deps: unknown[] = [],
): State<CommandResult<K>> & { reload: () => void } {
  const [state, setState] = useState<State<CommandResult<K>>>({
    data: null, loading: true, error: null,
  });
  const [nonce, setNonce] = useState(0);
  const argsRef = useRef(args);
  argsRef.current = args;

  useEffect(() => {
    let live = true;
    setState((s) => ({ ...s, loading: true }));
    invoke(name, argsRef.current)
      .then((data) => live && setState({ data, loading: false, error: null }))
      .catch((e: unknown) =>
        live && setState({
          data: null, loading: false,
          error: e instanceof Error ? e.message : String(e),
        }));
    return () => { live = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [name, nonce, ...deps]);

  const reload = useCallback(() => setNonce((n) => n + 1), []);
  return { ...state, reload };
}
