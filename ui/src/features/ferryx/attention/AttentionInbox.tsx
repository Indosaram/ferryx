import { X } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent } from "react";

import type { AgentPrompt, AgentPromptAnswer } from "../../../lib/tauri";

import { cn } from "../../../lib/cn";
import {
  ATTENTION_KIND_LABEL,
  ATTENTION_STATE_LABEL,
  ATTENTION_STATE_SHORT_LABEL,
  attentionKindLabel,
  countAttentionRows,
  formatAttentionTime,
  groupAttentionRows,
  sortAttentionRows,
  type AttentionRow,
  type AttentionState,
} from "./attentionModel";
import type { AttentionPrompts } from "./attentionPrompts";
import { userFacingError } from "../../../remote/userFacingError";

type AttentionFilter = "all" | AttentionState;

export type AttentionInboxProps = {
  rows: readonly AttentionRow[];
  /**
   * Entries whose pane reads working: each is listed, as needs-you, only once the daemon reads a
   * prompt on its pane. Needs `prompts`.
   */
  candidates?: readonly AttentionRow[];
  onOpen: (row: AttentionRow) => void;
  onDismiss?: (row: AttentionRow) => void;
  /** Present where a needs-you row may answer its pane's prompt inline (the desktop). */
  prompts?: AttentionPrompts;
  compact?: boolean;
  /**
   * The phone layout: 44px controls, 16px inputs, text no smaller than 12px, chat tokens, and the
   * session title as the row's primary line. Off on the desktop sidebar, which keeps its look.
   */
  touch?: boolean;
  now?: number;
  className?: string;
};

const CLOCK_TICK_MS = 30_000;
const NO_ROWS: readonly AttentionRow[] = [];

function useMinuteClock(fixed: number | undefined): number {
  const [now, setNow] = useState(() => fixed ?? Date.now());
  useEffect(() => {
    if (fixed !== undefined) return;
    const id = window.setInterval(() => setNow(Date.now()), CLOCK_TICK_MS);
    return () => window.clearInterval(id);
  }, [fixed]);
  return fixed ?? now;
}

const KIND: Record<AttentionState, string> = {
  "needs-you": "bg-status-warning/15 text-status-warning",
  done: "bg-status-success/15 text-status-success",
};

const MARKER: Record<AttentionState, string> = {
  "needs-you": "bg-status-warning",
  done: "bg-status-success",
};

/**
 * A done row, or a candidate whose pane reads working, is waiting on the user when the daemon can
 * read a prompt on its pane, whatever the activity detector said: it is shown as needs-you so its
 * answer is reachable. A candidate without a prompt is not shown. Only the daemon's prompt read
 * decides this, never the row's terminal text. A push re-reads only the row it names.
 */
function usePromptedRows(
  rows: readonly AttentionRow[],
  candidates: readonly AttentionRow[],
  prompts: AttentionPrompts | undefined,
): readonly AttentionRow[] {
  // Row id -> whether its pane holds a prompt, for the rows the daemon has answered about.
  const [prompted, setPrompted] = useState<ReadonlyMap<string, boolean>>(() => new Map());
  const probed = [...rows.filter((row) => row.state === "done"), ...candidates];
  const probedRef = useRef(probed);
  probedRef.current = probed;
  const probeKey = probed.map((row) => `${row.id}\u0000${row.revision}\u0000${row.text ?? ""}`).join("\u0001");

  useEffect(() => {
    if (!prompts) return;
    const generation = new Map<string, number>();
    let live = true;
    const probe = (row: AttentionRow) => {
      const turn = (generation.get(row.id) ?? 0) + 1;
      generation.set(row.id, turn);
      void prompts.read(row).then(
        (prompt) => prompt !== null,
        () => false,
      ).then((has) => {
        if (!live || generation.get(row.id) !== turn) return;
        setPrompted((current) => {
          if (current.get(row.id) === has) return current;
          const next = new Map(current);
          next.set(row.id, has);
          return next;
        });
      });
    };
    const rowsNow = probedRef.current;
    setPrompted((current) => {
      const ids = new Set(rowsNow.map((row) => row.id));
      const kept = [...current].filter(([id]) => ids.has(id));
      return kept.length === current.size ? current : new Map(kept);
    });
    rowsNow.forEach(probe);
    const stops = prompts.subscribe ? rowsNow.map((row) => prompts.subscribe!(row, () => probe(row))) : [];
    return () => {
      live = false;
      stops.forEach((stop) => stop());
    };
  }, [probeKey, prompts]);

  if (!prompts || probed.length === 0) return rows;
  let changed = false;
  const next = rows.map((row) => {
    if (row.state !== "done" || prompted.get(row.id) !== true) return row;
    changed = true;
    return { ...row, state: "needs-you" as const };
  });
  const ids = new Set(rows.map((row) => row.id));
  for (const candidate of candidates) {
    if (prompted.get(candidate.id) === true && !ids.has(candidate.id)) {
      next.push(candidate);
      changed = true;
    }
  }
  return changed ? sortAttentionRows(next) : rows;
}

