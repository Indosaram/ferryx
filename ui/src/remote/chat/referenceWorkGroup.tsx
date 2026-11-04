import React, { useEffect, useId, useRef, useState } from "react";
import { ChevronRight, SquarePen } from "lucide-react";
import { cn } from "../../lib/cn";
import type { ReferencePart, ReferencePayloadRef, ReferenceTaskResult, ReferenceTurnMeta } from "./referenceTypes";

/**
 * A native patch longer than this is not drawn inline. Matches the host's
 * `REFERENCE_NATIVE_DIFF_LIMIT` (8 KiB). Counts may still be shown.
 */
export const REFERENCE_DIFF_INLINE_LIMIT = 8 * 1024;

/** File rows in a diff summary. The rest are a count, not a dumped list. */
export const REFERENCE_DIFF_FILE_CAP = 6;

export interface ReferenceWorkMember {
  readonly part: ReferencePart;
  readonly index: number;
}

export type ReferenceWorkSegment =
  | { readonly kind: "part"; readonly part: ReferencePart; readonly index: number }
  | { readonly kind: "group"; readonly key: string; readonly members: readonly ReferenceWorkMember[] };

export interface ReferenceDiffFileSummary {
  readonly path: string;
  readonly additions: number;
  readonly deletions: number;
}

export interface ReferenceDiffSummary {
  readonly files: readonly ReferenceDiffFileSummary[];
  readonly additions: number;
  readonly deletions: number;
}

export interface ReferenceWorkNesting {
  readonly tasksByCallId: ReadonlyMap<string, readonly ReferenceTaskResult[]>;
  readonly localTasks: ReadonlyMap<number, readonly ReferenceTaskResult[]>;
}

/** Loader for an opaque payload ref. This module does not fetch. */
export interface ReferencePayloadLoadContext {
  onLoadPayload?: (payload: ReferencePayloadRef) => void;
  loadingPayloadRef?: string | null;
}

const CODEX_FILE = /^\*\*\* (?:Update|Add|Delete) File: (.+)$/;
const GIT_FILE = /^diff --git a\/(.+?) b\/(.+)$/;

export function referencePartId(part: { readonly id?: string | null }): string | null {
  const id = part.id;
  if (typeof id !== "string") return null;
  const trimmed = id.trim();
  return trimmed.length > 0 ? trimmed : null;
}

/**
 * Part key. A recorded id wins so an append, a replacement, or a window slide
 * that only changes the index does not remount the part. Index is the fallback
 * when the part has no id (a rev2 part).
 */
export function referencePartKey(part: ReferencePart, index: number): string {
  const id = referencePartId(part);
  return id ? `part:${id}` : `index:${index}`;
}

/**
 * Group key is the first member's part id, prefixed so it cannot collide with a
 * sibling part key. Appends do not change it. Without an id, the key is the
 * first member's index and will move if that index moves.
 */
export function referenceWorkGroupKey(members: readonly ReferenceWorkMember[]): string {
  const first = members[0];
  if (!first) return "group-index:0";
  const id = referencePartId(first.part);
  return id ? `group:${id}` : `group-index:${first.index}`;
}

export function isReferenceWorkPart(part: ReferencePart): boolean {
  return part.kind === "tool" || part.kind === "taskResult" || part.kind === "diff";
}

/** Consecutive tool, task, and diff parts. Anything else splits the group. */
export function groupReferenceWorkParts(parts: readonly ReferencePart[]): ReferenceWorkSegment[] {
  const segments: ReferenceWorkSegment[] = [];
  let members: ReferenceWorkMember[] = [];
  const flush = () => {
    if (members.length === 0) return;
    segments.push({ kind: "group", key: referenceWorkGroupKey(members), members });
    members = [];
  };
  parts.forEach((part, index) => {
    if (isReferenceWorkPart(part)) {
      members.push({ part, index });
      return;
    }
    flush();
    segments.push({ kind: "part", part, index });
  });
  flush();
  return segments;
}

/**
 * Nest a task under the tool whose callId equals parentCallId. An unknown parent
 * stays in the task part. Tasks are never paired by text or by position.
 */
