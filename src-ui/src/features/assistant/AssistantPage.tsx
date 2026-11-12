/**
 * The assistant — a conversation about what to watch (README §11).
 *
 * What makes this worth having over the recommendation rail: it can be *asked*. "Something
 * under ninety minutes", "what's on now", "more like that but lighter", "how many films do
 * I actually have" — all questions the rail cannot answer because the rail is one answer to
 * one implicit question.
 *
 * Two things in here are deliberate and easy to get wrong:
 *
 * **The cards are the point.** Every title the assistant offers is a real library row with
 * a real id, resolved by the host, so pressing play works. Prose with film names in it
 * would look similar and be useless.
 *
 * **The working is shown.** A turn can take several seconds because the model may look
 * through the library a few times before answering. The steps arrive as events and are
 * listed while it thinks, which is the difference between a pause that reads as thinking
 * and one that reads as broken.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import type { ChatItem, ChatMessage } from '@shared/ipc';
import { Button, EmptyState, Poster, TextField } from '@/components/Primitives';
import { Icon } from '@/components/Icon';
import { useCommand } from '@/hooks/useCommand';
import { invoke, onAssistantStep } from '@/ipc';
import { notify } from '@/lib/errors';
import { activeProfileId } from '@/state/profile';

/** Openers, so a blank screen is not also a blank prompt. */
const SUGGESTIONS = [
  'What should I watch tonight?',
  "What's on right now?",
  'Something under 90 minutes',
  'A series I can start and finish this week',
];

function ItemCard({
  item,
  onPlay,
  onOpen,
}: {
  item: ChatItem;
  onPlay: (item: ChatItem) => void;
  onOpen: (item: ChatItem) => void;
}) {
  return (
    <div
      data-testid={`assistant-item-${item.kind}-${item.id}`}
      className="dl-panel"
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 10,
        padding: 12,
        minWidth: 0,
      }}
    >
      <div style={{ position: 'relative' }}>
        <Poster src={item.poster} alt={item.title} ratio={1.28} radius="var(--r-lg)" />
        <span
          aria-hidden
          style={{
            position: 'absolute', inset: 'auto 0 0 0', padding: '28px 16px 12px',
            borderRadius: '0 0 var(--r-lg) var(--r-lg)',
            background: 'linear-gradient(to top, rgb(22 22 26 / 0.55), transparent)',
            color: 'var(--on-art)', fontWeight: 800, fontSize: 'var(--fs-xl)',
            letterSpacing: '-0.01em', lineHeight: 1.15,
          }}
        >
          {item.title}
        </span>
        {item.kind !== 'live' && (
          <Button
            size="sm" variant="secondary" icon="info" aria-label={`About ${item.title}`}
            onClick={() => onOpen(item)}
            style={{ position: 'absolute', top: 10, right: 10, width: 36, padding: 0 }}
          />
        )}
      </div>
      <div style={{ minWidth: 0, display: 'grid', gap: 2, padding: '0 2px' }}>
        <span className="sr-only">{item.title}</span>
        <span style={{ fontSize: 'var(--fs-sm)', fontWeight: 600, color: 'var(--text-muted)' }}>
          {item.year != null && <>{item.year}{item.note ? ' · ' : ''}</>}
          {item.note}
        </span>
      </div>
      <Button
        variant="primary"
        icon="play"
        iconFilled
        data-testid={`assistant-play-${item.kind}-${item.id}`}
        onClick={() => onPlay(item)}
        style={{ width: '100%', marginTop: 'auto' }}
      >
        Play
      </Button>
    </div>
  );
}

function Bubble({
  message,
  onPlay,
  onOpen,
}: {
  message: ChatMessage;
  onPlay: (item: ChatItem) => void;
  onOpen: (item: ChatItem) => void;
}) {
  const mine = message.role === 'user';
  return (
    <div
      data-testid={`assistant-${message.role}`}
      style={{
        display: 'grid',
        gap: 'var(--sp-3)',
        justifyItems: mine ? 'end' : 'start',
      }}
    >
      <div
        style={
          // Daylight: the viewer's turn is an ink bubble on the right; the answer is
          // bold text on the page, with its picks as cards under it.
          mine
            ? {
                maxWidth: '40rem', padding: '10px 18px', borderRadius: 18,
                background: 'var(--text)', color: 'var(--bg)',
                fontWeight: 700, whiteSpace: 'pre-wrap', lineHeight: 1.45,
              }
            : {
                maxWidth: '52rem', fontWeight: 700, fontSize: 'var(--fs-lg)',
                whiteSpace: 'pre-wrap', lineHeight: 1.5, color: 'var(--text)',
              }
        }
      >
        {message.text}
      </div>
      {message.items.length > 0 && (
        <div
          style={{
            display: 'grid', gap: 16, width: '100%', maxWidth: 1000,
            gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))',
          }}
        >
          {message.items.map((item) => (
            <ItemCard key={`${item.kind}:${item.id}`} item={item} onPlay={onPlay} onOpen={onOpen} />
          ))}
        </div>
      )}
    </div>
  );
}