export function AttentionInbox({
  rows: givenRows,
  candidates = NO_ROWS,
  onOpen,
  onDismiss,
  prompts,
  compact = false,
  touch = false,
  now: fixedNow,
  className,
}: AttentionInboxProps) {
  const now = useMinuteClock(fixedNow);
  const [filter, setFilter] = useState<AttentionFilter>("all");
  const rows = usePromptedRows(givenRows, candidates, prompts);
  const counts = countAttentionRows(rows);
  const bothStates = counts["needs-you"] > 0 && counts.done > 0;
  const activeFilter: AttentionFilter = bothStates ? filter : "all";
  const visible = activeFilter === "all" ? rows : rows.filter((row) => row.state === activeFilter);
  const groups = groupAttentionRows(visible);

  if (rows.length === 0) {
    return (
      <div
        data-testid="attention-inbox-empty"
        className={cn("flex min-h-0 flex-1 flex-col items-center justify-center px-6 py-8 text-center", className)}
      >
        <p className="text-[12.5px] font-semibold text-worktree-sidebar-foreground">
          Nobody is <span className="text-status-warning">waiting on you</span>.
        </p>
        <p className="mt-2 max-w-[260px] text-xs leading-relaxed text-muted-foreground">
          Agents show up here when they need your input or finish their work. Running sessions stay quiet.
        </p>
      </div>
    );
  }

  return (
    <div data-testid="attention-inbox" className={cn("flex min-h-0 flex-1 flex-col", className)}>
      {bothStates ? (
        <div
          role="group"
          aria-label="Status filter"
          className={cn(
            "flex shrink-0 gap-1.5 overflow-x-auto scrollbar-none",
            touch ? "gap-2 pb-2" : "border-b border-worktree-sidebar-border px-2.5 py-2",
          )}
        >
          {(["all", "needs-you", "done"] as const).map((id) => (
            <button
              key={id}
              type="button"
              aria-pressed={activeFilter === id}
              onClick={() => setFilter(id)}
              className={cn(
                "inline-flex shrink-0 items-center gap-1.5 rounded-full border transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                touch ? "h-11 px-4 text-sm" : "h-6 px-2.5 text-[11px]",
                activeFilter === id
                  ? "border-status-warning/35 bg-status-warning/15 font-semibold text-status-warning"
                  : touch
                    ? "border-chat-border text-chat-foreground-secondary hover:text-chat-foreground"
                    : "border-worktree-sidebar-border text-muted-foreground hover:text-worktree-sidebar-foreground",
              )}
            >
              {id === "all" ? "All" : touch ? ATTENTION_STATE_LABEL[id] : ATTENTION_STATE_SHORT_LABEL[id]}
              <span className={cn("tabular-nums opacity-75", touch ? "text-xs" : "text-[10px]")}>{counts[id]}</span>
            </button>
          ))}
        </div>
      ) : null}

      <div
        className={cn(
          "min-h-0 flex-1 overflow-x-hidden",
          touch ? "pt-1" : "overflow-y-auto px-2.5 pb-3.5 pt-2.5 scrollbar-sleek",
        )}
      >
        {groups.map((group) => (
          <section key={group.state} aria-label={ATTENTION_STATE_LABEL[group.state]} className={touch ? "[&+&]:mt-5" : "[&+&]:mt-3.5"}>
            {touch ? (
              <h3 className="flex items-center gap-1.5 px-1 pb-2 text-sm font-semibold text-chat-foreground">
                {ATTENTION_STATE_LABEL[group.state]}
                <span className="ml-auto text-xs font-normal tabular-nums text-chat-foreground-secondary">{group.rows.length}</span>
              </h3>
            ) : (
              <h3 className="flex items-center gap-1.5 px-0.5 pb-1.5 text-[11px] font-semibold text-worktree-sidebar-foreground/80">
                <span aria-hidden="true" className={cn("size-1.5 shrink-0 rounded-full", MARKER[group.state])} />
                {ATTENTION_STATE_LABEL[group.state]}
                <span className="ml-auto text-[10px] font-normal tabular-nums text-muted-foreground">{group.rows.length}</span>
              </h3>
            )}
            <ul className={cn("flex flex-col", touch ? "gap-2" : "gap-1.5")}>
              {group.rows.map((row) => (
                <AttentionInboxRow
                  key={row.id}
                  row={row}
                  now={now}
                  compact={compact}
                  touch={touch}
                  onOpen={onOpen}
                  onDismiss={onDismiss}
                  {...(prompts && row.state === "needs-you" ? { prompts } : {})}
                />
              ))}
            </ul>
          </section>
        ))}
      </div>
    </div>
  );
}

