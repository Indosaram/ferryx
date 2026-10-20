import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { resolveActivityIndicator } from "../lib/activity";
import type { TerminalLifecyclePayload, Worktree } from "../lib/types";

const lifecycleListeners = vi.hoisted(() => new Set<(payload: TerminalLifecyclePayload) => void>());

vi.mock("../lib/tauri", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/tauri")>(),
  onTerminalOutput: async () => () => undefined,
  onTerminalLifecycle: async (handler: (payload: TerminalLifecyclePayload) => void) => {
    lifecycleListeners.add(handler);
    return () => lifecycleListeners.delete(handler);
  },
  onNativeTerminalTitle: async () => () => undefined,
  onNativeTerminalBell: async () => () => undefined,
  onNativeTerminalAgentState: async () => () => undefined,
  onNativeTerminalFocus: async () => () => undefined,
  discoverAgentProviderSession: async () => null,
}));

const { terminalEventBus } = await import("../lib/terminalEvents");
const {
  selectWorktreeActivitySummariesAcrossWorkspaces,
  useWorkspaceStore,
} = await import("./workspaceStore");
type WorkspaceServices = import("./workspaceStore").WorkspaceServices;
const { clearWorkspaceSnapshot, getWorkspaceSnapshot } = await import("./workspaceSnapshotCache");

const alpha: Worktree = {
  path: "/repos/alpha",
  head: "aaa",
  branch: "refs/heads/orca/alpha/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function services(): WorkspaceServices {
  return {
    ensureTerminalEvents: async () => { await terminalEventBus.ensureStarted(); },
    spawnTerminal: async () => "backend-alpha",
    getTerminalCwd: async () => alpha.path,
    closeTerminal: async () => undefined,
    waitForTerminalExit: async () => undefined,
  };
}

beforeEach(() => {
  clearWorkspaceSnapshot();
  lifecycleListeners.clear();
  vi.spyOn(document, "hasFocus").mockReturnValue(false);
});

afterEach(() => {
  clearWorkspaceSnapshot();
  lifecycleListeners.clear();
  vi.restoreAllMocks();
});

describe("terminal lifecycle reaches a parked workspace", () => {
  it("settles a parked project's working agent when its PTY exits", async () => {
    const { result, rerender } = renderHook(
      ({ workspaceId }: { workspaceId: string }) =>
        useWorkspaceStore({ workspaceId, initialWorktrees: [alpha], services: services() }),
      { initialProps: { workspaceId: "project-a" } },
    );

    await act(async () => { await terminalEventBus.ensureStarted(); });
    expect(lifecycleListeners.size).toBe(1);

    let openedTabId: string | null = null;
    await act(async () => { openedTabId = await result.current.openTab(alpha); });
    const tabId: string = openedTabId ?? ((): never => { throw new Error("expected a tab"); })();
    const openedTab = result.current.state.layout.tabs[0];
    if (openedTab.kind === "browser" || openedTab.kind === "file") throw new Error("expected a terminal tab");
    const sessionId = openedTab.sessionId;

    act(() => result.current.dispatchWorkspaceAction({
      type: "SESSION_SCREEN_ACTIVITY",
      tabId,
      sessionId,
      state: "working",
      ruleId: "extension",
      manifestId: "omo",
    }));
    expect(result.current.state.activityBySessionId?.[sessionId]?.state).toBe("working");

    // The user switches to another project; project-a now lives only in the snapshot cache.
    rerender({ workspaceId: "project-b" });
    expect(
      resolveActivityIndicator(selectWorktreeActivitySummariesAcrossWorkspaces("project-b")[alpha.path]),
    ).toBe("working");

    // The parked project's PTY dies. Its row must stop reporting live work.
    act(() => {
      for (const listener of lifecycleListeners) {
        listener({ sessionId: "backend-alpha", state: "exited", exitCode: 0, reason: null });
      }
    });

    const parked = getWorkspaceSnapshot("project-a");
    expect(parked?.sessions[sessionId]?.lifecycle).toBe("exited");
    expect(parked?.activityBySessionId?.[sessionId]?.state).toBe("done");
    expect(
      resolveActivityIndicator(selectWorktreeActivitySummariesAcrossWorkspaces("project-b")[alpha.path]),
    ).not.toBe("working");
  });
});
