import { X } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent } from "react";

import type { AgentPrompt, AgentPromptAnswer } from "../../../lib/tauri";

import { cn } from "../../../lib/cn";
import {
  ATTENTION_KIND_LABEL,
  ATTENTION_STATE_LABEL,
  ATTENTION_STATE_SHORT_LABEL,
  countAttentionRows,
  formatAttentionTime,
  groupAttentionRows,
  sortAttentionRows,
  type AttentionRow,
  type AttentionState,
} from "./attentionModel";
import type { AttentionPrompts } from "./attentionPrompts";

type AttentionFilter = "all" | AttentionState;

export type AttentionInboxProps = {
  rows: readonly AttentionRow[];
  onOpen: (row: AttentionRow) => void;
  onDismiss?: (row: AttentionRow) => void;
  /** Present where a needs-you row may answer its pane's prompt inline (the desktop). */
  prompts?: AttentionPrompts;
  compact?: boolean;
  now?: number;
  className?: string;
};

const CLOCK_TICK_MS = 30_000;

function useMinuteClock(fixed: number | undefined): number {
  const [now, setNow] = useState(() => fixed ?? Date.now());
  useEffect(() => {
    if (fixed !== undefined) return;
    const id = window.setInterval(() => setNow(Date.now()), CLOCK_TICK_MS);
    return () => window.clearInterval(id);
  }, [fixed]);
  return fixed ?? now;
}

const ACCENT: Record<AttentionState, string> = {
  "needs-you": "before:bg-status-warning",
  done: "before:bg-status-success",
};

const KIND: Record<AttentionState, string> = {
  "needs-you": "bg-status-warning/15 text-status-warning",
  done: "bg-status-success/15 text-status-success",
};

const MARKER: Record<AttentionState, string> = {
  "needs-you": "bg-status-warning",
  done: "bg-status-success",
};

/**
 * A done row whose pane holds a prompt the daemon can read is waiting on the user, whatever the
 * activity detector last said: it is shown as needs-you so its answer is reachable. Only the
 * daemon's prompt read decides this, never the row's terminal text.
 */
function usePromptedRows(rows: readonly AttentionRow[], prompts: AttentionPrompts | undefined): readonly AttentionRow[] {
  const [prompted, setPrompted] = useState<ReadonlySet<string>>(() => new Set());
  const rowsRef = useRef(rows);
  rowsRef.current = rows;
  const [pushes, setPushes] = useState(0);
  const doneKey = rows
    .filter((row) => row.state === "done")
    .map((row) => `${row.id}\u0000${row.revision}\u0000${row.text ?? ""}`)
    .join("\u0001");

  useEffect(() => {
    if (!prompts?.subscribe) return;
    const stops = rowsRef.current
      .filter((row) => row.state === "done")
      .map((row) => prompts.subscribe!(row, () => setPushes((count) => count + 1)));
    return () => stops.forEach((stop) => stop());
  }, [doneKey, prompts]);

  useEffect(() => {
    const done = rowsRef.current.filter((row) => row.state === "done");
    if (!prompts || done.length === 0) {
      setPrompted((current) => (current.size === 0 ? current : new Set()));
      return;
    }
    let current = true;
    void Promise.all(
      done.map((row) => prompts.read(row).then((prompt) => (prompt ? row.id : null), () => null)),
    ).then((ids) => {
      if (current) setPrompted(new Set(ids.filter((id): id is string => id !== null)));
    });
    return () => {
      current = false;
    };
  }, [doneKey, prompts, pushes]);

  if (prompted.size === 0) return rows;
  let promoted = false;
  const next = rows.map((row) => {
    if (row.state !== "done" || !prompted.has(row.id)) return row;
    promoted = true;
    return { ...row, state: "needs-you" as const };
  });
  return promoted ? sortAttentionRows(next) : rows;
}

