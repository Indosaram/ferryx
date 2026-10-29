import { Columns2, GitCommitHorizontal, RefreshCw, Rows2, Upload } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { cn } from "../lib/cn";
import { highlightLines } from "../lib/previewHighlight";
import {
  commitWorktree,
  getWorktreeDiff,
  getChangesStatus,
  pushWorktree,
  stageWorktreePaths,
  toIpcError,
  unstageWorktreePaths,
  type ChangesTarget,
  type GitDiffScope,
} from "../lib/tauri";
import type { DirtyFile, StructuredIpcError, WorktreeIdentity } from "../lib/types";
import { SegmentedControl } from "./ui/SegmentedControl";

export type ChangesPanelProps = {
  workspaceId: string;
  /** `null` for the main checkout or an unmanaged worktree, addressed by `worktreePath`. */
  worktree: WorktreeIdentity | null;
  worktreePath: string;
  /** The tab is on screen; becoming visible refreshes the file list. */
  active?: boolean;
};

type ChangeGroup = "workingTree" | "staged";

export type ChangeEntry = {
  path: string;
  /** One-letter Git status for this side (M, A, D, R, ?, U...). */
  status: string;
};

type Selection = { group: ChangeGroup; path: string };

type DiffRowKind = "add" | "del" | "ctx" | "hunk" | "meta";

type DiffRow = {
  kind: DiffRowKind;
  /** Index into the patch lines, for the highlighted HTML. */
  index: number;
  text: string;
  oldLine: number | null;
  newLine: number | null;
};

type DiffViewMode = "unified" | "split";

/** Splits `git status --porcelain` rows into the unstaged and staged sides. */
export function groupChanges(files: readonly DirtyFile[]): Record<ChangeGroup, ChangeEntry[]> {
  const workingTree: ChangeEntry[] = [];
  const staged: ChangeEntry[] = [];
  for (const file of files) {
    const code = file.statusCode.padEnd(2, " ");
    const path = statusPath(file.path);
    if (!path) continue;
    if (code === "??" || code.trim() === "?") {
      workingTree.push({ path, status: "?" });
      continue;
    }
    const index = code[0];
    const tree = code[1];
    if (index !== " " && index !== "?" && index !== "!") staged.push({ path, status: index });
    if (tree !== " " && tree !== "!") workingTree.push({ path, status: tree });
  }
  return { workingTree, staged };
}

