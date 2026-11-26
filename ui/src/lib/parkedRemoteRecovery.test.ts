import { describe, expect, it, vi } from "vitest";
import { startParkedRemoteRecovery, type ParkedRemoteProject } from "./parkedRemoteRecovery";
import { createStandbyBackendSessionId } from "./sessionLifecycle";
import type { TerminalSession } from "./types";
import type { WorkspaceAction, WorkspaceState } from "../state/workspaceStore";

function session(id: string, workspaceId: string, overrides: Partial<TerminalSession> = {}): TerminalSession {
  return {
    id,
    workspaceId,
    cwd: "/repo",
    backendSessionId: `backend-${id}`,
    lifecycle: "working",
    remoteConnectionState: "reconnecting",
    ...overrides,
  } as TerminalSession;
}

function state(workspaceId: string, sessions: TerminalSession[]): WorkspaceState {
  return {
    workspaceId,
    worktrees: [],
    activeWorktreePath: null,
    layout: { activeTabId: null, tabs: [], layoutsByTabId: {} },
    unreadTabIds: {},
    unreadWorktreePaths: {},
    sessions: Object.fromEntries(sessions.map((s) => [s.id, s])),
  } as WorkspaceState;
}

function harness(projects: ParkedRemoteProject[], states: Record<string, WorkspaceState>, mounted = "local") {
  let mountedId = mounted;
  const listeners = new Set<() => void>();
  const sshStop = vi.fn();
  const pairedDispose = vi.fn();
  const pairedReady = new Map<string, (session: TerminalSession) => void>();
  const dispatch = vi.fn((_workspaceId: string, _action: WorkspaceAction) => undefined);
  const startSsh = vi.fn((_workspaceId: string, _sessions: TerminalSession[]) => sshStop);
  const refreshPaired = vi.fn((_workspaceId: string, sessionId: string, onReady: (session: TerminalSession) => void) => {
    pairedReady.set(sessionId, onReady);
    return pairedDispose;
  });
  const stop = startParkedRemoteRecovery({
    projects,
    isMounted: (workspaceId) => workspaceId === mountedId,
    getState: (workspaceId) => states[workspaceId] ?? null,
    dispatch,
    subscribe: (listener) => { listeners.add(listener); return () => listeners.delete(listener); },
    startSsh,
    refreshPaired,
  });
  const mount = (workspaceId: string) => {
    mountedId = workspaceId;
    for (const listener of listeners) listener();
  };
  return { stop, mount, startSsh, sshStop, refreshPaired, pairedDispose, pairedReady, dispatch, listeners };
}

