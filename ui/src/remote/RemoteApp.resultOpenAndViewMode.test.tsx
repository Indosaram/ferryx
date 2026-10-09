import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { RemoteApp } from "./RemoteApp";

// Harness shape copied from remoteAppChatFrames.test.tsx: RemoteTerminal is stubbed so the
// terminal surface renders a stable testid without deeper socket wiring.
vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: ({
    sessionId,
  }: {
    sessionId: string;
  }) => (
    <div data-testid="remote-terminal" data-session-id={sessionId}>
      Mirrored terminal {sessionId}
    </div>
  ),
}));

function jsonResponse(body: unknown, ok = true, status: number | null = null): Response {
  return {
    ok,
    status: status ?? (ok ? 200 : 500),
    json: vi.fn(async () => body),
    text: vi.fn(async () => JSON.stringify(body)),
  } as unknown as Response;
}

function ticketed(inner: typeof fetch): typeof fetch {
  return vi.fn<typeof fetch>(async (input, init) => {
    const url = String(input instanceof Request ? input.url : input);
    if (url.includes("/api/v1/socket-ticket")) {
      return jsonResponse({ ticket: "ui-test-ticket", expiresAt: 9999999999 });
    }
    return inner(input, init);
  }) as unknown as typeof fetch;
}

class StubWebSocket {
  static instances: StubWebSocket[] = [];
  readonly url: string;
  binaryType = "blob";
  readyState = 1;
  close = vi.fn();
  send = vi.fn();
  onmessage: ((event: MessageEvent) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;

  constructor(url: string) {
    this.url = url;
    StubWebSocket.instances.push(this);
  }
}

const remoteState = {
  activeContext: {
    workspaceId: "ferryx",
    worktreeSlug: "main",
    worktreeLabel: "main",
    activeTabId: "tab-main",
    activeTerminal: { sessionId: "sess-main", title: "terminal", running: true },
    terminalTabs: [
      { id: "tab-main", sessionId: "sess-main", label: "terminal", agentType: "shell", activityState: "idle", worktreeLabel: "main" },
    ],
  },
};

const emptyHistoryPage = {
  sessionId: "sess-main",
  items: [],
  nextCursor: null,
  partial: false,
  warnings: [],
};

// Real result-file list payload shape: RemoteManagedChatService.fetchResultFiles
// (chat/remoteManagedChatService.ts:558) requires `{ ok: true, files: [{ fileId, displayName }] }`
// from POST /api/v1/files/results/list (same file :560).
const RESULT_FILE = { fileId: "result-123", displayName: "step_1.txt" };
const PREVIEW_TOKEN = "preview-token-abc";

function routedFetch(historyResponse: Response) {
  return vi.fn<typeof fetch>(async (input, init) => {
    const url = String(input instanceof Request ? input.url : input);
    if (url.includes("/api/v1/agent-history/")) return historyResponse;
    if (url.includes("/api/v1/sessions")) return jsonResponse({ sessions: [{ sessionId: "sess-main", daemonEpoch: "41" }, { sessionId: "sess-second", daemonEpoch: "41" }] });
    if (url.includes("/api/v1/capabilities")) return jsonResponse({ daemonEpoch: "41" });
    if (url.includes("/api/v1/chat/send")) {
      const body = JSON.parse(String(init?.body ?? "{}"));
      return jsonResponse({ ok: true, data: { requestId: body.requestId, target: body.target, stage: "accepted" }, requestId: body.requestId });
    }
    if (url.includes("/api/v1/files/results/list")) return jsonResponse({ ok: true, files: [RESULT_FILE] });
    if (url.includes("/api/v1/files/preview/token")) return jsonResponse({ ok: true, token: PREVIEW_TOKEN, expiresAt: 9999999999 });
    return jsonResponse(remoteState);
  });
}

describe("RemoteApp.resultOpenAndViewMode", () => {
  let originalInnerWidth: number;

  beforeEach(() => {
    originalInnerWidth = window.innerWidth;
    StubWebSocket.instances = [];
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal("WebSocket", StubWebSocket);
  });

  afterEach(() => {
    Object.defineProperty(window, "innerWidth", { value: originalInnerWidth, configurable: true, writable: true });
    cleanup();
    localStorage.clear();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("result-file open is wired end to end at the RemoteApp level: list, preview token, scoped URL", async () => {
    // LOAD-BEARING: a single removal makes this test fail —
    //   (a) deleting the `onOpenResultFile={openResultFile}` prop wiring (RemoteApp.tsx:2399), or
    //   (b) deleting the `managedChatService.openResultPreview(target, fileId)` call inside
    //       `openResultFile` (RemoteApp.tsx:1714).
    // Either removal leaves the click inert: the workspace child optional-chains
    // `onOpenResultFile?.()` (MobileChatWorkspace.tsx:249), and the service has its own direct
    // test — only this app-level path proves the two are actually connected.
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const request = routedFetch(jsonResponse(emptyHistoryPage));
    vi.stubGlobal("fetch", ticketed(request));

    // RemoteApp.tsx:1717 opens a blank window, then hands the scoped URL minted from the
    // capability token to previewWindow.location.replace (RemoteApp.tsx:1717-1721), so the
    // scoped URL is asserted on `location.replace`, NOT on window.open's own arguments.
    const replace = vi.fn();
    const openSpy = vi.spyOn(window, "open").mockReturnValue({ opener: null, location: { replace } } as unknown as Window);

    render(<RemoteApp />);

    const chatViewButton = await screen.findByTestId("remote-view-mode-chat");
    await act(async () => {
      fireEvent.click(chatViewButton);
    });

    // Real control: data-testid={`result-file-open-${fileId}`} (MobileChatWorkspace.tsx:248),
    // rendered inside `chat-result-files` (same file :243) from RemoteApp's resultFiles state.
    const openControl = await screen.findByTestId("result-file-open-result-123", {}, { timeout: 10000 });
    expect(openControl).toBeTruthy();
    expect(within(openControl).getByText("step_1.txt")).toBeInTheDocument();

    const listCalls = request.mock.calls.filter(([input]) =>
      String(input instanceof Request ? input.url : input).includes("/api/v1/files/results/list"),
    );
    expect(listCalls.length).toBeGreaterThan(0);

    fireEvent.click(openControl);

    await waitFor(() => {
      expect(replace).toHaveBeenCalledTimes(1);
    }, { timeout: 10000 });

    expect(openSpy).toHaveBeenCalledWith("about:blank", "_blank");
    // Scoped URL minted from the preview capability token:
    // openResultPreview returns `…/api/v1/files/preview/<token>` (remoteManagedChatService.ts:582).
    expect(replace).toHaveBeenCalledWith(
      expect.stringContaining(`/api/v1/files/preview/${PREVIEW_TOKEN}`),
    );

    const tokenCalls = request.mock.calls.filter(([input]) =>
      String(input instanceof Request ? input.url : input).includes("/api/v1/files/preview/token"),
    );
    expect(tokenCalls).toHaveLength(1);
    const tokenBody = JSON.parse(String(tokenCalls[0][1]?.body ?? "{}"));
    expect(tokenCalls[0][1]?.method).toBe("POST");
    expect(tokenBody.fileId).toBe("result-123");
    expect(tokenBody.target.backendSessionId).toBe("sess-main");
    expect(tokenBody.target.epoch).toBe("41");
  });

  it("chat and terminal surfaces are mutually exclusive across view-mode switches", async () => {
    // Both surfaces carry the SAME `data-testid="chat-mode"` and differ only by aria-label:
    //   chat     → RemoteApp.tsx:2359 aria-label="Managed Codex chat"
    //   terminal → RemoteApp.tsx:2738 aria-label="Terminal"
    // The exclusivity invariant is therefore the COUNT: both rendered → 2 → this test fails.
    // The terminal branch additionally gates on `effectiveSessionId && token`
    // (RemoteApp.tsx:2738); the harness supplies a session (remoteState.activeTerminal.sessionId
    // = "sess-main" plus localStorage ferryx_remote_token), so a gate failure yields ZERO
    // surfaces and the length-1 assertions fail — never a vacuous pass.
    Object.defineProperty(window, "innerWidth", { value: 390, configurable: true, writable: true });
    const request = routedFetch(jsonResponse(emptyHistoryPage));
    vi.stubGlobal("fetch", ticketed(request));

    render(<RemoteApp />);

    const chatViewButton = await screen.findByTestId("remote-view-mode-chat");
    await act(async () => {
      fireEvent.click(chatViewButton);
    });

    // Chat mode: exactly one surface, and it is the chat one.
    const chatSurfaces = await screen.findAllByTestId("chat-mode", {}, { timeout: 10000 });
    expect(chatSurfaces).toHaveLength(1);
    expect(chatSurfaces[0]).toHaveAttribute("aria-label", "Managed Codex chat");
    // Chat surface content lives inside that container (mobile-chat-workspace: MobileChatWorkspace.tsx:143).
    expect(within(chatSurfaces[0]).getByTestId("mobile-chat-workspace")).toBeInTheDocument();
    // Terminal surface absent while in chat mode.
    expect(screen.queryByTestId("remote-terminal")).toBeNull();

    const terminalButton = await screen.findByTestId("remote-view-mode-terminal");
    await act(async () => {
      fireEvent.click(terminalButton);
    });

    // Terminal mode: still exactly one surface, and now it is the terminal one. If both
    // surfaces rendered simultaneously, getAllByTestId would return 2 and this would fail.
    await waitFor(() => {
      const surfaces = screen.getAllByTestId("chat-mode");
      expect(surfaces).toHaveLength(1);
      expect(surfaces[0]).toHaveAttribute("aria-label", "Terminal");
    }, { timeout: 10000 });

    // Chat surface unmounted after the switch.
    expect(screen.queryByTestId("mobile-chat-workspace")).toBeNull();
    expect(screen.getByTestId("remote-terminal")).toBeInTheDocument();
    expect(screen.getAllByTestId("chat-mode")).toHaveLength(1);
  });
});

