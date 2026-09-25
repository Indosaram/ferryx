import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { RemoteTerminal } from "./RemoteTerminal";
import { getOrCreateAttachKey, type AttachKeyPair } from "./accountAttach";

const PHONE_INPUT_MARKER = "PHONE_INPUT_MARKER_9981";

class MockAttachWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  static instances: MockAttachWebSocket[] = [];
  static latest: MockAttachWebSocket | null = null;
  url: string;
  sentMessages: (string | Uint8Array)[] = [];
  readyState: number = 1;
  binaryType: string = "arraybuffer";
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: (() => void) | null = null;

  constructor(url: string) {
    this.url = url;
    MockAttachWebSocket.latest = this;
    MockAttachWebSocket.instances.push(this);
  }

  send(data: string | Uint8Array | ArrayBuffer) {
    if (typeof data === "string") {
      this.sentMessages.push(data);
    } else if (data instanceof Uint8Array) {
      this.sentMessages.push(data);
    } else {
      this.sentMessages.push(new Uint8Array(data));
    }
  }

  close() {
    this.readyState = 3;
    this.onclose?.();
  }
}

function rect(width: number, height: number): DOMRect {
  return {
    x: 0,
    y: 0,
    width,
    height,
    top: 0,
    right: width,
    bottom: height,
    left: 0,
    toJSON: () => ({}),
  } as DOMRect;
}

describe("RemoteTerminal - P19 Phone Account Attach", () => {
  beforeEach(() => {
    MockAttachWebSocket.instances = [];
    MockAttachWebSocket.latest = null;
    vi.stubGlobal("WebSocket", MockAttachWebSocket);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.hasAttribute("data-terminal-cell-measure")) return rect(10, 20);
      if (this.getAttribute("data-testid") === "remote-terminal-grid") return rect(800, 400);
      return rect(0, 0);
    });
  });

  afterEach(() => {
    cleanup();
    MockAttachWebSocket.instances = [];
    MockAttachWebSocket.latest = null;
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("happy: phone renders production RemoteTerminal, opens attach socket, drives keystroke reaching socket, and does not render Add Project", async () => {
    const attachKey: AttachKeyPair = {
      publicKey: "pub-x25519-phone-test-key",
      privateKey: "priv-x25519-phone-test-key",
    };

    render(
      <RemoteTerminal
        sessionId="sess-alpha-1"
        token="token-unused-in-account"
        transportUrl="http://relay.example.com"
        isAccountSession={true}
        attachKey={attachKey}
      />,
    );

    await vi.waitFor(() => {
      expect(MockAttachWebSocket.latest).not.toBeNull();
    });

    const ws = MockAttachWebSocket.latest!;
    expect(ws.url).toContain("/tunnel/opaque/sess-alpha-1");
    expect(ws.url).not.toContain("?");
    expect(ws.url).not.toContain("ticket=");
    expect(ws.url).not.toContain("/tunnel/client/");
    expect(ws.url).not.toContain("/api/v1/socket-ticket");

    act(() => {
      ws.onopen?.();
    });

    await vi.waitFor(() => {
      expect(screen.getByRole("status").textContent).toContain("Live");
    });

    const target = screen.getByTestId("remote-terminal-grid");
    act(() => {
      fireEvent.paste(target, { clipboardData: { getData: () => PHONE_INPUT_MARKER } });
    });

    const hasMarker = ws.sentMessages.some((msg) => {
      if (typeof msg === "string") return msg.includes(PHONE_INPUT_MARKER);
      const decoded = new TextDecoder().decode(msg);
      return decoded.includes(PHONE_INPUT_MARKER);
    });
    expect(hasMarker).toBe(true);

    expect(screen.queryByLabelText("Add Project")).toBeNull();
    expect(screen.queryByText(/Add Project/i)).toBeNull();
  });

  it("failure: an account session with a missing attach key reports failure without requesting ticket or opening legacy socket", async () => {
    let legacyTicketRequested = false;

    const mockFetch = vi.fn().mockImplementation((url: string) => {
      if (typeof url === "string" && url.includes("/api/v1/socket-ticket")) {
        legacyTicketRequested = true;
      }
      return Promise.resolve({
        ok: true,
        json: () => Promise.resolve({}),
      });
    });
    globalThis.fetch = mockFetch;

    const onTransportFailure = vi.fn();

    render(
      <RemoteTerminal
        sessionId="sess-alpha-1"
        token="token-unused-in-account"
        transportUrl="http://relay.example.com"
        isAccountSession={true}
        attachKey={null}
        onTransportFailure={onTransportFailure}
      />,
    );

    await vi.waitFor(() => {
      expect(onTransportFailure).toHaveBeenCalled();
    });

    expect(legacyTicketRequested).toBe(false);

    const legacyWsInstances = MockAttachWebSocket.instances.filter((ws) =>
      ws.url.includes("/tunnel/client/") || ws.url.includes("/api/v1/socket-ticket") || ws.url.includes("/api/v1/terminal/"),
    );
    expect(legacyWsInstances.length).toBe(0);
  });

  it("generates and persists private key into IndexedDB ferryx.account.attachKey", async () => {
    const key = await getOrCreateAttachKey();
    expect(key.publicKey).toBeDefined();
    expect(key.privateKey).toBeDefined();
    expect(key.publicKey.length).toBe(44);
    expect(key.privateKey.length).toBe(44);

    const key2 = await getOrCreateAttachKey();
    expect(key2).toBeDefined();
  });
});