function TouchInboxRow({
  row,
  now,
  onOpen,
  onDismiss,
  prompts,
}: {
  row: AttentionRow;
  now: number;
  onOpen: (row: AttentionRow) => void;
  onDismiss?: (row: AttentionRow) => void;
  prompts?: AttentionPrompts;
}) {
  const when = row.at !== undefined ? formatAttentionTime(row.at, now) : undefined;
  const title = row.text ?? row.who;
  const agent = row.text ? row.who : undefined;
  const meta = [agent, row.location].filter(Boolean).join(" · ");
  const summary = [title, ATTENTION_KIND_LABEL[row.state], row.supervisorLabel, row.unavailable, meta, when].filter(Boolean).join(", ");
  return (
    <li className="rounded-xl bg-chat-surface">
      <div className="flex min-w-0 items-stretch">
        <button
          type="button"
          data-testid="attention-row"
          data-attention-state={row.state}
          aria-label={summary}
          aria-disabled={row.unavailable ? "true" : undefined}
          onClick={() => {
            if (!row.unavailable) onOpen(row);
          }}
          className="flex min-h-11 min-w-0 flex-1 flex-col rounded-xl aria-disabled:cursor-not-allowed px-3 py-2.5 text-left transition-colors hover:bg-chat-row-hover active:bg-chat-row-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <span className="flex min-w-0 items-start gap-2">
            <span data-testid="attention-row-title" className="min-w-0 flex-1 line-clamp-2 break-words text-base font-medium text-chat-foreground">
              {title}
            </span>
            <span className={cn("shrink-0 rounded-full px-2 py-0.5 text-xs font-semibold", KIND[row.state])}>
              {row.state === "needs-you" ? "Needs you" : ATTENTION_KIND_LABEL[row.state]}
            </span>
          </span>
          {meta ? <span className="mt-0.5 min-w-0 truncate text-sm text-chat-foreground-secondary">{meta}</span> : null}
          <span className="mt-0.5 flex min-w-0 items-baseline gap-2 text-xs text-chat-foreground-secondary">
            {row.supervisorLabel ? (
              <span data-testid="attention-row-supervisor" className="min-w-0 truncate">{row.supervisorLabel}</span>
            ) : null}
            {row.unavailable ? (
              <span data-testid="attention-row-unavailable" className="min-w-0 truncate">{row.unavailable}</span>
            ) : null}
            {when ? <span className="ml-auto shrink-0 tabular-nums">{when}</span> : null}
          </span>
        </button>
        {onDismiss ? (
          <button
            type="button"
            aria-label={`Dismiss ${title}`}
            onClick={() => onDismiss(row)}
            className="flex size-11 shrink-0 items-center justify-center self-center rounded-lg text-chat-foreground-secondary transition-colors hover:bg-chat-row-hover hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <X className="size-4" aria-hidden="true" />
          </button>
        ) : null}
      </div>
      {prompts ? <AttentionPromptPanel row={row} prompts={prompts} touch /> : null}
    </li>
  );
}

