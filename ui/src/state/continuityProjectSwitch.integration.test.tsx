import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { deriveContinuitySelection } from "../lib/continuitySelection";
import type { CommittedDesktopSelection } from "../lib/useDesktopContinuity";
import { useDesktopContinuity } from "../lib/useDesktopContinuity";
import type { RegisteredProject } from "../lib/tauri";
import type { DesktopContinuityDependencies } from "../lib/desktopContinuityPublisher";
import { clearWorkspaceSnapshot } from "./workspaceSnapshotCache";
import { useWorkspaceStore, type WorkspaceServices } from "./workspaceStore";

const mocks = vi.hoisted(() => ({
  select: vi.fn(), invalidate: vi.fn(), begin: vi.fn(),
  read: vi.fn(), write: vi.fn(), resolveIdentity: vi.fn(), remoteStatus: vi.fn(),
}));

vi.mock("../lib/desktopContinuityPublisher", () => ({
  createDesktopContinuityPublisher: (dependencies: DesktopContinuityDependencies) => {
    void dependencies;
    return { select: mocks.select, invalidate: mocks.invalidate, begin: mocks.begin };
  },
}));
vi.mock("../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/tauri")>()),
  getRemoteStatus: mocks.remoteStatus,
}));
vi.mock("../remote/accountSession", () => ({
  ACCOUNT_SESSION_CHANGED_EVENT: "account-session-changed",
  getConfiguredAccountOrigin: () => "https://account.test",
  getStoredAccountSessionToken: () => "token-a",
  getStoredAccountId: () => "account-a",
  resolveStoredAccountId: mocks.resolveIdentity,
  getAccountContinuity: mocks.read, putAccountContinuity: mocks.write,
}));

const storeServices: WorkspaceServices = {
  ensureTerminalEvents: vi.fn(async () => undefined),
  spawnTerminal: vi.fn(async () => "backend-1"),
  getTerminalCwd: vi.fn(async (path: string) => path),
  closeTerminal: vi.fn(async () => undefined),
  waitForTerminalExit: vi.fn(async () => undefined),
};

const alpha = { workspaceId: "alpha", repoRoot: "/repos/alpha", gitRoot: "/repos/alpha" } as RegisteredProject;
const beta = { workspaceId: "beta", repoRoot: "/repos/beta", gitRoot: "/repos/beta" } as RegisteredProject;

/**
 * Switching to a project whose workspace has no tab yet commits a selection with `workspaceId`
 * matching but `tabId` null and `local` false. That must leave the recorded intent pending so the
 * terminal created after the swap still publishes; clearing it there loses the user's selection.
 */
describe("desktop continuity across a project switch into an empty project", () => {
  beforeEach(() => {
    clearWorkspaceSnapshot();
    vi.clearAllMocks();
    mocks.begin.mockReturnValue({
      credentials: { accountOrigin: "https://account.test", sessionToken: "token-a" },
      generation: 0,
      baseline: Promise.resolve({ ok: true, revision: 0, error: null }),
    });
    mocks.resolveIdentity.mockResolvedValue(null);
    mocks.remoteStatus.mockResolvedValue({ machineId: "local-machine" });
  });

  it("publishes the terminal opened after switching into a project with no tabs", async () => {
    function useShell(workspaceId: string, project: RegisteredProject) {
      const store = useWorkspaceStore({ workspaceId, services: storeServices });
      const selection: CommittedDesktopSelection = deriveContinuitySelection(store.state, project, null);
      const record = useDesktopContinuity({ enabled: true, leader: true, selection, onError: vi.fn() });
      return { store, record };
    }

    const { result, rerender } = renderHook(
      ({ workspaceId, project }: { workspaceId: string; project: RegisteredProject }) =>
        useShell(workspaceId, project),
      { initialProps: { workspaceId: "alpha", project: alpha } },
    );

    act(() => {
      result.current.record({ workspaceId: "alpha" });
      result.current.store.dispatchWorkspaceAction({
        type: "ADD_TAB_WITH_SESSION",
        tab: { id: "tab-alpha", label: "alpha", sessionId: "sess-alpha" },
        session: {
          id: "sess-alpha", cwd: "/repos/alpha", worktreePath: "/repos/alpha", workspaceId: "alpha",
          backendSessionId: "backend-alpha", worktree: null, lifecycle: "working",
        },
      });
    });
    await waitFor(() => expect(mocks.select).toHaveBeenCalledTimes(1));
    expect(mocks.select).toHaveBeenLastCalledWith(
      expect.objectContaining({ sessionId: "backend-alpha" }),
      expect.anything(),
    );

    act(() => { result.current.record({ workspaceId: "beta" }); });
    await act(async () => { rerender({ workspaceId: "beta", project: beta }); });
    expect(result.current.store.state.layout.tabs).toHaveLength(0);
    expect(mocks.select).toHaveBeenCalledTimes(1);

    act(() => {
      result.current.store.dispatchWorkspaceAction({
        type: "ADD_TAB_WITH_SESSION",
        tab: { id: "tab-beta", label: "beta", sessionId: "sess-beta" },
        session: {
          id: "sess-beta", cwd: "/repos/beta", worktreePath: "/repos/beta", workspaceId: "beta",
          backendSessionId: "backend-beta", worktree: null, lifecycle: "working",
        },
      });
    });
    await waitFor(() => expect(mocks.select).toHaveBeenCalledTimes(2));
    expect(mocks.select).toHaveBeenLastCalledWith(expect.objectContaining({
      workspaceId: "beta", sessionId: "backend-beta",
    }), expect.anything());
    expect(mocks.select.mock.calls.some(([target]) => target.workspaceId === "beta" && target.sessionId === "backend-alpha"))
      .toBe(false);
  });
});
