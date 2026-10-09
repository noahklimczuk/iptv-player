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
      style={{
        display: 'flex',
        gap: 'var(--sp-3)',
        padding: 'var(--sp-3)',
        borderRadius: 'var(--r-md)',
        background: 'color-mix(in srgb, var(--surface) 70%, transparent)',
        border: '1px solid var(--border)',
        alignItems: 'center',
      }}
    >
      <div style={{ width: 48, flexShrink: 0 }}>
        <Poster src={item.poster} alt={item.title} ratio={item.kind === 'live' ? 1 : 2 / 3} />
      </div>
      <div style={{ flex: 1, minWidth: 0, display: 'grid', gap: 2 }}>
        <span style={{ fontWeight: 600, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {item.title}
          {item.year != null && (
            <span style={{ color: 'var(--text-faint)', fontWeight: 400 }}> · {item.year}</span>
          )}
        </span>
        {item.note && (
          <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>{item.note}</span>
        )}
      </div>
      <Button
        size="sm"
        variant="primary"
        icon="play"
        data-testid={`assistant-play-${item.kind}-${item.id}`}
        onClick={() => onPlay(item)}
      >
        Play
      </Button>
      {item.kind !== 'live' && (
        <Button size="sm" variant="ghost" icon="info" aria-label={`About ${item.title}`} onClick={() => onOpen(item)} />
      )}
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
        style={{
          maxWidth: '46rem',
          padding: 'var(--sp-3) var(--sp-4)',
          borderRadius: 'var(--r-lg)',
          background: mine
            ? 'color-mix(in srgb, var(--accent) 22%, transparent)'
            : 'color-mix(in srgb, var(--surface) 70%, transparent)',
          border: `1px solid ${mine ? 'color-mix(in srgb, var(--accent) 40%, transparent)' : 'var(--border)'}`,
          whiteSpace: 'pre-wrap',
          lineHeight: 1.5,
        }}
      >
        {message.text}
      </div>
      {message.items.length > 0 && (
        <div style={{ display: 'grid', gap: 'var(--sp-2)', width: '100%', maxWidth: '46rem' }}>
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
          <h1 style={{ margin: 0, fontSize: 24 }}>Assistant</h1>
          <p style={{ margin: 0, color: 'var(--text-muted)' }}>
            Ask for something to watch. It can look through your library and hand you
            titles to play.
          </p>
        </div>
        {shown.length > 0 && (
          <Button variant="ghost" icon="close" onClick={clear} data-testid="assistant-clear">
            Clear
          </Button>
        )}
      </header>
    ),
    [shown.length, clear],
  );

  if (needsKey) {
    return (
      <div style={{ padding: 'var(--sp-6)', display: 'grid', gap: 'var(--sp-5)' }}>
        {header}
        <EmptyState
          icon="sparkle"
          title="The assistant needs a key"
          body="Add a Gemini API key in Settings → Metadata and recommendations, and this becomes a conversation about your own library."
        />
      </div>
    );
  }

  return (
    <div
      style={{
        padding: 'var(--sp-6)',
        display: 'grid',
        gap: 'var(--sp-5)',
        gridTemplateRows: 'auto 1fr auto',
        height: '100%',
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
                <Button key={s} size="sm" onClick={() => send(s)} data-testid="assistant-suggestion">
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
              padding: 'var(--sp-3) var(--sp-4)',
              borderRadius: 'var(--r-lg)',
              background: 'color-mix(in srgb, var(--surface) 70%, transparent)',
              border: '1px solid var(--border)',
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
        style={{ display: 'flex', gap: 'var(--sp-2)', alignItems: 'center' }}
      >
        <TextField
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="Ask for something to watch…"
          aria-label="Ask the assistant"
          data-testid="assistant-input"
          style={{ flex: 1 }}
          disabled={thinking}
        />
        <Button
          type="submit"
          variant="primary"
          icon="chevronRight"
          disabled={thinking || draft.trim().length === 0}
          data-testid="assistant-send"
        >
          Ask
        </Button>
      </form>
    </div>
  );
}
