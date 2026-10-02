import React from 'react';
import ReactDOM from 'react-dom/client';
import { HashRouter } from 'react-router-dom';
import App from './App';
import { ErrorBoundary } from './components/ErrorBoundary';
import { installGlobalErrorHandlers } from './lib/errors';
import { failNextMock, invoke } from './ipc';
import './styles/app.css';

// Before the first render, so a failure during mount has somewhere to go too.
installGlobalErrorHandlers();

// Test hook: lets end-to-end tests arrange state through the same command surface
// the UI uses, rather than reaching into internals.
(window as unknown as { __auroraInvoke?: unknown }).__auroraInvoke = invoke;
// …and lets them make one refuse, so "the failure reaches the viewer" is testable.
(window as unknown as { __auroraFailNext?: unknown }).__auroraFailNext = failNextMock;

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
