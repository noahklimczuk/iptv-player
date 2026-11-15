/**
 * The floating tab bar, as iOS draws it.
 *
 * Three things it has to keep doing, because the suite and a remote control both depend
 * on them: the accessible name stays "Main", every destination stays a link with its
 * label as its accessible name, and tab order stays the reading order.
 *
 * What makes it Apple's rather than a row of links on glass:
 *   - every tab is the same width, labels in the primary colour, the current one in
 *     the accent colour on a soft platter -- not a solid block;
 *   - the platter is one element that slides to the tab chosen, on a spring;
 *   - pressing the bar makes the platter swell into a lens, and dragging moves it from
 *     tab to tab under the pointer; letting go there goes there.
 *
 * Colours and the material are `.glass` and `.nav-glass` in styles/glass.css.
 */
import { useCallback, useLayoutEffect, useRef, useState } from 'react';
import { NavLink, useLocation, useNavigate } from 'react-router-dom';
import { Icon, type IconName } from '@/components/Icon';
import { useLiquidGlass } from '@/components/liquidGlass';

export interface TabItem { to: string; icon: IconName; label: string }

/** How far a press must travel, in pixels, before it is a drag rather than a click. */
const DRAG_SLOP = 6;

const tabsOf = (nav: HTMLElement | null) =>
  Array.from(nav?.querySelectorAll<HTMLAnchorElement>('a[data-tab]') ?? []);

export function TabBar({
  items, hidden, onNavigate,
}: {
  items: TabItem[];
  /** While a picture is behind the page, the chrome is not drawn over it. */
  hidden: boolean;
  onNavigate: () => void;
}) {
  const nav = useLiquidGlass<HTMLElement>({ bezel: 20 });
  const platter = useRef<HTMLDivElement>(null);
  const location = useLocation();
  const navigate = useNavigate();
  const [pressed, setPressed] = useState(false);
  const [dragging, setDragging] = useState(false);
  const drag = useRef<{ x: number; moved: boolean; id: number } | null>(null);
  const suppressClick = useRef(false);

  /** Put the platter on a tab, or under the pointer while dragging. */
  const place = useCallback((left: number, width: number) => {
    const el = platter.current;
    if (!el) return;
    el.style.transform = `translateX(${left}px)`;
    el.style.width = `${width}px`;
    el.style.opacity = '1';
  }, []);

  const toCurrent = useCallback(() => {
    const current = tabsOf(nav.current).find((a) => a.getAttribute('aria-current') === 'page');
    if (!current) {
      if (platter.current) platter.current.style.opacity = '0';
      return;
    }
    place(current.offsetLeft, current.offsetWidth);
  }, [place, nav]);

  useLayoutEffect(() => {
    if (hidden) return;
    toCurrent();
    const el = nav.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => toCurrent());
    ro.observe(el);
    return () => ro.disconnect();
  }, [location.pathname, hidden, toCurrent, nav]);

  const nearest = (clientX: number) => {
    let best: HTMLAnchorElement | null = null;
    let gap = Infinity;
    for (const a of tabsOf(nav.current)) {
      const b = a.getBoundingClientRect();
      const d = Math.abs(clientX - (b.left + b.width / 2));
      if (d < gap) { gap = d; best = a; }
    }
    return best;
  };

  const onPointerDown = (e: React.PointerEvent<HTMLElement>) => {
    if (e.button !== 0) return;
    drag.current = { x: e.clientX, moved: false, id: e.pointerId };
    setPressed(true);
  };
  const onPointerMove = (e: React.PointerEvent<HTMLElement>) => {
    const d = drag.current;
    const el = nav.current;
    if (!d || !el || d.id !== e.pointerId) return;
    if (!d.moved && Math.abs(e.clientX - d.x) < DRAG_SLOP) return;
    if (!d.moved) {
      d.moved = true;
      setDragging(true);
      el.setPointerCapture?.(e.pointerId);
    }
    const tab = nearest(e.clientX);
    const width = tab?.offsetWidth ?? 80;
    const box = el.getBoundingClientRect();
    // Follow the pointer, held inside the bar.
    const x = e.clientX - box.left + el.scrollLeft - width / 2;
    const max = el.scrollWidth - width - 6;
    place(Math.min(Math.max(x, 6), max), width);
  };
  const end = (e: React.PointerEvent<HTMLElement>, commit: boolean) => {
    const d = drag.current;
    drag.current = null;
    setPressed(false);
    setDragging(false);
    if (!d || !d.moved) return;
    nav.current?.releasePointerCapture?.(e.pointerId);
    // The click that follows a drag lands on whatever was under the pointer; the drag
    // has already decided where to go.
    suppressClick.current = true;
    window.setTimeout(() => { suppressClick.current = false; }, 0);
    const tab = commit ? nearest(e.clientX) : null;
    const to = tab?.dataset.tab;
    if (to && to !== location.pathname) {
      onNavigate();
      navigate(to);
    } else {
      toCurrent();
    }
  };

  return (
    <nav
      ref={nav}
      aria-label="Main"
      data-testid="nav-pill"
      data-pressed={pressed || undefined}
      data-dragging={dragging || undefined}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={(e) => end(e, true)}
      onPointerCancel={(e) => end(e, false)}
      onClickCapture={(e) => {
        if (suppressClick.current) { e.preventDefault(); e.stopPropagation(); }
      }}
      style={{
        position: 'fixed',
        bottom: 20,
        left: '50%',
        transform: 'translateX(-50%)',
        maxWidth: 'calc(100vw - var(--sp-6) * 2)',
        height: 'var(--navpill-h)',
        display: hidden ? 'none' : 'flex',
        alignItems: 'center',
        gap: 0,
        padding: 6,
        borderRadius: 'var(--r-pill)',
        zIndex: 50,
        // It scrolls horizontally rather than wrapping or shrinking: a narrow window
        // cannot show every destination, and a bar that reflowed to two lines would
        // stop being a bar.
        overflowX: 'auto',
        scrollbarWidth: 'none',
        touchAction: 'pan-x',
        userSelect: 'none',
      }}
      className="no-scrollbar glass nav-glass"
    >
      <div ref={platter} aria-hidden className="nav-platter" style={{ opacity: 0 }} />
      {items.map((n) => (
        <NavLink
          key={n.to}
          to={n.to}
          end={n.to === '/'}
          data-tab={n.to}
          onClick={onNavigate}
          draggable={false}
          style={{
            display: 'grid',
            placeItems: 'center',
            alignContent: 'center',
            gap: 3,
            height: '100%',
            // The same width for every tab, as on iOS.
            width: 84,
            flexShrink: 0,
            padding: '0 8px',
            borderRadius: 'calc(var(--r-pill) - 6px)',
            textDecoration: 'none',
            // 11px at desktop density, larger in TV mode.
            fontSize: 'calc(var(--fs-xs) - 1px)',
            fontWeight: 650,
            letterSpacing: '0.01em',
            whiteSpace: 'nowrap',
            transition: 'color var(--t-fast) var(--ease)',
          }}
        >
          <Icon name={n.icon} size={21} strokeWidth={2} />
          {n.label}
        </NavLink>
      ))}
    </nav>
  );
}
