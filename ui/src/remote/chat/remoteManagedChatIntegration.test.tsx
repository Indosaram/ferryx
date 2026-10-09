import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { RemoteApp } from "../RemoteApp";
import { computeSha256Hex } from "./remoteAttachmentAdapter";

const initialWorkspace = {
  activeContext: {
    workspaceId: "ferryx",
    worktreeSlug: "main",
    worktreeLabel: "main",
    activeTabId: "tab-1",
    activeTerminal: { sessionId: "sess-1", title: "Shell 1", running: true },
    terminalTabs: [
      { id: "tab-1", sessionId: "sess-1", label: "Shell 1", agentType: "shell", activityState: "idle", worktreeLabel: "main" },
      { id: "tab-2", sessionId: "sess-2", label: "Shell 2", agentType: "shell", activityState: "waiting", worktreeLabel: "main" },
    ],
  },
};

function jsonResponse(data: unknown, status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: vi.fn(async () => data),
    text: vi.fn(async () => JSON.stringify(data)),
  } as unknown as Response;
}

function createMockFile(name: string, content: string | Uint8Array, type = ""): File {
  const sourceBytes = typeof content === "string" ? new TextEncoder().encode(content) : content;
  const copy = new Uint8Array(sourceBytes.byteLength);
  copy.set(sourceBytes);

  const file = new File([copy], name, { type });
  Object.defineProperty(file, "size", { value: copy.byteLength, writable: false });
  Object.defineProperty(file, "name", { value: name, writable: false });
  Object.defineProperty(file, "type", { value: type, writable: false });

  Object.defineProperty(file, "arrayBuffer", {
    value: async () => copy.buffer.slice(copy.byteOffset, copy.byteOffset + copy.byteLength),
    writable: true,
    configurable: true,
  });

  Object.defineProperty(file, "text", {
    value: async () => new TextDecoder().decode(copy),
    writable: true,
    configurable: true,
  });

  Object.defineProperty(file, "slice", {
    value: (start?: number, end?: number, contentType?: string) => {
      const len = copy.byteLength;
      const s = start !== undefined ? (start < 0 ? Math.max(len + start, 0) : Math.min(start, len)) : 0;
      const e = end !== undefined ? (end < 0 ? Math.max(len + end, 0) : Math.min(end, len)) : len;
      const sliced = copy.subarray(s, Math.max(s, e));
      return createMockFile(name, sliced, contentType ?? type);
    },
    writable: true,
    configurable: true,
  });

  return file;
}

beforeEach(() => {
  localStorage.setItem("ferryx_remote_token", "test-token");
  localStorage.setItem("ferryx_device_id_local:http://localhost:8899", "device-1");
  localStorage.setItem(`ferryx_remote_token_local:${window.location.origin}`, "test-token");
  localStorage.setItem(`ferryx_device_id_local:${window.location.origin}`, "device-1");
  localStorage.setItem("ferryx_remote_token_localhost:8899", "test-token");
});

afterEach(() => {
  cleanup();
  localStorage.clear();
  vi.unstubAllGlobals();
});