export function planReferenceWorkNesting(parts: readonly ReferencePart[]): ReferenceWorkNesting {
  const toolIndexByCallId = new Map<string, number>();
  parts.forEach((part, index) => {
    if (part.kind !== "tool") return;
    const callId = part.callId;
    if (typeof callId !== "string" || callId.length === 0) return;
    if (!toolIndexByCallId.has(callId)) toolIndexByCallId.set(callId, index);
  });
  const tasksByCallId = new Map<string, ReferenceTaskResult[]>();
  const localTasks = new Map<number, readonly ReferenceTaskResult[]>();
  parts.forEach((part, index) => {
    if (part.kind !== "taskResult") return;
    const local: ReferenceTaskResult[] = [];
    for (const task of part.tasks) {
      const parent = task.parentCallId;
      if (typeof parent === "string" && parent.length > 0 && toolIndexByCallId.has(parent)) {
        const held = tasksByCallId.get(parent);
        if (held) held.push(task);
        else tasksByCallId.set(parent, [task]);
      } else {
        local.push(task);
      }
    }
    localTasks.set(index, local);
  });
  return { tasksByCallId, localTasks };
}

export function nestedTasksForTool(
  part: Extract<ReferencePart, { kind: "tool" }>,
  nesting: ReferenceWorkNesting,
): readonly ReferenceTaskResult[] {
  const callId = part.callId;
  if (typeof callId !== "string" || callId.length === 0) return [];
  return nesting.tasksByCallId.get(callId) ?? [];
}

export function referenceWorkGroupRunning(members: readonly ReferenceWorkMember[]): boolean {
  for (const { part } of members) {
    if (part.kind === "tool" && part.status === "running") return true;
    if (part.kind === "taskResult" && part.tasks.some((task) => task.status === "running")) return true;
  }
  return false;
}

/** Completed only when every tool and task recorded a completed status. Absent is not completed. */
export function referenceWorkGroupCompleted(members: readonly ReferenceWorkMember[]): boolean {
  if (referenceWorkGroupRunning(members)) return false;
  let saw = false;
  for (const { part } of members) {
    if (part.kind === "tool") {
      if (part.status !== "completed") return false;
      saw = true;
    } else if (part.kind === "taskResult") {
      if (part.tasks.length === 0) return false;
      for (const task of part.tasks) {
        if (task.status !== "completed") return false;
        saw = true;
      }
    }
  }
  return saw;
}

/**
 * Chrome (the collapse control) is for rev3 evidence: ids, lifecycle, payload
 * refs, diffs, or a parent call id. A rev2 run of tools has none of these and
 * keeps today's rows.
 */
export function referenceWorkGroupNeedsChrome(members: readonly ReferenceWorkMember[]): boolean {
  for (const { part } of members) {
    if (referencePartId(part) !== null) return true;
    if (part.kind === "diff") return true;
    if (part.kind === "tool") {
      if (part.status != null) return true;
      if (part.outputPayload != null) return true;
      if (typeof part.callId === "string" && part.callId.length > 0) return true;
    }
    if (part.kind === "taskResult") {
      for (const task of part.tasks) {
        if (typeof task.parentCallId === "string" && task.parentCallId.length > 0) return true;
        if (task.status === "running" || task.status === "unverifiable") return true;
      }
    }
  }
  return false;
}

export function referenceWorkGroupSharedStatus(members: readonly ReferenceWorkMember[]): string | null {
  const statuses: string[] = [];
  for (const { part } of members) {
    if (part.kind === "tool") {
      if (part.status == null) return null;
      statuses.push(part.status);
    } else if (part.kind === "taskResult") {
      for (const task of part.tasks) statuses.push(task.status);
    }
  }
  if (statuses.length === 0) return null;
  const first = statuses[0];
  return statuses.every((status) => status === first) ? first : null;
}

export function referenceWorkGroupLabel(
  members: readonly ReferenceWorkMember[],
  nesting: ReferenceWorkNesting,
): string {
  let tools = 0;
  let tasks = 0;
  let diffs = 0;
  for (const { part, index } of members) {
    if (part.kind === "tool") tools += 1;
    else if (part.kind === "diff") diffs += 1;
    else if (part.kind === "taskResult") tasks += (nesting.localTasks.get(index) ?? part.tasks).length;
  }
  const bits = ["Work"];
  if (tools > 0) bits.push(`${tools} ${tools === 1 ? "tool" : "tools"}`);
  if (tasks > 0) bits.push(`${tasks} ${tasks === 1 ? "task" : "tasks"}`);
  if (diffs > 0) bits.push(`${diffs} ${diffs === 1 ? "diff" : "diffs"}`);
  return bits.join(" · ");
}

