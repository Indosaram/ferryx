import React, { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, ChevronUp, X } from "lucide-react";
import { cn } from "../../../lib/cn";
import { isImeConfirmingKey } from "../imeKeys";

/**
 * Find in thread (t3code, docs/user/activity-log.md). One provider per thread:
 * Cmd/Ctrl+F opens the bar, Escape closes it, next/prev walk the matches, and
 * while the bar is open the zen-hidden in-between messages show again.
 */

export interface ThreadFindState {
  /** Empty when the bar is closed; a closed bar never highlights. */
  readonly query: string;
  readonly findActive: boolean;
  /** Which registered match is current: a message id plus its ordinal in that message. */
  readonly active: { readonly messageId: string; readonly ordinal: number } | null;
  readonly totalMatches: number;
  openFind: (initialQuery?: string) => void;
  closeFind: () => void;
  setQuery: (query: string) => void;
  nextMatch: () => void;
  prevMatch: () => void;
  /** A message reports the matches its rendered text produced, in render order. */
  registerMatches: (messageId: string, count: number) => void;
}

const ThreadFindContext = createContext<ThreadFindState | null>(null);

export function useThreadFind(): ThreadFindState | null {
  return useContext(ThreadFindContext);
}

/** True when find is open, so zen-hidden messages show (activity-log.md). */
export function useThreadFindActive(): boolean {
  const find = useThreadFind();
  return find?.findActive ?? false;
}

/** Every match, in document order, derived from the per-message registrations. */
interface RegisteredMatches {
  readonly order: readonly { readonly messageId: string; readonly ordinal: number }[];
}

function flattenMatches(
  counts: ReadonlyMap<string, number>,
): RegisteredMatches["order"] {
  const order: { messageId: string; ordinal: number }[] = [];
  for (const [messageId, count] of counts) {
    for (let ordinal = 0; ordinal < count; ordinal += 1) {
      order.push({ messageId, ordinal });
    }
  }
  return order;
}

export function ThreadFindProvider({ children }: { children: React.ReactNode }) {
  const [open, setOpen] = useState(false);
  const [query, setQueryState] = useState("");
  const [counts, setCounts] = useState<ReadonlyMap<string, number>>(() => new Map());
  const inputRef = useRef<HTMLInputElement | null>(null);

  const matches = useMemo(() => (open && query ? flattenMatches(counts) : []), [open, query, counts]);

  const openFind = useCallback((initialQuery?: string) => {
    setOpen(true);
    if (typeof initialQuery === "string") setQueryState(initialQuery);
  }, []);
  const closeFind = useCallback(() => {
    setOpen(false);
    setCounts(new Map());
  }, []);
  const setQuery = useCallback((next: string) => setQueryState(next), []);
  const registerMatches = useCallback((messageId: string, count: number) => {
    setCounts((prev) => {
      // A message with no matches has no entry, so compare against the absent count:
      // returning a new map here would re-render, re-register, and never settle.
      if ((prev.get(messageId) ?? 0) === count) return prev;
      const next = new Map(prev);
      if (count === 0) next.delete(messageId);
      else next.set(messageId, count);
      return next;
    });
  }, []);

  const [activeIndex, setActiveIndex] = useState(0);
  const total = matches.length;
  useEffect(() => {
    setActiveIndex(0);
  }, [query]);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  const focusInput = useCallback(() => inputRef.current?.focus(), []);
  useEffect(() => {
    if (open) focusInput();
  }, [open, focusInput]);

  const state: ThreadFindState = useMemo(
    () => ({
      query: open ? query : "",
      findActive: open,
      active: total > 0 ? matches[Math.min(activeIndex, total - 1)]! : null,
      totalMatches: total,
      openFind,
      closeFind,
      setQuery,
      nextMatch: () => setActiveIndex((prev) => (total === 0 ? 0 : (prev + 1) % total)),
      prevMatch: () => setActiveIndex((prev) => (total === 0 ? 0 : (prev - 1 + total) % total)),
      registerMatches,
    }),
    [open, query, matches, total, activeIndex, openFind, closeFind, setQuery, registerMatches],
  );

  return (
    <ThreadFindContext.Provider value={state}>
      {children}
      {open && (
        <div
          data-testid="thread-find-bar"
          role="search"
          className="fixed inset-x-3 top-0 z-50 flex items-center gap-1 rounded-b-lg border border-chat-border bg-chat-surface px-2 py-1 shadow-lg"
        >
          <input
            ref={inputRef}
            data-testid="thread-find-input"
            value={query}
            placeholder="Find in thread"
            aria-label="Find in thread"
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (isImeConfirmingKey(event, false)) return;
              if (event.key === "Enter") {
                event.preventDefault();
                if (event.shiftKey) state.prevMatch();
                else state.nextMatch();
              }
            }}
            className="min-h-9 min-w-0 flex-1 rounded border border-chat-border bg-chat-screen px-2 text-sm text-chat-foreground placeholder:text-chat-foreground-tertiary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          />
          <span
            data-testid="thread-find-count"
            className="shrink-0 font-mono text-xs text-chat-foreground-secondary"
          >
            {query ? `${total === 0 ? 0 : Math.min(activeIndex, total - 1) + 1}/${total}` : ""}
          </span>
          <button
            type="button"
            data-testid="thread-find-prev"
            aria-label="Previous match"
            onClick={state.prevMatch}
            className="flex size-9 items-center justify-center rounded text-chat-foreground-secondary transition-colors hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <ChevronUp aria-hidden="true" className="size-4" />
          </button>
          <button
            type="button"
            data-testid="thread-find-next"
            aria-label="Next match"
            onClick={state.nextMatch}
            className="flex size-9 items-center justify-center rounded text-chat-foreground-secondary transition-colors hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <ChevronDown aria-hidden="true" className="size-4" />
          </button>
          <button
            type="button"
            data-testid="thread-find-close"
            aria-label="Close find"
            onClick={closeFind}
            className="flex size-9 items-center justify-center rounded text-chat-foreground-secondary transition-colors hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <X aria-hidden="true" className="size-4" />
          </button>
        </div>
      )}
    </ThreadFindContext.Provider>
  );
}

