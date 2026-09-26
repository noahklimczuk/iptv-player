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

/**
 * Every host event in this app was dead, and this is where.
 *
 * The listener used to be looked up as `window.__TAURI__.event`. That global only
 * exists when `app.withGlobalTauri` is set in `tauri.conf.json`, and it is not —
 * so the lookup returned null and `subscribe` returned a do-nothing unsubscribe,
 * silently, on every call.
 *
 * Nothing caught it. `invoke` reaches the bridge through a *different* object,
 * `window.__TAURI_INTERNALS__`, which Tauri always injects — so commands worked
 * perfectly while every event was discarded, and `isNativeHost()` (which tests for
 * the internals) cheerfully reported a real host. In the browser the mock has its own
 * event fan-out and never goes near this file, so all 98 journeys passed.
 *
 * What a viewer saw: the OSD stuck on "Nothing playing" with a real stream running
 * behind it, because `player.state` never arrived — so every transport button was
 * drawn from empty state and the whole player looked dead. An update that downloaded
 * with the progress bar at zero forever. A refresh with a spinner that never moved.
 * DVR, metadata and artwork progress, all the same.
 *
 * `@tauri-apps/api` is the supported way to do this and is versioned alongside the
 * Rust crate, which is the point: the internals it wraps are free to change between
 * versions, and reaching past it by hand is what broke this in the first place.
 *
 * And a second fault underneath it, which the first one hid. Tauri 2 will not accept
 * an event name containing a `.`:
 *
 *     Event name must include only alphanumeric characters, `-`, `/`, `:` and `_`.
 *
 * Every event this app defines is dotted — `player.state`, `ingest.progress`,
 * `update.download`, `dvr.tick` — so both ends were being refused: `listen` rejected
 * the subscription, and on the host `emit` returned `Err(IllegalEventName)` into a
 * `let _ =`. Fixing only the global would have changed nothing.
 */
import { listen } from '@tauri-apps/api/event';

/**
 * The name an event travels under, which is not the name the app calls it.
 *
 * Mirrors `aurora_app::wire_event_name`. The dotted names stay: they are what the
 * `Events` map in `shared/ipc.ts` is keyed by, what the mock transport fans out, and
 * what every call site reads. Only the wire is different, and only in these two
 * places, so the two ends cannot drift apart.
 */
export const wireEventName = (event: string): string => event.replace(/\./g, ':');

/**
 * Listen to a host event. Returns the unsubscribe, which is safe to call at any point
 * — including before the subscription has finished being made.
 */
export function subscribe<T>(event: string, fn: (payload: T) => void): () => void {
  let cancelled = false;
  let dispose: (() => void) | undefined;

  void listen<T>(wireEventName(event), (e) => {
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
      // act on, and the screen still works without the live updates. Not silent
      // though: this is the failure that hid for so long.
      console.error(`aurora: could not subscribe to ${event}`);
    });

  return () => {
    cancelled = true;
    dispose?.();
    dispose = undefined;
  };
}
