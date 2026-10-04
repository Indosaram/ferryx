import { X } from "lucide-react";
import { useEffect, useState } from "react";

import { cn } from "../../../lib/cn";
import { AttentionMascot } from "./AttentionMascot";
import {
  ATTENTION_KIND_LABEL,
  ATTENTION_STATE_LABEL,
  ATTENTION_STATE_SHORT_LABEL,
  countAttentionRows,
  formatAttentionTime,
  groupAttentionRows,
  type AttentionRow,
  type AttentionState,
} from "./attentionModel";

type AttentionFilter = "all" | AttentionState;

export type AttentionInboxProps = {
  rows: readonly AttentionRow[];
  onOpen: (row: AttentionRow) => void;
  onDismiss?: (row: AttentionRow) => void;
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

export function AttentionInbox({
  rows,
  onOpen,
  onDismiss,
  compact = false,
  now: fixedNow,
  className,
}: AttentionInboxProps) {
  const now = useMinuteClock(fixedNow);
  const [filter, setFilter] = useState<AttentionFilter>("all");
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
        <AttentionMascot />
        <p className="text-[12.5px] font-semibold text-worktree-sidebar-foreground">
          Nobody is waiting on you.
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
}: {
  row: AttentionRow;
  now: number;
  compact: boolean;
  onOpen: (row: AttentionRow) => void;
  onDismiss?: (row: AttentionRow) => void;
}) {
  const when = row.at !== undefined ? formatAttentionTime(row.at, now) : undefined;
  const summary = [row.who, ATTENTION_STATE_LABEL[row.state], row.location, row.text].filter(Boolean).join(", ");
  return (
    <li className="group relative">
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
          <span className={cn("shrink-0 rounded px-1.5 py-px text-[10.5px] font-semibold", KIND[row.state])}>
            {ATTENTION_KIND_LABEL[row.state]}
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
    </li>
  );
}