export function AssistantPage({
  onPlay,
  onOpen,
}: {
  /** Tune a channel or start a title. */
  onPlay: (item: ChatItem) => void;
  /** Open the detail modal for a film or show. */
  onOpen: (item: ChatItem) => void;
}) {
  const profileId = activeProfileId();
  const [nonce, setNonce] = useState(0);
  const { data: status } = useCommand('assistant.status', { profileId }, [profileId, nonce]);
  const { data: loaded } = useCommand('assistant.history', { profileId }, [profileId, nonce]);

  const [messages, setMessages] = useState<ChatMessage[] | null>(null);
  const [draft, setDraft] = useState('');
  const [thinking, setThinking] = useState(false);
  const [steps, setSteps] = useState<string[]>([]);
  const bottom = useRef<HTMLDivElement | null>(null);

  // The stored history is the starting point; everything after is local, so a reply
  // appears without refetching the whole conversation.
  useEffect(() => {
    if (loaded) setMessages(loaded);
  }, [loaded]);

  // The steps arrive while the turn is in flight.
  useEffect(() => onAssistantStep(({ step }) => setSteps((s) => [...s, step])), []);

  /**
   * A question carried in on the URL, asked once.
   *
   * This is what links the recommendation rail to the assistant: "Because of what you
   * watch" can hand over the thing it just suggested and the conversation starts there,
   * rather than dumping somebody on an empty chat and expecting them to retype what was
   * already on screen.
   *
   * Guarded by a ref rather than by clearing the hash. Navigating to replace the URL
   * remounts the route under `HashRouter`, which would ask again, and asking twice costs
   * a model call and puts the same question in the thread twice.
   */
  const askedFromUrl = useRef<string | null>(null);
  const [params] = useSearchParams();

  const shown = messages ?? [];
  useEffect(() => {
    bottom.current?.scrollIntoView({ behavior: 'smooth', block: 'end' });
  }, [shown.length, steps.length, thinking]);

  const send = useCallback(
    async (text: string) => {
      const asked = text.trim();
      if (!asked || thinking) return;
      setDraft('');
      setSteps([]);
      setThinking(true);
      // The question goes up immediately rather than when the answer lands: a model
      // taking six seconds should not also make it look like the typing was lost.
      const at = Math.floor(Date.now() / 1000);
      setMessages((m) => [...(m ?? []), { role: 'user', text: asked, items: [], at }]);
      try {
        const reply = await invoke('assistant.send', { profileId, text: asked });
        setMessages((m) => [...(m ?? []), reply.message]);
      } catch (e) {
        notify('The assistant could not answer', e);
        // The question stays on screen — it is still what they asked — but it is marked
        // so an unanswered one is not mistaken for a conversation that worked.
        setMessages((m) => [
          ...(m ?? []),
          {
            role: 'assistant',
            text: 'I could not answer that one. Try again, or check the key in Settings.',
            items: [],
            at: Math.floor(Date.now() / 1000),
          },
        ]);
      } finally {
        setThinking(false);
        setSteps([]);
      }
    },
    [profileId, thinking],
  );

  // Asked after `send` is defined, and only once the key is known — firing it before
  // `status` has arrived would send a question the host is about to refuse for want of
  // a key, and the refusal would be the first thing in the thread.
  const question = params.get('ask');
  useEffect(() => {
    if (!question || !status?.hasKey) return;
    if (askedFromUrl.current === question) return;
    askedFromUrl.current = question;
    void send(question);
  }, [question, status?.hasKey, send]);

  const clear = useCallback(async () => {
    try {
      await invoke('assistant.clear', { profileId });
      setMessages([]);
      setNonce((n) => n + 1);
    } catch (e) {
      notify('Could not clear the conversation', e);
    }
  }, [profileId]);

  const needsKey = status != null && !status.hasKey;

  const header = useMemo(
    () => (
      <header
        style={{
          display: 'flex',
          alignItems: 'baseline',
          justifyContent: 'space-between',
          gap: 'var(--sp-4)',
        }}
      >
        <div style={{ display: 'grid', gap: 4 }}>
          {/* The board says "Ask Aurora"; the heading's name stays "Assistant". */}
          <h1 className="dl-page-title">
            <span aria-hidden>Ask Aurora</span>
            <span className="sr-only">Assistant</span>
          </h1>
          <p style={{ margin: 0, color: 'var(--text-muted)', fontWeight: 500 }}>
            Ask for something to watch. It can look through your library and hand you
            titles to play.
          </p>
        </div>
        {shown.length > 0 && (
          <Button variant="outline" onClick={clear} data-testid="assistant-clear">
            Clear
          </Button>
        )}
      </header>
    ),
    [shown.length, clear],
  );

  if (needsKey) {
    return (
      <div style={{ padding: '4px 36px var(--sp-6)', display: 'grid', gap: 'var(--sp-5)' }}>
        {header}
        <EmptyState
          icon="sparkle"
          title="The assistant needs a key"
          body="Add a Gemini API key in Settings → Recommendations, and this becomes a conversation about your own library."
        />
      </div>
    );
  }

  return (
    <div
      style={{
        padding: '4px 36px 16px',
        display: 'grid',
        gap: 'var(--sp-5)',
        gridTemplateRows: 'auto 1fr auto',
        // The top bar shares the scroller with this page, so "the rest of the window" is
        // the scroller less the bar -- or the input ends up under the nav pill.
        height: 'calc(100% - var(--topbar-h))',
        minHeight: 0,
      }}
    >
      {header}

      <div
        data-testid="assistant-thread"
        style={{ display: 'grid', gap: 'var(--sp-5)', overflowY: 'auto', minHeight: 0, alignContent: 'start' }}
      >
        {shown.length === 0 && !thinking && (
          <div style={{ display: 'grid', gap: 'var(--sp-3)', justifyItems: 'start' }}>
            <p style={{ margin: 0, color: 'var(--text-muted)' }}>Try one of these:</p>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 'var(--sp-2)' }}>
              {SUGGESTIONS.map((s) => (
                <Button key={s} onClick={() => send(s)} data-testid="assistant-suggestion">
                  {s}
                </Button>
              ))}
            </div>
          </div>
        )}

        {shown.map((message, i) => (
          <Bubble key={`${message.at}:${i}`} message={message} onPlay={onPlay} onOpen={onOpen} />
        ))}

        {thinking && (
          <div
            data-testid="assistant-thinking"
            style={{
              display: 'grid',
              gap: 4,
              padding: '12px 18px',
              borderRadius: 18,
              background: 'var(--surface)',
              boxShadow: 'var(--shadow-1)',
              justifySelf: 'start',
              maxWidth: '46rem',
            }}
          >
            <span style={{ display: 'flex', alignItems: 'center', gap: 'var(--sp-2)' }}>
              <Icon name="sparkle" size={16} />
              <span style={{ color: 'var(--text-muted)' }}>Thinking…</span>
            </span>
            {steps.map((step, i) => (
              <span key={i} style={{ fontSize: 12, color: 'var(--text-faint)' }}>
                {step}
              </span>
            ))}
          </div>
        )}
        <div ref={bottom} />
      </div>

      <form
        onSubmit={(e) => {
          e.preventDefault();
          send(draft);
        }}
        className="dl-panel"
        style={{
          display: 'flex', gap: 'var(--sp-2)', alignItems: 'center', padding: 10,
          maxWidth: 780, width: '100%',
        }}
      >
        <div style={{ flex: 1, display: 'grid' }}>
        <TextField
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="Ask for something to watch…"
          aria-label="Ask the assistant"
          data-testid="assistant-input"
          style={{ background: 'transparent', fontSize: 'var(--fs-md)' }}
          disabled={thinking}
        />
        </div>
        <Button
          type="submit"
          variant="primary"
          disabled={thinking || draft.trim().length === 0}
          data-testid="assistant-send"
        >
          Ask
        </Button>
      </form>
    </div>
  );
}
