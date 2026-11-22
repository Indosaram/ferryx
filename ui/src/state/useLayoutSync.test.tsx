import { act, renderHook } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { pairedTerminalReattach } from "../lib/tauri";
import type { PairedDaemonProjectAdapter } from "../lib/pairedDaemonProject";
import type { RegisteredProject } from "../lib/types";
import { createLayoutState } from "./layout";
import type { WorkspaceState } from "./workspaceStore";
import { useLayoutSync } from "./useLayoutSync";

vi.mock("../lib/tauri", () => ({ layoutGet: vi.fn(), layoutPut: vi.fn(), pairedTerminalReattach: vi.fn() }));

it("adopts a paired proxy for an unseen host leaf without publishing the proxy id", async () => {
  const target = { machineId: "machine", daemonEpoch: "7", sessionId: "host-session" };
  const doc = { workspaceId: "remote", revision: 1, tabs: [{ id: "tab", kind: "terminal", label: "tab", pinned: false,
    paneTree: { type: "leaf", leafId: "leaf", backendSessionId: target.sessionId } }] };
  let finish!: () => void;
  const adopted = new Promise<void>((resolve) => { finish = resolve; });
  const reattach = vi.mocked(pairedTerminalReattach).mockResolvedValue("daemon-session:proxy");
  const adapter = {
    context: { hostId: "host", generation: "2" },
    layoutGet: vi.fn().mockResolvedValueOnce({ layout: doc }).mockImplementation(() => new Promise(() => {})),
    layoutPut: vi.fn(),
    sessions: vi.fn().mockResolvedValue({ sessions: [{ target, cwd: "/repo", running: true }] }),
  } as unknown as PairedDaemonProjectAdapter;
  let state: WorkspaceState = { workspaceId: "local", worktrees: [], activeWorktreePath: null, sessions: {},
    layout: createLayoutState(), unreadTabIds: {}, unreadWorktreePaths: {} };
  const project = { workspaceId: "local", remoteWorkspaceId: "remote", target: { kind: "pairedDaemon", hostId: "host" } } as RegisteredProject;
  let rerender: () => void = () => {};
  const hook = renderHook(() => useLayoutSync({ project, state, adapter, enabled: true, dispatch: (action) => {
    if (action.type !== "RESTORE_WORKSPACE") return;
    state = action.state;
    rerender();
    if (state.sessions["layout-sync:host-session"]?.backendSessionId === "daemon-session:proxy") finish();
  } }));
  rerender = hook.rerender;
  await act(async () => { await adopted; });
  expect(reattach).toHaveBeenCalledWith({ hostId: "host", generation: "2", target });
  expect(state.sessions["layout-sync:host-session"]).toMatchObject({ backendSessionId: "daemon-session:proxy", daemonEpoch: "7", cwd: "/repo" });
  expect(adapter.layoutPut).not.toHaveBeenCalled();
  hook.unmount();
});
