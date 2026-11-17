/**
 * Whether the window is fullscreen, and the ways to change it (C14–C16).
 *
 * One copy of the answer for the whole app, because there are several ways in (the
 * dock's button, F, F11, a double click on the picture) and one way out that nobody
 * presses: leaving the player leaves fullscreen (C15).
 *
 * The state is what the window ended up in, not what was asked for: `window.fullscreen`
 * answers with the state it reached, and in a browser `fullscreenchange` reports Escape
 * and F11 handled by the browser itself.
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke, isNativeHost } from '@/ipc';
import { report } from '@/lib/errors';

export interface Fullscreen {
  fullscreen: boolean;
  toggle: () => void;
  /** Leave fullscreen if in it. Safe to call at any time, as often as wanted. */
  leave: () => void;
}

export function useFullscreen(): Fullscreen {
  const [fullscreen, setFullscreen] = useState(false);
  const known = useRef(false);
  known.current = fullscreen;

  useEffect(() => {
    if (isNativeHost()) return;
    const sync = () => setFullscreen(document.fullscreenElement !== null);
    document.addEventListener('fullscreenchange', sync);
    return () => document.removeEventListener('fullscreenchange', sync);
  }, []);

  const set = useCallback((want: boolean | undefined) => {
    void invoke('window.fullscreen', want === undefined ? {} : { fullscreen: want })
      .then(setFullscreen)
      .catch(report('Could not change fullscreen'));
  }, []);

  const toggle = useCallback(() => set(undefined), [set]);

  const leave = useCallback(() => {
    // Under the native host the window may be fullscreen without this hook having
    // seen it happen, so it is always asked; the command is idempotent. In a browser
    // the document knows, and leaving when not fullscreen is a refused promise.
    if (isNativeHost() ? true : document.fullscreenElement !== null || known.current) {
      // Marked at once, not when the window answers: a second Escape pressed before the
      // answer arrives is the viewer leaving the player, not leaving fullscreen again.
      known.current = false;
      setFullscreen(false);
      set(false);
    }
  }, [set]);

  return { fullscreen, toggle, leave };
}
