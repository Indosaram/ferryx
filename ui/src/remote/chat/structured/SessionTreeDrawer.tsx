/**
 * `SessionTreeDrawer` (lane H5): the branch navigator and the session actions popover.
 *
 * Two panels in one file because they share one authority: `tree`. `navigate_tree`, `fork` and
 * `edit_message` all address provider *entry* ids, and only `get_tree` hands those out, so a
 * control that acts on a branch is drawn from the tree the panel just read - never from a
 * transcript seq, which is a different number in a different space.
 *
 * Every action is gated on the capability that makes it real (`treeNavigation`, `fork`,
 * `cloneSession`, `sessionRename`, `sessionStats`, `reload`, `compactCustomInstructions`,
 * `favoriteModels`, `scopedModels`). A control the agent cannot honour is not drawn, because a
 * button that can only fail is worse than no button (referenceTypes.ts:302).
 */
import React, { useCallback, useEffect, useMemo, useState } from "react";
import {
  BarChart3,
  ChevronRight,
  GitBranch,
  Copy,
  Loader2,
  Pencil,
  RefreshCw,
  Scissors,
  Trash2,
  X,
} from "lucide-react";
import { cn } from "../../../lib/cn";
import type {
  StructuredCapabilities,
  StructuredForkResult,
  StructuredModelPreferences,
  StructuredSessionSettings,
  StructuredSessionStats,
  StructuredSessionTree,
  StructuredSessionTreeNode,
} from "../../../lib/structuredChatTypes";

const BUTTON =
  "flex min-h-9 items-center gap-1 rounded-md px-2 text-xs transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-40";
const SECONDARY = cn(BUTTON, "text-chat-foreground-secondary hover:bg-chat-surface-hover hover:text-chat-foreground");
const PRIMARY = cn(BUTTON, "bg-chat-surface-hover text-chat-foreground hover:bg-chat-border");
const FIELD =
  "min-h-9 w-full rounded-md border border-chat-border bg-chat-surface px-2 text-xs text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring";

/** One node's depth: how many `parentId` hops it takes to reach a root. */
function depthOf(node: StructuredSessionTreeNode, byId: ReadonlyMap<string, StructuredSessionTreeNode>): number {
  let depth = 0;
  let cursor: StructuredSessionTreeNode | undefined = node;
  const seen = new Set<string>([node.id]);
  while (cursor?.parentId !== undefined) {
    const parentId: string = cursor.parentId;
    const parent: StructuredSessionTreeNode | undefined = byId.get(parentId);
    // A cycle (or a parent the page did not include) stops the walk: an indented row is a hint,
    // not a fact worth hanging the render on.
    if (parent === undefined || seen.has(parent.id)) break;
    seen.add(parent.id);
    depth += 1;
    cursor = parent;
  }
  return depth;
}

/** The tree's nodes in reading order, each with its depth, parents before children. */
export function structuredTreeRows(
  tree: StructuredSessionTree,
): readonly { readonly node: StructuredSessionTreeNode; readonly depth: number }[] {
  const byId = new Map(tree.nodes.map((node) => [node.id, node]));
  const children = new Map<string | undefined, StructuredSessionTreeNode[]>();
  for (const node of tree.nodes) {
    // A parent the page did not include is a root here, so no node is lost to a partial read.
    const key = node.parentId !== undefined && byId.has(node.parentId) ? node.parentId : undefined;
    children.set(key, [...(children.get(key) ?? []), node]);
  }
  const rows: { node: StructuredSessionTreeNode; depth: number }[] = [];
  const seen = new Set<string>();
  const walk = (parentId: string | undefined, depth: number): void => {
    for (const node of children.get(parentId) ?? []) {
      if (seen.has(node.id)) continue;
      seen.add(node.id);
      rows.push({ node, depth });
      walk(node.id, depth + 1);
    }
  };
  walk(undefined, 0);
  // A cycle leaves nodes unreachable from a root; they are still branches the reader may pick.
  for (const node of tree.nodes) {
    if (seen.has(node.id)) continue;
    seen.add(node.id);
    rows.push({ node, depth: depthOf(node, byId) });
  }
  return rows;
}

