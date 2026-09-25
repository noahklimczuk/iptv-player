/**
 * The last thing between a render-time throw and a black window.
 *
 * React unmounts the whole tree when a render throws and there is nobody to catch it.
 * In a browser that leaves a blank page and a stack trace in the console; inside
 * WebView2 there is no console to look at, so it leaves a blank window and nothing
 * else. This turns that into a sentence and a way out.
 */
import { Component, type ErrorInfo, type ReactNode } from 'react';

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    // Not swallowed: a release build has no console, but it does have a host that
    // logs, and this is the one place that knows a component tree died.
    // eslint-disable-next-line no-console
    console.error('Aurora UI crashed', error, info.componentStack);
  }

  render(): ReactNode {
    const { error } = this.state;
    if (!error) return this.props.children;

    return (
      <div
        role="alert"
        style={{
          position: 'fixed', inset: 0, display: 'grid', placeItems: 'center',
          background: 'var(--bg, #0b0b12)', color: 'var(--text, #f4f4f8)',
          padding: 'var(--sp-6, 32px)', textAlign: 'center',
        }}
      >
        <div style={{ maxWidth: 560, display: 'grid', gap: 'var(--sp-4, 20px)' }}>
          <h1 style={{ margin: 0, fontSize: 'var(--fs-xl, 22px)', fontWeight: 800 }}>
            Aurora ran into a problem
          </h1>
          <p style={{ margin: 0, color: 'var(--text-muted, #a6a6b8)' }}>
            The screen you were on could not be drawn. Reloading usually clears it; if
            it keeps happening, the log beside your library has the details.
          </p>
          <pre
            style={{
              margin: 0, padding: 'var(--sp-3, 14px)', textAlign: 'left',
              background: 'var(--surface, #16161f)', borderRadius: 'var(--r-md, 10px)',
              fontSize: 'var(--fs-xs, 12px)', overflow: 'auto', maxHeight: 180,
              whiteSpace: 'pre-wrap', wordBreak: 'break-word',
            }}
          >
            {error.message}
          </pre>
          <div>
            <button
              type="button"
              onClick={() => window.location.reload()}
              style={{
                padding: '10px 22px', borderRadius: 'var(--r-full, 999px)',
                border: 'none', cursor: 'pointer', fontWeight: 650,
                background: 'var(--text, #f4f4f8)', color: 'var(--text-invert, #0b0b12)',
              }}
            >
              Reload Aurora
            </button>
          </div>
        </div>
      </div>
    );
  }
}