export function referenceTurnUsageText(meta: ReferenceTurnMeta | null | undefined): string | null {
  if (meta == null) return null;
  const bits: string[] = [];
  if (typeof meta.model === "string" && meta.model.length > 0) bits.push(meta.model);
  const usage = meta.usage;
  if (usage != null) {
    const push = (value: number | null | undefined, label: string) => {
      if (typeof value === "number") bits.push(`${value} ${label}`);
    };
    push(usage.inputTokens, "in");
    push(usage.outputTokens, "out");
    push(usage.cacheReadTokens, "cache read");
    push(usage.cacheWriteTokens, "cache write");
  }
  return bits.length === 0 ? null : bits.join(" · ");
}

export function formatPayloadBytes(byteLength: number): string {
  if (!Number.isFinite(byteLength) || byteLength < 0) return "";
  if (byteLength < 1024) return `${byteLength} B`;
  const kb = byteLength / 1024;
  if (kb < 1024) {
    const rounded = Math.round(kb * 10) / 10;
    return `${rounded} KB`;
  }
  return `${Math.round((kb / 1024) * 10) / 10} MB`;
}

function utf8Length(text: string): number {
  return new TextEncoder().encode(text).length;
}

export function referenceDiffBodyInline(body: string): boolean {
  return utf8Length(body) <= REFERENCE_DIFF_INLINE_LIMIT;
}

/**
 * Inline patch text, if a reader put it on the part. The rev3 wire type omits it;
 * current readers keep a payload ref only. An empty string is not a patch.
 */
export function referenceDiffInlineBody(part: Extract<ReferencePart, { kind: "diff" }>): string | null {
  const body = (part as { readonly body?: unknown }).body;
  if (typeof body !== "string" || body.trim().length === 0) return null;
  return body;
}