function AttentionInboxRow({
  row,
  now,
  compact,
  touch,
  onOpen,
  onDismiss,
  prompts,
}: {
  row: AttentionRow;
  now: number;
  compact: boolean;
  touch: boolean;
  onOpen: (row: AttentionRow) => void;
  onDismiss?: (row: AttentionRow) => void;
  prompts?: AttentionPrompts;
}) {
  if (touch) return <TouchInboxRow row={row} now={now} onOpen={onOpen} {...(onDismiss ? { onDismiss } : {})} {...(prompts ? { prompts } : {})} />;
  const when = row.at !== undefined ? formatAttentionTime(row.at, now) : undefined;
  const summary = [row.who, row.supervisorLabel ?? ATTENTION_STATE_LABEL[row.state], row.location, row.text, row.unavailable].filter(Boolean).join(", ");
  return (
    <li>
      <div className="group relative">
      <button
        type="button"
        data-testid="attention-row"
        data-attention-state={row.state}
        aria-label={summary}
        onClick={() => onOpen(row)}
        className={cn(
          "relative flex w-full min-w-0 flex-col rounded-lg border border-white/[0.06] bg-white/[0.035] py-2 pl-3 text-left transition-colors",
          "hover:bg-white/[0.06] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
          onDismiss ? "pr-8" : "pr-2.5",
        )}
      >
        <span className="flex min-w-0 items-baseline gap-1.5">
          <span className="max-w-[60%] shrink-0 truncate text-[12.5px] font-semibold text-worktree-sidebar-foreground">{row.who}</span>
          {!compact && row.location ? (
            <span className="min-w-0 truncate text-[11px] text-muted-foreground">{row.location}</span>
          ) : null}
          {when ? (
            <span className="ml-auto shrink-0 text-[10.5px] tabular-nums text-muted-foreground">{when}</span>
          ) : null}
        </span>
        {compact && row.location ? (
          <span className="mt-0.5 min-w-0 truncate text-[11px] text-muted-foreground">{row.location}</span>
        ) : null}
        <span className="mt-1 flex min-w-0 items-center gap-1.5">
          <span
            data-testid={row.supervisorLabel ? "attention-row-supervisor" : undefined}
            className={cn("min-w-0 truncate rounded px-1.5 py-px text-[10.5px] font-semibold", row.supervisorLabel ? "" : "shrink-0", KIND[row.state])}
          >
            {row.supervisorLabel ?? attentionKindLabel(row)}
          </span>
        </span>
        {row.text ? (
          <span
            data-testid="attention-row-text"
            title={row.text}
            className="mt-1 line-clamp-2 min-w-0 break-words rounded border border-worktree-sidebar-border bg-black/20 px-1.5 py-0.5 font-mono text-[10.5px] leading-snug text-worktree-sidebar-foreground/85"
          >
            {row.text}
          </span>
        ) : null}
        {row.unavailable ? (
          <span data-testid="attention-row-unavailable" className="mt-1 min-w-0 break-words text-xs text-muted-foreground">
            {row.unavailable}
          </span>
        ) : null}
      </button>
      {onDismiss ? (
        <button
          type="button"
          aria-label={`Dismiss ${row.who}`}
          onClick={() => onDismiss(row)}
          className="absolute right-1.5 top-1/2 flex size-6 -translate-y-1/2 items-center justify-center rounded-md text-muted-foreground opacity-0 transition-opacity hover:bg-white/[0.08] hover:text-worktree-sidebar-foreground focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring group-hover:opacity-100"
        >
          <X className="size-3" aria-hidden="true" />
        </button>
      ) : null}
      </div>
      {prompts ? <AttentionPromptPanel row={row} prompts={prompts} /> : null}
    </li>
  );
}

