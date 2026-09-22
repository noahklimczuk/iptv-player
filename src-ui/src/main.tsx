import React from 'react';
import ReactDOM from 'react-dom/client';
import { HashRouter } from 'react-router-dom';
import App from './App';
import { invoke } from './ipc';
import './styles/app.css';

// Test hook: lets end-to-end tests arrange state through the same command surface
// the UI uses, rather than reaching into internals.
(window as unknown as { __auroraInvoke?: unknown }).__auroraInvoke = invoke;

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <HashRouter>
      <App />
    </HashRouter>
  </React.StrictMode>,
);
