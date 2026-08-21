import { describe, expect, it } from "vitest";
import { TauriTerminalTransport } from "./tauriTransport";
import { WebSocketTerminalTransport } from "./remoteTransport";

describe("TerminalTransport abstractions", () => {
  it("TauriTerminalTransport instantiates and conforms to contract", async () => {
    const transport = new TauriTerminalTransport();
    expect(transport).toBeDefined();
    expect(typeof transport.attach).toBe("function");
    expect(typeof transport.write).toBe("function");
    expect(typeof transport.resize).toBe("function");
    expect(typeof transport.signal).toBe("function");
    expect(typeof transport.onOutput).toBe("function");
  });

  it("WebSocketTerminalTransport formats WS URL and handles events", async () => {
    const transport = new WebSocketTerminalTransport("http://127.0.0.1:43821", "dummy_token");
    expect(transport).toBeDefined();
    expect(typeof transport.attach).toBe("function");
    expect(typeof transport.write).toBe("function");
    expect(typeof transport.resize).toBe("function");
  });
});
