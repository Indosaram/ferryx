import { useEffect, useMemo } from "react";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { Sidebar } from "../components/Sidebar";
import { TerminalSplitView } from "../components/TerminalSplitView";
import { NotificationCoordinator } from "../lib/notificationCoordinator";
import { projectRootWorktree } from "../lib/projectIdentity";
import type { NativeTerminalAgentStatePayload, RegisteredProject } from "../lib/types";
import { useWorkspaceStore, type WorkspaceServices } from "./workspaceStore";

const native = vi.hoisted(() => ({
  listeners: new Set<(payload: NativeTerminalAgentStatePayload) => void>(),
  notify: vi.fn(async () => undefined),
  sound: vi.fn(async () => undefined),
}));

vi.mock("../lib/tauri", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/tauri")>(),
  onNativeTerminalAgentState: async (handler: (payload: NativeTerminalAgentStatePayload) => void) => {
    native.listeners.add(handler);
    return () => native.listeners.delete(handler);
  },
  onNativeTerminalTitle: async () => () => undefined,
  onNativeTerminalBell: async () => () => undefined,
  onNativeTerminalFocus: async () => () => undefined,
  discoverAgentProviderSession: async () => null,
  dispatchNotification: native.notify,
  playNotificationSound: native.sound,
}));
vi.mock("../components/NativeTerminalPane", () => ({
  NativeTerminalPane: () => <div data-testid="native-terminal-placeholder" />,
}));
vi.mock("../lib/sshHosts", () => ({
  useSshHosts: () => ({ hosts: [{ id: "build", label: "Build machine" }] }),
  getCachedSshHosts: () => [{ id: "build", label: "Build machine" }],
}));

beforeEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
  vi.spyOn(document, "hasFocus").mockReturnValue(false);
});
afterEach(() => {
  cleanup();
  native.listeners.clear();
  vi.restoreAllMocks();
});

type Store = ReturnType<typeof useWorkspaceStore>;

function Harness({ project, services, capture }: {
  readonly project: RegisteredProject;
  readonly services: WorkspaceServices;
  readonly capture: (store: Store) => void;
}) {
  const store = useWorkspaceStore({
    workspaceId: project.workspaceId,
    initialWorktrees: [projectRootWorktree(project)],
    services,
  });
  capture(store);
  const coordinator = useMemo(() => new NotificationCoordinator({ isWindowFocused: () => false }), []);
  useEffect(() => store.subscribeActivityNotification((event) => {
    coordinator.handleAgentStateChange({ ...event, nextState: event.state });
  }), [store.subscribeActivityNotification, coordinator]);
  return (
    <>
      <Sidebar projects={[project]} activeProjectId={project.workspaceId}
        worktrees={store.state.worktrees} agents={store.agents}
        activePath={store.state.activeWorktreePath ?? ""}
        activityByWorktreePath={store.worktreeActivity}
        unreadWorktreePaths={store.state.unreadWorktreePaths}
        onSelectWorktree={(worktree) => store.dispatchWorkspaceAction({ type: "SELECT_WORKTREE", path: worktree.path })}
        onCreateWorktree={() => undefined} />
      <TerminalSplitView layout={store.state.layout} sessions={store.state.sessions}
        activityByTabId={store.tabActivity} activityBySessionId={store.state.activityBySessionId}
        unreadTabIds={store.state.unreadTabIds} />
    </>
  );
}

it.each([
  { path: "/srv/repo", state: "blocked", expected: "waiting" },
  { path: "/srv/repo", state: "idle", expected: "done" },
  { path: "C:\\work\\repo", state: "blocked", expected: "waiting" },
  { path: "C:\\work\\repo", state: "idle", expected: "done" },
] as const)("routes SSH $state at $path into notification, spinner and pane highlight", async ({ path, state, expected }) => {
  const project: RegisteredProject = {
    workspaceId: "ssh:build", repoRoot: path, gitRoot: path,
    target: { kind: "ssh", hostId: "build" },
  };
  const services: WorkspaceServices = {
    ensureTerminalEvents: async () => undefined,
    spawnTerminal: async () => "ssh-backend",
    getTerminalCwd: async () => path,
    closeTerminal: async () => undefined,
    waitForTerminalExit: async () => undefined,
  };
  let current: Store | undefined;
  const getStore = () => {
    if (!current) throw new Error("Workspace did not mount");
    return current;
  };
  render(<Harness project={project} services={services} capture={(store) => { current = store; }} />);
  await act(async () => { await getStore().openTab(projectRootWorktree(project)); });
  expect(native.listeners.size).toBe(1);
  const emit = (next: NativeTerminalAgentStatePayload["state"]) => {
    for (const listener of native.listeners) {
      listener({ sessionId: "ssh-backend", state: next, ruleId: "extension", manifestId: "omo" });
    }
  };

  act(() => emit("working"));
  const root = document.querySelector('[data-shortcut-workspace-id="ssh:build"]');
  if (!root) throw new Error("Missing SSH worktree row");
  expect(root.querySelector('[data-status-state="working"]')).toHaveClass("animate-spin");
  expect(screen.getByTestId("tab-working-indicator")).toBeInTheDocument();
  expect(screen.queryByTestId("attention-frame-bottom")).not.toBeInTheDocument();
  expect(native.notify).not.toHaveBeenCalled();

  act(() => emit(state));
  expect(root.querySelector('[data-status-state="working"]')).not.toBeInTheDocument();
  expect(screen.getByTestId("attention-frame-bottom")).toBeInTheDocument();
  const target = getStore().activityNotificationTargets[0];
  expect(target).toMatchObject({ workspaceId: project.workspaceId, worktreePath: path, state: expected });
  expect(native.notify).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
    source: "agent-task-complete", attentionReason: expected,
    target: { workspaceId: project.workspaceId, sessionId: target?.sessionId },
  }));
  expect(native.sound).toHaveBeenCalledTimes(1);
  expect(getStore().unreadBadgeCount).toBe(1);

  act(() => emit(state));
  expect(native.notify).toHaveBeenCalledTimes(1);
  if (!target) throw new Error("Missing notification target");
  act(() => getStore().dispatchWorkspaceAction({ type: "MARK_SESSION_ACTIVITY_SEEN", sessionId: target.sessionId }));
  expect(screen.queryByTestId("attention-frame-bottom")).not.toBeInTheDocument();
});