describe("startParkedRemoteRecovery", () => {
  it("starts SSH reconciliation for each parked SSH project with only its live sessions", () => {
    const live = session("s1", "ssh:host");
    const exited = session("s2", "ssh:host", { lifecycle: "exited" });
    const { startSsh } = harness(
      [{ workspaceId: "local" }, { workspaceId: "ssh:host", target: { kind: "ssh" } }],
      { local: state("local", [session("l1", "local")]), "ssh:host": state("ssh:host", [live, exited]) },
    );
    expect(startSsh).toHaveBeenCalledTimes(1);
    expect(startSsh).toHaveBeenCalledWith("ssh:host", [live]);
  });

  it("skips the mounted project and projects with nothing to reconnect", () => {
    const { startSsh, refreshPaired } = harness(
      [
        { workspaceId: "ssh:mounted", target: { kind: "ssh" } },
        { workspaceId: "ssh:empty", target: { kind: "ssh" } },
      ],
      { "ssh:mounted": state("ssh:mounted", [session("s1", "ssh:mounted")]), "ssh:empty": state("ssh:empty", []) },
      "ssh:mounted",
    );
    expect(startSsh).not.toHaveBeenCalled();
    expect(refreshPaired).not.toHaveBeenCalled();
  });

  it("hands SSH reconciliation back when its project becomes mounted", () => {
    const { mount, sshStop } = harness(
      [{ workspaceId: "ssh:host", target: { kind: "ssh" } }],
      { "ssh:host": state("ssh:host", [session("s1", "ssh:host")]) },
    );
    mount("local-other");
    expect(sshStop).not.toHaveBeenCalled();
    mount("ssh:host");
    expect(sshStop).toHaveBeenCalledTimes(1);
    mount("local-other");
    expect(sshStop).toHaveBeenCalledTimes(1);
  });

  it("starts the paired retry only for disconnected sessions with a real backend", () => {
    const paired = "daemon:studio:site";
    const { refreshPaired } = harness(
      [{ workspaceId: paired, target: { kind: "pairedDaemon" } }],
      {
        [paired]: state(paired, [
          session("reconnecting", paired),
          session("disconnected", paired, { remoteConnectionState: "disconnected" }),
          session("connected", paired, { remoteConnectionState: "connected" }),
          session("standby", paired, { backendSessionId: createStandbyBackendSessionId("standby") }),
          session("no-backend", paired, { backendSessionId: null }),
          session("exited", paired, { lifecycle: "exited" }),
        ]),
      },
    );
    expect(refreshPaired.mock.calls.map(([workspaceId, sessionId]) => [workspaceId, sessionId])).toEqual([
      [paired, "reconnecting"],
      [paired, "disconnected"],
    ]);
  });

  it("writes a ready paired session back into its own project", () => {
    const paired = "daemon:studio:site";
    const states = { [paired]: state(paired, [session("p1", paired)]) };
    const { pairedReady, dispatch } = harness([{ workspaceId: paired, target: { kind: "pairedDaemon" } }], states);
    const ready = session("p1", paired, { remoteConnectionState: "connected", remoteGeneration: 3 });
    pairedReady.get("p1")?.(ready);
    expect(dispatch).toHaveBeenCalledWith(paired, {
      type: "RESTORE_WORKSPACE",
      state: { ...states[paired], sessions: { p1: ready } },
    });
  });

  it("ignores a ready result for a session the project no longer holds", () => {
    const paired = "daemon:studio:site";
    const states: Record<string, WorkspaceState> = { [paired]: state(paired, [session("p1", paired)]) };
    const { pairedReady, dispatch } = harness([{ workspaceId: paired, target: { kind: "pairedDaemon" } }], states);
    states[paired] = state(paired, []);
    pairedReady.get("p1")?.(session("p1", paired, { remoteConnectionState: "connected" }));
    expect(dispatch).not.toHaveBeenCalled();
  });

  it("disposes every paired retry once on mount and ignores queued callbacks after handoff", () => {
    // Given two parked sessions and another parked workspace.
    const paired = "daemon:studio:site";
    const other = "daemon:other:site";
    const h = harness(
      [{ workspaceId: paired }, { workspaceId: other }],
      { [paired]: state(paired, [session("p1", paired), session("p2", paired)]), [other]: state(other, [session("p3", other)]) },
    );
    // When the first workspace mounts, then becomes parked again.
    h.mount(paired);
    h.mount("local-other");
    h.pairedReady.get("p1")?.(session("p1", paired, { remoteConnectionState: "connected" }));
    h.pairedReady.get("p2")?.(session("p2", paired, { remoteConnectionState: "connected" }));
    // Then ownership remains handed off while the other workspace remains active.
    expect(h.pairedDispose).toHaveBeenCalledTimes(2);
    expect(h.dispatch).not.toHaveBeenCalled();
    h.stop();
    h.stop();
    expect(h.pairedDispose).toHaveBeenCalledTimes(3);
  });

  it("ignores a queued paired callback after stop", () => {
    // Given a pending paired recovery callback.
    const paired = "daemon:studio:site";
    const h = harness([{ workspaceId: paired }], { [paired]: state(paired, [session("p1", paired)]) });
    // When the owner stops before callback delivery.
    h.stop();
    h.pairedReady.get("p1")?.(session("p1", paired, { remoteConnectionState: "connected" }));
    // Then no state restoration is dispatched.
    expect(h.dispatch).not.toHaveBeenCalled();
  });

  it("stop ends every SSH reconciliation, paired retry and the mount subscription", () => {
    const paired = "daemon:studio:site";
    const { stop, sshStop, pairedDispose, listeners } = harness(
      [
        { workspaceId: "ssh:host", target: { kind: "ssh" } },
        { workspaceId: paired, target: { kind: "pairedDaemon" } },
      ],
      { "ssh:host": state("ssh:host", [session("s1", "ssh:host")]), [paired]: state(paired, [session("p1", paired)]) },
    );
    stop();
    expect(sshStop).toHaveBeenCalledTimes(1);
    expect(pairedDispose).toHaveBeenCalledTimes(1);
    expect(listeners.size).toBe(0);
  });
});
