/**
 * Whether the window is fullscreen, and one way to change it.
 *
 * Shared rather than local to the button, because there are two ways in — the OSD button
 * and the `f` key — and a button that keeps its own copy of the answer starts lying as
 * soon as somebody uses the other one.
 *
 * The state is what the host reports back, not what was asked for: `window.fullscreen`
 * returns the state it ended up in, so a request the window manager refuses leaves the
 * label describing what is actually on screen.
 */
import { useCallback, useEffect, useState } from 'react';
import { invoke, isNativeHost } from '@/ipc';
import { report } from '@/lib/errors';

export function useFullscreen(): { fullscreen: boolean; toggle: () => void } {
  const [fullscreen, setFullscreen] = useState(false);

  // Escape and F11 leave fullscreen without going through either of our paths, and in a
  // browser the DOM is the only thing that knows. Under the native host this event does
  // not fire, and the returned value from `toggle` is what keeps the state honest.
  useEffect(() => {
    if (isNativeHost()) return;
    const sync = () => setFullscreen(document.fullscreenElement !== null);
    document.addEventListener('fullscreenchange', sync);
    return () => document.removeEventListener('fullscreenchange', sync);
  }, []);

  const toggle = useCallback(() => {
    void invoke('window.fullscreen', {})
      .then(setFullscreen)
      .catch(report('Could not change fullscreen'));
  }, []);

  return { fullscreen, toggle };
}
