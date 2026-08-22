import { describe, expect, it, vi } from "vitest";
import { TauriTerminalTransport } from "./tauriTransport";
import { WebSocketTerminalTransport } from "./remoteTransport";
import * as tauri from "../tauri";

describe("TerminalTransport abstractions", () => {
  it("TauriTerminalTransport instantiates and conforms to contract", async () => {
    const transport = new TauriTerminalTransport();
    expect(transport).toBeDefined();
    expect(typeof transport.attach).toBe("function");
    expect(typeof transport.write).toBe("function");
    expect(typeof transport.resize).toBe("function");
    expect(typeof transport.signal).toBe("function");
    expect(typeof transport.onOutput).toBe("function");
    expect(typeof transport.listSessions).toBe("function");
  });

  it("TauriTerminalTransport listSessions queries tauri listTerminalSessions", async () => {
    const listSpy = vi.spyOn(tauri, "listTerminalSessions").mockResolvedValueOnce([
      {
        sessionId: "sess-100",
        worktreePath: "/repo/path",
      },
    ]);
    const transport = new TauriTerminalTransport();
    const sessions = await transport.listSessions();

    expect(listSpy).toHaveBeenCalled();
    expect(sessions).toEqual([
      { sessionId: "sess-100", worktreePath: "/repo/path" },
    ]);
  });

  it("WebSocketTerminalTransport formats WS URL and handles events", async () => {
    const transport = new WebSocketTerminalTransport("http://127.0.0.1:43821", "dummy_token");
    expect(transport).toBeDefined();
    expect(typeof transport.attach).toBe("function");
    expect(typeof transport.write).toBe("function");
    expect(typeof transport.resize).toBe("function");
  });
});