describe("remote managed chat integration", () => {
  it("routes composer send through ManagedChat service with zero PTY fallback on success or failure", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });

    let sentChatPayloads: any[] = [];
    let ptySocketSends: string[] = [];

    class FakeWebSocket {
      url: string;
      readyState = 1;
      onopen: any = null;
      onmessage: any = null;
      onerror: any = null;
      onclose: any = null;

      constructor(url: string) {
        this.url = url;
        setTimeout(() => this.onopen?.(), 0);
      }
      send(data: string) {
        ptySocketSends.push(data);
      }
      close() {}
    }

    vi.stubGlobal("WebSocket", FakeWebSocket as any);

    let failNextSend = false;
    const requestMock = vi.fn<typeof fetch>(async (input, init) => {
      const url = String(input instanceof Request ? input.url : input);

      if (url.includes("/api/v1/workspace")) {
        return jsonResponse(initialWorkspace);
      }
      if (url.includes("/api/v1/sessions")) {
        return jsonResponse({ sessions: [{ sessionId: "sess-1", daemonEpoch: "41" }, { sessionId: "sess-2", daemonEpoch: "41" }] });
      }
      if (url.includes("/api/v1/capabilities")) {
        return jsonResponse({ daemonEpoch: "41" });
      }
      if (url.includes("/api/v1/agent-history/")) {
        return jsonResponse({
          sessionId: "sess-1",
          items: [{ ordinal: 0, role: "assistant", text: "Ready for tasks." }],
          nextCursor: null,
          partial: false,
          warnings: [],
        });
      }
      if (url.includes("/api/v1/chat/callbacks")) {
        return jsonResponse({ ok: true, data: [], requestId: "chat-callbacks-query" });
      }
      if (url.includes("/api/v1/chat/send")) {
        const body = JSON.parse(String(init?.body));
        sentChatPayloads.push(body);

        if (failNextSend) {
          return jsonResponse(
            {
              ok: false,
              error: { code: "SERVICE_UNAVAILABLE", message: "Gateway busy", retryable: true },
              requestId: body.requestId,
            },
            503
          );
        }

        return jsonResponse({
          ok: true,
          data: {
            requestId: body.requestId,
            target: body.target,
            stage: "accepted",
          },
          requestId: body.requestId,
        });
      }
      if (url.includes("/api/v1/socket-ticket")) {
        return jsonResponse({ ticket: "mock-ticket-xyz" });
      }
      return jsonResponse({});
    });

    vi.stubGlobal("fetch", requestMock);

    render(<RemoteApp />);

    const chatViewButton = await screen.findByTestId("remote-view-mode-chat", {}, { timeout: 10000 });
    fireEvent.click(chatViewButton);

    const composerTextarea = await screen.findByTestId("chat-composer-textarea", {}, { timeout: 10000 });
    expect(composerTextarea).toBeInTheDocument();

    const sendButton = await screen.findByTestId("send-button", {}, { timeout: 10000 });
    expect(sendButton).toBeDisabled();

    fireEvent.change(composerTextarea, { target: { value: "Run integration tests" } });
    expect(sendButton).not.toBeDisabled();

    fireEvent.click(sendButton);

    await waitFor(() => {
      expect(sentChatPayloads.length).toBe(1);
    }, { timeout: 10000 });

    expect(sentChatPayloads[0].target.backendSessionId).toBe("sess-1");
    expect(sentChatPayloads[0].draft.text).toBe("Run integration tests");
    expect(sentChatPayloads[0].draft.attachments).toEqual([]);
    expect(ptySocketSends).toHaveLength(0);

    const stopButton = await screen.findByTestId("stop-button", {}, { timeout: 10000 });
    fireEvent.click(stopButton);

    failNextSend = true;
    fireEvent.change(composerTextarea, { target: { value: "Second prompt that fails" } });

    const activeSendButton = await screen.findByTestId("send-button", {}, { timeout: 10000 });
    fireEvent.click(activeSendButton);

    await waitFor(() => {
      expect(sentChatPayloads.length).toBe(2);
    }, { timeout: 10000 });

    expect(sentChatPayloads[1].draft.text).toBe("Second prompt that fails");
    expect(ptySocketSends).toHaveLength(0);

    expect(
      await screen.findByText(/Message delivery failed: Gateway busy/, {}, { timeout: 10000 })
    ).toBeInTheDocument();

    expect(ptySocketSends).toHaveLength(0);
  }, 20000);

  it("keeps the sent snapshot held through staged receipts and retries only on explicit action", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const sends: Array<{ requestId: string; target: any; draft: any }> = [];
    let resolveFirst!: (response: Response) => void;
    const firstReceipt = new Promise<Response>((resolve) => { resolveFirst = resolve; });
    const requestMock = vi.fn<typeof fetch>(async (input, init) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/workspace")) return jsonResponse(initialWorkspace);
      if (url.includes("/api/v1/agent-history/")) return jsonResponse({ sessionId: "sess-1", items: [], nextCursor: null, partial: false, warnings: [] });
      if (url.includes("/api/v1/chat/callbacks")) return jsonResponse({ ok: true, data: [], requestId: "callbacks" });
      if (url.includes("/api/v1/capabilities")) return jsonResponse({ daemonEpoch: "41" });
      if (url.includes("/api/v1/sessions")) return jsonResponse({ sessions: [{ sessionId: "sess-1", daemonEpoch: "41" }] });
      if (url.includes("/api/v1/chat/send")) {
        const body = JSON.parse(String(init?.body));
        sends.push(body);
        if (sends.length === 1) return firstReceipt;
        return jsonResponse({ ok: true, data: { requestId: body.requestId, target: body.target, stage: "accepted" }, requestId: body.requestId });
      }
      if (url.includes("/api/v1/socket-ticket")) return jsonResponse({ ticket: "ticket" });
      if (url.includes("/api/v1/capabilities")) return jsonResponse({ daemonEpoch: "41" });
      if (url.includes("/api/v1/sessions")) return jsonResponse({ sessions: [{ sessionId: "sess-1", daemonEpoch: "41" }] });
      return jsonResponse({});
    });
    vi.stubGlobal("fetch", requestMock);
    render(<RemoteApp />);
    fireEvent.click(await screen.findByTestId("remote-view-mode-chat", {}, { timeout: 10000 }));
    await screen.findByTestId("chat-mode", {}, { timeout: 10000 });
    const textarea = await screen.findByTestId("chat-composer-textarea", {}, { timeout: 10000 });
    await waitFor(() => expect(screen.getByTestId("send-button")).toBeDisabled(), { timeout: 10000 });
    fireEvent.change(textarea, { target: { value: "snapshot one" } });
    fireEvent.click(screen.getByTestId("send-button"));
    await waitFor(() => expect(sends).toHaveLength(1), { timeout: 10000 });
    fireEvent.change(textarea, { target: { value: "new edit" } });
    resolveFirst(jsonResponse({ ok: true, data: { requestId: sends[0].requestId, target: sends[0].target, stage: "staged" }, requestId: sends[0].requestId }));
    expect(await screen.findByTestId("mobile-chat-held", {}, { timeout: 10000 })).toBeInTheDocument();
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("new edit");
    expect(sends).toHaveLength(1);
    fireEvent.click(screen.getByTestId("mobile-chat-retry"));
    await waitFor(() => expect(sends).toHaveLength(2), { timeout: 10000 });
    expect(sends[1].requestId).toBe(sends[0].requestId);
    expect(sends[1].draft).toEqual(sends[0].draft);
    await waitFor(() => expect(screen.queryByTestId("mobile-chat-held")).not.toBeInTheDocument(), { timeout: 10000 });
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("new edit");
  }, 20000);

  it("does not let an old target receipt clear the successor target pending state", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const sends: any[] = [];
    const resolvers = new Map<string, (response: Response) => void>();
    const pending = new Map<string, Promise<Response>>();
    const requestMock = vi.fn<typeof fetch>(async (input, init) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/workspace")) return jsonResponse(initialWorkspace);
      if (url.includes("/api/v1/sessions")) return jsonResponse({ sessions: [{ sessionId: "sess-1", daemonEpoch: "41" }, { sessionId: "sess-2", daemonEpoch: "41" }] });
      if (url.includes("/api/v1/capabilities")) return jsonResponse({ daemonEpoch: "41" });
      if (url.includes("/api/v1/agent-history/")) return jsonResponse({ sessionId: "sess-1", items: [], nextCursor: null, partial: false, warnings: [] });
      if (url.includes("/api/v1/chat/callbacks")) return jsonResponse({ ok: true, data: [], requestId: "callbacks" });
      if (url.includes("/api/v1/capabilities")) return jsonResponse({ daemonEpoch: "41" });
      if (url.includes("/api/v1/chat/send")) {
        const body = JSON.parse(String(init?.body));
        sends.push(body);
        const response = new Promise<Response>((resolve) => resolvers.set(body.target.backendSessionId, resolve));
        pending.set(body.target.backendSessionId, response);
        return response;
      }
      if (url.includes("/api/v1/socket-ticket")) return jsonResponse({ ticket: "ticket" });
      return jsonResponse({});
    });
    vi.stubGlobal("fetch", requestMock);
    render(<RemoteApp />);
    fireEvent.click(await screen.findByTestId("remote-view-mode-chat"));
    await screen.findByTestId("chat-mode", {}, { timeout: 10000 });
    const oldTextarea = await screen.findByTestId("chat-composer-textarea", {}, { timeout: 10000 });
    fireEvent.change(oldTextarea, { target: { value: "old target request" } });
    fireEvent.click(await screen.findByTestId("send-button", {}, { timeout: 10000 }));
    await waitFor(() => expect(sends).toHaveLength(1), { timeout: 10000 });
    fireEvent.click(await screen.findByTestId("stop-button", {}, { timeout: 10000 }));

    fireEvent.click(screen.getByRole("button", { name: /change workspace context/i }));
    const tabs = await screen.findByRole("tablist", { name: /terminal tabs/i });
    fireEvent.click(within(tabs).getByRole("tab", { name: /shell 2/i }));
    const newTextarea = await screen.findByTestId("chat-composer-textarea", {}, { timeout: 10000 });
    await waitFor(() => expect(screen.getByTestId("agent-start")).toBeEnabled(), { timeout: 10000 });
    fireEvent.change(newTextarea, { target: { value: "new target request" } });
    fireEvent.click(await screen.findByTestId("send-button", {}, { timeout: 10000 }));
    await waitFor(() => expect(sends).toHaveLength(2), { timeout: 10000 });
    expect(sends[1].target.backendSessionId).toBe("sess-2");
    await waitFor(() => expect(screen.getByTestId("stop-button")).toBeInTheDocument(), { timeout: 10000 });

    await act(async () => {
      resolvers.get("sess-1")?.(jsonResponse({ ok: true, data: { requestId: sends[0].requestId, target: sends[0].target, stage: "accepted" }, requestId: sends[0].requestId }));
      await pending.get("sess-1");
    });
    expect(screen.getByTestId("stop-button")).toBeInTheDocument();

    await act(async () => {
      resolvers.get("sess-2")?.(jsonResponse({ ok: true, data: { requestId: sends[1].requestId, target: sends[1].target, stage: "accepted" }, requestId: sends[1].requestId }));
      await pending.get("sess-2");
    });
    await waitFor(() => expect(screen.getByTestId("stop-button")).toBeInTheDocument());
  }, 20000);

  it("retains a draft when the service returns a mismatched request receipt", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    let sent: any;
    const requestMock = vi.fn<typeof fetch>(async (input, init) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/workspace")) return jsonResponse(initialWorkspace);
      if (url.includes("/api/v1/sessions")) return jsonResponse({ sessions: [{ sessionId: "sess-1", daemonEpoch: "41" }] });
      if (url.includes("/api/v1/capabilities")) return jsonResponse({ daemonEpoch: "41" });
      if (url.includes("/api/v1/agent-history/")) return jsonResponse({ sessionId: "sess-1", items: [], nextCursor: null, partial: false, warnings: [] });
      if (url.includes("/api/v1/chat/callbacks")) return jsonResponse({ ok: true, data: [], requestId: "callbacks" });
      if (url.includes("/api/v1/chat/send")) {
        sent = JSON.parse(String(init?.body));
        return jsonResponse({ ok: true, data: { requestId: "different-request", target: sent.target, stage: "accepted" }, requestId: sent.requestId });
      }
      if (url.includes("/api/v1/socket-ticket")) return jsonResponse({ ticket: "ticket" });
      return jsonResponse({});
    });
    vi.stubGlobal("fetch", requestMock);
    render(<RemoteApp />);
    fireEvent.click(await screen.findByTestId("remote-view-mode-chat"));
    await screen.findByTestId("chat-mode", {}, { timeout: 10000 });
    const textarea = await screen.findByTestId("chat-composer-textarea", {}, { timeout: 10000 });
    fireEvent.change(textarea, { target: { value: "retain mismatched" } });
    fireEvent.click(screen.getByTestId("send-button"));
    expect(await screen.findByTestId("mobile-chat-held")).toBeInTheDocument();
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("retain mismatched");
    expect(sent.draft.text).toBe("retain mismatched");
  });

  it("keeps a mismatched held retry recoverable without changing its frozen request", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const sends: any[] = [];
    let retryReceipt!: (response: Response) => void;
    const retryResponse = new Promise<Response>((resolve) => { retryReceipt = resolve; });
    const requestMock = vi.fn<typeof fetch>(async (input, init) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/workspace")) return jsonResponse(initialWorkspace);
      if (url.includes("/api/v1/sessions")) return jsonResponse({ sessions: [{ sessionId: "sess-1", daemonEpoch: "41" }] });
      if (url.includes("/api/v1/capabilities")) return jsonResponse({ daemonEpoch: "41" });
      if (url.includes("/api/v1/agent-history/")) return jsonResponse({ sessionId: "sess-1", items: [], nextCursor: null, partial: false, warnings: [] });
      if (url.includes("/api/v1/chat/callbacks")) return jsonResponse({ ok: true, data: [], requestId: "callbacks" });
      if (url.includes("/api/v1/chat/send")) {
        const body = JSON.parse(String(init?.body));
        sends.push(body);
        if (sends.length === 1) return jsonResponse({ ok: true, data: { requestId: body.requestId, target: body.target, stage: "staged" }, requestId: body.requestId });
        return retryResponse;
      }
      if (url.includes("/api/v1/socket-ticket")) return jsonResponse({ ticket: "ticket" });
      return jsonResponse({});
    });
    vi.stubGlobal("fetch", requestMock);
    render(<RemoteApp />);
    fireEvent.click(await screen.findByTestId("remote-view-mode-chat"));
    const textarea = await screen.findByTestId("chat-composer-textarea", {}, { timeout: 10000 });
    fireEvent.change(textarea, { target: { value: "held payload" } });
    fireEvent.click(screen.getByTestId("send-button"));
    expect(await screen.findByTestId("mobile-chat-held")).toBeInTheDocument();
    const heldRequest = sends[0];
    fireEvent.change(textarea, { target: { value: "newer draft" } });
    fireEvent.click(screen.getByTestId("mobile-chat-retry"));
    await waitFor(() => expect(sends).toHaveLength(2));
    expect(sends[1].requestId).toBe(heldRequest.requestId);
    expect(sends[1].draft).toEqual(heldRequest.draft);
    await act(async () => {
      retryReceipt(jsonResponse({ ok: true, data: { requestId: "wrong-request", target: heldRequest.target, stage: "accepted" }, requestId: heldRequest.requestId }));
      await retryResponse;
    });
    expect(screen.getByTestId("mobile-chat-held")).toBeInTheDocument();
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("newer draft");
    expect(await screen.findByText(/receipt did not match this held request/i)).toBeInTheDocument();
    expect(sends).toHaveLength(2);
    expect(sends[0].requestId).toBe(sends[1].requestId);
  });

  it("uses distinct request ids for separate accepted sends with zero PTY writes", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const sends: any[] = [];
    const ptyWrites: string[] = [];
    class FakeWebSocket {
      url: string; readyState = 1; onopen: any = null; onmessage: any = null; onerror: any = null; onclose: any = null;
      constructor(url: string) { this.url = url; }
      send(data: string) { ptyWrites.push(data); }
      close() {}
    }
    vi.stubGlobal("WebSocket", FakeWebSocket as any);
    const requestMock = vi.fn<typeof fetch>(async (input, init) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/workspace")) return jsonResponse(initialWorkspace);
      if (url.includes("/api/v1/sessions")) return jsonResponse({ sessions: [{ sessionId: "sess-1", daemonEpoch: "41" }] });
      if (url.includes("/api/v1/capabilities")) return jsonResponse({ daemonEpoch: "41" });
      if (url.includes("/api/v1/agent-history/")) return jsonResponse({ sessionId: "sess-1", items: [], nextCursor: null, partial: false, warnings: [] });
      if (url.includes("/api/v1/chat/callbacks")) return jsonResponse({ ok: true, data: [], requestId: "callbacks" });
      if (url.includes("/api/v1/chat/send")) {
        const body = JSON.parse(String(init?.body));
        sends.push(body);
        return jsonResponse({ ok: true, data: { requestId: body.requestId, target: body.target, stage: "accepted" }, requestId: body.requestId });
      }
      if (url.includes("/api/v1/socket-ticket")) return jsonResponse({ ticket: "ticket" });
      return jsonResponse({});
    });
    vi.stubGlobal("fetch", requestMock);
    render(<RemoteApp />);
    fireEvent.click(await screen.findByTestId("remote-view-mode-chat"));
    await screen.findByTestId("chat-mode", {}, { timeout: 10000 });
    let textarea = await screen.findByTestId("chat-composer-textarea", {}, { timeout: 10000 });
    fireEvent.change(textarea, { target: { value: "first independent send" } });
    fireEvent.click(await screen.findByTestId("send-button", {}, { timeout: 10000 }));
    await waitFor(() => expect(sends).toHaveLength(1));
    await screen.findByTestId("mobile-chat-delivery-stage", {}, { timeout: 10000 });
    await waitFor(() => expect(screen.getByTestId("chat-composer-textarea")).toHaveValue(""));
    fireEvent.click(await screen.findByTestId("stop-button", {}, { timeout: 10000 }));
    textarea = await screen.findByTestId("chat-composer-textarea", {}, { timeout: 10000 });
    fireEvent.change(textarea, { target: { value: "second independent send" } });
    fireEvent.click(await screen.findByTestId("send-button", {}, { timeout: 10000 }));
    await waitFor(() => expect(sends).toHaveLength(2));
    await screen.findByTestId("mobile-chat-delivery-stage", {}, { timeout: 10000 });
    expect(sends[0].requestId).not.toBe(sends[1].requestId);
    expect(ptyWrites).toEqual([]);
  });

  it("retains newly staged attachment edits while the send snapshot is pending", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    let resolveSend!: (response: Response) => void;
    let sent: any;
    const pendingSend = new Promise<Response>((resolve) => { resolveSend = resolve; });
    const requestMock = vi.fn<typeof fetch>(async (input, init) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/workspace")) return jsonResponse(initialWorkspace);
      if (url.includes("/api/v1/sessions")) return jsonResponse({ sessions: [{ sessionId: "sess-1", daemonEpoch: "41" }] });
      if (url.includes("/api/v1/capabilities")) return jsonResponse({ daemonEpoch: "41" });
      if (url.includes("/api/v1/agent-history/")) return jsonResponse({ sessionId: "sess-1", items: [], nextCursor: null, partial: false, warnings: [] });
      if (url.includes("/api/v1/chat/callbacks")) return jsonResponse({ ok: true, data: [], requestId: "callbacks" });
      if (url.includes("/api/v1/chat/attachments/upload")) {
        const data = JSON.parse(String(init?.body));
        const bytes = Uint8Array.from(atob(data.data), (char) => char.charCodeAt(0));
        return jsonResponse({ ok: true, data: { hostId: data.target.hostId, attachmentId: data.attachmentId, sizeBytes: data.totalBytes, sha256: await computeSha256Hex(bytes.buffer), mediaType: data.mediaType }, requestId: "upload" });
      }
      if (url.includes("/api/v1/chat/send")) {
        sent = JSON.parse(String(init?.body));
        return pendingSend;
      }
      if (url.includes("/api/v1/socket-ticket")) return jsonResponse({ ticket: "ticket" });
      return jsonResponse({});
    });
    vi.stubGlobal("fetch", requestMock);
    render(<RemoteApp />);
    fireEvent.click(await screen.findByTestId("remote-view-mode-chat"));
    await screen.findByTestId("chat-mode", {}, { timeout: 10000 });
    const fileInput = await screen.findByTestId("file-upload-input");
    fireEvent.change(fileInput, { target: { files: [createMockFile("first.txt", "first", "text/plain")] } });
    await waitFor(() => expect(screen.getByTestId("send-button")).not.toBeDisabled(), { timeout: 10000 });
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "send first attachment" } });
    fireEvent.click(screen.getByTestId("send-button"));
    await waitFor(() => expect(sent).toBeDefined());
    expect(sent.draft.attachments).toHaveLength(1);

    const secondFile = createMockFile("second.txt", "second", "text/plain");
    const uploadRequestIndexes = requestMock.mock.calls.length;
    fireEvent.change(fileInput, { target: { files: [secondFile] } });
    await waitFor(() => {
      const uploads = requestMock.mock.calls.slice(uploadRequestIndexes).filter(([input]) => String(input instanceof Request ? input.url : input).includes("/api/v1/chat/attachments/upload"));
      expect(uploads).toHaveLength(1);
    });
    await waitFor(() => {
      const previews = screen.getByTestId("chat-composer-attachments").querySelectorAll("[data-testid^='attachment-preview-']");
      expect(previews).toHaveLength(2);
      expect(previews[1].getAttribute("data-testid")).not.toBe(previews[0].getAttribute("data-testid"));
      expect(screen.getByTestId("chat-composer-attachments")).toHaveTextContent("second.txt");
    });
    const secondPreview = screen.getByTestId("chat-composer-attachments").querySelector("[data-testid^='attachment-preview-']:last-child");
    expect(secondPreview).toBeInTheDocument();
    const secondAttachmentId = secondPreview?.getAttribute("data-testid")?.replace("attachment-preview-", "");
    expect(secondAttachmentId).toBeTruthy();
    await act(async () => {
      resolveSend(jsonResponse({ ok: true, data: { requestId: sent.requestId, target: sent.target, stage: "accepted" }, requestId: sent.requestId }));
      await pendingSend;
    });
    await waitFor(() => expect(screen.getByTestId("chat-composer-attachments").querySelectorAll("[data-testid^='attachment-preview-']")).toHaveLength(1));
    expect(screen.getByTestId("chat-composer-attachments")).toHaveTextContent("second.txt");
    expect(screen.getByTestId("chat-composer-attachments").querySelector(`[data-testid="attachment-preview-${secondAttachmentId}"]`)).toBeInTheDocument();
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("");
  });

  it("settles a matching providerRead receipt directly", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    let sent: any;
    const requestMock = vi.fn<typeof fetch>(async (input, init) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/workspace")) return jsonResponse(initialWorkspace);
      if (url.includes("/api/v1/sessions")) return jsonResponse({ sessions: [{ sessionId: "sess-1", daemonEpoch: "41" }] });
      if (url.includes("/api/v1/capabilities")) return jsonResponse({ daemonEpoch: "41" });
      if (url.includes("/api/v1/agent-history/")) return jsonResponse({ sessionId: "sess-1", items: [], nextCursor: null, partial: false, warnings: [] });
      if (url.includes("/api/v1/chat/callbacks")) return jsonResponse({ ok: true, data: [], requestId: "callbacks" });
      if (url.includes("/api/v1/chat/send")) {
        sent = JSON.parse(String(init?.body));
        return jsonResponse({ ok: true, data: { requestId: sent.requestId, target: sent.target, stage: "providerRead" }, requestId: sent.requestId });
      }
      if (url.includes("/api/v1/socket-ticket")) return jsonResponse({ ticket: "ticket" });
      return jsonResponse({});
    });
    vi.stubGlobal("fetch", requestMock);
    render(<RemoteApp />);
    fireEvent.click(await screen.findByTestId("remote-view-mode-chat"));
    const textarea = await screen.findByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "provider read directly" } });
    fireEvent.click(screen.getByTestId("send-button"));
    await waitFor(() => expect(sent).toBeDefined());
    expect(await screen.findByTestId("mobile-chat-delivery-stage")).toHaveTextContent("Provider read");
    await waitFor(() => expect(screen.getByTestId("chat-composer-textarea")).toHaveValue(""));
    expect(screen.queryByTestId("mobile-chat-held")).not.toBeInTheDocument();
  });

  it("keeps staged, accepted, and providerRead receipt settlement distinct without PTY writes", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const sends: any[] = [];
    const ptyWrites: string[] = [];
    class FakeWebSocket {
      url: string; readyState = 1; onopen: any = null; onmessage: any = null; onerror: any = null; onclose: any = null;
      constructor(url: string) { this.url = url; }
      send(data: string) { ptyWrites.push(data); }
      close() {}
    }
    vi.stubGlobal("WebSocket", FakeWebSocket as any);
    const stages = ["staged", "accepted", "providerRead"] as const;
    const requestMock = vi.fn<typeof fetch>(async (input, init) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/workspace")) return jsonResponse(initialWorkspace);
      if (url.includes("/api/v1/sessions")) return jsonResponse({ sessions: [{ sessionId: "sess-1", daemonEpoch: "41" }] });
      if (url.includes("/api/v1/capabilities")) return jsonResponse({ daemonEpoch: "41" });
      if (url.includes("/api/v1/agent-history/")) return jsonResponse({ sessionId: "sess-1", items: [], nextCursor: null, partial: false, warnings: [] });
      if (url.includes("/api/v1/chat/callbacks")) return jsonResponse({ ok: true, data: [], requestId: "callbacks" });
      if (url.includes("/api/v1/chat/send")) {
        const body = JSON.parse(String(init?.body));
        sends.push(body);
        const stage = stages[sends.length === 1 ? 0 : sends.length === 2 ? 1 : 2];
        return jsonResponse({ ok: true, data: { requestId: body.requestId, target: body.target, stage }, requestId: body.requestId });
      }
      if (url.includes("/api/v1/socket-ticket")) return jsonResponse({ ticket: "ticket" });
      return jsonResponse({});
    });
    vi.stubGlobal("fetch", requestMock);
    render(<RemoteApp />);
    fireEvent.click(await screen.findByTestId("remote-view-mode-chat"));
    await screen.findByTestId("chat-mode", {}, { timeout: 10000 });
    let textarea = await screen.findByTestId("chat-composer-textarea", {}, { timeout: 10000 });
    fireEvent.change(textarea, { target: { value: "stage contract staged" } });
    fireEvent.click(screen.getByTestId("send-button"));
    await waitFor(() => expect(sends).toHaveLength(1));
    expect(await screen.findByTestId("mobile-chat-held")).toBeInTheDocument();
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("stage contract staged");
    fireEvent.click(screen.getByTestId("mobile-chat-retry"));
    await waitFor(() => expect(sends).toHaveLength(2));
    expect(sends[1].requestId).toBe(sends[0].requestId);
    await waitFor(() => expect(screen.queryByTestId("mobile-chat-held")).not.toBeInTheDocument());
    await waitFor(() => expect(screen.getByTestId("chat-composer-textarea")).toHaveValue(""));
    fireEvent.click(await screen.findByTestId("stop-button", {}, { timeout: 10000 }));
    textarea = await screen.findByTestId("chat-composer-textarea", {}, { timeout: 10000 });
    fireEvent.change(textarea, { target: { value: "stage contract providerRead" } });
    fireEvent.click(screen.getByTestId("send-button"));
    await waitFor(() => expect(sends).toHaveLength(3));
    await waitFor(() => expect(screen.getByTestId("chat-composer-textarea")).toHaveValue(""));
    expect(screen.getByTestId("mobile-chat-delivery-stage")).toHaveTextContent("Provider read");
    expect(ptyWrites).toEqual([]);
  });

  it("holds transport failures and never replays a pending request into a new target", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const sends: any[] = [];
    const resolvers = new Map<string, (response: Response) => void>();
    const pending = new Map<string, Promise<Response>>();
    const ptyWrites: string[] = [];
    class FakeWebSocket {
      url: string; readyState = 1; onopen: any = null; onmessage: any = null; onerror: any = null; onclose: any = null;
      constructor(url: string) { this.url = url; }
      send(data: string) { ptyWrites.push(data); }
      close() {}
    }
    vi.stubGlobal("WebSocket", FakeWebSocket as any);
    let rejectFirst = true;
    const requestMock = vi.fn<typeof fetch>(async (input, init) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/workspace")) return jsonResponse(initialWorkspace);
      if (url.includes("/api/v1/sessions")) return jsonResponse({ sessions: [{ sessionId: "sess-1", daemonEpoch: "41" }, { sessionId: "sess-2", daemonEpoch: "41" }] });
      if (url.includes("/api/v1/capabilities")) return jsonResponse({ daemonEpoch: "41" });
      if (url.includes("/api/v1/agent-history/")) return jsonResponse({ sessionId: "sess-1", items: [], nextCursor: null, partial: false, warnings: [] });
      if (url.includes("/api/v1/chat/callbacks")) return jsonResponse({ ok: true, data: [], requestId: "callbacks" });
      if (url.includes("/api/v1/chat/send")) {
        const body = JSON.parse(String(init?.body));
        sends.push(body);
        if (rejectFirst) { rejectFirst = false; throw new Error("connection lost"); }
        const response = new Promise<Response>((resolve) => resolvers.set(body.target.backendSessionId, resolve));
        pending.set(body.target.backendSessionId, response);
        return response;
      }
      if (url.includes("/api/v1/socket-ticket")) return jsonResponse({ ticket: "ticket" });
      return jsonResponse({});
    });
    vi.stubGlobal("fetch", requestMock);
    render(<RemoteApp />);
    fireEvent.click(await screen.findByTestId("remote-view-mode-chat"));
    const textarea = await screen.findByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "failed draft" } });
    fireEvent.click(screen.getByTestId("send-button"));
    expect(await screen.findByTestId("mobile-chat-held")).toBeInTheDocument();
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("failed draft");
    expect(await screen.findByText(/Message delivery failed:.*connection lost/i)).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("mobile-chat-retry"));
    await waitFor(() => expect(sends).toHaveLength(2));
    expect(sends[1].requestId).toBe(sends[0].requestId);
    expect(sends[1].target.backendSessionId).toBe("sess-1");
    fireEvent.click(screen.getByRole("button", { name: /change workspace context/i }));
    const tabs = await screen.findByRole("tablist", { name: /terminal tabs/i });
    fireEvent.click(within(tabs).getByRole("tab", { name: /shell 2/i }));
    const newTextarea = await screen.findByTestId("chat-composer-textarea");
    fireEvent.change(newTextarea, { target: { value: "new target draft" } });
    fireEvent.click(screen.getByTestId("send-button"));
    await waitFor(() => expect(sends).toHaveLength(3));
    expect(sends[2].target.backendSessionId).toBe("sess-2");
    await act(async () => {
      resolvers.get("sess-1")?.(jsonResponse({ ok: true, data: { requestId: sends[1].requestId, target: sends[1].target, stage: "accepted" }, requestId: sends[1].requestId }));
      await pending.get("sess-1");
    });
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("new target draft");
    expect(screen.queryByTestId("mobile-chat-held")).not.toBeInTheDocument();
    expect(sends.map((send) => send.target.backendSessionId)).toEqual(["sess-1", "sess-1", "sess-2"]);
    expect(ptyWrites).toEqual([]);
  });

  it("switches target cleanly when selecting a different session, fencing late transcript updates", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });

    let sess1Calls = 0;
    const requestMock = vi.fn<typeof fetch>(async (input) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/workspace")) {
        return jsonResponse(initialWorkspace);
      }
      if (url.includes("/api/v1/sessions")) {
        return jsonResponse({ sessions: [{ sessionId: "sess-1", daemonEpoch: "41" }, { sessionId: "sess-2", daemonEpoch: "41" }] });
      }
      if (url.includes("/api/v1/capabilities")) {
        return jsonResponse({ daemonEpoch: "41" });
      }
      if (url.includes("/api/v1/chat/callbacks")) {
        return jsonResponse({
          ok: true,
          data: [],
          requestId: "chat-callbacks-query",
        });
      }
      if (url.includes("/api/v1/capabilities")) {
        return jsonResponse({ daemonEpoch: "41" });
      }
      if (url.includes("/api/v1/agent-history/sess-1")) {
        sess1Calls++;
        if (sess1Calls > 1) {
          await new Promise((r) => setTimeout(r, 80));
          return jsonResponse({
            sessionId: "sess-1",
            items: [
              { ordinal: 0, role: "user", text: "Stale update from sess-1" },
              { ordinal: 1, role: "assistant", text: "Should never leak to sess-2" },
            ],
            nextCursor: null,
            partial: false,
            warnings: [],
          });
        }
        return jsonResponse({
          sessionId: "sess-1",
          items: [{ ordinal: 0, role: "assistant", text: "Welcome to Session 1" }],
          nextCursor: null,
          partial: false,
          warnings: [],
        });
      }
      if (url.includes("/api/v1/agent-history/sess-2")) {
        return jsonResponse({
          sessionId: "sess-2",
          items: [{ ordinal: 0, role: "assistant", text: "Welcome to Session 2" }],
          nextCursor: null,
          partial: false,
          warnings: [],
        });
      }
      if (url.includes("/api/v1/socket-ticket")) {
        return jsonResponse({ ticket: "mock-ticket-xyz" });
      }
      return jsonResponse({});
    });

    vi.stubGlobal("fetch", requestMock);

    render(<RemoteApp />);

    const chatViewButton = await screen.findByTestId("remote-view-mode-chat");
    fireEvent.click(chatViewButton);

    expect(await screen.findByText("Welcome to Session 1")).toBeInTheDocument();

    const changeContextButton = screen.getByRole("button", { name: "Change workspace context" });
    fireEvent.click(changeContextButton);

    const sheet = await screen.findByRole("tablist", { name: /terminal tabs/i });
    const sess2Option = within(sheet).getByRole("tab", { name: /Shell 2/i });
    fireEvent.click(sess2Option);

    expect(await screen.findByText("Welcome to Session 2")).toBeInTheDocument();

    await new Promise((r) => setTimeout(r, 120));

    expect(screen.queryByText("Should never leak to sess-2")).not.toBeInTheDocument();
    expect(screen.getByText("Welcome to Session 2")).toBeInTheDocument();
  });

  it("handles live approval callbacks and resolutions streaming from machine_events over /events socket", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });

    let replyCalls: any[] = [];
    let replyFailure: { code: string; message: string } | null = null;
    let pendingReply: (() => void) | null = null;
    let releasePendingReply: (() => void) | null = null;

    class FakeWebSocket {
      static instances: FakeWebSocket[] = [];
      url: string;
      readyState = 1;
      closed = false;
      onopen: any = null;
      onmessage: any = null;
      onerror: any = null;
      onclose: any = null;

      constructor(url: string) {
        this.url = url;
        FakeWebSocket.instances.push(this);
        setTimeout(() => this.onopen?.(), 0);
      }
      send() {}
      close() {
        this.closed = true;
      }
    }

    vi.stubGlobal("WebSocket", FakeWebSocket as any);

    const requestMock = vi.fn<typeof fetch>(async (input, init) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/workspace")) {
        return jsonResponse(initialWorkspace);
      }
      if (url.includes("/api/v1/sessions")) {
        return jsonResponse({ sessions: [{ sessionId: "sess-1", daemonEpoch: "41" }, { sessionId: "sess-2", daemonEpoch: "41" }] });
      }
      if (url.includes("/api/v1/capabilities")) {
        return jsonResponse({ daemonEpoch: "41" });
      }
      if (url.includes("/api/v1/agent-history/")) {
        return jsonResponse({
          sessionId: "sess-1",
          items: [],
          nextCursor: null,
          partial: false,
          warnings: [],
        });
      }
      if (url.includes("/api/v1/chat/callbacks")) {
        return jsonResponse({
          ok: true,
          data: [],
          requestId: "chat-callbacks-query",
        });
      }
      if (url.includes("/api/v1/chat/reply")) {
        const body = JSON.parse(String(init?.body));
        replyCalls.push(body);
        if (pendingReply) await new Promise<void>((resolve) => { releasePendingReply = resolve; });
        if (replyFailure) {
          const failure = replyFailure;
          replyFailure = null;
          return jsonResponse({ ok: false, error: failure, requestId: body.requestId }, 422);
        }
        return jsonResponse({
          ok: true,
          data: {
            requestId: body.requestId,
            target: body.target,
            callbackId: body.callbackId,
            resolved: true,
          },
          requestId: body.requestId,
        });
      }
      if (url.includes("/api/v1/socket-ticket")) {
        return jsonResponse({ ticket: "mock-ticket-xyz" });
      }
      return jsonResponse({});
    });

    vi.stubGlobal("fetch", requestMock);

    render(<RemoteApp />);

    const chatViewButton = await screen.findByTestId("remote-view-mode-chat");
    fireEvent.click(chatViewButton);

    await screen.findByTestId("chat-mode", {}, { timeout: 10000 });
    await screen.findByTestId("chat-composer-textarea", {}, { timeout: 10000 });

    await waitFor(() => {
      const socket = FakeWebSocket.instances.find((s) => s.url.includes("/api/v1/events") && !s.closed);
      expect(socket).toBeDefined();
    });
    await waitFor(() => expect(screen.getByTestId("agent-start")).toBeEnabled());

    await act(async () => {
      FakeWebSocket.instances
        .filter((s) => s.url.includes("/api/v1/events") && !s.closed)
        .forEach((s) => {
          s.onmessage?.({
            data: JSON.stringify({
              type: "callback",
              sessionId: "sess-1",
              callback: {
                id: "cb-req-99",
                threadId: "th-1",
                turnId: "tu-1",
                callbackIncarnation: 7,
                target: { hostId: `local:${window.location.origin}`, ownerId: "device-1", epoch: "41", backendSessionId: "sess-1" },
                kind: "approval",
                text: "Allow modification of package.json?",
              },
            }),
          } as any);
        });
    });

    expect(
      await screen.findByText("Allow modification of package.json?", {}, { timeout: 10000 })
    ).toBeInTheDocument();

    const acceptButton = screen.getByTestId("approval-accept");
    fireEvent.click(acceptButton);

    await waitFor(() => {
      expect(replyCalls.length).toBe(1);
    });

    expect(replyCalls[0].callbackId).toBe("cb-req-99");
    expect(replyCalls[0].result).toEqual({ decision: "accept" });
    expect(replyCalls[0].target.backendSessionId).toBe("sess-1");

    await act(async () => {
      FakeWebSocket.instances
        .filter((s) => s.url.includes("/api/v1/events") && !s.closed)
        .forEach((s) => {
          s.onmessage?.({
            data: JSON.stringify({
              type: "callback_resolved",
              sessionId: "sess-1",
              callbackId: "cb-req-99",
              callbackIncarnation: 7,
            }),
          } as any);
        });
    });

    await waitFor(() => {
      expect(
        screen.queryByText("Allow modification of package.json?")
      ).not.toBeInTheDocument();
    });

    const sockets = () => FakeWebSocket.instances.filter((socket) => socket.url.includes("/api/v1/events") && !socket.closed);
    const emitCallback = async (callback: unknown) => act(async () => sockets().forEach((socket) => socket.onmessage?.({
      data: JSON.stringify({ type: "callback", sessionId: "sess-1", callback }),
    } as any)));
    await emitCallback({
      id: "cb-question-1", threadId: "th-1", turnId: "tu-2", callbackIncarnation: 8,
      target: { hostId: `local:${window.location.origin}`, ownerId: "device-1", epoch: "41", backendSessionId: "sess-1" },
      kind: "question", text: "Choose and provide credentials", questions: [
        { id: "environment", question: "Environment", required: true, options: [{ label: "staging", description: "Pre-production" }, { label: "production", description: "Live" }] },
        { id: "token", question: "Access token", required: true, isSecret: true },
      ],
    });
    const questionCard = await screen.findByTestId("callback-card-cb-question-1");
    const environment = within(questionCard).getByLabelText("Environment");
    const secret = within(questionCard).getByLabelText("Access token");
    expect(environment).toHaveValue("");
    expect(within(environment).getByRole("option", { name: "staging - Pre-production" })).toBeInTheDocument();
    expect(secret).toHaveAttribute("type", "password");
    fireEvent.change(environment, { target: { value: "staging" } });
    fireEvent.change(secret, { target: { value: "secret-value-never-echo" } });
    expect(questionCard).not.toHaveTextContent("secret-value-never-echo");
    replyFailure = { code: "INVALID_ANSWERS", message: "A supplied answer was rejected" };
    fireEvent.click(within(questionCard).getByTestId("question-submit"));
    await screen.findByRole("alert");
    expect(screen.getByTestId("callback-card-cb-question-1")).toBeInTheDocument();
    expect(replyCalls.at(-1).result).toEqual({ answers: { environment: "staging", token: "secret-value-never-echo" } });
    fireEvent.click(within(questionCard).getByTestId("question-submit"));
    await waitFor(() => expect(replyCalls).toHaveLength(3));
    await waitFor(() => expect(screen.queryByTestId("callback-card-cb-question-1")).not.toBeInTheDocument());

    await emitCallback({
      id: "cb-stale", threadId: "th-1", turnId: "tu-3", callbackIncarnation: 9,
      target: { hostId: `local:${window.location.origin}`, ownerId: "device-1", epoch: "40", backendSessionId: "sess-1" },
      kind: "approval", text: "Stale epoch request",
    });
    expect(screen.queryByTestId("callback-card-cb-stale")).not.toBeInTheDocument();

    pendingReply = () => {};
    await emitCallback({
      id: "cb-pending", threadId: "th-1", turnId: "tu-4", callbackIncarnation: 10,
      target: { hostId: `local:${window.location.origin}`, ownerId: "device-1", epoch: "41", backendSessionId: "sess-1" },
      kind: "approval", text: "Resolve while pending",
    });
    fireEvent.click(await screen.findByTestId("approval-accept"));
    const pendingCard = await screen.findByTestId("callback-card-cb-pending");
    await act(async () => sockets().forEach((socket) => socket.onmessage?.({ data: JSON.stringify({
      type: "callback_resolved", sessionId: "sess-1", callbackId: "cb-pending", callbackIncarnation: 10,
      target: { hostId: `local:${window.location.origin}`, ownerId: "device-1", epoch: "41", backendSessionId: "sess-1" },
    }) } as any)));
    await waitFor(() => expect(screen.queryByTestId("callback-card-cb-pending")).not.toBeInTheDocument());
    (releasePendingReply as (() => void) | null)?.();
    await waitFor(() => expect(screen.queryByTestId("callback-card-cb-pending")).not.toBeInTheDocument());
    expect(pendingCard).not.toBeInTheDocument();
  }, 20000);

  it("exposes explicit user Start action invoking /api/v1/chat/start with zero auto-launch on view/reconnect/poll/send failure", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });

    let startCalls: any[] = [];
    let ptySocketSends: string[] = [];

    class FakeWebSocket {
      url: string;
      readyState = 1;
      onopen: any = null;
      onmessage: any = null;
      onerror: any = null;
      onclose: any = null;

      constructor(url: string) {
        this.url = url;
        setTimeout(() => this.onopen?.(), 0);
      }
      send(data: string) {
        ptySocketSends.push(data);
      }
      close() {}
    }

    vi.stubGlobal("WebSocket", FakeWebSocket as any);

    let failStart = false;
    const requestMock = vi.fn<typeof fetch>(async (input, init) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/workspace")) {
        return jsonResponse(initialWorkspace);
      }
      if (url.includes("/api/v1/sessions")) {
        return jsonResponse({ sessions: [{ sessionId: "sess-1", daemonEpoch: "41" }, { sessionId: "sess-2", daemonEpoch: "41" }] });
      }
      if (url.includes("/api/v1/capabilities")) {
        return jsonResponse({ daemonEpoch: "41" });
      }
      if (url.includes("/api/v1/agent-history/")) {
        return jsonResponse({
          sessionId: "sess-1",
          items: [],
          nextCursor: null,
          partial: false,
          warnings: [],
        });
      }
      if (url.includes("/api/v1/chat/callbacks")) {
        return jsonResponse({
          ok: true,
          data: [],
          requestId: "chat-callbacks-query",
        });
      }
      if (url.includes("/api/v1/sessions")) {
        return jsonResponse({ sessions: [{ sessionId: "sess-1", daemonEpoch: "41" }] });
      }
      if (url.includes("/api/v1/capabilities")) {
        return jsonResponse({ daemonEpoch: "41" });
      }
      if (url.includes("/api/v1/chat/start")) {
        const body = JSON.parse(String(init?.body));
        startCalls.push(body);

        if (failStart) {
          return jsonResponse(
            {
              ok: false,
              error: { code: "SERVICE_UNAVAILABLE", message: "Agent daemon unavailable", retryable: false },
              requestId: body.requestId,
            },
            503
          );
        }

        return jsonResponse({
          ok: true,
          data: {
            requestId: body.requestId,
            target: body.target,
            provider: body.provider,
            threadId: "th-codex-1",
          },
          requestId: body.requestId,
        });
      }
      if (url.includes("/api/v1/socket-ticket")) {
        return jsonResponse({ ticket: "mock-ticket-xyz" });
      }
      return jsonResponse({});
    });

    vi.stubGlobal("fetch", requestMock);

    render(<RemoteApp />);

    const chatViewButton = await screen.findByTestId("remote-view-mode-chat");
    fireEvent.click(chatViewButton);

    await screen.findByTestId("chat-mode", {}, { timeout: 10000 });
    await screen.findByTestId("chat-composer-textarea", {}, { timeout: 10000 });
    expect(startCalls).toHaveLength(0);

    const startButton = await screen.findByTestId("agent-start", {}, { timeout: 10000 });
    expect(startButton).toBeInTheDocument();

    fireEvent.click(startButton);

    await waitFor(() => {
      expect(startCalls.length).toBe(1);
    });

    expect(startCalls[0].provider).toBe("codex");
    expect(startCalls[0].target.backendSessionId).toBe("sess-1");
    expect(ptySocketSends).toHaveLength(0);

    failStart = true;
    fireEvent.click(startButton);

    await waitFor(() => {
      expect(startCalls.length).toBe(2);
    });

    expect(
      await screen.findByText(/Failed to start agent: Agent daemon unavailable/)
    ).toBeInTheDocument();

    expect(ptySocketSends).toHaveLength(0);
  });

  it("stages attachment via file-upload-input, blocks send until staged, and delivers send payload with staged attachments", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });

    let sentChatPayloads: any[] = [];
    let uploadPayloads: any[] = [];
    let ptySocketSends: string[] = [];

    class FakeWebSocket {
      url: string;
      readyState = 1;
      onopen: any = null;
      onmessage: any = null;
      onerror: any = null;
      onclose: any = null;

      constructor(url: string) {
        this.url = url;
        setTimeout(() => this.onopen?.(), 0);
      }
      send(data: string) {
        ptySocketSends.push(data);
      }
      close() {}
    }

    vi.stubGlobal("WebSocket", FakeWebSocket as any);

    const mockFile = createMockFile("architecture.txt", "test-content-15", "text/plain");
    const localHash = await computeSha256Hex(await mockFile.arrayBuffer());

    const requestMock = vi.fn<typeof fetch>(async (input, init) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/workspace")) {
        return jsonResponse(initialWorkspace);
      }
      if (url.includes("/api/v1/sessions")) {
        return jsonResponse({ sessions: [{ sessionId: "sess-1", daemonEpoch: "41" }, { sessionId: "sess-2", daemonEpoch: "41" }] });
      }
      if (url.includes("/api/v1/capabilities")) {
        return jsonResponse({ daemonEpoch: "41" });
      }
      if (url.includes("/api/v1/agent-history/")) {
        return jsonResponse({
          sessionId: "sess-1",
          items: [],
          nextCursor: null,
          partial: false,
          warnings: [],
        });
      }
      if (url.includes("/api/v1/chat/callbacks")) {
        return jsonResponse({
          ok: true,
          data: [],
          requestId: "chat-callbacks-query",
        });
      }
      if (url.includes("/api/v1/chat/attachments/upload")) {
        const body = JSON.parse(String(init?.body));
        uploadPayloads.push(body);
        return jsonResponse({
          ok: true,
          data: {
            hostId: body.target.hostId,
            attachmentId: body.attachmentId,
            sizeBytes: body.totalBytes,
            sha256: localHash,
            mediaType: body.mediaType,
          },
          requestId: "upload-req-1",
        });
      }
      if (url.includes("/api/v1/chat/send")) {
        const body = JSON.parse(String(init?.body));
        sentChatPayloads.push(body);
        return jsonResponse({
          ok: true,
          data: {
            requestId: body.requestId,
            target: body.target,
            stage: "accepted",
          },
          requestId: body.requestId,
        });
      }
      if (url.includes("/api/v1/socket-ticket")) {
        return jsonResponse({ ticket: "mock-ticket-xyz" });
      }
      return jsonResponse({});
    });

    vi.stubGlobal("fetch", requestMock);

    render(<RemoteApp />);

    const chatViewButton = await screen.findByTestId("remote-view-mode-chat");
    fireEvent.click(chatViewButton);

    await screen.findByTestId("chat-mode", {}, { timeout: 10000 });
    await screen.findByTestId("chat-composer-textarea", {}, { timeout: 10000 });

    const fileInput = await screen.findByTestId("file-upload-input");
    expect(fileInput).toBeInTheDocument();

    await act(async () => {
      fireEvent.change(fileInput, { target: { files: [mockFile] } });
    });

    await waitFor(() => {
      expect(uploadPayloads).toHaveLength(1);
      expect(screen.getByTestId(`attachment-preview-${uploadPayloads[0].attachmentId}`)).toBeInTheDocument();
      expect(screen.queryByTestId("chat-composer-attachments-staging")).not.toBeInTheDocument();
    });

    const composerTextarea = await screen.findByTestId("chat-composer-textarea", {}, { timeout: 10000 });
    fireEvent.change(composerTextarea, { target: { value: "Review architecture document" } });

    const sendButton = await screen.findByTestId("send-button", {}, { timeout: 10000 });
    fireEvent.click(sendButton);

    await waitFor(() => {
      expect(sentChatPayloads.length).toBe(1);
    });

    expect(sentChatPayloads[0].draft.text).toBe("Review architecture document");
    expect(sentChatPayloads[0].draft.attachments).toHaveLength(1);
    expect(sentChatPayloads[0].draft.attachments[0].sizeBytes).toBe(mockFile.size);
    expect(sentChatPayloads[0].draft.attachments[0].attachmentId).toBe(uploadPayloads[0].attachmentId);
    expect(ptySocketSends).toHaveLength(0);
  });

  it("cancels staged attachment before send, removing it from draft and invoking cancel endpoint with zero PTY fallback", async () => {
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });

    let sentChatPayloads: any[] = [];
    let cancelCalls: any[] = [];
    let ptySocketSends: string[] = [];

    class FakeWebSocket {
      url: string;
      readyState = 1;
      onopen: any = null;
      onmessage: any = null;
      onerror: any = null;
      onclose: any = null;

      constructor(url: string) {
        this.url = url;
        setTimeout(() => this.onopen?.(), 0);
      }
      send(data: string) {
        ptySocketSends.push(data);
      }
      close() {}
    }

    vi.stubGlobal("WebSocket", FakeWebSocket as any);

    const mockFile = createMockFile("scratch.txt", "test-content-20-chars", "text/plain");
    const localHash = await computeSha256Hex(await mockFile.arrayBuffer());
    let uploadedBytesHash: string | undefined;

    let stagedAttachmentId = "";
    const requestMock = vi.fn<typeof fetch>(async (input, init) => {
      const url = String(input instanceof Request ? input.url : input);
      if (url.includes("/api/v1/workspace")) {
        return jsonResponse(initialWorkspace);
      }
      if (url.includes("/api/v1/sessions")) {
        return jsonResponse({ sessions: [{ sessionId: "sess-1", daemonEpoch: "41" }] });
      }
      if (url.includes("/api/v1/capabilities")) {
        return jsonResponse({ daemonEpoch: "41" });
      }
      if (url.includes("/api/v1/agent-history/")) {
        return jsonResponse({
          sessionId: "sess-1",
          items: [],
          nextCursor: null,
          partial: false,
          warnings: [],
        });
      }
      if (url.includes("/api/v1/chat/callbacks")) {
        return jsonResponse({
          ok: true,
          data: [],
          requestId: "chat-callbacks-query",
        });
      }
      if (url.includes("/api/v1/chat/attachments/upload")) {
        const body = JSON.parse(String(init?.body));
        stagedAttachmentId = body.attachmentId;
        const bytes = Uint8Array.from(atob(body.data), (char) => char.charCodeAt(0));
        uploadedBytesHash = await computeSha256Hex(bytes.buffer);
        expect(uploadedBytesHash).toBe(localHash);
        return jsonResponse({
          ok: true,
          data: {
            hostId: body.target.hostId,
            attachmentId: body.attachmentId,
            sizeBytes: body.totalBytes,
            sha256: uploadedBytesHash,
            mediaType: body.mediaType,
          },
          requestId: "upload-req-2",
        });
      }
      if (url.includes("/api/v1/chat/attachments/cancel")) {
        const body = JSON.parse(String(init?.body));
        cancelCalls.push(body);
        return jsonResponse({
          ok: true,
          cleaned: true,
        });
      }
      if (url.includes("/api/v1/chat/send")) {
        const body = JSON.parse(String(init?.body));
        sentChatPayloads.push(body);
        return jsonResponse({
          ok: true,
          data: {
            requestId: body.requestId,
            target: body.target,
            stage: "accepted",
          },
          requestId: body.requestId,
        });
      }
      if (url.includes("/api/v1/socket-ticket")) {
        return jsonResponse({ ticket: "mock-ticket-xyz" });
      }
      return jsonResponse({});
    });

    vi.stubGlobal("fetch", requestMock);

    render(<RemoteApp />);

    const chatViewButton = await screen.findByTestId("remote-view-mode-chat");
    fireEvent.click(chatViewButton);

    await screen.findByTestId("chat-composer-textarea", {}, { timeout: 10000 });

    const fileInput = await screen.findByTestId("file-upload-input");

    await act(async () => {
      fireEvent.change(fileInput, { target: { files: [mockFile] } });
    });

    await waitFor(() => {
      expect(stagedAttachmentId).not.toBe("");
      expect(screen.getByTestId(`attachment-preview-${stagedAttachmentId}`)).toBeInTheDocument();
      expect(screen.queryByTestId("chat-composer-attachments-staging")).not.toBeInTheDocument();
    });

    const removeBtn = await screen.findByRole("button", { name: /Remove attachment scratch\.txt/i });
    fireEvent.click(removeBtn);

    await waitFor(() => {
      expect(cancelCalls.length).toBe(1);
    });

    expect(cancelCalls[0].attachmentId).toBe(stagedAttachmentId);

    const composerTextarea = await screen.findByTestId("chat-composer-textarea", {}, { timeout: 10000 });
    fireEvent.change(composerTextarea, { target: { value: "Sent without attachment" } });

    const sendButton = await screen.findByTestId("send-button", {}, { timeout: 10000 });
    fireEvent.click(sendButton);

    await waitFor(() => {
      expect(sentChatPayloads.length).toBe(1);
    });

    expect(sentChatPayloads[0].draft.text).toBe("Sent without attachment");
    expect(sentChatPayloads[0].draft.attachments).toHaveLength(0);
    expect(ptySocketSends).toHaveLength(0);
  });
});