export function AttentionInbox({
  rows: givenRows,
  onOpen,
  onDismiss,
  prompts,
  compact = false,
  now: fixedNow,
  className,
}: AttentionInboxProps) {
  const now = useMinuteClock(fixedNow);
  const [filter, setFilter] = useState<AttentionFilter>("all");
  const rows = usePromptedRows(givenRows, prompts);
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
        <span aria-hidden="true" className="mb-3 text-2xl leading-none text-status-warning">✧</span>
        <p className="text-[12.5px] font-semibold text-worktree-sidebar-foreground">
          Nobody is <span className="text-status-warning">waiting on you</span>.
        </p>
        <p className="mt-2 max-w-[260px] text-[10.5px] leading-relaxed text-muted-foreground/80">
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
          className="flex shrink-0 gap-1.5 overflow-x-auto border-b border-worktree-sidebar-border px-2.5 py-2 scrollbar-none"
        >
          {(["all", "needs-you", "done"] as const).map((id) => (
            <button
              key={id}
              type="button"
              aria-pressed={activeFilter === id}
              onClick={() => setFilter(id)}
              className={cn(
                "inline-flex h-6 shrink-0 items-center gap-1.5 rounded-full border px-2.5 text-[11px] transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                activeFilter === id
                  ? "border-status-warning/35 bg-status-warning/15 font-semibold text-status-warning"
                  : "border-worktree-sidebar-border text-muted-foreground hover:text-worktree-sidebar-foreground",
              )}
            >
              {id === "all" ? "All" : ATTENTION_STATE_SHORT_LABEL[id]}
              <span className="text-[10px] tabular-nums opacity-75">{counts[id]}</span>
            </button>
          ))}
        </div>
      ) : null}

      <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-2.5 pb-3.5 pt-2.5 scrollbar-sleek">
        {groups.map((group) => (
          <section key={group.state} aria-label={ATTENTION_STATE_LABEL[group.state]} className="[&+&]:mt-3.5">
            <h3 className="flex items-center gap-1.5 px-0.5 pb-1.5 text-[11px] font-semibold text-worktree-sidebar-foreground/80">
              <span aria-hidden="true" className={cn("size-1.5 shrink-0 rounded-full", MARKER[group.state])} />
              {ATTENTION_STATE_LABEL[group.state]}
              <span className="ml-auto text-[10px] font-normal tabular-nums text-muted-foreground">{group.rows.length}</span>
            </h3>
            <ul className="flex flex-col gap-1.5">
              {group.rows.map((row) => (
                <AttentionInboxRow
                  key={row.id}
                  row={row}
                  now={now}
                  compact={compact}
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

function AttentionInboxRow({
  row,
  now,
  compact,
  onOpen,
  onDismiss,
  prompts,
}: {
  row: AttentionRow;
  now: number;
  compact: boolean;
  onOpen: (row: AttentionRow) => void;
  onDismiss?: (row: AttentionRow) => void;
  prompts?: AttentionPrompts;
}) {
  const when = row.at !== undefined ? formatAttentionTime(row.at, now) : undefined;
  const summary = [row.who, row.supervisorLabel ?? ATTENTION_STATE_LABEL[row.state], row.location, row.text].filter(Boolean).join(", ");
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
          "before:absolute before:bottom-2 before:left-0 before:top-2 before:w-[3px] before:rounded-r before:content-['']",
          ACCENT[row.state],
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
            {row.supervisorLabel ?? ATTENTION_KIND_LABEL[row.state]}
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
function AttentionPromptPanel({ row, prompts }: { row: AttentionRow; prompts: AttentionPrompts }) {
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
      <p data-testid="attention-prompt-stale" role="status" className="mx-1 mt-1 text-[11px] text-status-warning">
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
      const message = (failure as { message?: unknown } | null)?.message;
      setError(typeof message === "string" && message ? message : "The answer could not be sent");
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
      className="mx-1 mt-1 flex flex-col gap-1 rounded-md border border-status-warning/25 bg-status-warning/[0.06] px-2 py-1.5"
    >
      <p className="line-clamp-3 break-words text-[11px] font-medium leading-snug text-worktree-sidebar-foreground">
        {prompt.question}
      </p>
      {confirming ? (
        <div className="flex flex-col gap-1">
          <p className="break-words text-[11px] text-muted-foreground">
            Send <span className="font-semibold text-worktree-sidebar-foreground">{confirming.label}</span>?
          </p>
          <div className="flex gap-1">
            <button
              type="button"
              disabled={pending}
              onClick={() => void send({ key: confirming.key })}
              className="h-6 rounded-md bg-status-warning/20 px-2.5 text-[11px] font-semibold text-status-warning hover:bg-status-warning/30 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
            >
              Confirm
            </button>
            <button
              type="button"
              disabled={pending}
              onClick={() => setConfirming(null)}
              className="h-6 rounded-md px-2.5 text-[11px] text-muted-foreground hover:bg-white/[0.07] hover:text-worktree-sidebar-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
            >
              Cancel
            </button>
          </div>
        </div>
      ) : prompt.kind === "freeText" || prompt.options.length === 0 ? (
        <form onSubmit={submitText} className="flex gap-1">
          <input
            type="text"
            aria-label={`Answer ${row.who}`}
            value={text}
            disabled={pending}
            onChange={(event) => setText(event.target.value)}
            className="h-6 min-w-0 flex-1 rounded-md border border-worktree-sidebar-border bg-black/20 px-1.5 text-[11px] text-worktree-sidebar-foreground placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          />
          <button
            type="submit"
            disabled={pending || !text.trim()}
            className="h-6 shrink-0 rounded-md bg-status-warning/20 px-2.5 text-[11px] font-semibold text-status-warning hover:bg-status-warning/30 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
          >
            Send
          </button>
        </form>
      ) : (
        <div role="group" aria-label={prompt.question} className="flex flex-col gap-1">
          {prompt.options.map((option, index) => (
            <button
              key={option.key}
              type="button"
              data-testid="attention-prompt-option"
              data-option-key={option.key}
              disabled={pending}
              onClick={() => pick(option)}
              className={OPTION_BUTTON}
            >
              <span aria-hidden="true" className="shrink-0 font-mono text-[10px] text-muted-foreground">{index + 1}.</span>
              <span className="min-w-0 break-words">{option.label}</span>
            </button>
          ))}
        </div>
      )}
      {error ? <p role="alert" className="break-words text-[10.5px] text-destructive">{error}</p> : null}
    </div>
  );
}
