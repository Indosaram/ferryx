import { useMemo, useState } from "react";

import { selectTabActivitySummaries, selectWorktreeActivitySummaries, workspaceReducer, type WorkspaceAction, type WorkspaceState } from "../state/workspaceStore";
import { TabBar } from "../components/TabBar";
import { WorktreeList } from "../components/WorktreeList";
import type { Worktree } from "../lib/types";

const worktreeMain: Worktree = {
  path: "/repo/main",
  head: "abc123",
  branch: "refs/heads/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const worktreeFeature: Worktree = {
  path: "/repo/feature",
  head: "def456",
  branch: "refs/heads/orca/ws-main/feature",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function initialState(): WorkspaceState {
  return {
    worktrees: [worktreeMain, worktreeFeature],
    activeWorktreePath: worktreeMain.path,
    sessions: {
      "session-fg": {
        id: "session-fg",
        cwd: worktreeMain.path,
        workspaceId: "default",
        worktree: { wsId: "ws-main", slug: "main" },
        backendSessionId: "backend-fg",
        lifecycle: "working",
      },
      "session-bg": {
        id: "session-bg",
        cwd: worktreeFeature.path,
        workspaceId: "default",
        worktree: { wsId: "ws-main", slug: "feature" },
        backendSessionId: "backend-bg",
        lifecycle: "working",
      },
    },
    layout: {
      tabs: [
        { id: "tab-fg", label: "main", sessionId: "session-fg" },
        { id: "tab-bg", label: "feature", sessionId: "session-bg" },
      ],
      activeTabId: "tab-fg",
      layoutsByTabId: {
        "tab-fg": {
          root: { type: "leaf", leafId: "leaf-fg" },
          activeLeafId: "leaf-fg",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-fg": "session-fg" },
        },
        "tab-bg": {
          root: { type: "leaf", leafId: "leaf-bg" },
          activeLeafId: "leaf-bg",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-bg": "session-bg" },
        },
      },
    },
    worktreeLayouts: {},
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
  } as unknown as WorkspaceState;
}

/**
 * Browser-rendered harness for agent-activity QA. It mounts the REAL TabBar and WorktreeList and
 * drives them through the REAL workspaceReducer using the same SESSION_TITLE_ACTIVITY payload the
 * native title listener dispatches, so a screenshot of this page is evidence about shipped
 * rendering rather than about a mock.
 */
export function ActivitySurfaceHarness() {
  const [state, setState] = useState<WorkspaceState>(initialState);

  const dispatch = (action: WorkspaceAction) => setState((prev) => workspaceReducer(prev, action));

  const title = (sessionId: string, tabId: string, value: string) =>
    dispatch({ type: "SESSION_TITLE_ACTIVITY", tabId, sessionId, title: value } as WorkspaceAction);

  const screen = (
    sessionId: string,
    tabId: string,
    state: "working" | "blocked" | "idle",
    ruleId: string,
    manifestId?: string,
  ) =>
    dispatch({
      type: "SESSION_SCREEN_ACTIVITY",
      tabId,
      sessionId,
      state,
      ruleId,
      manifestId,
    } as WorkspaceAction);

  const tabActivity = useMemo(() => selectTabActivitySummaries(state), [state]);
  const worktreeActivity = useMemo(() => selectWorktreeActivitySummaries(state), [state]);

  const scenarios: Array<{ id: string; label: string; run: () => void }> = [
    {
      id: "qa-working-active",
      label: "active tab: working",
      run: () => title("session-fg", "tab-fg", "\u280b codex: running tests"),
    },
    {
      id: "qa-working-background",
      label: "background tab: working",
      run: () => title("session-bg", "tab-bg", "\u280b omo: building"),
    },
    {
      id: "qa-nonstatus-after-working",
      label: "background tab: non-status title after working",
      run: () => title("session-bg", "tab-bg", "omo: src/lib/activity.ts"),
    },
    {
      id: "qa-shell-repaint",
      label: "background tab: shell prompt repaint",
      run: () => title("session-bg", "tab-bg", "~/code/project/orca-lite"),
    },
    {
      id: "qa-done-background",
      label: "background tab: done (attention)",
      run: () => title("session-bg", "tab-bg", "codex: done"),
    },
    {
      id: "qa-waiting-background",
      label: "background tab: needs input",
      run: () => title("session-bg", "tab-bg", "omo: permission required"),
    },
    {
      id: "qa-screen-working-background",
      label: "screen rule: background working (bare title agent)",
      run: () => {
        title("session-bg", "tab-bg", "OmO - orca-lite");
        screen("session-bg", "tab-bg", "working", "esc_cancel_working");
      },
    },
    {
      id: "qa-screen-title-cannot-override",
      label: "screen rule: contradictory title cannot clear working",
      run: () => title("session-bg", "tab-bg", "OmO - orca-lite"),
    },
    {
      id: "qa-screen-blocked-background",
      label: "screen rule: background blocked (needs input)",
      run: () => screen("session-bg", "tab-bg", "blocked", "approval_footer_blocked"),
    },
    {
      id: "qa-screen-idle-background",
      label: "screen rule: background idle after working (attention)",
      run: () => screen("session-bg", "tab-bg", "idle", "prompt_idle"),
    },
    { id: "qa-reset", label: "reset", run: () => setState(initialState()) },
  ];

  return (
    <div className="min-h-screen bg-background p-4 text-foreground">
      <div className="mb-3 flex flex-wrap gap-2">
        {scenarios.map((scenario) => (
          <button
            key={scenario.id}
            type="button"
            data-testid={scenario.id}
            onClick={scenario.run}
            className="rounded border border-border px-2 py-1 text-xs"
          >
            {scenario.label}
          </button>
        ))}
      </div>

      <div data-testid="harness-tabbar" className="mb-4 border border-border">
        <TabBar
          tabs={state.layout.tabs}
          activeTabId={state.layout.activeTabId ?? ""}
          onActivate={(id) => dispatch({ type: "ACTIVATE_TAB", tabId: id } as WorkspaceAction)}
          onClose={() => undefined}
          onAdd={() => undefined}
          unreadTabIds={state.unreadTabIds}
          activityByTabId={tabActivity}
        />
      </div>

      <div data-testid="harness-worktrees" className="max-w-xs border border-border p-2">
        <WorktreeList
          worktrees={state.worktrees}
          activePath={state.activeWorktreePath ?? ""}
          agents={[]}
          statuses={{}}
          unreadWorktreePaths={state.unreadWorktreePaths}
          activityByWorktreePath={worktreeActivity}
          onSelect={() => undefined}
          onDelete={() => undefined}
        />
      </div>

      <pre data-testid="harness-state" className="mt-4 overflow-auto text-[10px] leading-tight text-muted-foreground">
        {JSON.stringify(
          {
            activityBySessionId: state.activityBySessionId,
            unreadTabIds: state.unreadTabIds,
            unreadWorktreePaths: state.unreadWorktreePaths,
          },
          null,
          1,
        )}
      </pre>
    </div>
  );
}
