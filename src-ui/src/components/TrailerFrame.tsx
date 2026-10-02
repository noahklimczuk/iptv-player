/**
 * A trailer, actually playing.
 *
 * Three places claimed to be playing one and none of them were. The hero billboard
 * printed "NOW PLAYING TRAILER" over a slowly zooming backdrop with a mute button bound
 * to a state variable that drove no audio; the rail cards said "Preview playing" after a
 * dwell; the detail modal offered nothing at all. Nothing fetched a trailer and nothing
 * played one.
 *
 * **Why an iframe and not mpv.** mpv cannot open a YouTube page without yt-dlp beside
 * it, which is a third-party binary in the installer that has to be kept current or it
 * stops working — a real cost for a background preview. WebView2 can play the embed
 * directly, and the embed is what TMDB's data is for: it gives a `key`, which is a
 * YouTube id and nothing else.
 *
 * **The video surface is behind this page**, so the iframe composites over it exactly
 * like any other element. It must never be mounted while something is actually playing,
 * which is the caller's business — every caller here is a browse screen that the player
 * overlay covers.
 */
import { useEffect, useRef, useState } from 'react';

/** Where the embed is served from. The `-nocookie` host sets no tracking cookie. */
const EMBED_ORIGIN = 'https://www.youtube-nocookie.com';

export function TrailerFrame({
  trailerKey,
  muted,
  title,
  controls = false,
  loop = true,
  radius,
}: {
  /** The YouTube id. The caller has already decided there is one. */
  trailerKey: string;
  /**
   * Muted autoplay is the only autoplay a browser engine allows, so a preview that is
   * meant to start by itself must start silent. Changing this after mount does not
   * remount the frame — it is sent as a command, so unmuting does not restart the
   * trailer from the beginning.
   */
  muted: boolean;
  /** For the iframe's accessible name; screen readers otherwise read the URL. */
  title: string;
  /** YouTube's own controls. Off for a background preview, on when it is the subject. */
  controls?: boolean;
  /** Loop a short preview; off when somebody asked to watch the trailer. */
  loop?: boolean;
  radius?: string;
}) {
  const frame = useRef<HTMLIFrameElement>(null);
  /**
   * Whether the frame has navigated to the embed yet.
   *
   * Before it has, its window is still `about:blank`, whose origin is not YouTube's — and
   * `postMessage` with a target origin that does not match the recipient throws. It did
   * so on every mount, into the console, for a command the URL's own `mute` parameter had
   * already applied.
   */
  const [ready, setReady] = useState(false);

  // Mute and unmute through the iframe API rather than through the URL, because a URL
  // change reloads the frame and the trailer would jump back to its first frame every
  // time somebody pressed the speaker. `enablejsapi=1` below is what allows this, and
  // `postMessage` is the whole of the API that is needed — loading YouTube's helper
  // script is not possible here anyway, since the CSP allows scripts from `self` only.
  useEffect(() => {
    if (!ready) return;
    const win = frame.current?.contentWindow;
    if (!win) return;
    const send = (func: 'mute' | 'unMute') => {
      try {
        win.postMessage(JSON.stringify({ event: 'command', func, args: [] }), EMBED_ORIGIN);
      } catch {
        // A frame that has not finished loading rejects this; the URL's own `mute`
        // parameter already has it right for the initial state, which is the one that
        // matters for autoplay.
      }
    };
    send(muted ? 'mute' : 'unMute');
  }, [muted, ready]);

  // Everything here is deliberate:
  //   autoplay=1   a preview nobody asked to start
  //   mute=1/0     the initial state; later changes go through postMessage above
  //   controls     off for a background preview — it is not something to be scrubbed
  //   loop+playlist  YouTube needs the id repeating in `playlist` for `loop` to work
  //   modestbranding, rel=0  no logo, and no grid of other videos at the end
  //   playsinline  so a small screen does not take the video fullscreen by itself
  //   disablekb    the app owns the keyboard; YouTube's bindings would fight the hotkeys
  //   iv_load_policy=3  no annotation cards over the picture
  const src =
    `${EMBED_ORIGIN}/embed/${encodeURIComponent(trailerKey)}` +
    `?autoplay=1&mute=${muted ? 1 : 0}&controls=${controls ? 1 : 0}` +
    `${loop ? `&loop=1&playlist=${encodeURIComponent(trailerKey)}` : ''}` +
    '&modestbranding=1&rel=0&playsinline=1&disablekb=1&iv_load_policy=3&enablejsapi=1';

  return (
    <iframe
      ref={frame}
      data-testid="trailer-frame"
      title={title}
      src={src}
      onLoad={() => setReady(true)}
      // No `allowFullScreen`: fullscreen here is the window's job (`window.fullscreen`),
      // and an iframe going fullscreen by itself would cover the app's own chrome with
      // YouTube's.
      allow="autoplay; encrypted-media"
      referrerPolicy="strict-origin-when-cross-origin"
      style={{
        width: '100%',
        height: '100%',
        border: 'none',
        borderRadius: radius,
        // A preview is scenery: clicks belong to the card or the hero underneath it, not
        // to YouTube's player. The detail modal turns this back on by passing `controls`.
        pointerEvents: controls ? 'auto' : 'none',
        display: 'block',
      }}
    />
  );
}
