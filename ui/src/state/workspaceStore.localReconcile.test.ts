import { describe, expect, it } from "vitest";

import type { TerminalSession } from "../lib/types";
import { createLayoutState } from "./layout";
import { workspaceReducer, type WorkspaceState } from "./workspaceStore";

const PAIRED_ID = `daemon:${"b".repeat(64)}`;

function session(id: string, workspaceId: string, backendSessionId: string | null): TerminalSession {
  return {
    id,
    workspaceId,
    worktree: null,
    backendSessionId,
    worktreePath: "/repo/main",
    cwd: "/repo/main",
    lifecycle: "working",
    daemonEpoch: "epoch-old",
    lastOutputSequence: "450",
    remoteConnectionState: "reconnecting",
  };
}

function stateWith(sessions: TerminalSession[]): WorkspaceState {
  return {
    workspaceId: "local",
    worktrees: [],
    activeWorktreePath: "/repo/main",
    sessions: Object.fromEntries(sessions.map((candidate) => [candidate.id, candidate])),
    layout: createLayoutState(
      sessions.map((candidate) => ({
        id: `tab-${candidate.id}`,
        sessionId: candidate.id,
        label: candidate.id,
        kind: "terminal" as const,
      })),
    ),
    unreadTabIds: {},
    unreadWorktreePaths: {},
  };
}

describe("LOCAL_SESSIONS_RECONCILED", () => {
  it("clears reconnecting on a live local session and adopts the live epoch", () => {
    const state = stateWith([session("s1", "local", "b1")]);
    const next = workspaceReducer(state, {
      type: "LOCAL_SESSIONS_RECONCILED",
      live: new Map([["b1", { daemonEpoch: "epoch-new", running: true }]]),
    });
    expect(next.sessions.s1.remoteConnectionState).toBeUndefined();
    expect(next.sessions.s1).toMatchObject({
      backendSessionId: "b1",
      lifecycle: "running",
      daemonEpoch: "epoch-new",
      lastOutputSequence: null,
    });
  });

  it("keeps the replay cursor when the live epoch is unchanged", () => {
    const state = stateWith([session("s1", "local", "b1")]);
    const next = workspaceReducer(state, {
      type: "LOCAL_SESSIONS_RECONCILED",
      live: new Map([["b1", { daemonEpoch: "epoch-old", running: true }]]),
    });
    expect(next.sessions.s1).toMatchObject({ lifecycle: "running", lastOutputSequence: "450" });
  });

  it("marks a local session missing from the inventory as exited", () => {
    const state = stateWith([session("s1", "local", "b1")]);
    const next = workspaceReducer(state, { type: "LOCAL_SESSIONS_RECONCILED", live: new Map() });
    expect(next.sessions.s1.remoteConnectionState).toBeUndefined();
    expect(next.sessions.s1).toMatchObject({ backendSessionId: null, lifecycle: "exited" });
  });

  it("treats a listed but not running session as exited", () => {
    const state = stateWith([session("s1", "local", "b1")]);
    const next = workspaceReducer(state, {
      type: "LOCAL_SESSIONS_RECONCILED",
      live: new Map([["b1", { daemonEpoch: null, running: false }]]),
    });
    expect(next.sessions.s1).toMatchObject({ backendSessionId: null, lifecycle: "exited" });
  });

  it("marks local sessions disconnected when reconciliation gave up", () => {
    const state = stateWith([session("s1", "local", "b1")]);
    const next = workspaceReducer(state, { type: "LOCAL_SESSIONS_RECONCILED", live: null });
    expect(next.sessions.s1).toMatchObject({ backendSessionId: "b1", remoteConnectionState: "disconnected" });
  });

  it("leaves ssh and paired sessions untouched", () => {
    const state = stateWith([session("ssh-s", "ssh:host-a", "b-ssh"), session("paired-s", PAIRED_ID, "b-paired")]);
    expect(workspaceReducer(state, { type: "LOCAL_SESSIONS_RECONCILED", live: new Map() })).toBe(state);
    expect(workspaceReducer(state, { type: "LOCAL_SESSIONS_RECONCILED", live: null })).toBe(state);
  });

  it("ignores local sessions that are not reconnecting", () => {
    const connected = { ...session("s1", "local", "b1"), remoteConnectionState: undefined };
    const state = stateWith([connected]);
    expect(workspaceReducer(state, { type: "LOCAL_SESSIONS_RECONCILED", live: new Map() })).toBe(state);
  });
});
