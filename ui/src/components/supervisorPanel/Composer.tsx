import { useId, useState, type ChangeEvent, type KeyboardEvent } from 'react';
import { ArrowRight } from 'lucide-react';

import { cn } from '../../lib/cn';
import type { ComposerProps, MentionOption } from './types';

const MENTION_AT_CARET = /(^|\s)@([^\s@]*)$/;

function mentionLabel(option: MentionOption): string {
  return `${option.title} · ${option.computer}`;
}

export function Composer({ sessions, touch, onSend }: ComposerProps) {
  const listId = useId();
  const [text, setText] = useState('');
  const [caret, setCaret] = useState(0);
  const [picked, setPicked] = useState<MentionOption[]>([]);
  const [active, setActive] = useState(0);
  const [dismissed, setDismissed] = useState(false);

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
    setPicked((prev) => (prev.some((p) => p.sessionRef === option.sessionRef) ? prev : [...prev, option]));
    setActive(0);
  };

  const send = () => {
    const value = text.trim();
    if (!value) return;
    const mentions = picked.filter((p) => value.includes(`@${mentionLabel(p)}`)).map((p) => p.sessionRef);
    onSend(value, mentions);
    setText('');
    setCaret(0);
    setPicked([]);
  };

  const onChange = (e: ChangeEvent<HTMLTextAreaElement>) => {
    setText(e.target.value);
    setCaret(e.target.selectionStart);
    setActive(0);
    setDismissed(false);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
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
      send();
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
          aria-autocomplete="list"
          className={cn(
            'min-w-0 flex-1 resize-none bg-transparent py-1.5 leading-snug text-foreground outline-none placeholder:text-muted-foreground',
            touch ? 'text-base' : 'text-[13px]',
          )}
        />
        <button
          type="button"
          onClick={send}
          aria-label="Send"
          className={cn(
            'grid shrink-0 place-items-center rounded-lg bg-primary text-primary-foreground transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
            touch ? 'size-11' : 'size-8',
          )}
        >
          <ArrowRight aria-hidden="true" className="size-4" />
        </button>
      </div>
    </div>
  );
}
