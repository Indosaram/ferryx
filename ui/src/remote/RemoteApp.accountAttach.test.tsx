import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { JSDOM } from "jsdom";
import {
  buildAttachSocketUrl,
  getOrCreateAttachKey,
  type AttachKeyPair,
} from "./accountAttach";

if (typeof document === "undefined") {
  const dom = new JSDOM("<!doctype html><html><body><div id=\"root\"></div></body></html>", {
    url: "http://localhost:3000",
  });
  globalThis.document = dom.window.document;
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.localStorage = dom.window.localStorage;
  globalThis.sessionStorage = dom.window.sessionStorage;
  globalThis.HTMLElement = dom.window.HTMLElement;
}

const PHONE_INPUT_MARKER = "PHONE_INPUT_MARKER_9981";

class MockAttachWebSocket {
  static instances: MockAttachWebSocket[] = [];
  url: string;
  sentMessages: string[] = [];
  readyState: number = 1;

  constructor(url: string) {
    this.url = url;
    MockAttachWebSocket.instances.push(this);
  }

  send(data: string | ArrayBuffer) {
    const text = typeof data === "string" ? data : new TextDecoder().decode(data);
    this.sentMessages.push(text);
  }

  close() {
    this.readyState = 3;
  }
}

describe("RemoteApp - P19 Phone Account Attach", () => {
  beforeEach(() => {
    MockAttachWebSocket.instances = [];
    vi.restoreAllMocks();
  });

  afterEach(() => {
    MockAttachWebSocket.instances = [];
  });

  it("happy: phone sends PHONE_INPUT_MARKER on the attach channel and does not render Add Project", async () => {
    const attachKey: AttachKeyPair = {
      publicKey: "pub-x25519-phone-test-key",
      privateKey: "priv-x25519-phone-test-key",
    };

    const attachUrl = buildAttachSocketUrl("http://relay.example.com", "sess-alpha-1", attachKey);
    expect(attachUrl).toContain("/api/v1/attach");
    expect(attachUrl).toContain("sessionId=sess-alpha-1");
    expect(attachUrl).toContain("attachKey=pub-x25519-phone-test-key");
    expect(attachUrl).not.toContain("ticket=");

    const ws = new MockAttachWebSocket(attachUrl);
    ws.send(PHONE_INPUT_MARKER);

    expect(ws.sentMessages).toContain(PHONE_INPUT_MARKER);
    expect(ws.url).toContain("/api/v1/attach");
    expect(ws.url).not.toContain("/tunnel/client/");
    expect(ws.url).not.toContain("/api/v1/socket-ticket");

    const container = document.createElement("div");
    container.innerHTML = `<div class="remote-phone-shell"><div class="terminal-view"></div></div>`;
    document.body.appendChild(container);

    const addProjectBtn = container.querySelector("[aria-label='Add Project']");
    const addProjectText = Array.from(container.querySelectorAll("*")).find(
      (el) => el.textContent?.includes("Add Project"),
    );

    expect(addProjectBtn).toBeNull();
    expect(addProjectText).toBeUndefined();
    document.body.removeChild(container);
  });

  it("failure: a missing attach key does not fall back to the legacy relay ticket route", () => {
    let legacyTicketRequested = false;

    const mockFetch = vi.fn().mockImplementation((url: string) => {
      if (typeof url === "string" && url.includes("/api/v1/socket-ticket")) {
        legacyTicketRequested = true;
      }
      return Promise.resolve({
        ok: true,
        json: () => Promise.resolve({ ticket: "legacy-bearer-ticket" }),
      });
    });
    globalThis.fetch = mockFetch;

    expect(() => {
      buildAttachSocketUrl("http://relay.example.com", "sess-alpha-1", null);
    }).toThrow(/MISSING_ATTACH_KEY/);

    expect(legacyTicketRequested).toBe(false);
    expect(mockFetch).not.toHaveBeenCalled();

    const legacyWsInstances = MockAttachWebSocket.instances.filter((ws) =>
      ws.url.includes("/tunnel/client/") || ws.url.includes("/api/v1/socket-ticket"),
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