const PROMPT_CHANGED = "The question changed - open the pane";

const OPTION_BUTTON =
  "flex w-full min-w-0 items-baseline gap-1.5 rounded-md border border-worktree-sidebar-border bg-white/[0.03] px-2 py-1 text-left text-[11px] text-worktree-sidebar-foreground transition-colors hover:bg-white/[0.07] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50";

/**
 * The question a needs-you row's pane is asking, answered in place. Read when the row renders and
 * again whenever its entry moves; a pane with no readable prompt shows nothing here.
 */
const TOUCH_PANEL = {
  panel: "mx-3 mb-3 flex flex-col gap-2 rounded-lg bg-status-warning/[0.08] px-3 py-2.5",
  question: "line-clamp-4 break-words text-sm font-medium leading-snug text-chat-foreground",
  meta: "break-words text-sm text-chat-foreground-secondary",
  strong: "font-semibold text-chat-foreground",
  primary:
    "flex h-11 items-center justify-center rounded-lg bg-status-warning/20 px-4 text-sm font-semibold text-status-warning transition-colors hover:bg-status-warning/30 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50",
  secondary:
    "flex h-11 items-center justify-center rounded-lg bg-chat-surface-raised px-4 text-sm font-medium text-chat-foreground transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50",
  input:
    "h-11 min-w-0 flex-1 rounded-lg border border-chat-border bg-chat-screen px-3 text-base text-chat-foreground placeholder:text-chat-foreground-secondary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
  option:
    "flex min-h-11 w-full min-w-0 items-center gap-2 rounded-lg bg-chat-surface-raised px-3 py-2 text-left text-sm text-chat-foreground transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50",
  ordinal: "shrink-0 text-xs tabular-nums text-chat-foreground-secondary",
  error: "break-words text-sm text-chat-danger",
  stale: "mx-3 mb-3 text-sm text-status-warning",
} as const;

const COMPACT_PANEL = {
  panel: "mx-1 mt-1 flex flex-col gap-1 rounded-md border border-status-warning/25 bg-status-warning/[0.06] px-2 py-1.5",
  question: "line-clamp-3 break-words text-[11px] font-medium leading-snug text-worktree-sidebar-foreground",
  meta: "break-words text-[11px] text-muted-foreground",
  strong: "font-semibold text-worktree-sidebar-foreground",
  primary:
    "h-6 rounded-md bg-status-warning/20 px-2.5 text-[11px] font-semibold text-status-warning hover:bg-status-warning/30 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50",
  secondary:
    "h-6 rounded-md px-2.5 text-[11px] text-muted-foreground hover:bg-white/[0.07] hover:text-worktree-sidebar-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50",
  input:
    "h-6 min-w-0 flex-1 rounded-md border border-worktree-sidebar-border bg-black/20 px-1.5 text-[11px] text-worktree-sidebar-foreground placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
  option: OPTION_BUTTON,
  ordinal: "shrink-0 font-mono text-[10px] text-muted-foreground",
  error: "break-words text-[10.5px] text-destructive",
  stale: "mx-1 mt-1 text-[11px] text-status-warning",
} as const;

