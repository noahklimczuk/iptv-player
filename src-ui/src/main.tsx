import React from 'react';
import ReactDOM from 'react-dom/client';
import { HashRouter } from 'react-router-dom';
import App from './App';
import { ErrorBoundary } from './components/ErrorBoundary';
import { installGlobalErrorHandlers } from './lib/errors';
import { failNextMock, hangMock, invoke, setSilentDurationMock } from './ipc';
import './styles/app.css';

// Before the first render, so a failure during mount has somewhere to go too.
installGlobalErrorHandlers();

// Test hook: lets end-to-end tests arrange state through the same command surface
// the UI uses, rather than reaching into internals.
(window as unknown as { __auroraInvoke?: unknown }).__auroraInvoke = invoke;
// …and lets them make one refuse, so "the failure reaches the viewer" is testable.
(window as unknown as { __auroraFailNext?: unknown }).__auroraFailNext = failNextMock;
// …and lets them play something whose duration the stream never reports, which is the
// ordinary case on provider VOD and the one in which Skip Credits and autoplay used to
// be dead.
(window as unknown as { __auroraSilentDuration?: unknown }).__auroraSilentDuration =
  setSilentDurationMock;
// …and lets them make one go quiet rather than refuse, which is the failure the boot
// screen exists to survive and the one nothing could express before.
(window as unknown as { __auroraHang?: unknown }).__auroraHang = hangMock;

// …and on the query string as well as on `window`, because the commands that decide the
// first screen are already in flight by the time a test can evaluate anything. Same
// mechanism as `?video`. `?hang=providers.list,profiles.list` silences those; `?bootStuck`
// shortens the boot screen's own patience so asserting on it does not cost 20 seconds.
{
  const params = new URLSearchParams(window.location.search);
  for (const name of params.get('hang')?.split(',') ?? []) {
    if (name.trim()) hangMock(name.trim());
  }
  const stuck = Number(params.get('bootStuck'));
  if (Number.isFinite(stuck) && stuck > 0) {
    (window as unknown as { __auroraBootStuckMs?: number }).__auroraBootStuckMs = stuck;
  }
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ErrorBoundary>
      <HashRouter>
        <App />
      </HashRouter>
    </ErrorBoundary>
  </React.StrictMode>,
);

/**
 * Tell the host there is something on screen worth showing.
 *
 * The window is created hidden, because it is transparent so mpv can composite behind
 * it, and a transparent window with an unpainted WebView2 in it shows the desktop.
 *
 * Two frames, not zero: `render` only schedules the work, so a call on this line would
 * reveal the window before React has committed anything to it. One frame gets the commit
 * scheduled and the second runs after the paint that follows it.
 *
 * Not awaited and never surfaced. The host shows the window by itself a few seconds in,
 * so the worst a failure here can cost is that fallback — and a toast about a window
 * that is plainly visible would be its own small absurdity.
 */
requestAnimationFrame(() => {
  requestAnimationFrame(() => {
    void invoke('window.ready').catch(() => {});
  });
});