/**
 * Cmd/Ctrl+F opens the bar; a plain keydown listener, so it never swallows IME input.
 *
 * The listener is on `window`, which every mounted pane shares, so `isActive` decides whether
 * this pane answers. A pane that is hidden, or one of two visible leaves that the user is not
 * looking at, leaves the keystroke alone and never opens a bar the reader cannot see
 * (t3code: background threads do not react to Find).
 */
export function useThreadFindHotkey(
  find: ThreadFindState | null,
  isActive: () => boolean = () => true,
): void {
  useEffect(() => {
    if (!find) return;
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "f") {
        if (!isActive()) return;
        event.preventDefault();
        find.openFind();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [find, isActive]);
}

const escapeRegExp = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** Case-insensitive match count for one message's searchable text. */
export function countMatches(text: string, query: string): number {
  if (!query) return 0;
  const pattern = new RegExp(escapeRegExp(query), "gi");
  return (text.match(pattern) ?? []).length;
}

/**
 * Split one string into plain parts and highlighted marks. `ordinalBase` is the
 * ordinal of this string's first match within its message, so ordinals are stable
 * across renders of unchanged text.
 */
export function highlightMatches(
  text: string,
  query: string,
  messageId: string,
  ordinalBase: number,
  active: ThreadFindState["active"],
): React.ReactNode[] {
  if (!query) return [text];
  const nodes: React.ReactNode[] = [];
  const pattern = new RegExp(`(${escapeRegExp(query)})`, "gi");
  let last = 0;
  let ordinal = ordinalBase;
  for (const match of text.matchAll(pattern)) {
    const start = match.index!;
    if (start > last) nodes.push(text.slice(last, start));
    const isActive = active?.messageId === messageId && active.ordinal === ordinal;
    nodes.push(
      <mark
        key={`${ordinal}`}
        data-testid="thread-find-match"
        data-active={isActive || undefined}
        className={cn(isActive ? "bg-chat-primary/40 text-chat-foreground" : "bg-status-warning/30")}
      >
        {match[1]}
      </mark>,
    );
    ordinal += 1;
    last = start + match[1]!.length;
  }
  if (last < text.length) nodes.push(text.slice(last));
  return nodes;
}

/**
 * Register a message's match count against the provider and reveal the active
 * match by scrolling its mark into view. Returns whether this message holds the
 * active match, and the matching helper bound to stable ordinals.
 *
 * `searchable` is every string the message renders a match mark for, in render
 * order, so the registered count always equals the number of marks drawn.
 */
export function useThreadFindMessage(
  messageId: string,
  searchable: string | readonly string[],
): {
  activeHere: boolean;
  matchCount: number;
  /** The active query; empty while the bar is closed, so nothing highlights. */
  query: string;
  highlight: (text: string, ordinalBase: number) => React.ReactNode[];
} {
  const find = useThreadFind();
  const query = find?.findActive ? find.query : "";
  const active = find?.active ?? null;
  const segments = useMemo(
    () => (typeof searchable === "string" ? [searchable] : [...searchable]),
    [searchable],
  );
  const matchCount = useMemo(
    () => segments.reduce((sum, text) => sum + countMatches(text, query), 0),
    [segments, query],
  );

  const registerMatches = find?.registerMatches;
  useEffect(() => {
    if (!registerMatches) return;
    registerMatches(messageId, matchCount);
  }, [registerMatches, messageId, matchCount]);

  // A turn that leaves the transcript (steer, branch switch, virtualized unmount) must drop
  // its count, or the bar keeps walking to marks that are no longer in the DOM. This is an
  // unmount-only effect on purpose: the provider rebuilds its state object whenever a count
  // changes, so unregistering in the effect above would delete the entry it just wrote.
  const unregisterRef = useRef<(() => void) | null>(null);
  useEffect(() => {
    unregisterRef.current =
      registerMatches === undefined ? null : () => registerMatches(messageId, 0);
  }, [registerMatches, messageId]);
  useEffect(() => () => unregisterRef.current?.(), []);

  const activeHere = active?.messageId === messageId;
  useEffect(() => {
    if (!activeHere) return;
    const frame = requestAnimationFrame(() => {
      const mark = document.querySelector<HTMLElement>('mark[data-testid="thread-find-match"][data-active="true"]');
      mark?.scrollIntoView({ block: "center", behavior: "smooth" });
    });
    return () => cancelAnimationFrame(frame);
  }, [activeHere, active?.ordinal]);

  return {
    activeHere,
    matchCount,
    query,
    highlight: (text: string, ordinalBase: number) => highlightMatches(text, query, messageId, ordinalBase, active),
  };
}
