import { describe, expect, it, vi, afterEach } from "vitest";
import { WebSocketTerminalTransport } from "../lib/terminalTransport/remoteTransport";

describe("WebSocketTerminalTransport - listSessions", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("preserves daemonEpoch, machineId, and target in listSessions", async () => {
    const mockSessions = [
      {
        sessionId: "sess-1",
        target: {
          machineId: "mach-alpha",
          daemonEpoch: "1790310731270",
          sessionId: "sess-1",
        },
        workspaceId: "ws-1",
      },
      {
        sessionId: "sess-2",
        target: {
          machineId: "mach-beta",
          daemonEpoch: 1790310739999,
          sessionId: "sess-2",
        },
        worktreePath: "/tmp/wt2",
        running: true,
      },
      {
        sessionId: "sess-3",
        daemonEpoch: "1790310740000",
        machineId: "mach-gamma",
      },
    ];

    globalThis.fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(mockSessions),
    } as Response);

    const transport = new WebSocketTerminalTransport("http://127.0.0.1:8080", "test-token");
    const sessions = await transport.listSessions();

    expect(sessions).toHaveLength(3);
    expect(sessions[0]).toEqual({
      sessionId: "sess-1",
      target: {
        machineId: "mach-alpha",
        daemonEpoch: "1790310731270",
        sessionId: "sess-1",
      },
      daemonEpoch: "1790310731270",
      machineId: "mach-alpha",
      worktreePath: undefined,
      running: undefined,
    });
    expect(sessions[1]).toEqual({
      sessionId: "sess-2",
      target: {
        machineId: "mach-beta",
        daemonEpoch: 1790310739999,
        sessionId: "sess-2",
      },
      daemonEpoch: "1790310739999",
      machineId: "mach-beta",
      worktreePath: "/tmp/wt2",
      running: true,
    });
    expect(sessions[2]).toEqual({
      sessionId: "sess-3",
      target: undefined,
      daemonEpoch: "1790310740000",
      machineId: "mach-gamma",
      worktreePath: undefined,
      running: undefined,
    });
  });
});
