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
