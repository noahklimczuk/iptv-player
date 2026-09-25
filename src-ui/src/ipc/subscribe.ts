/**
 * One place that listens to a host event, because six copies of it all leaked.
 *
 * `listen()` is asynchronous: it returns a promise that later yields the function
 * that unsubscribes. The six wrappers in `ipc/index.ts` each did
 *
 *     let dispose: (() => void) | undefined;
 *     void listen(event, fn).then((d) => { dispose = d; });
 *     return () => dispose?.();
 *
 * which is correct only if the component outlives the round trip. A component that
 * unmounts first runs the returned cleanup while `dispose` is still undefined — so it
 * does nothing — and the listener is then registered a moment later with nobody
 * holding its unsubscribe. It never comes off.
 *
 * That is not a rare interleaving. Opening and closing Settings faster than an IPC
 * round trip is easy, and it was *easiest* exactly when the round trip was slow: while
 * a long refresh was running, which is when the Updates panel is watching. Each cycle
 * left one more live listener holding its dead component's captured state.
 *
 * Tracking cancellation and disposing on arrival is the whole fix.
 */

interface TauriEvents {
  listen: <T>(event: string, cb: (p: { payload: T }) => void) => Promise<() => void>;
}

function events(): TauriEvents | null {
  const w = window as unknown as { __TAURI__?: { event?: TauriEvents } };
  return w.__TAURI__?.event ?? null;
}

/**
 * Listen to a host event. Returns the unsubscribe, which is safe to call at any point
 * — including before the subscription has finished being made.
 */
export function subscribe<T>(event: string, fn: (payload: T) => void): () => void {
  const api = events();
  if (!api) return () => {};

  let cancelled = false;
  let dispose: (() => void) | undefined;

  void api
    .listen<T>(event, (e) => {
      // A payload that arrives between teardown and the dispose below belongs to a
      // component that is gone.
      if (!cancelled) fn(e.payload);
    })
    .then((d) => {
      if (cancelled) d();
      else dispose = d;
    })
    .catch(() => {
      // A host that will not take the subscription is not something the viewer can
      // act on, and the screen still works without the live updates.
    });

  return () => {
    cancelled = true;
    dispose?.();
    dispose = undefined;
  };
}