/** Porcelain v1 shows renames as `old -> new` and quotes unusual names. */
function statusPath(raw: string): string {
  const arrow = raw.lastIndexOf(" -> ");
  const target = arrow >= 0 ? raw.slice(arrow + 4) : raw;
  if (target.length >= 2 && target.startsWith('"') && target.endsWith('"')) {
    return target.slice(1, -1).replace(/\\(["\\])/g, "$1").replace(/\\t/g, "\t");
  }
  return target;
}

export function parseUnifiedDiff(patch: string): DiffRow[] {
  const rows: DiffRow[] = [];
  const lines = patch.split(/\r\n|\r|\n/);
  if (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();
  let oldLine = 0;
  let newLine = 0;
  let inHunk = false;
  lines.forEach((text, index) => {
    const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(text);
    if (hunk) {
      oldLine = Number(hunk[1]);
      newLine = Number(hunk[2]);
      inHunk = true;
      rows.push({ kind: "hunk", index, text, oldLine: null, newLine: null });
      return;
    }
    if (!inHunk || text.startsWith("diff --git ")) {
      inHunk = false;
      rows.push({ kind: "meta", index, text, oldLine: null, newLine: null });
      return;
    }
    if (text.startsWith("+")) {
      rows.push({ kind: "add", index, text, oldLine: null, newLine: newLine++ });
    } else if (text.startsWith("-")) {
      rows.push({ kind: "del", index, text, oldLine: oldLine++, newLine: null });
    } else if (text.startsWith("\\")) {
      rows.push({ kind: "meta", index, text, oldLine: null, newLine: null });
    } else {
      rows.push({ kind: "ctx", index, text, oldLine: oldLine++, newLine: newLine++ });
    }
  });
  return rows;
}

type SplitRow = { left: DiffRow | null; right: DiffRow | null; full?: DiffRow };

function splitRows(rows: readonly DiffRow[]): SplitRow[] {
  const result: SplitRow[] = [];
  let dels: DiffRow[] = [];
  let adds: DiffRow[] = [];
  const flush = () => {
    const count = Math.max(dels.length, adds.length);
    for (let i = 0; i < count; i++) result.push({ left: dels[i] ?? null, right: adds[i] ?? null });
    dels = [];
    adds = [];
  };
  for (const row of rows) {
    if (row.kind === "del") {
      if (adds.length > 0) flush();
      dels.push(row);
    } else if (row.kind === "add") {
      adds.push(row);
    } else {
      flush();
      if (row.kind === "ctx") result.push({ left: row, right: row });
      else result.push({ left: null, right: null, full: row });
    }
  }
  flush();
  return result;
}

/** Friendly inline text for the structured codes todo 12 returns; anything else keeps its message. */
function errorText(error: StructuredIpcError): string {
  switch (error.code) {
    case "GIT_NOTHING_TO_COMMIT":
      return "Nothing is staged. Tick files under Working tree to stage them, then commit.";
    case "GIT_NO_REMOTE":
      return "This branch has no upstream and the repository has no 'origin' remote to push to.";
    case "GIT_PUSH_REJECTED":
      return "The remote rejected the push. Pull or rebase onto the remote branch, then push again.";
    default:
      return error.message;
  }
}

const ROW_TONE: Record<DiffRowKind, string> = {
  add: "bg-emerald-500/15",
  del: "bg-red-500/15",
  ctx: "",
  hunk: "bg-sky-500/10 text-muted-foreground",
  meta: "text-muted-foreground",
};

const GUTTER_TONE: Record<DiffRowKind, string> = {
  add: "border-l-2 border-emerald-500",
  del: "border-l-2 border-red-500",
  ctx: "border-l-2 border-transparent",
  hunk: "border-l-2 border-transparent",
  meta: "border-l-2 border-transparent",
};

export function ChangesPanel({ workspaceId, worktree, worktreePath, active = true }: ChangesPanelProps) {
  const [groups, setGroups] = useState<Record<ChangeGroup, ChangeEntry[]>>({ workingTree: [], staged: [] });
  const [loaded, setLoaded] = useState(false);
  const [selection, setSelection] = useState<Selection | null>(null);
  const [patch, setPatch] = useState<string | null>(null);
  const [diffError, setDiffError] = useState<StructuredIpcError | null>(null);
  const [mode, setMode] = useState<DiffViewMode>("unified");
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState<null | "stage" | "commit" | "push">(null);
  const [actionError, setActionError] = useState<StructuredIpcError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const wsId = worktree?.wsId;
  const slug = worktree?.slug;
  const request = useMemo<ChangesTarget>(
    () => (wsId !== undefined && slug !== undefined ? { workspaceId, worktree: { wsId, slug } } : { workspaceId, worktreePath }),
    [workspaceId, wsId, slug, worktreePath],
  );
  const refreshSeq = useRef(0);
  const diffSeq = useRef(0);
  // `busy` state lags a render behind; this ref stops a second action (e.g. a repeated
  // Cmd+Enter) from starting while one is still in flight.
  const inFlight = useRef(false);

  const refresh = useCallback(async () => {
    const seq = ++refreshSeq.current;
    try {
      const status = await getChangesStatus(request);
      if (seq !== refreshSeq.current) return;
      const next = groupChanges(status.files);
      setGroups(next);
      setLoaded(true);
      setSelection((current) => {
        if (current && next[current.group].some((entry) => entry.path === current.path)) return current;
        const first = next.workingTree[0] ? { group: "workingTree" as const, path: next.workingTree[0].path }
          : next.staged[0] ? { group: "staged" as const, path: next.staged[0].path }
            : null;
        return first;
      });
    } catch (error) {
      if (seq !== refreshSeq.current) return;
      setActionError(toIpcError(error));
      setLoaded(true);
    }
  }, [request]);

  const loadDiff = useCallback(async (target: Selection | null) => {
    const seq = ++diffSeq.current;
    if (!target) {
      setPatch(null);
      setDiffError(null);
      return;
    }
    const scope: GitDiffScope = { kind: target.group };
    try {
      const text = await getWorktreeDiff({ ...request, scope, file: target.path });
      if (seq !== diffSeq.current) return;
      setPatch(text);
      setDiffError(null);
    } catch (error) {
      if (seq !== diffSeq.current) return;
      setPatch(null);
      setDiffError(toIpcError(error));
    }
  }, [request]);

  useEffect(() => {
    if (active) void refresh();
  }, [active, refresh]);

  useEffect(() => {
    if (!active) return;
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [active, refresh]);

  // Reload the diff whenever the selection or the file list changes (the file may have changed).
  useEffect(() => {
    void loadDiff(selection);
  }, [groups, loadDiff, selection]);

  const toggleStage = async (group: ChangeGroup, path: string) => {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy("stage");
    setActionError(null);
    setNotice(null);
    try {
      if (group === "workingTree") await stageWorktreePaths({ ...request, paths: [path] });
      else await unstageWorktreePaths({ ...request, paths: [path] });
    } catch (error) {
      setActionError(toIpcError(error));
    } finally {
      inFlight.current = false;
      setBusy(null);
      await refresh();
    }
  };

  const commit = async () => {
    const trimmed = message.trim();
    if (!trimmed || inFlight.current) return;
    inFlight.current = true;
    setBusy("commit");
    setActionError(null);
    setNotice(null);
    try {
      const sha = await commitWorktree({ ...request, message });
      setMessage("");
      setNotice(`Committed ${sha.slice(0, 8)}`);
    } catch (error) {
      setActionError(toIpcError(error));
    } finally {
      inFlight.current = false;
      setBusy(null);
      await refresh();
    }
  };

  const push = async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy("push");
    setActionError(null);
    setNotice(null);
    try {
      const result = await pushWorktree(request);
      setNotice(`Pushed ${result.branch} to ${result.remote}`);
    } catch (error) {
      setActionError(toIpcError(error));
    } finally {
      inFlight.current = false;
      setBusy(null);
      await refresh();
    }
  };

  const rows = useMemo(() => (patch ? parseUnifiedDiff(patch) : []), [patch]);
  const highlighted = useMemo(() => (patch ? highlightLines(patch, "diff") : null), [patch]);
  const empty = loaded && groups.workingTree.length === 0 && groups.staged.length === 0;

  return (
    <div data-testid="changes-panel" data-worktree-path={worktreePath} className="flex h-full min-h-0 w-full flex-col bg-background text-foreground">
      <div className="flex min-h-0 flex-1">
        <div className="flex w-64 shrink-0 flex-col border-r border-border">
          <div className="flex h-8 items-center justify-between border-b border-border px-2">
            <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Changes</span>
            <button
              type="button"
              aria-label="Refresh changes"
              onClick={() => void refresh()}
              className="rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <RefreshCw className="size-3" />
            </button>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto py-1 scrollbar-sleek">
            {empty ? (
              <div role="status" className="px-3 py-4 text-xs text-muted-foreground">No changes in this worktree.</div>
            ) : null}
            {(["staged", "workingTree"] as const).map((group) =>
              groups[group].length > 0 ? (
                <section key={group} aria-label={group === "staged" ? "Staged" : "Working tree"} className="mb-1">
                  <h3 className="flex items-center justify-between px-2 py-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                    <span>{group === "staged" ? "Staged" : "Working tree"}</span>
                    <span className="tabular-nums">{groups[group].length}</span>
                  </h3>
                  <ul className="m-0 list-none p-0">
                    {groups[group].map((entry) => {
                      const selected = selection?.group === group && selection.path === entry.path;
                      return (
                        <li
                          key={`${group}:${entry.path}`}
                          className={cn(
                            "flex items-center gap-1.5 px-2 text-xs",
                            selected ? "bg-accent text-foreground" : "text-muted-foreground hover:bg-accent/50",
                          )}
                        >
                          <input
                            type="checkbox"
                            checked={group === "staged"}
                            disabled={busy !== null}
                            aria-label={group === "staged" ? `Unstage ${entry.path}` : `Stage ${entry.path}`}
                            onChange={() => void toggleStage(group, entry.path)}
                            className="size-3 shrink-0 accent-primary"
                          />
                          <button
                            type="button"
                            aria-pressed={selected}
                            onClick={() => setSelection({ group, path: entry.path })}
                            title={entry.path}
                            className="flex h-6 min-w-0 flex-1 items-center gap-1.5 text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                          >
                            <span className="min-w-0 flex-1 truncate font-mono">{entry.path}</span>
                            <span className={cn("shrink-0 font-mono text-[10px]", statusTone(entry.status))}>{entry.status}</span>
                          </button>
                        </li>
                      );
                    })}
                  </ul>
                </section>
              ) : null,
            )}
          </div>
        </div>

        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex h-8 shrink-0 items-center gap-2 border-b border-border px-3">
            <span className="min-w-0 flex-1 truncate font-mono text-xs">
              {selection ? selection.path : ""}
              {selection ? (
                <span className="ml-2 font-sans text-[10px] text-muted-foreground">
                  {selection.group === "staged" ? "Staged" : "Working tree"}
                </span>
              ) : null}
            </span>
            <SegmentedControl<DiffViewMode>
              label="Diff layout"
              value={mode}
              onChange={setMode}
              className="w-40"
              options={[
                { value: "unified", label: <span className="inline-flex items-center gap-1"><Rows2 className="size-3" />Unified</span>, ariaLabel: "Unified" },
                { value: "split", label: <span className="inline-flex items-center gap-1"><Columns2 className="size-3" />Split</span>, ariaLabel: "Split" },
              ]}
            />
          </div>
          <div data-testid="changes-diff" data-mode={mode} className="min-h-0 flex-1 overflow-auto font-mono text-xs leading-5 scrollbar-sleek">
            {diffError ? (
              <div role="alert" data-error-code={diffError.code} className="px-3 py-2 text-destructive">{errorText(diffError)}</div>
            ) : !selection ? null : rows.length === 0 && patch !== null ? (
              <div role="status" className="px-3 py-4 font-sans text-muted-foreground">
                {selection.group === "workingTree" && isUntracked(groups, selection.path)
                  ? "New untracked file with no text preview (empty, binary or too large)."
                  : "No textual diff for this file."}
              </div>
            ) : mode === "unified" ? (
              <div className="min-w-max py-1">
                {rows.map((row) => (
                  <div key={row.index} data-testid="diff-row" data-kind={row.kind} className={cn("flex", ROW_TONE[row.kind], GUTTER_TONE[row.kind])}>
                    <span aria-hidden className="w-10 shrink-0 select-none pr-1 text-right text-muted-foreground/70 tabular-nums">{row.oldLine ?? ""}</span>
                    <span aria-hidden className="w-10 shrink-0 select-none pr-2 text-right text-muted-foreground/70 tabular-nums">{row.newLine ?? ""}</span>
                    <DiffText row={row} html={highlighted?.[row.index]} />
                  </div>
                ))}
              </div>
            ) : (
              <div className="py-1">
                {splitRows(rows).map((pair, i) =>
                  pair.full ? (
                    <div key={i} data-testid="diff-row" data-kind={pair.full.kind} className={cn("flex", ROW_TONE[pair.full.kind])}>
                      <span className="w-10 shrink-0" />
                      <DiffText row={pair.full} html={highlighted?.[pair.full.index]} />
                    </div>
                  ) : (
                    <div key={i} className="grid grid-cols-2">
                      <SplitCell row={pair.left} line={pair.left?.oldLine ?? null} html={pair.left ? highlighted?.[pair.left.index] : undefined} />
                      <SplitCell row={pair.right} line={pair.right?.newLine ?? null} html={pair.right ? highlighted?.[pair.right.index] : undefined} />
                    </div>
                  ),
                )}
              </div>
            )}
          </div>
        </div>
      </div>

      <div className="shrink-0 border-t border-border p-2">
        {actionError ? (
          <div role="alert" data-error-code={actionError.code} className="mb-2 rounded border border-destructive/40 bg-destructive/10 px-2 py-1.5 text-xs text-destructive">
            {errorText(actionError)}
          </div>
        ) : notice ? (
          <div role="status" className="mb-2 px-1 text-xs text-muted-foreground">{notice}</div>
        ) : null}
        <div className="flex items-end gap-2">
          <textarea
            value={message}
            onChange={(event) => setMessage(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                event.preventDefault();
                void commit();
              }
            }}
            aria-label="Commit message"
            placeholder="Commit message"
            rows={2}
            className="min-h-[2.5rem] min-w-0 flex-1 resize-y rounded-md border border-input bg-transparent px-2 py-1 text-xs outline-none placeholder:text-muted-foreground focus-visible:ring-1 focus-visible:ring-ring"
          />
          <div className="flex shrink-0 flex-col gap-1">
            <button
              type="button"
              onClick={() => void commit()}
              disabled={busy !== null || message.trim().length === 0}
              className="inline-flex h-7 items-center gap-1 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-50"
            >
              <GitCommitHorizontal className="size-3.5" />
              Commit
            </button>
            <button
              type="button"
              onClick={() => void push()}
              disabled={busy !== null}
              className="inline-flex h-7 items-center gap-1 rounded-md border border-input px-3 text-xs hover:bg-accent disabled:pointer-events-none disabled:opacity-50"
            >
              <Upload className="size-3.5" />
              Push
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function isUntracked(groups: Record<ChangeGroup, ChangeEntry[]>, path: string): boolean {
  return groups.workingTree.some((entry) => entry.path === path && entry.status === "?");
}

function statusTone(status: string): string {
  if (status === "A" || status === "?") return "text-emerald-500";
  if (status === "D") return "text-red-500";
  if (status === "U") return "text-amber-500";
  return "text-sky-500";
}

function DiffText({ row, html }: { row: DiffRow; html: string | undefined }) {
  // Safe only because highlight.js escapes every source character and highlightLines adds
  // nothing but its own <span class> tags (same contract as FilePreviewText).
  return html !== undefined ? (
    <span className="whitespace-pre pr-4 hljs-line" dangerouslySetInnerHTML={{ __html: html }} />
  ) : (
    <span className="whitespace-pre pr-4">{row.text}</span>
  );
}

function SplitCell({ row, line, html }: { row: DiffRow | null; line: number | null; html: string | undefined }) {
  if (!row) return <div className="min-w-0 bg-muted/20" />;
  return (
    <div data-testid="diff-row" data-kind={row.kind} className={cn("flex min-w-0 overflow-hidden", ROW_TONE[row.kind], GUTTER_TONE[row.kind])}>
      <span aria-hidden className="w-10 shrink-0 select-none pr-2 text-right text-muted-foreground/70 tabular-nums">{line ?? ""}</span>
      <DiffText row={row} html={html} />
    </div>
  );
}
