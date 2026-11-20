/**
 * `data-scrolling` on the root while anything scrolls, so the glass can stand down.
 *
 * Reported as "scrolling on the home page is very laggy". A `backdrop-filter` samples
 * what is behind it, so a glass surface over content that moves is re-blurred on every
 * frame: the top bar's frosted fade, the tab bar and the search pill (whose lens is an
 * SVG displacement on top of the blur). Measured on Home, frames over 25 ms while
 * scrolling: 62 to 108 as shipped, 31 with every backdrop filter off; the lens alone
 * or the bar alone did not move it out of the noise. So while a scroll is under way
 * the glass is drawn as a tint (`styles/glass.css`) and the billboard's slow zoom holds
 * still (`styles/hero.css`), and both come back once it has stopped for a moment.
 * Content in motion hides the difference. In the real app the two together took a
 * scroll down Home from frames over 50 ms in every run to none.
 *
 * One listener on the document, in the capture phase so it hears every scroller (scroll
 * does not bubble), passive so it can never hold a scroll up. It writes to the DOM only
 * on the edges -- once when a scroll starts, once when it settles -- never per event.
 */
export const SETTLE_MS = 160;

export function installScrollingFlag(doc: Document = document, settleMs = SETTLE_MS): () => void {
  const root = doc.documentElement;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const settle = () => {
    timer = undefined;
    delete root.dataset.scrolling;
  };
  const onScroll = () => {
    if (timer === undefined) root.dataset.scrolling = '';
    else clearTimeout(timer);
    timer = setTimeout(settle, settleMs);
  };
  doc.addEventListener('scroll', onScroll, { capture: true, passive: true });
  return () => {
    doc.removeEventListener('scroll', onScroll, { capture: true });
    if (timer !== undefined) clearTimeout(timer);
    delete root.dataset.scrolling;
  };
}
