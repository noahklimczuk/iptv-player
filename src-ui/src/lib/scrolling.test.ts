import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { installScrollingFlag } from './scrolling';

/**
 * The unit tests run without a DOM (`vitest.config.ts`), and all this needs of one is
 * somewhere to listen and a root to flag. The capture phase is what lets it hear an
 * inner scroller, so the stand-in records the options it was given.
 */
class FakeDocument extends EventTarget {
  documentElement = { dataset: {} as Record<string, string> };
  options: unknown;
  override addEventListener(type: string, fn: EventListener, options?: AddEventListenerOptions) {
    this.options = options;
    super.addEventListener(type, fn, options);
  }
}

describe('the scrolling flag', () => {
  let doc: FakeDocument;
  let stop: () => void;
  const flagged = () => doc.documentElement.dataset.scrolling !== undefined;
  const scroll = () => doc.dispatchEvent(new Event('scroll'));

  beforeEach(() => {
    vi.useFakeTimers();
    doc = new FakeDocument();
    stop = installScrollingFlag(doc as unknown as Document, 100);
  });
  afterEach(() => {
    stop();
    vi.useRealTimers();
  });

  it('is up while a scroll is under way and down once it settles', () => {
    expect(flagged()).toBe(false);
    scroll();
    expect(flagged()).toBe(true);
    vi.advanceTimersByTime(60);
    scroll();
    vi.advanceTimersByTime(60);
    // Still scrolling: each event pushes the settle back.
    expect(flagged()).toBe(true);
    vi.advanceTimersByTime(50);
    expect(flagged()).toBe(false);
  });

  it('listens in the capture phase, since scroll does not bubble, and never blocks', () => {
    expect(doc.options).toEqual({ capture: true, passive: true });
  });

  it('leaves nothing behind when it is removed mid-scroll', () => {
    scroll();
    stop();
    expect(flagged()).toBe(false);
    scroll();
    expect(flagged()).toBe(false);
    stop = () => {};
  });
});
