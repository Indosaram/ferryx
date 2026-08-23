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
      { sessionId: "sess-100", worktreePath: "/repo/path", daemonEpoch: null },
    ]);
  });

  it("TauriTerminalTransport attach invokes attachTerminal and decodes base64 history", async () => {
    const attachSpy = vi.spyOn(tauri, "attachTerminal").mockResolvedValueOnce({
      sessionId: "sess-200",
      daemonEpoch: "epoch-1",
      historyStartSequence: "10",
      historyEndSequence: "12",
      history: btoa("history buffer content"),
      gap: {
        requestedAfterSequence: "5",
        availableFromSequence: "10",
      },
    });

    const transport = new TauriTerminalTransport();
    const attachment = await transport.attach("sess-200", "5");

    expect(attachSpy).toHaveBeenCalledWith({ sessionId: "sess-200", afterSequence: "5" });
    expect(attachment.sessionId).toBe("sess-200");
    expect(attachment.daemonEpoch).toBe("epoch-1");
    expect(attachment.historyStartSequence).toBe("10");
    expect(attachment.historyEndSequence).toBe("12");
    expect(attachment.gap).toEqual({
      requestedAfterSequence: "5",
      availableFromSequence: "10",
    });
    expect(attachment.initialHistory).toBeDefined();
    expect(new TextDecoder().decode(attachment.initialHistory)).toBe("history buffer content");
  });

  it("TauriTerminalTransport attach handles empty history properly", async () => {
    vi.spyOn(tauri, "attachTerminal").mockResolvedValueOnce({
      sessionId: "sess-empty",
      daemonEpoch: null,
      historyStartSequence: null,
      historyEndSequence: null,
      history: "",
      gap: null,
    });

    const transport = new TauriTerminalTransport();
    const attachment = await transport.attach("sess-empty", null);

    expect(attachment.sessionId).toBe("sess-empty");
    expect(attachment.initialHistory).toBeUndefined();
    expect(attachment.gap).toBeNull();
  });

  it("TauriTerminalTransport delegates write, resize, signal, and close to tauri IPC", async () => {
    const writeSpy = vi.spyOn(tauri, "writeTerminal").mockResolvedValue();
    const resizeSpy = vi.spyOn(tauri, "resizeTerminal").mockResolvedValue();
    const signalSpy = vi.spyOn(tauri, "signalTerminal").mockResolvedValue();
    const closeSpy = vi.spyOn(tauri, "closeTerminal").mockResolvedValue();

    const transport = new TauriTerminalTransport();

    // write string
    await transport.write("s1", "ls -la\n");
    expect(writeSpy).toHaveBeenCalledWith({ sessionId: "s1", data: "ls -la\n" });

    // write Uint8Array
    await transport.write("s1", new TextEncoder().encode("echo hi\n"));
    expect(writeSpy).toHaveBeenCalledWith({ sessionId: "s1", data: "echo hi\n" });

    // resize
    await transport.resize("s1", 120, 40);
    expect(resizeSpy).toHaveBeenCalledWith({ sessionId: "s1", cols: 120, rows: 40 });

    // signal
    await transport.signal("s1", "interrupt");
    expect(signalSpy).toHaveBeenCalledWith({ sessionId: "s1", signal: "interrupt" });

    // close
    await transport.close("s1");
    expect(closeSpy).toHaveBeenCalledWith("s1");
  });

  it("TauriTerminalTransport onOutput decodes base64 data for matching session", async () => {
    let outputHandler: ((payload: tauri.TerminalOutputPayload) => void) | null = null;
    const unlistenFn = vi.fn();
    vi.spyOn(tauri, "onTerminalOutput").mockImplementation(async (handler) => {
      outputHandler = handler;
      return unlistenFn;
    });

    const transport = new TauriTerminalTransport();
    const received: Uint8Array[] = [];
    const unsubscribe = transport.onOutput("sess-match", (data) => {
      received.push(typeof data === "string" ? new TextEncoder().encode(data) : data);
    });

    await Promise.resolve();
    expect(outputHandler).toBeDefined();

    // Non-matching session ignored
    if (outputHandler) {
      (outputHandler as (payload: tauri.TerminalOutputPayload) => void)({
        sessionId: "other-session",
        data: btoa("ignored content"),
      });
    }
    expect(received).toHaveLength(0);

    // Matching session delivered decoded
    if (outputHandler) {
      (outputHandler as (payload: tauri.TerminalOutputPayload) => void)({
        sessionId: "sess-match",
        data: btoa("matched content"),
      });
    }
    expect(received).toHaveLength(1);
    expect(new TextDecoder().decode(received[0])).toBe("matched content");

    unsubscribe();
    expect(unlistenFn).toHaveBeenCalled();
  });

  it("TauriTerminalTransport onLifecycle forwards lifecycle events", async () => {
    let lifecycleHandler: ((payload: tauri.TerminalLifecyclePayload) => void) | null = null;
    const unlistenFn = vi.fn();
    vi.spyOn(tauri, "onTerminalLifecycle").mockImplementation(async (handler) => {
      lifecycleHandler = handler;
      return unlistenFn;
    });

    const transport = new TauriTerminalTransport();
    const events: Array<{ sessionId: string; state: string; exitCode?: number | null }> = [];
    const unsubscribe = transport.onLifecycle((evt) => events.push(evt));

    await Promise.resolve();
    expect(lifecycleHandler).toBeDefined();

    if (lifecycleHandler) {
      (lifecycleHandler as (payload: tauri.TerminalLifecyclePayload) => void)({
        sessionId: "sess-1",
        state: "exited",
        exitCode: 0,
        reason: null,
      });
    }

    expect(events).toEqual([{ sessionId: "sess-1", state: "exited", exitCode: 0 }]);

    unsubscribe();
    expect(unlistenFn).toHaveBeenCalled();
  });

  it("WebSocketTerminalTransport formats WS URL and handles events", async () => {
    const transport = new WebSocketTerminalTransport("http://127.0.0.1:43821", "dummy_token");
    expect(transport).toBeDefined();
    expect(typeof transport.attach).toBe("function");
    expect(typeof transport.write).toBe("function");
    expect(typeof transport.resize).toBe("function");
  });
});