export function summarizeReferenceDiff(body: string): ReferenceDiffSummary {
  const files = new Map<string, { additions: number; deletions: number }>();
  let current = "";
  const ensure = (path: string) => {
    if (!files.has(path)) files.set(path, { additions: 0, deletions: 0 });
    current = path;
  };
  const add = (kind: "additions" | "deletions") => {
    if (!files.has(current)) ensure(current);
    files.get(current)![kind] += 1;
  };
  for (const line of body.split("\n")) {
    const codex = CODEX_FILE.exec(line);
    if (codex) {
      ensure(codex[1].trim());
      continue;
    }
    const git = GIT_FILE.exec(line);
    if (git) {
      ensure(git[2].trim());
      continue;
    }
    if (line.startsWith("+++ ") && !line.startsWith("+++ /dev/null")) {
      const path = line.slice(4).trim().replace(/^b\//, "");
      if (path.length > 0) ensure(path);
      continue;
    }
    if (line.startsWith("--- ") || line.startsWith("@@") || line.startsWith("*** ")) continue;
    if (line.startsWith("+")) {
      add("additions");
      continue;
    }
    if (line.startsWith("-")) add("deletions");
  }
  const list = [...files.entries()]
    .filter(([path, counts]) => path.length > 0 || counts.additions > 0 || counts.deletions > 0)
    .map(([path, counts]) => ({ path, additions: counts.additions, deletions: counts.deletions }));
  return {
    files: list,
    additions: list.reduce((sum, file) => sum + file.additions, 0),
    deletions: list.reduce((sum, file) => sum + file.deletions, 0),
  };
}

export function referenceDiffSummaryText(
  part: Extract<ReferencePart, { kind: "diff" }>,
  body: string | null,
): string {
  if (body !== null) {
    const summary = summarizeReferenceDiff(body);
    const named = summary.files.filter((file) => file.path.length > 0);
    const fileLabel =
      named.length === 1 ? named[0].path : named.length > 1 ? `${named.length} files` : "Diff";
    return `${fileLabel} +${summary.additions} -${summary.deletions}`;
  }
  const bits = ["Diff", formatPayloadBytes(part.patch.byteLength)].filter((bit) => bit.length > 0);
  if (part.patch.truncated) bits.push("truncated");
  return bits.join(" · ");
}

export function ReferenceStatusBadge({ status }: { status: string }) {
  return (
    <span
      data-testid="reference-status-badge"
      data-status={status}
      className={cn(
        "shrink-0 text-xs",
        status === "failed"
          ? "text-chat-danger"
          : status === "completed"
            ? "text-status-success"
            : "text-chat-foreground-secondary",
      )}
    >
      {status}
    </span>
  );
}

export function ReferenceTurnUsage({ meta }: { meta?: ReferenceTurnMeta | null }) {
  const text = referenceTurnUsageText(meta);
  if (text === null) return null;
  return (
    <p data-testid="reference-turn-usage" className="font-mono text-xs text-chat-foreground-secondary">
      {text}
    </p>
  );
}

export function ReferenceWorkGroup({
  groupKey,
  label,
  running,
  completed,
  status,
  pinned,
  children,
}: {
  groupKey: string;
  label: string;
  running: boolean;
  completed: boolean;
  status?: string | null;
  /** Diff summaries stay visible when the tool rows are collapsed, so an append does not hide them. */
  pinned?: React.ReactNode;
  children: React.ReactNode;
}) {
  const [expanded, setExpanded] = useState(() => running || !completed);
  const wasRunning = useRef(running);
  const bodyId = `${useId()}-work-group`;
  useEffect(() => {
    if (running && !wasRunning.current) setExpanded(true);
    wasRunning.current = running;
  }, [running]);
  return (
    <div
      role="group"
      aria-label={label}
      data-testid="reference-work-group"
      data-group-key={groupKey}
      className="w-full"
    >
      <button
        type="button"
        data-testid="reference-work-group-toggle"
        aria-expanded={expanded}
        aria-controls={bodyId}
        onClick={() => setExpanded((prev) => !prev)}
        className="flex min-h-11 w-full items-center gap-1.5 rounded-md px-1 py-1 text-left text-sm text-chat-foreground-secondary transition-colors hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        <ChevronRight
          aria-hidden="true"
          className={cn(
            "size-3.5 shrink-0 transition-transform duration-200 ease-out motion-reduce:transition-none",
            expanded && "rotate-90",
          )}
        />
        <span className="min-w-0 flex-1 truncate">{label}</span>
        {running && <span className="sr-only">Running</span>}
        {status ? <ReferenceStatusBadge status={status} /> : null}
      </button>
      {pinned}
      <div id={bodyId} hidden={!expanded} data-testid="reference-work-group-body" className="flex flex-col gap-1.5">
        {children}
      </div>
    </div>
  );
}

export function ReferenceDiffPart({
  part,
  context,
  partKey,
  className,
}: {
  part: Extract<ReferencePart, { kind: "diff" }>;
  context?: ReferencePayloadLoadContext;
  partKey?: string;
  className?: string;
}) {
  const body = referenceDiffInlineBody(part);
  const summary = body === null ? null : summarizeReferenceDiff(body);
  const showBody = body !== null && referenceDiffBodyInline(body);
  const named = summary?.files.filter((file) => file.path.length > 0) ?? [];
  const extraFiles = Math.max(0, named.length - REFERENCE_DIFF_FILE_CAP);
  const onLoadPayload = context?.onLoadPayload;
  const canLoad = body === null && onLoadPayload !== undefined;
  const loading = context?.loadingPayloadRef === part.patch.ref;
  return (
    <div data-testid="reference-diff-part" data-part-key={partKey} className={cn("w-full", className)}>
      <div className="flex min-h-11 w-full items-center gap-2 px-1 text-sm text-chat-foreground-secondary">
        <SquarePen aria-hidden="true" className="size-3.5 shrink-0" />
        <span data-testid="reference-diff-summary" className="min-w-0 flex-1 truncate">
          {referenceDiffSummaryText(part, body)}
        </span>
        {canLoad && (
          <button
            type="button"
            data-testid="reference-load-payload"
            aria-busy={loading || undefined}
            disabled={loading}
            onClick={() => onLoadPayload(part.patch)}
            className="min-h-11 shrink-0 rounded-md border border-chat-border bg-chat-surface-raised px-3 text-sm text-chat-foreground transition-colors hover:bg-chat-surface-hover disabled:opacity-60 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            {loading ? "Loading details…" : "Load details"}
          </button>
        )}
      </div>
      {named.length > 1 && (
        <ul data-testid="reference-diff-files" className="ml-7 space-y-0.5 text-xs text-chat-foreground-secondary">
          {named.slice(0, REFERENCE_DIFF_FILE_CAP).map((file) => (
            <li key={file.path} className="truncate font-mono">
              {file.path} +{file.additions} -{file.deletions}
            </li>
          ))}
          {extraFiles > 0 && <li>{extraFiles} more files</li>}
        </ul>
      )}
      {showBody && body !== null && (
        <pre
          data-testid="reference-diff-body"
          tabIndex={0}
          className="ml-7 max-h-40 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-chat-surface px-3 py-2 font-mono text-[0.8125rem] leading-relaxed text-chat-foreground"
        >
          {body}
        </pre>
      )}
    </div>
  );
}

