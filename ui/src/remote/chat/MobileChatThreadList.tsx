import React, { useMemo, useState } from "react";
import { Check, Terminal } from "lucide-react";
import { cn } from "../../lib/cn";

export interface ThreadListRow {
  readonly id: string;
  readonly title: string;
  readonly worktreeLabel?: string | null;
  readonly agentLabel?: string | null;
  readonly status?: "working" | "waiting" | "done";
  readonly relativeTime?: string | null;
}

export interface MobileChatThreadListProps {
  readonly rows: readonly ThreadListRow[];
  readonly activeRowId?: string | null;
  readonly onSelectRow: (row: ThreadListRow) => void;
  readonly workspaceLabel?: string | null;
  readonly className?: string;
}

const STATUS_LABEL: Record<NonNullable<ThreadListRow["status"]>, string> = {
  working: "Working",
  waiting: "Approval",
  done: "Done",
};

const STATUS_COLOR: Record<NonNullable<ThreadListRow["status"]>, string> = {
  working: "text-status-working",
  waiting: "text-status-warning",
  done: "text-status-success",
};

type ThreadGroup = {
  readonly name: string;
  count: number;
  rows: ThreadListRow[];
};

function metaText(row: ThreadListRow): string {
  return [row.worktreeLabel, row.agentLabel].filter(Boolean).join(" · ");
}

function searchableText(row: ThreadListRow): string {
  const statusLabel = row.status ? STATUS_LABEL[row.status] : "";
  return `${row.title} ${metaText(row)} ${statusLabel}`.toLowerCase();
}

export const MobileChatThreadList: React.FC<MobileChatThreadListProps> = ({
  rows,
  activeRowId,
  onSelectRow,
  className,
}) => {
  const [query, setQuery] = useState("");
  const hasQuery = query.trim().length > 0;

  const filteredRows = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    if (!normalized) return rows;
    return rows.filter((row) => searchableText(row).includes(normalized));
  }, [query, rows]);

  const groups = useMemo(() => {
    if (hasQuery) return null;
    const ordered: ThreadGroup[] = [];
    const byName = new Map<string, ThreadGroup>();
    for (const row of filteredRows) {
      const name = row.worktreeLabel ?? "unknown";
      let group = byName.get(name);
      if (!group) {
        group = { name, count: 0, rows: [] };
        byName.set(name, group);
        ordered.push(group);
      }
      group.count += 1;
      group.rows.push(row);
    }
    return ordered;
  }, [filteredRows, hasQuery]);

  const renderRow = (row: ThreadListRow) => {
    const active = row.id === activeRowId;
    const meta = metaText(row);
    const statusLabel = row.status ? STATUS_LABEL[row.status] : null;
    return (
      <button
        key={row.id}
        type="button"
        data-testid={`thread-row-${row.id}`}
        aria-current={active ? "true" : undefined}
        onClick={() => onSelectRow(row)}
        className={cn(
          "flex w-full items-center gap-2.5 px-3 py-2 text-left transition-colors hover:bg-accent/60 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
          active && "bg-accent/40",
        )}
      >
        <span
          data-testid="thread-row-glyph"
          className="flex size-7 shrink-0 items-center justify-center rounded-md border border-border/60 bg-zinc-900"
        >
          <Terminal className="size-3.5" aria-hidden="true" />
        </span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm font-medium text-zinc-100">{row.title}</span>
          {meta ? (
            <span className="block truncate font-mono text-[11px] text-muted-foreground">{meta}</span>
          ) : null}
        </span>
        {statusLabel ? (
          <span
            data-testid="thread-row-status"
            data-status={row.status}
            className={cn("shrink-0 text-right text-[11px] font-medium", STATUS_COLOR[row.status!])}
          >
            {statusLabel}
          </span>
        ) : row.relativeTime ? (
          <span className="shrink-0 text-right text-[11px] text-muted-foreground">{row.relativeTime}</span>
        ) : null}
        {active ? (
          <Check
            data-testid="thread-row-active"
            className="size-3.5 shrink-0 text-muted-foreground"
            aria-hidden="true"
          />
        ) : null}
      </button>
    );
  };

  return (
    <div className={cn("flex min-h-0 flex-1 flex-col bg-zinc-950 text-zinc-100", className)}>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {groups ? (
          groups.map((group) => (
            <section key={group.name} aria-label={group.name}>
              <h2
                data-testid={`thread-group-${group.name}`}
                className="px-3 pb-1 pt-3 text-[11px] font-medium uppercase tracking-wide text-zinc-500"
              >
                {`${group.name} · ${group.count}`}
              </h2>
              {group.rows.map(renderRow)}
            </section>
          ))
        ) : filteredRows.length === 0 ? (
          <p data-testid="thread-list-empty" className="px-4 py-8 text-center text-sm text-muted-foreground">
            No matching threads
          </p>
        ) : (
          filteredRows.map(renderRow)
        )}
      </div>
      <div className="shrink-0 border-t border-border/60 bg-zinc-950 p-2">
        <input
          data-testid="thread-search-input"
          placeholder="Search threads"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          className="w-full rounded-md border border-border/60 bg-zinc-900 px-3 py-1.5 text-sm text-zinc-100 placeholder:text-zinc-500 focus-visible:outline-none"
        />
      </div>
    </div>
  );
};