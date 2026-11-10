import { useEffect, useId, useRef, useState, type ChangeEvent, type KeyboardEvent } from 'react';
import { ArrowRight } from 'lucide-react';

import { cn } from '../../lib/cn';
import { VoiceConversationButton } from '../supervisor/VoiceConversationButton';
import { parseMentions } from '../../lib/supervisorPanel/model';
import type { ComposerProps, ComposerSendResult, MentionOption } from './types';

const MENTION_AT_CARET = /(^|\s)@([^\s@]*)$/;

function mentionLabel(option: MentionOption): string {
  return `${option.title} · ${option.computer}`;
}

export function Composer({ sessions, touch, onSend, seed, voiceConversation }: ComposerProps) {
  const listId = useId();
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const [text, setText] = useState('');
  const [caret, setCaret] = useState(0);
  const [active, setActive] = useState(0);
  const [dismissed, setDismissed] = useState(false);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reply, setReply] = useState<{ text: string; unverified: boolean } | null>(null);

  useEffect(() => {
    if (!seed) return;
    setText(seed.text);
    setCaret(seed.text.length);
    setError(null);
    inputRef.current?.focus();
  }, [seed]);

  const match = MENTION_AT_CARET.exec(text.slice(0, caret));
  const query = match ? match[2].toLowerCase() : null;
  const options =
    query === null || dismissed
      ? []
      : sessions.filter((s) => mentionLabel(s).toLowerCase().includes(query)).slice(0, 6);
  const open = options.length > 0;

  const choose = (option: MentionOption) => {
    if (!match) return;
    const atStart = caret - match[2].length - 1;
    const label = `@${mentionLabel(option)} `;
    const next = text.slice(0, atStart) + label + text.slice(caret);
    setText(next);
    setCaret(atStart + label.length);
    setActive(0);
  };

  const send = async () => {
    const value = text.trim();
    if (!value || sending) return;
    // Mentions come from the text itself, picked or typed: a typed name or prefix counts, and
    // a name that fits several sessions is never guessed.
    const parsed = parseMentions(
      value,
      sessions.map((s) => ({ sessionRef: s.sessionRef, title: s.title, hostLabel: s.computer })),
    );
    if (parsed.ambiguous.length > 0) {
      const first = parsed.ambiguous[0];
      setError(`${first.mention} fits ${first.candidates.length} sessions. Pick one from the list.`);
      return;
    }
    setError(null);
    setSending(true);
    let result: ComposerSendResult | undefined;
    try {
      result = (await onSend(value, parsed.targets)) as ComposerSendResult | undefined;
    } catch {
      result = { ok: false, message: 'That did not go through.' };
    }
    setSending(false);
    if (result && !result.ok) {
      setError(result.message);
      return;
    }
    setText('');
    setCaret(0);
    setReply(result?.ok ? (result.reply ?? null) : null);
  };

  const onChange = (e: ChangeEvent<HTMLTextAreaElement>) => {
    setText(e.target.value);
    setCaret(e.target.selectionStart);
    setActive(0);
    setDismissed(false);
    setError(null);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    // An IME confirming a candidate also reports Enter: that is not a send or a pick.
    if (e.nativeEvent.isComposing || e.keyCode === 229) return;
    // Tab belongs to the mention list only while it is showing; otherwise focus moves on.
    if (open) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setActive((i) => (i + 1) % options.length);
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setActive((i) => (i - 1 + options.length) % options.length);
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        choose(options[active]);
        return;
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        setDismissed(true);
        return;
      }
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void send();
    }
  };

  return (
    <div className="relative" data-testid="supervisor-composer">
      {open ? (
        <ul
          id={listId}
          role="listbox"
          aria-label="Sessions"
          className="absolute inset-x-0 bottom-full mb-2 flex flex-col overflow-hidden rounded-lg border border-border bg-popover py-1"
        >
          {options.map((option, i) => (
            <li
              key={option.sessionRef}
              id={`${listId}-${i}`}
              role="option"
              aria-selected={i === active}
              onMouseDown={(e) => {
                e.preventDefault();
                choose(option);
              }}
              className={cn(
                'flex cursor-pointer items-center px-3 text-foreground',
                touch ? 'min-h-11 text-sm' : 'min-h-8 text-xs',
                i === active && 'bg-accent',
              )}
            >
              <span className="truncate">{mentionLabel(option)}</span>
            </li>
          ))}
        </ul>
      ) : null}
      <div className="flex items-end gap-2 rounded-xl border border-input bg-card py-2 pl-3 pr-2">
        <textarea
          ref={inputRef}
          rows={1}
          value={text}
          onChange={onChange}
          onKeyDown={onKeyDown}
          onSelect={(e) => setCaret(e.currentTarget.selectionStart)}
          placeholder="Tell the supervisor anything"
          aria-label="Tell the supervisor anything"
          role="combobox"
          aria-expanded={open}
          aria-controls={open ? listId : undefined}
          aria-activedescendant={open ? `${listId}-${active}` : undefined}
          aria-autocomplete="list"
          className={cn(
            'min-w-0 flex-1 resize-none bg-transparent py-1.5 leading-snug text-foreground outline-none placeholder:text-muted-foreground',
            touch ? 'min-h-11 text-base' : 'text-[13px]',
          )}
        />
        <button
          type="button"
          onClick={() => void send()}
          disabled={sending}
          aria-label="Send"
          className={cn(
            'grid shrink-0 place-items-center rounded-lg bg-primary text-primary-foreground transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50',
            touch ? 'size-11' : 'size-8',
          )}
        >
          <ArrowRight aria-hidden="true" className="size-4" />
        </button>
        {voiceConversation ? <VoiceConversationButton {...voiceConversation} /> : null}
      </div>
      {error ? (
        <p role="alert" data-testid="supervisor-composer-error" className={cn('mt-1 px-1 text-status-warning', touch ? 'text-sm' : 'text-xs')}>
          {error}
        </p>
      ) : null}
      {reply ? (
        <p data-testid="supervisor-reply" className={cn('mt-2 px-1 leading-snug text-foreground', touch ? 'text-sm' : 'text-xs')}>
          {reply.text}
          {reply.unverified ? (
            <span className="ml-1.5 rounded-sm bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">Unverified</span>
          ) : null}
        </p>
      ) : null}
    </div>
  );
}
