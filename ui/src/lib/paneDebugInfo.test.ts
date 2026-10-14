import { describe, expect, it } from "vitest";

import { formatPaneDebugInfo, formatPaneDebugInfoAsync } from "./paneDebugInfo";
import type { TerminalSession } from "./types";

describe("formatPaneDebugInfo", () => {
  it("emits one JSON line carrying the identity triad and daemon binding", () => {
    const session = {
      id: "session-a",
      cwd: "/repo",
      workspaceId: "daemon:machine-a",
      worktree: null,
      backendSessionId: "backend-a",
      lifecycle: "working",
      daemonEpoch: "epoch-2",
      remoteGeneration: 3,
    } as TerminalSession;

    const text = formatPaneDebugInfo("leaf-a", session);

    expect(text).not.toContain("\n");
    const parsed = JSON.parse(text);
    expect(parsed).toHaveProperty("liveness");
    expect(parsed).toMatchObject({
      leafId: "leaf-a",
      sessionId: "session-a",
      backendSessionId: "backend-a",
      daemonEpoch: "epoch-2",
      workspaceId: "daemon:machine-a",
      cwd: "/repo",
      lifecycle: "working",
      remoteGeneration: 3,
    });
  });

  it("reports a missing backend binding as null instead of dropping the key", () => {
    const parsed = JSON.parse(formatPaneDebugInfo("leaf-b", undefined));

    expect(parsed).toHaveProperty("backendSessionId", null);
    expect(parsed).toHaveProperty("sessionId", null);
  });

  it("reports the provider session that agent resume uses", () => {
    const session = {
      id: "session-c",
      cwd: "/repo",
      workspaceId: "ferryx",
      worktree: null,
      backendSessionId: null,
      lifecycle: "exited",
      agentType: "omo",
      agentSessionId: null,
      providerSession: { key: "session_id", id: "01a134da-cb1f-7998-90a3-3bce6322e0fd" },
    } as TerminalSession;

    const parsed = JSON.parse(formatPaneDebugInfo("leaf-c", session));

    expect(parsed.providerSession).toEqual({ key: "session_id", id: "01a134da-cb1f-7998-90a3-3bce6322e0fd" });
  });
  it("formats pane debug info asynchronously awaiting native observation", async () => {
    const session = {
      id: "session-async",
      backendSessionId: "backend-async",
      daemonEpoch: "epoch-3",
      workspaceId: "ws-async",
      cwd: "/repo/async",
      lifecycle: "working",
      remoteGeneration: 1,
      remoteConnectionState: "connected",
      agentType: null,
      agentSessionId: null,
      title: "async-term",
      history: [],
    } as unknown as TerminalSession;

    const text = await formatPaneDebugInfoAsync("leaf-async", session);
    const parsed = JSON.parse(text);
    expect(parsed).toHaveProperty("liveness");
    expect(parsed).toMatchObject({
      leafId: "leaf-async",
      sessionId: "session-async",
      backendSessionId: "backend-async",
      daemonEpoch: "epoch-3",
    });
  });

});