export interface SessionTreeDrawerProps {
  readonly tree: StructuredSessionTree | null;
  readonly capabilities: StructuredCapabilities | null;
  readonly settings: StructuredSessionSettings;
  readonly stats: StructuredSessionStats | null;
  readonly sessionName: string | null;
  readonly running: boolean;
  readonly onClose: () => void;
  readonly fetchTree: () => Promise<StructuredSessionTree | null>;
  readonly navigateTree: (targetId: string, options?: { readonly summarize?: boolean }) => Promise<boolean>;
  readonly fork: (entryId: string) => Promise<StructuredForkResult | null>;
  readonly cloneSession: () => Promise<StructuredForkResult | null>;
  readonly renameSession: (name: string) => Promise<boolean>;
  readonly editMessage: (entryId: string, role: string, text: string) => Promise<boolean>;
  readonly fetchStats: () => Promise<StructuredSessionStats | null>;
  readonly reloadSession: () => Promise<boolean>;
  readonly clearQueue: () => Promise<boolean>;
  readonly setSettings: (settings: StructuredSessionSettings) => Promise<boolean>;
  readonly setModelPreferences: (preferences: StructuredModelPreferences) => Promise<boolean>;
  readonly compact: (customInstructions?: string) => Promise<void>;
  /** The models the agent lists, for the favorites/scoped pickers. */
  readonly models: readonly { readonly id: string; readonly label: string }[];
}

/**
 * The drawer. It reads the tree when it opens rather than taking one the pane may have read
 * minutes ago: a branch list that is stale sends the reader to the wrong place, and the ids it
 * hands `navigate_tree` have to exist on the provider right now.
 */