function AttentionPromptPanel({ row, prompts, touch = false }: { row: AttentionRow; prompts: AttentionPrompts; touch?: boolean }) {
  const ui = touch ? TOUCH_PANEL : COMPACT_PANEL;
  const [prompt, setPrompt] = useState<AgentPrompt | null>(null);
  const [confirming, setConfirming] = useState<{ key: string; label: string } | null>(null);
  const [text, setText] = useState("");
  const [pending, setPending] = useState(false);
  const [stale, setStale] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const promptsRef = useRef(prompts);
  promptsRef.current = prompts;
  const rowRef = useRef(row);
  rowRef.current = row;
  const shownRef = useRef<AgentPrompt | null>(null);
  shownRef.current = prompt;
  const [pushes, setPushes] = useState(0);

  // The daemon's agentPromptChanged push: re-read now rather than wait for the activity to flip.
  useEffect(
    () => promptsRef.current.subscribe?.(rowRef.current, () => setPushes((count) => count + 1)),
    [row.workspaceId, row.sessionId],
  );

  useEffect(() => {
    let current = true;
    const pushed = pushes > 0;
    setStale(false);
    setError(null);
    setConfirming(null);
    void promptsRef.current.read(rowRef.current).then(
      (next) => {
        if (!current) return;
        // A pushed re-read that finds the card gone means it was answered elsewhere (the phone,
        // the pane): the row no longer waits on the user.
        if (pushed && !next && shownRef.current) promptsRef.current.onAnswered(rowRef.current);
        setPrompt(next);
      },
      () => {
        if (current) setPrompt(null);
      },
    );
    return () => {
      current = false;
    };
    // Re-read on the row's own moves: its entry revision, the activity detail it shows, and pushes.
  }, [row.workspaceId, row.sessionId, row.revision, row.text, pushes]);

  if (stale) {
    return (
      <p data-testid="attention-prompt-stale" role="status" className={ui.stale}>
        {PROMPT_CHANGED}
      </p>
    );
  }
  if (!prompt) return null;

  const send = async (answer: AgentPromptAnswer) => {
    setPending(true);
    setError(null);
    try {
      const outcome = await promptsRef.current.answer(row, prompt, answer);
      if (outcome === "stale") {
        setStale(true);
        setConfirming(null);
      } else {
        promptsRef.current.onAnswered(row);
      }
    } catch (failure) {
      if (touch) {
        setError(userFacingError(failure, { action: "send the answer" }));
      } else {
        const message = (failure as { message?: unknown } | null)?.message;
        setError(typeof message === "string" && message ? message : "The answer could not be sent");
      }
    } finally {
      setPending(false);
    }
  };

  const pick = (option: { key: string; label: string }) => {
    // The daemon says which answers act on a stray pick (approvals, plans, menus); a daemon that
    // does not say is asked to confirm.
    if (prompt.needsConfirmation !== false) setConfirming(option);
    else void send({ key: option.key });
  };

  const submitText = (event: FormEvent) => {
    event.preventDefault();
    const answer = text.trim();
    if (answer) void send({ text: answer });
  };

  return (
    <div
      data-testid="attention-prompt"
      data-prompt-kind={prompt.kind}
      className={ui.panel}
    >
      <p className={ui.question}>
        {prompt.question}
      </p>
      {confirming ? (
        <div className={cn("flex flex-col", touch ? "gap-2" : "gap-1")}>
          <p className={ui.meta}>
            Send <span className={ui.strong}>{confirming.label}</span>?
          </p>
          <div className={cn("flex", touch ? "justify-between gap-6" : "gap-1")}>
            <button
              type="button"
              disabled={pending}
              onClick={() => void send({ key: confirming.key })}
              className={ui.primary}
            >
              Confirm
            </button>
            <button
              type="button"
              disabled={pending}
              onClick={() => setConfirming(null)}
              className={ui.secondary}
            >
              Cancel
            </button>
          </div>
        </div>
      ) : prompt.kind === "freeText" || prompt.options.length === 0 ? (
        <form onSubmit={submitText} className={cn("flex", touch ? "gap-2" : "gap-1")}>
          <input
            type="text"
            aria-label={`Answer ${row.who}`}
            value={text}
            disabled={pending}
            onChange={(event) => setText(event.target.value)}
            className={ui.input}
          />
          <button
            type="submit"
            disabled={pending || !text.trim()}
            className={cn("shrink-0", ui.primary)}
          >
            Send
          </button>
        </form>
      ) : (
        <div role="group" aria-label={prompt.question} className={cn("flex flex-col", touch ? "gap-2" : "gap-1")}>
          {prompt.options.map((option, index) => (
            <button
              key={option.key}
              type="button"
              data-testid="attention-prompt-option"
              data-option-key={option.key}
              disabled={pending}
              onClick={() => pick(option)}
              className={ui.option}
            >
              <span aria-hidden="true" className={ui.ordinal}>{index + 1}.</span>
              <span className="min-w-0 break-words">{option.label}</span>
            </button>
          ))}
        </div>
      )}
      {error ? <p role="alert" className={ui.error}>{error}</p> : null}
    </div>
  );
}
