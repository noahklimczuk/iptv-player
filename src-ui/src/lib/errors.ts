/**
 * Somewhere for a failure to go (README §17).
 *
 * The app had nowhere. Twenty-three of its twenty-seven `invoke` calls were
 * `void invoke(...)` with no `.catch`, so a host command that refused produced an
 * unhandled promise rejection and nothing else. In a browser that is a red line in a
 * console; inside WebView2 with no devtools it is a click that does nothing. Tuning a
 * channel whose every source was dead showed the banner, opened the player, and never
 * said why the picture was black.
 *
 * So: one place that turns a rejection into something a person can read, a subscriber
 * the shell renders, and a pair of window-level listeners for everything that escapes
 * anyway.
 */

export interface AppNotice {
  id: number;
  /** What the viewer was trying to do, in their terms. */
  title: string;
  /** What the host said, when it said anything useful. */
  detail?: string;
}

type Listener = (notices: AppNotice[]) => void;

let notices: AppNotice[] = [];
let nextId = 1;
const listeners = new Set<Listener>();

function emit(): void {
  const snapshot = notices;
  for (const l of listeners) l(snapshot);
}

export function subscribeToNotices(listener: Listener): () => void {
  listeners.add(listener);
  listener(notices);
  return () => {
    listeners.delete(listener);
  };
}

export function dismissNotice(id: number): void {
  notices = notices.filter((n) => n.id !== id);
  emit();
}

export function clearNotices(): void {
  notices = [];
  emit();
}

/**
 * A tune the viewer replaced before it finished.
 *
 * `AppError::Superseded` is the host saying "you pressed a second button" — it is the
 * expected outcome of zapping quickly, not a failure, and putting it on screen would
 * mean a toast for every double press of Ch+.
 */
function isSuperseded(message: string): boolean {
  return message.includes('superseded by a newer request');
}

/** Whatever was thrown, as a line of text. */
export function messageOf(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  if (error && typeof error === 'object' && 'message' in error) {
    return String((error as { message: unknown }).message);
  }
  return String(error);
}

/** Show a failure. Returns the notice, or null when it was not worth showing. */
export function notify(title: string, error?: unknown): AppNotice | null {
  const detail = error === undefined ? undefined : messageOf(error);
  if (detail !== undefined && isSuperseded(detail)) return null;

  const notice: AppNotice = { id: nextId++, title, detail };
  // Newest first, and bounded: a provider that is down can refuse every request on a
  // screen, and forty stacked toasts are a second failure rather than a report of the
  // first.
  notices = [notice, ...notices.filter((n) => n.title !== title)].slice(0, 3);
  emit();
  return notice;
}

/**
 * A `.catch` handler that says what the viewer was trying to do.
 *
 *     invoke('player.play', …).catch(report('Could not play BBC One'))
 *
 * Curried because that is the shape a `.catch` wants, and because the useful half of
 * the message — the intent — is known at the call site and nowhere else.
 */
export function report(title: string): (error: unknown) => void {
  return (error: unknown) => {
    notify(title, error);
  };
}

/**
 * Catch what escapes anyway.
 *
 * Even with every call site handling its own rejection, something will be missed, and
 * a missed one is currently invisible. These two listeners are the backstop: they
 * cannot say what the viewer was doing, so they say so plainly rather than guessing.
 */
export function installGlobalErrorHandlers(): () => void {
  const onRejection = (e: PromiseRejectionEvent) => {
    notify('Something went wrong', e.reason);
  };
  const onError = (e: ErrorEvent) => {
    notify('Something went wrong', e.error ?? e.message);
  };
  window.addEventListener('unhandledrejection', onRejection);
  window.addEventListener('error', onError);
  return () => {
    window.removeEventListener('unhandledrejection', onRejection);
    window.removeEventListener('error', onError);
  };
}