export const SessionTreeDrawer: React.FC<SessionTreeDrawerProps> = ({
  tree,
  capabilities,
  settings,
  stats,
  sessionName,
  running,
  onClose,
  fetchTree,
  navigateTree,
  fork,
  cloneSession,
  renameSession,
  editMessage,
  fetchStats,
  reloadSession,
  clearQueue,
  setSettings,
  setModelPreferences,
  compact,
  models,
}) => {
  const [current, setCurrent] = useState<StructuredSessionTree | null>(tree);
  const [reading, setReading] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [name, setName] = useState(sessionName ?? "");
  const [instructions, setInstructions] = useState("");
  const [statsOpen, setStatsOpen] = useState(false);
  /** The totals this panel read, which may be fresher than the one the pane held on mount. */
  const [readStats, setReadStats] = useState<StructuredSessionStats | null>(stats);
  const [forked, setForked] = useState<StructuredForkResult | null>(null);
  const [favorites, setFavorites] = useState<readonly string[]>([]);
  const [scoped, setScoped] = useState<readonly string[]>([]);
  /** The entry being edited, and the text the reader is writing in its place. */
  const [editing, setEditing] = useState<{ readonly id: string; readonly text: string } | null>(null);

  const canNavigate = capabilities?.treeNavigation === true;
  const canFork = capabilities?.fork === true;
  const canClone = capabilities?.cloneSession === true;
  const canRename = capabilities?.sessionRename === true;
  const canStats = capabilities?.sessionStats === true;
  const canReload = capabilities?.reload === true;
  const canCompactWithInstructions = capabilities?.compactCustomInstructions === true;
  const canFavorite = capabilities?.favoriteModels === true;
  const canScope = capabilities?.scopedModels === true;
  const canEditUser = capabilities?.editUserMessage === true;
  const canEditAssistant = capabilities?.editAssistantMessage === true;

  const read = useCallback(async (): Promise<void> => {
    setReading(true);
    try {
      const next = await fetchTree();
      if (next !== null) setCurrent(next);
    } finally {
      setReading(false);
    }
  }, [fetchTree]);

  useEffect(() => {
    if (!canNavigate) return;
    void read();
  }, [canNavigate, read]);

  useEffect(() => {
    setName(sessionName ?? "");
  }, [sessionName]);

  useEffect(() => {
    setReadStats(stats);
  }, [stats]);

  const rows = useMemo(() => (current === null ? [] : structuredTreeRows(current)), [current]);

  const run = useCallback(async (key: string, action: () => Promise<unknown>): Promise<void> => {
    setBusy(key);
    try {
      await action();
    } finally {
      setBusy(null);
    }
  }, []);

  const navigate = useCallback(
    (targetId: string) => void run(`go:${targetId}`, async () => {
      if (await navigateTree(targetId)) await read();
    }),
    [navigateTree, read, run],
  );

  const branch = useCallback(
    (entryId: string) =>
      void run(`fork:${entryId}`, async () => {
        const result = await fork(entryId);
        if (result !== null) setForked(result);
      }),
    [fork, run],
  );

  const toggleModel = (
    list: readonly string[],
    set: (next: readonly string[]) => void,
    id: string,
  ): void => {
    set(list.includes(id) ? list.filter((row) => row !== id) : [...list, id]);
  };

  const savePreferences = useCallback(
    () =>
      void run("prefs", async () => {
        await setModelPreferences({
          ...(canFavorite ? { favoriteModels: [...favorites] } : {}),
          ...(canScope ? { scopedModels: [...scoped] } : {}),
        });
      }),
    [canFavorite, canScope, favorites, scoped, setModelPreferences, run],
  );

  return (
    <aside
      data-testid="session-tree-drawer"
      aria-label="Session branches and actions"
      className="flex h-full w-full max-w-sm flex-col gap-2 overflow-y-auto border-l border-chat-border bg-chat-surface-raised p-3"
    >
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-sm font-medium text-chat-foreground">Branches</h2>
        <div className="flex items-center gap-1">
          {canNavigate ? (
            <button
              type="button"
              data-testid="session-tree-refresh"
              aria-label="Refresh the branch list"
              disabled={reading}
              onClick={() => void read()}
              className={cn(SECONDARY, "size-9 justify-center px-0")}
            >
              {reading ? (
                <Loader2 aria-hidden="true" className="size-4 animate-spin" />
              ) : (
                <RefreshCw aria-hidden="true" className="size-4" />
              )}
            </button>
          ) : null}
          <button
            type="button"
            data-testid="session-tree-close"
            aria-label="Close the branch list"
            onClick={onClose}
            className={cn(SECONDARY, "size-9 justify-center px-0")}
          >
            <X aria-hidden="true" className="size-4" />
          </button>
        </div>
      </div>

      {!canNavigate ? (
        <p data-testid="session-tree-unsupported" className="text-xs text-chat-foreground-secondary">
          This agent does not expose a session tree.
        </p>
      ) : rows.length === 0 ? (
        <p data-testid="session-tree-empty" className="text-xs text-chat-foreground-secondary">
          {reading ? "Reading branches…" : "This session has no branches yet."}
        </p>
      ) : (
        <ul data-testid="session-tree-nodes" className="flex flex-col">
          {rows.map(({ node, depth }) => {
            const active = node.active || current?.currentLeafId === node.id;
            // Editing follows the role: an agent that can rewrite what the user said is not the
            // same agent that can rewrite its own reply, so each gets its own capability bit.
            const canEdit =
              (node.role === "user" && canEditUser) || (node.role === "assistant" && canEditAssistant);
            return (
              <li
                key={node.id}
                data-testid={`session-tree-node-${node.id}`}
                data-active={active}
                data-role={node.role}
                style={{ paddingLeft: `${depth * 12}px` }}
                className={cn(
                  "flex items-center gap-1 rounded-md pr-1",
                  active ? "bg-chat-surface-hover" : "",
                )}
              >
                <button
                  type="button"
                  data-testid={`session-tree-go-${node.id}`}
                  aria-label={`Go to ${node.label ?? (node.preview || node.id)}`}
                  aria-current={active ? "true" : undefined}
                  // The leaf the session is on cannot be navigated to: it is where it already is.
                  disabled={busy !== null || active}
                  onClick={() => navigate(node.id)}
                  className="flex min-h-9 min-w-0 flex-1 items-center gap-1 text-left text-xs text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60"
                >
                  <ChevronRight aria-hidden="true" className="size-3 shrink-0 text-chat-foreground-secondary" />
                  <span className="shrink-0 text-chat-foreground-secondary">{node.role}</span>
                  <span className="min-w-0 flex-1 truncate">{node.label ?? node.preview ?? node.id}</span>
                </button>
                {canEdit ? (
                  <button
                    type="button"
                    data-testid={`session-tree-edit-${node.id}`}
                    aria-label={`Edit the ${node.role} message at ${node.label ?? (node.preview || node.id)}`}
                    disabled={busy !== null}
                    onClick={() => setEditing({ id: node.id, text: node.preview })}
                    className={cn(SECONDARY, "size-9 shrink-0 justify-center px-0")}
                  >
                    <Pencil aria-hidden="true" className="size-3.5" />
                  </button>
                ) : null}
                {canFork ? (
                  <button
                    type="button"
                    data-testid={`session-tree-fork-${node.id}`}
                    aria-label={`Branch from ${node.label ?? (node.preview || node.id)}`}
                    disabled={busy !== null}
                    onClick={() => branch(node.id)}
                    className={cn(SECONDARY, "size-9 shrink-0 justify-center px-0")}
                  >
                    <GitBranch aria-hidden="true" className="size-3.5" />
                  </button>
                ) : null}
              </li>
            );
          })}
        </ul>
      )}

      {forked !== null ? (
        <p
          data-testid="session-tree-forked"
          role="status"
          className="rounded-md border border-chat-border bg-chat-surface px-2 py-1 text-xs text-chat-foreground"
        >
          Branched into {forked.sessionId}
        </p>
      ) : null}

      {editing !== null ? (
        <form
          data-testid="session-tree-edit-form"
          className="flex flex-col gap-1"
          onSubmit={(event) => {
            event.preventDefault();
            const text = editing.text;
            if (text.trim().length === 0) return;
            const role = rows.find((row) => row.node.id === editing.id)?.node.role ?? "user";
            void run(`edit:${editing.id}`, async () => {
              if (await editMessage(editing.id, role, text)) {
                setEditing(null);
                await read();
              }
            });
          }}
        >
          <textarea
            data-testid="session-tree-edit-text"
            aria-label="Replace this message"
            rows={3}
            value={editing.text}
            disabled={busy !== null}
            onChange={(event) => setEditing({ id: editing.id, text: event.currentTarget.value })}
            className="w-full resize-y rounded-md border border-chat-border bg-chat-surface px-2 py-1.5 text-xs text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          />
          <div className="flex items-center justify-end gap-1">
            <button
              type="button"
              data-testid="session-tree-edit-cancel"
              onClick={() => setEditing(null)}
              className={SECONDARY}
            >
              Cancel
            </button>
            <button
              type="submit"
              data-testid="session-tree-edit-save"
              disabled={busy !== null || editing.text.trim().length === 0}
              className={PRIMARY}
            >
              Save
            </button>
          </div>
        </form>
      ) : null}

      <hr className="my-1 border-chat-border" />

      <h2 className="text-sm font-medium text-chat-foreground">Session</h2>

      {canClone ? (
        <button
          type="button"
          data-testid="session-action-clone"
          disabled={busy !== null}
          onClick={() =>
            void run("clone", async () => {
              const result = await cloneSession();
              if (result !== null) setForked(result);
            })
          }
          className={cn(SECONDARY, "justify-start")}
        >
          <Copy aria-hidden="true" className="size-3.5" />
          Clone session
        </button>
      ) : null}

      {canRename ? (
        <form
          data-testid="session-action-rename-form"
          className="flex items-center gap-1"
          onSubmit={(event) => {
            event.preventDefault();
            const trimmed = name.trim();
            if (trimmed.length === 0) return;
            void run("rename", () => renameSession(trimmed));
          }}
        >
          <input
            data-testid="session-action-rename-input"
            aria-label="Session name"
            value={name}
            disabled={busy !== null}
            onChange={(event) => setName(event.currentTarget.value)}
            className={FIELD}
          />
          <button
            type="submit"
            data-testid="session-action-rename"
            aria-label="Rename this session"
            disabled={busy !== null || name.trim().length === 0}
            className={cn(PRIMARY, "shrink-0")}
          >
            <Pencil aria-hidden="true" className="size-3.5" />
            Rename
          </button>
        </form>
      ) : null}

      {canCompactWithInstructions ? (
        <form
          data-testid="session-action-compact-form"
          className="flex flex-col gap-1"
          onSubmit={(event) => {
            event.preventDefault();
            const trimmed = instructions.trim();
            void run("compact", async () => {
              await compact(trimmed.length === 0 ? undefined : trimmed);
              setInstructions("");
            });
          }}
        >
          <textarea
            data-testid="session-action-compact-instructions"
            aria-label="Instructions for the compaction"
            rows={2}
            value={instructions}
            disabled={busy !== null || running}
            placeholder="What should the summary keep?"
            onChange={(event) => setInstructions(event.currentTarget.value)}
            className="w-full resize-y rounded-md border border-chat-border bg-chat-surface px-2 py-1.5 text-xs text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          />
          <button
            type="submit"
            data-testid="session-action-compact"
            // A compaction mid-turn would summarize a conversation still being written.
            disabled={busy !== null || running}
            className={cn(PRIMARY, "justify-start")}
          >
            <Scissors aria-hidden="true" className="size-3.5" />
            Compact with instructions
          </button>
        </form>
      ) : null}

      <label className="flex items-center justify-between gap-2 text-xs text-chat-foreground-secondary">
        <span>Auto-compaction</span>
        <input
          type="checkbox"
          data-testid="session-action-auto-compaction"
          checked={settings.autoCompaction === true}
          disabled={busy !== null}
          onChange={(event) => void run("autoCompaction", () => setSettings({ autoCompaction: event.currentTarget.checked }))}
          className="size-4 accent-current"
        />
      </label>

      <label className="flex items-center justify-between gap-2 text-xs text-chat-foreground-secondary">
        <span>Auto-retry</span>
        <input
          type="checkbox"
          data-testid="session-action-auto-retry"
          checked={settings.autoRetry === true}
          disabled={busy !== null}
          onChange={(event) => void run("autoRetry", () => setSettings({ autoRetry: event.currentTarget.checked }))}
          className="size-4 accent-current"
        />
      </label>

      {canStats ? (
        <>
          <button
            type="button"
            data-testid="session-action-stats"
            disabled={busy !== null}
            onClick={() =>
              void run("stats", async () => {
                const next = await fetchStats();
                // The panel draws what it read: a stale prop would show the totals from before the
                // turn the reader just finished.
                if (next !== null) setReadStats(next);
                setStatsOpen(true);
              })
            }
            className={cn(SECONDARY, "justify-start")}
          >
            <BarChart3 aria-hidden="true" className="size-3.5" />
            Session stats
          </button>
          {statsOpen && readStats !== null ? (
            <dl
              data-testid="session-action-stats-view"
              className="grid grid-cols-2 gap-x-2 gap-y-0.5 rounded-md border border-chat-border bg-chat-surface px-2 py-1 text-xs text-chat-foreground-secondary"
            >
              <dt>Turns</dt>
              <dd data-testid="session-stats-turns" className="text-chat-foreground">{readStats.turns}</dd>
              <dt>Total tokens</dt>
              <dd data-testid="session-stats-total" className="text-chat-foreground">{readStats.totalTokens}</dd>
              <dt>Input</dt>
              <dd data-testid="session-stats-input" className="text-chat-foreground">{readStats.inputTokens}</dd>
              <dt>Output</dt>
              <dd data-testid="session-stats-output" className="text-chat-foreground">{readStats.outputTokens}</dd>
              {readStats.contextWindow !== undefined ? (
                <>
                  <dt>Context window</dt>
                  <dd data-testid="session-stats-context" className="text-chat-foreground">{readStats.contextWindow}</dd>
                </>
              ) : null}
              {readStats.costUsd !== undefined ? (
                <>
                  <dt>Cost</dt>
                  <dd data-testid="session-stats-cost" className="text-chat-foreground">${readStats.costUsd.toFixed(4)}</dd>
                </>
              ) : null}
            </dl>
          ) : null}
        </>
      ) : null}

      {canReload ? (
        <button
          type="button"
          data-testid="session-action-reload"
          disabled={busy !== null}
          onClick={() => void run("reload", reloadSession)}
          className={cn(SECONDARY, "justify-start")}
        >
          <RefreshCw aria-hidden="true" className="size-3.5" />
          Reload session
        </button>
      ) : null}

      <button
        type="button"
        data-testid="session-action-clear-queue"
        disabled={busy !== null}
        onClick={() => void run("clearQueue", clearQueue)}
        className={cn(SECONDARY, "justify-start")}
      >
        <Trash2 aria-hidden="true" className="size-3.5" />
        Clear queue
      </button>

      {canFavorite || canScope ? (
        <>
          <hr className="my-1 border-chat-border" />
          <h2 className="text-sm font-medium text-chat-foreground">Models</h2>
          {models.length === 0 ? (
            <p className="text-xs text-chat-foreground-secondary">This agent lists no models.</p>
          ) : (
            <ul data-testid="session-model-preferences" className="flex flex-col gap-1">
              {models.map((model) => (
                <li key={model.id} className="flex items-center justify-between gap-2 text-xs">
                  <span className="min-w-0 flex-1 truncate text-chat-foreground">{model.label}</span>
                  {canFavorite ? (
                    <label className="flex items-center gap-1 text-chat-foreground-secondary">
                      <input
                        type="checkbox"
                        data-testid={`session-model-favorite-${model.id}`}
                        aria-label={`Favorite ${model.label}`}
                        checked={favorites.includes(model.id)}
                        onChange={() => toggleModel(favorites, setFavorites, model.id)}
                        className="size-3.5 accent-current"
                      />
                      <span>Fav</span>
                    </label>
                  ) : null}
                  {canScope ? (
                    <label className="flex items-center gap-1 text-chat-foreground-secondary">
                      <input
                        type="checkbox"
                        data-testid={`session-model-scoped-${model.id}`}
                        aria-label={`Scope ${model.label}`}
                        checked={scoped.includes(model.id)}
                        onChange={() => toggleModel(scoped, setScoped, model.id)}
                        className="size-3.5 accent-current"
                      />
                      <span>Scoped</span>
                    </label>
                  ) : null}
                </li>
              ))}
            </ul>
          )}
          <button
            type="button"
            data-testid="session-model-preferences-save"
            disabled={busy !== null}
            onClick={savePreferences}
            className={cn(PRIMARY, "justify-start")}
          >
            Save model preferences
          </button>
        </>
      ) : null}
    </aside>
  );
};
