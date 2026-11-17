/**
 * When the controls are on screen (C1–C5).
 *
 * Any pointer move, press or key shows them. They hide once `mayHide` says so: after
 * 3.2 s without input, 5 s with the pointer resting on them, and never while something
 * is held down, a menu is being read or the content is paused. A key that wakes them is
 * not swallowed (C4): this only listens, and the key's own handler still runs.
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import { isTyping } from '@/hooks/useHotkeys';
import { mayHide, nextCheckMs, type Holds } from './model';

/** Marks an element as part of the controls, for "is the pointer on the controls". */
export const CHROME_ATTR = 'data-osd-chrome';

export interface Chrome {
  visible: boolean;
  /** Show the controls and restart the countdown. */
  wake: () => void;
  /** Tell the hook whether a menu is open, so reading it holds the controls (C2). */
  setMenuOpen: (open: boolean) => void;
  /** Spread on the player's root element. */
  root: {
    onPointerMove: () => void;
    onPointerDown: () => void;
    onPointerOver: (e: React.PointerEvent) => void;
    onPointerLeave: () => void;
  };
}

interface Inputs { lastInput: number; over: boolean; pressed: boolean; menu: boolean; paused: boolean }

function holdsOf(s: Inputs): Holds {
  return {
    idleMs: performance.now() - s.lastInput,
    overControls: s.over,
    pressed: s.pressed,
    readingMenu: s.menu && s.over,
    paused: s.paused,
  };
}

export function useChrome({ paused, onHide }: { paused: boolean; onHide: () => void }): Chrome {
  const [visible, setVisible] = useState(true);
  const state = useRef<Inputs>({ lastInput: performance.now(), over: false, pressed: false, menu: false, paused });
  state.current.paused = paused;
  const hidden = useRef(onHide);
  hidden.current = onHide;
  const timer = useRef<number | undefined>(undefined);

  const schedule = useCallback(() => {
    window.clearTimeout(timer.current);
    const check = () => {
      const h = holdsOf(state.current);
      if (mayHide(h)) {
        setVisible(false);
        hidden.current();
        return;
      }
      timer.current = window.setTimeout(check, nextCheckMs(h));
    };
    timer.current = window.setTimeout(check, nextCheckMs(holdsOf(state.current)));
  }, []);

  const wake = useCallback(() => {
    state.current.lastInput = performance.now();
    setVisible(true);
    schedule();
  }, [schedule]);

  // Pausing holds the controls; resuming starts the countdown again from now.
  useEffect(() => {
    if (!paused) wake();
  }, [paused, wake]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!isTyping(e.target)) wake();
    };
    const released = () => { state.current.pressed = false; };
    window.addEventListener('keydown', onKey);
    window.addEventListener('pointerup', released);
    window.addEventListener('pointercancel', released);
    wake();
    return () => {
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('pointerup', released);
      window.removeEventListener('pointercancel', released);
      window.clearTimeout(timer.current);
    };
  }, [wake]);

  const setMenuOpen = useCallback((open: boolean) => { state.current.menu = open; }, []);

  const root = {
    onPointerMove: wake,
    onPointerDown: () => {
      state.current.pressed = true;
      wake();
    },
    onPointerOver: (e: React.PointerEvent) => {
      state.current.over = !!(e.target as Element).closest?.(`[${CHROME_ATTR}]`);
    },
    onPointerLeave: () => { state.current.over = false; },
  };

  return { visible, wake, setMenuOpen, root };
}
