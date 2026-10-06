import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useId } from "react";
import { afterEach, beforeEach, describe, expect, it, onTestFailed, vi } from "vitest";

/**
 * Failure-only diagnostics for the post-merge run: the fetch order (method + PATHNAME only) and the
 * terminal-related selector state at the point of failure. Pathnames only - query strings can carry
 * tickets and tokens - and no header, body or DOM dump is ever read or printed. Bounded to 40 calls
 * per mock. Registered through onTestFailed, so a passing test prints nothing and the original
 * assertion error is untouched.
 */
/**
 * Writes a diagnostic line straight to the process stdout. The JSON reporter does not implement
 * onUserConsoleLog, so a console.log never reaches a --reporter=json receipt; process.stdout does.
 */
function emitLine(line: string): void {
  try {
    process.stdout.write(`${line}\n`);
  } catch {
    /* a diagnostic must never change the outcome of the test it reports on */
  }
}

function reportFetchOrder(label: string, ...mocks: unknown[]): void {
  try {
    mocks.forEach((mock, mockIndex) => {
      const calls = (mock as { mock?: { calls?: unknown[][] } })?.mock?.calls ?? [];
      const shown = calls.slice(0, 40).map((args, index) => {
        const raw = String(args[0] instanceof Request ? args[0].url : args[0]);
        let pathname = "(unparseable-url)";
        try {
          pathname = new URL(raw, "http://localhost").pathname;
        } catch {
          /* keep the marker: the raw value is never printed */
        }
        const init = args[1] as RequestInit | undefined;
        return `${index + 1} ${(init?.method ?? "GET").toUpperCase()} ${pathname}`;
      });
      const more = calls.length > 40 ? ` (+${calls.length - 40} more)` : "";
      emitLine(
        `[ui-diag] ${label} | mock${mockIndex + 1} order (${calls.length}): ${shown.join(" | ") || "(none)"}${more}`,
      );
    });
    const selectors = ["remote-view-mode-terminal", "remote-terminal", "remote-terminal-grid", "mobile-chat-workspace"]
      .map((id) => `${id}=${document.querySelector(`[data-testid="${id}"]`) ? "present" : "absent"}`)
      .join(", ");
    const trigger = document.querySelector('button[aria-label="Change workspace context"]') ? "present" : "absent";
    emitLine(`[ui-diag] ${label} | selectors: ${selectors}, context-trigger=${trigger}`);
  } catch {
    // A diagnostic must never change the outcome of the test it reports on.
  }
}

import { resolveAgentLogo } from "../lib/agentIcon";
import { MobileKeyDock } from "../components/MobileKeyDock";
import { PairingPage } from "./PairingPage";
import { RemoteApp, RemoteHostConnection } from "./RemoteApp";
import { normalizeRemoteWorkspaceState } from "./RemoteSessionList";
import { remoteHostStore } from "../state/remoteHostStore";
import { clearRemoteAuthToken, setRemoteAuthToken } from "../lib/remoteClient";
import { clearStoredAccountSessionToken, storeAccountSessionToken } from "./accountSession";

/**
 * Chat is the default surface at every width now (plan task 12), so a test that asserts the
 * terminal asks for it explicitly through the mode switch the header always offers.
 */
async function switchToTerminalMode(): Promise<void> {
  // Timer-free readiness, then switch ONLY when the chat surface is actually showing.
  //
  // Flushing the pending microtasks inside act() is the exact readiness step - it lets the mocked
  // state read settle - and it must NOT be a polling wait: several tests here run under
  // vi.useFakeTimers(), where a polling helper never sees its own timers fire and hangs the test.
  //
  // The switch is idempotent on purpose. Calling it twice must not toggle back to chat (a caller
  // that already switched would otherwise silently end up in chat and fail a terminal assertion for
  // the wrong reason), and a screen that legitimately offers no switch - the sign-in screen a magic
  // link lands on - must not fail here: the test's own terminal assertion decides that.
  await act(async () => {});
  if (screen.queryByTestId("mobile-chat-workspace") === null) return;
  await act(async () => {
    fireEvent.click(screen.getByTestId("remote-view-mode-terminal"));
  });
}


vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: ({
    sessionId,
    onSocketLifecycle,
  }: {
    sessionId: string;
    onSocketLifecycle?: (sessionId: string, state: "open" | "closed") => void;
  }) => (
    <div
      data-testid="remote-terminal"
      data-session-id={sessionId}
      data-instance-id={useId()}
      onClick={() => onSocketLifecycle?.(sessionId, "closed")}
      onDoubleClick={() => onSocketLifecycle?.(sessionId, "open")}
    >
      Mirrored terminal {sessionId}
    </div>
  ),
}));

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

function jsonResponse(body: unknown, ok = true): Response {
  return {
    ok,
    status: ok ? 200 : 500,
    json: vi.fn(async () => body),
  } as unknown as Response;
}

/**
 * The chat lane's own reads. Chat is the default surface, so these fire on mount in every test in
 * this file - and a mock that answers by CALL ORDER would hand them the workspace states a test
 * wrote for its own terminal sequence. They are answered by URL instead, so the terminal lane's
 * request order stays exactly what each test wrote.
 */
function chatLaneFixture(url: string): Response | null {
  if (url.includes("/api/v1/capabilities")) {
    // A truthful capabilities answer from a gateway that publishes no daemon incarnation, which
    // leaves the chat lane idle: these tests are about the terminal, not about chat polling.
    return jsonResponse({ apiVersion: 1, machineId: "mach-1", platform: "linux" });
  }
  if (url.includes("/api/v1/sessions")) {
    return jsonResponse({ sessions: [] });
  }
  if (url.includes("/reference-chat/") && url.includes("/history")) {
    return jsonResponse({
      source: "claude-transcript",
      availability: "native",
      turns: [],
      cursor: null,
      hasMore: false,
      generation: "gen-1",
      unavailableReason: null,
    });
  }
  if (url.includes("/reference-chat/") && url.includes("/prompt")) {
    return jsonResponse({ prompt: null, screenRevision: "rev-1", cols: 80, rows: 24 });
  }
  return null;
}

/** One recorded call: the URL it asked for and the init it carried. */
interface RecordedCall {
  readonly url: string;
  readonly init: RequestInit | undefined;
}

function recordedCalls(mock: unknown): RecordedCall[] {
  const calls = (mock as { mock: { calls: unknown[][] } }).mock.calls;
  return calls.map((args) => ({
    url: String(args[0] instanceof Request ? (args[0] as Request).url : args[0]),
    init: args[1] as RequestInit | undefined,
  }));
}

/**
 * Spy counts and ordinals are scoped to the endpoints that carry the terminal contract, so a
 * request made by another lane can never change what "called twice" or "the 2nd call" means.
 * `stateReads` is the no-extra-read assertion's own counter: the selection flow must not add one.
 */
function stateReads(mock: unknown): RecordedCall[] {
  return recordedCalls(mock).filter((call) => call.url.includes("/api/v1/workspace/state"));
}

function selectCalls(mock: unknown): RecordedCall[] {
  return recordedCalls(mock).filter((call) => call.url.includes("/api/v1/workspace/select"));
}

/** The workspace-state and selection calls together, in the order they were made. */
function workspaceCalls(mock: unknown): RecordedCall[] {
  return recordedCalls(mock).filter((call) => /\/api\/v1\/workspace\/(state|select)/.test(call.url));
}

function ticketed(inner: typeof fetch): typeof fetch {
  return vi.fn<typeof fetch>(async (input, init) => {
    const url = String(input instanceof Request ? input.url : input);
    if (url.includes("/api/v1/socket-ticket")) {
      return jsonResponse({ ticket: "ui-test-ticket", expiresAt: 9999999999 });
    }
    const lane = chatLaneFixture(url);
    if (lane !== null) return lane;
    return inner(input, init);
  }) as unknown as typeof fetch;
}

class EventWebSocket {
  static latest: EventWebSocket | null = null;
  readonly url: string;
  close = vi.fn();
  onmessage: ((event: MessageEvent) => void) | null = null;

  constructor(url: string) {
    this.url = url;
    EventWebSocket.latest = this;
  }
}

function eventSocket(): EventWebSocket {
  const socket = EventWebSocket.latest;
  if (!socket) throw new Error("Expected active-selection event socket");
  return socket;
}

const focusedState = {
  activeContext: {
    workspaceId: "ferryx-ui",
    worktreeSlug: "main",
    worktreeLabel: "main",
    activeTerminal: {
      sessionId: "focused-terminal",
      title: "Focused desktop terminal",
      running: true,
    },
  },
  projects: [
    {
      workspaceId: "ferryx-ui",
      repoRoot: "/Users/alice/secret/ferryx-ui",
      worktrees: [
        {
          slug: "main",
          label: "main",
          path: "/Users/alice/secret/ferryx-ui",
        },
      ],
    },
    {
      workspaceId: "api-service",
      repoRoot: "/Volumes/private/api-service",
      worktrees: [
        {
          slug: "feature/remote-safe",
          label: "feature/remote-safe",
          path: "/Volumes/private/api-service-worktree",
        },
      ],
    },
  ],
  sessions: [
    {
      sessionId: "focused-terminal",
      title: "Focused desktop terminal",
      workspaceId: "ferryx-ui",
      worktreeLabel: "main",
      running: true,
    },
    {
      sessionId: "background-terminal",
      title: "Hidden /Users/alice/secret shell",
      workspaceId: "api-service",
      worktreeLabel: "private-background",
      running: true,
    },
  ],
};

const currentNativeState = {
  activeWorkspaceId: "ferryx-ui",
  projects: [
    { workspaceId: "ferryx-ui", repoRoot: "/Users/alice/secret/ferryx-ui" },
    { workspaceId: "api-service", repoRoot: "/Volumes/private/api-service" },
  ],
  worktrees: [
    {
      path: "/Users/alice/secret/ferryx-ui",
      branch: "refs/heads/orca/ferryx-ui/main",
    },
  ],
  sessions: [
    {
      sessionId: "focused-terminal",
      workspaceId: "ferryx-ui",
      worktreeLabel: "main",
      running: true,
    },
  ],
};

const confirmedNoFocusState = {
  activeContext: {
    workspaceId: "api-service",
    worktreeSlug: "feature/remote-safe",
    worktreeLabel: "feature/remote-safe",
    activeTerminal: null,
  },
  projects: focusedState.projects,
  sessions: focusedState.sessions,
};

const secondFocusedState = {
  activeContext: {
    workspaceId: "api-service",
    worktreeSlug: "feature/remote-safe",
    worktreeLabel: "feature/remote-safe",
    sessionId: "new-focused-terminal",
  },
  activeWorkspaceId: "api-service",
  projects: focusedState.projects,
  worktrees: [{ worktreeSlug: "feature/remote-safe", worktreeLabel: "feature/remote-safe" }],
  sessions: [
    {
      sessionId: "new-focused-terminal",
      workspaceId: "api-service",
      worktreeLabel: "feature/remote-safe",
      running: true,
    },
  ],
};

const safeServerWorkspaceState = {
  activeContext: {
    workspaceId: "remote-e2e",
    worktreeSlug: "mobile-control",
    worktreeLabel: "mobile-control",
    sessionId: "focused-terminal",
  },
  activeWorkspaceId: "remote-e2e",
  projects: [
    {
      workspaceId: "remote-e2e",
      worktrees: [{ worktreeSlug: "mobile-control", worktreeLabel: "mobile-control" }],
    },
    {
      workspaceId: "other-project",
      worktrees: [{ worktreeSlug: "other-worktree", worktreeLabel: "other-worktree" }],
    },
  ],
  worktrees: [{ worktreeSlug: "mobile-control", worktreeLabel: "mobile-control" }],
  sessions: [
    {
      sessionId: "focused-terminal",
      workspaceId: "remote-e2e",
      worktreeLabel: "mobile-control",
      running: true,
    },
  ],
};

beforeEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

afterEach(() => {
  cleanup();
  localStorage.clear();
  EventWebSocket.latest = null;
  vi.unstubAllGlobals();
});

// The pane list now lives inside the worktree sheet, so pane queries open it first.
async function openWorktreeSheet(): Promise<HTMLElement> {
  const existing = screen.queryByRole("tablist", { name: /terminal tabs/i });
  if (existing) return existing;
  // act() flushes the sheet open without touching timers, which some suites fake.
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));
  });
  return screen.getByRole("tablist", { name: /terminal tabs/i });
}

describe("selection request lifetime", () => {
  const snapshot = (tabId: string) => ({
    ...focusedState,
    activeContext: {
      ...focusedState.activeContext,
      tabId,
      activeTerminal: { sessionId: `session-${tabId}`, running: true },
      terminalTabs: ["editor", "dev", "tests"].map((id) => ({
        id, label: id, sessionId: `session-${id}`,
      })),
    },
  });

  // Every request and socket subscription is installed before its triggering
  // action. Vitest's test timeout bounds these signal awaits, not polling.
  async function mountSelectionHost() {
    const initialRead = deferred<void>();
    const socketCreated = deferred<EventWebSocket>();
    const postA = deferred<void>();
    const postB = deferred<void>();
    const responseA = deferred<Response>();
    const responseB = deferred<Response>();
    const refresh = deferred<void>();
    const confirmationRead = deferred<void>();
    let state = snapshot("editor");
    let posts = 0;
    let reads = 0;
    let rejectA = false;
    const request = vi.fn<typeof fetch>(async (_input, init) => {
      if (init?.method === "POST") {
        posts += 1;
        if (posts === 1) {
          postA.resolve();
          const response = await responseA.promise;
          if (rejectA) throw new TypeError("selection connection lost");
          return response;
        }
        postB.resolve();
        return responseB.promise;
      }
      reads += 1;
      if (reads === 1) initialRead.resolve();
      else if (reads === 2) refresh.resolve();
      else confirmationRead.resolve();
      return jsonResponse(state);
    });
    class SelectionEventSocket extends EventWebSocket {
      constructor(url: string) {
        super(url);
        socketCreated.resolve(this);
      }
    }
    localStorage.setItem("ferryx_remote_token_selection-lifetime", "test-token");
    vi.stubGlobal("WebSocket", SelectionEventSocket);
    vi.stubGlobal("fetch", ticketed(request));
    const { unmount } = render(<RemoteHostConnection hostId="selection-lifetime" relayUrl={window.location.origin} readUrlHints={false} />);
    await act(async () => {
      await initialRead.promise;
      await socketCreated.promise;
    });
    const socket = await socketCreated.promise;
    const publish = (tabId: string) => {
      const onMessage = socket.onmessage;
      if (!onMessage) throw new Error("Selection event subscription missing");
      onMessage(new MessageEvent("message", {
        data: JSON.stringify({ event: "remote_active_selection_changed", payload: snapshot(tabId).activeContext }),
      }));
    };
    return {
      postA, postB, responseA, responseB, refresh, confirmationRead, publish, unmount,
      setState: (tabId: string) => { state = snapshot(tabId); },
      rejectA: () => { rejectA = true; },
      readCount: () => reads,
    };
  }

  it("releases selection when the request never settles", async () => {
    vi.useFakeTimers();
    try {
      const host = await mountSelectionHost();
      await switchToTerminalMode();
      const target = within(await openWorktreeSheet()).getByRole("tab", { name: "dev" });
      await act(async () => {
        fireEvent.click(target);
        await host.postA.promise;
      });
      expect(target).toBeDisabled();
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-dev");
      await act(async () => { await vi.advanceTimersByTimeAsync(5999); });
      expect(target).toBeDisabled();
      await act(async () => { await vi.advanceTimersByTimeAsync(1); });
      expect(target).toBeEnabled();
      await openWorktreeSheet();
      expect(screen.getByRole("button", { name: "Next terminal tab" })).toBeEnabled();
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-editor");
      expect(within(await openWorktreeSheet()).getByRole("tab", { name: "editor" })).toHaveAttribute("aria-selected", "true");
      expect(host.readCount()).toBe(1);
      host.unmount();
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      cleanup();
      vi.useRealTimers();
    }
  });

  it.each([
    { outcome: "success", acceptedB: true },
    { outcome: "http-failure", acceptedB: true },
    { outcome: "network-failure", acceptedB: true },
    { outcome: "success", acceptedB: false },
    { outcome: "http-failure", acceptedB: false },
    { outcome: "network-failure", acceptedB: false },
  ])("ignores an obsolete selection response while a newer selection is pending ($outcome, acceptedB=$acceptedB)", async ({ outcome, acceptedB }) => {
    vi.useFakeTimers();
    try {
      const host = await mountSelectionHost();
      await switchToTerminalMode();
      const sheet = await openWorktreeSheet();
      await act(async () => {
        fireEvent.click(within(sheet).getByRole("tab", { name: "dev" }));
        await host.postA.promise;
      });
      // Desktop replaces A with tests, releasing the picker before A settles.
      host.setState("tests");
      await act(async () => {
        host.publish("tests");
        await host.refresh.promise;
      });
      const targetB = within(await openWorktreeSheet()).getByRole("tab", { name: "editor" });
      expect(targetB).toBeEnabled();
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-tests");
      await act(async () => {
        fireEvent.click(targetB);
        await host.postB.promise;
        if (acceptedB) {
          host.responseB.resolve(jsonResponse({ accepted: true }));
          await host.responseB.promise;
        }
      });
      await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
      await act(async () => {
        if (outcome === "network-failure") host.rejectA();
        host.responseA.resolve(jsonResponse({}, outcome !== "http-failure"));
        await host.responseA.promise;
      });
      expect(targetB).toBeDisabled();
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-editor");
      expect(within(await openWorktreeSheet()).getByRole("tab", { name: "tests" })).toHaveAttribute("aria-selected", "true");
      if (!acceptedB) {
        // A's success must not accept B. Only B's own headers may start the
        // event-triggered refresh; a stale snapshot still cannot confirm B.
        await act(async () => { host.publish("editor"); });
        expect(host.readCount()).toBe(2);
        await act(async () => {
          host.responseB.resolve(jsonResponse({ accepted: true }));
          await host.responseB.promise;
          await host.confirmationRead.promise;
        });
        expect(host.readCount()).toBe(3);
        expect(targetB).toBeDisabled();
      }
      // B expires at its original start + 6000, not at either response + 6000.
      await act(async () => { await vi.advanceTimersByTimeAsync(3999); });
      expect(targetB).toBeDisabled();
      await act(async () => { await vi.advanceTimersByTimeAsync(1); });
      expect(targetB).toBeEnabled();
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-tests");
      host.unmount();
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      cleanup();
      vi.useRealTimers();
    }
  });
});

describe("Remote UI Components", () => {
  it("creates a terminal in an empty selected worktree and waits for desktop publication", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal("WebSocket", EventWebSocket);
    let snapshot: { activeContext: { workspaceId: string; worktreeSlug: string; worktreeLabel: string; activeTerminal: { sessionId: string; running: boolean } | null } } = { ...confirmedNoFocusState };
    const request = vi.fn<typeof fetch>(async (_input, init) => {
      if (init?.method === "POST") return jsonResponse({});
      return jsonResponse(snapshot);
    });
    vi.stubGlobal("fetch", ticketed(request));
    await act(async () => { render(<RemoteApp />); });
    await switchToTerminalMode();
    await openWorktreeSheet();
    const button = screen.getByRole("button", { name: "New terminal tab" });
    await act(async () => { fireEvent.click(button); });
    expect(request).toHaveBeenCalledWith(expect.stringContaining("/api/v1/workspace/select"), expect.objectContaining({
      method: "POST",
      headers: expect.objectContaining({ Authorization: "Bearer test-token" }),
      body: JSON.stringify({ workspaceId: "api-service", worktreeSlug: "feature/remote-safe", createTerminal: true }),
    }));
    expect(button).toBeDisabled();
    expect(screen.queryByTestId("remote-terminal")).toBeNull();
    snapshot = { ...confirmedNoFocusState, activeContext: { ...confirmedNoFocusState.activeContext, activeTerminal: { sessionId: "created-terminal", running: true } } };
    await act(async () => {
      eventSocket().onmessage?.(new MessageEvent("message", { data: JSON.stringify({ event: "remote_active_selection_changed", payload: snapshot.activeContext }) }));
    });
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "created-terminal");
    expect(button).toBeEnabled();
  });

  it("does not confirm creation from an unchanged terminal and reports rejected requests", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal("WebSocket", EventWebSocket);
    const response = deferred<Response>();
    const request = vi.fn<typeof fetch>(async (_input, init) => init?.method === "POST" ? response.promise : jsonResponse(focusedState));
    vi.stubGlobal("fetch", ticketed(request));
    await act(async () => { render(<RemoteApp />); });
    await switchToTerminalMode();
    await openWorktreeSheet();
    const button = screen.getByRole("button", { name: "New terminal tab" });
    await act(async () => { fireEvent.click(button); fireEvent.click(button); });
    expect(request.mock.calls.filter(([, init]) => init?.method === "POST")).toHaveLength(1);
    await act(async () => {
      eventSocket().onmessage?.(new MessageEvent("message", { data: JSON.stringify({ event: "remote_active_selection_changed", payload: focusedState.activeContext }) }));
    });
    expect(button).toBeDisabled();
    await act(async () => { response.resolve(jsonResponse({}, false)); });
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(button).toBeEnabled();
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "focused-terminal");
  });

  it("shrinks the mobile shell on visual viewport resize without a screen-height minimum", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const viewport = Object.assign(new EventTarget(), { height: 720 });
    vi.stubGlobal("visualViewport", viewport);
    vi.stubGlobal("fetch", ticketed(vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(focusedState))));
    await act(async () => { render(<RemoteApp />); });
    const shell = screen.getByLabelText("Current desktop context").closest("header")?.parentElement;
    expect(shell).toHaveStyle({ height: "720px" });
    act(() => { viewport.height = 320; viewport.dispatchEvent(new Event("resize")); });
    expect(shell).toHaveStyle({ height: "320px" });
    expect(shell).not.toHaveClass("min-h-screen");
  });

  it("keeps long workspace identifiers within the vertical worktree picker", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const workspaceId = "workspace-" + "unbroken".repeat(40);
    vi.stubGlobal("fetch", ticketed(vi.fn<typeof fetch>().mockResolvedValue(jsonResponse({
      activeContext: { workspaceId, activeTerminal: null },
      projects: [{ workspaceId, worktrees: [{ worktreeSlug: "feature", worktreeLabel: "long".repeat(100) }] }],
    }))));
    await act(async () => { render(<RemoteApp />); });
    fireEvent.click(screen.getByRole("button", { name: "Change workspace context" }));
    const dialog = screen.getByRole("dialog", { name: "Workspace context" });
    expect(dialog.querySelector(".overflow-y-auto")).toHaveClass("overflow-x-hidden", "min-h-0");
    expect(within(dialog).getByRole("heading", { name: workspaceId })).toHaveClass("min-w-0", "truncate");
    await openWorktreeSheet();
    expect(screen.getByRole("button", { name: "New terminal tab" })).toBeEnabled();
  });

  it("keeps the hook order stable when a session token is cleared", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal("WebSocket", EventWebSocket);
    vi.stubGlobal("fetch", ticketed(vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(focusedState))));
    await act(async () => { render(<RemoteApp />); });
    await switchToTerminalMode();
    expect(screen.getByTestId("remote-terminal")).toBeInTheDocument();
    // Disconnect drops the session token, so the same mount renders the login
    // screen; a hook declared below that early return changes the hook count.
    await openWorktreeSheet();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Machines" })); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Machines" })); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Remove pairing" })); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Confirm disconnect" })); });
    expect(screen.getByRole("heading", { name: "Sign In to Ferryx" })).toBeInTheDocument();
    expect(screen.queryByTestId("remote-terminal")).toBeNull();
  });

  it("PairingPage renders the Ferryx Desktop PIN flow", () => {
    render(<PairingPage onPaired={vi.fn()} />);

    const input = screen.getByPlaceholderText(/6-digit PIN/i);
    expect(input).toHaveAttribute("maxLength", "6");
    expect(screen.getByText(/Ferryx Desktop settings/i)).toBeInTheDocument();
  });

  it("renders the native active-only state as one mirrored terminal without exposing paths", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal(
      "fetch",
      ticketed(vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(currentNativeState))),
    );

    render(<RemoteApp />);
    await switchToTerminalMode();

    const terminal = await screen.findByTestId("remote-terminal");
    expect(terminal).toHaveAttribute("data-session-id", "focused-terminal");
    expect(screen.getAllByTestId("remote-terminal")).toHaveLength(1);
    expect(screen.getByLabelText("Current desktop context")).toHaveTextContent(
      "ferryx-ui / main",
    );
    expect(document.body).not.toHaveTextContent("/Users/alice/secret");
  });

  it("renders the current safe server workspace contract without local paths", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal(
      "fetch",
      ticketed(vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(safeServerWorkspaceState))),
    );

    render(<RemoteApp />);
    await switchToTerminalMode();

    expect(await screen.findByTestId("remote-terminal")).toHaveAttribute(
      "data-session-id",
      "focused-terminal",
    );
    expect(screen.getByLabelText("Current desktop context")).toHaveTextContent(
      "remote-e2e / mobile-control",
    );
    fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));
    const selector = screen.getByRole("dialog", { name: /Workspace context/i });
    expect(
      within(selector).getByRole("button", { name: /other-project.*other-worktree/i }),
    ).toBeEnabled();
    expect(document.body.textContent).not.toMatch(/\/(Users|private|Volumes)\//);
  });

  it("mirrors only the server-declared terminal and safely confirms a context selection", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const selectionResponse = deferred<Response>();
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(focusedState))
      .mockImplementationOnce(() => selectionResponse.promise)
      .mockResolvedValueOnce(jsonResponse(confirmedNoFocusState));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    const terminal = await screen.findByTestId("remote-terminal");
    expect(terminal).toHaveAttribute("data-session-id", "focused-terminal");
    expect(screen.getAllByTestId("remote-terminal")).toHaveLength(1);
    expect(screen.queryByText(/background-terminal|private-background|Hidden/i)).not.toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("/Users/alice/secret");
    expect(document.body).not.toHaveTextContent("/Volumes/private");

    expect(screen.getByLabelText("Current desktop context")).toHaveTextContent(
      "ferryx-ui / main",
    );
    fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));

    const selector = screen.getByRole("dialog", { name: /Workspace context/i });
    const target = within(selector).getByRole("button", {
      name: /api-service.*feature\/remote-safe/i,
    });
    fireEvent.click(target);

    // Picking a worktree dismisses the selector instead of leaving it stuck open.
    expect(screen.queryByRole("dialog", { name: /Workspace context/i })).toBeNull();
    // Scoped to the terminal contract: the selection POST is the 2nd of the workspace calls
    // (the first is the initial state read), whatever any other lane requests.
    expect(workspaceCalls(fetchMock)).toHaveLength(2);
    expect(workspaceCalls(fetchMock)[1]).toMatchObject({
      url: "/api/v1/workspace/select",
      init: expect.objectContaining({
        method: "POST",
        headers: { "Content-Type": "application/json", Authorization: "Bearer test-token" },
        body: JSON.stringify({
          workspaceId: "api-service",
          worktreeSlug: "feature/remote-safe",
        }),
      }),
    });

    await act(async () => {
      selectionResponse.resolve(jsonResponse({ accepted: true }));
      await selectionResponse.promise;
    });

    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "api-service",
              worktreeSlug: "feature/remote-safe",
            },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByLabelText("Current desktop context")).toHaveTextContent(
        "api-service / feature/remote-safe",
      );
    });
    expect(screen.queryByTestId("remote-terminal")).not.toBeInTheDocument();
    expect(screen.getByText("No focused terminal")).toBeInTheDocument();
    expect(screen.getByText(/mirror it here/i)).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("background-terminal");
  });

  it("waits for the desktop active-selection event when the first confirmation read is stale", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(focusedState))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(confirmedNoFocusState));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    await screen.findByTestId("remote-terminal");
    expect(eventSocket().url).toMatch(/\/api\/v1\/events\?ticket=ui-test-ticket$/);
    fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));
    const selector = screen.getByRole("dialog", { name: /Workspace context/i });
    fireEvent.click(
      within(selector).getByRole("button", {
        name: /api-service.*feature\/remote-safe/i,
      }),
    );

    // The selection POST is in flight; the desktop has not confirmed yet. Counted per endpoint,
    // so the assertion says exactly what it means: one state read and one selection POST.
    await waitFor(() => {
      expect(stateReads(fetchMock)).toHaveLength(1);
      expect(selectCalls(fetchMock)).toHaveLength(1);
    });

    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "api-service",
              worktreeSlug: "feature/remote-safe",
              sessionId: "new-focused-terminal",
            },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByLabelText("Current desktop context")).toHaveTextContent(
        "api-service / feature/remote-safe",
      );
    });
  });

  it("recovers from a desktop that never confirms so the picker stays usable", async () => {
    vi.useFakeTimers();
    try {
      localStorage.setItem("ferryx_remote_token", "test-token");
      // The desktop accepts the request over HTTP but never republishes a
      // matching selection, which is exactly what a stale/unreachable desktop
      // listener looks like from the phone.
      const fetchMock = vi
        .fn<typeof fetch>()
        .mockResolvedValueOnce(jsonResponse(focusedState))
        .mockResolvedValueOnce(jsonResponse({ accepted: true }))
        .mockResolvedValue(jsonResponse(focusedState));
      vi.stubGlobal("fetch", ticketed(fetchMock));
      vi.stubGlobal("WebSocket", EventWebSocket);
      onTestFailed(() => reportFetchOrder("recovers from a desktop that never confirms", fetchMock));

      render(<RemoteApp />);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));
      fireEvent.click(
        within(screen.getByRole("dialog", { name: /Workspace context/i })).getByRole("button", {
          name: /api-service.*feature\/remote-safe/i,
        }),
      );

      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/workspace/select"),
        expect.anything(),
      );

      // A selection that is never confirmed must not strand the UI forever:
      // the pending lock has to expire so the picker becomes usable again.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(10_000);
      });

      fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));
      expect(
        within(screen.getByRole("dialog", { name: /Workspace context/i })).getByRole("button", {
          name: /api-service.*feature\/remote-safe/i,
        }),
      ).toBeEnabled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("refreshes the mirrored terminal after an unsolicited desktop focus change", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(focusedState))
      .mockResolvedValueOnce(jsonResponse(secondFocusedState));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);
    onTestFailed(() => reportFetchOrder("refreshes the mirrored terminal on unsolicited focus", fetchMock));

    render(<RemoteApp />);
    await switchToTerminalMode();

    expect(await screen.findByTestId("remote-terminal")).toHaveAttribute(
      "data-session-id",
      "focused-terminal",
    );

    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "api-service",
              worktreeSlug: "feature/remote-safe",
              sessionId: "new-focused-terminal",
            },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
        "data-session-id",
        "new-focused-terminal",
      );
    });
    expect(screen.getByLabelText("Current desktop context")).toHaveTextContent(
      "api-service / feature/remote-safe",
    );
  });

  it("keeps the newest desktop focus when focus events arrive during a refresh", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const firstFocusRefresh = deferred<Response>();
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(focusedState))
      .mockImplementationOnce(() => firstFocusRefresh.promise)
      .mockResolvedValueOnce(jsonResponse(secondFocusedState));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    await screen.findByTestId("remote-terminal");
    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: { workspaceId: "ferryx-ui", worktreeSlug: "main" },
          }),
        }),
      );
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: { workspaceId: "api-service", worktreeSlug: "feature/remote-safe" },
          }),
        }),
      );
    });

    await act(async () => {
      firstFocusRefresh.resolve(jsonResponse(focusedState));
      await firstFocusRefresh.promise;
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
        "data-session-id",
        "new-focused-terminal",
      );
    });
  });

  it("clears the mirrored terminal when desktop no longer focuses a terminal", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(focusedState))
      .mockResolvedValueOnce(jsonResponse(confirmedNoFocusState));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    await screen.findByTestId("remote-terminal");
    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: null,
          }),
        }),
      );
    });

    expect(await screen.findByText("No focused terminal")).toBeInTheDocument();
    expect(screen.queryByTestId("remote-terminal")).not.toBeInTheDocument();
  });

  it("shows no focused terminal when only undeclared background sessions are present", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal(
      "fetch",
      ticketed(vi.fn<typeof fetch>().mockResolvedValue(
        jsonResponse({
          ...confirmedNoFocusState,
          sessions: [focusedState.sessions[1]],
        }),
      )),
    );

    render(<RemoteApp />);
    await switchToTerminalMode();

    expect(await screen.findByText("No focused terminal")).toBeInTheDocument();
    expect(screen.queryByTestId("remote-terminal")).not.toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("background-terminal");
    expect(document.body).not.toHaveTextContent("Hidden /Users/alice/secret shell");
  });

  it("MobileKeyDock dispatches primary key actions and latches modifiers", () => {
    const handleSendKey = vi.fn();
    render(<MobileKeyDock onSendKey={handleSendKey} />);

    fireEvent.click(screen.getByText("Ctrl-C"));
    expect(handleSendKey).toHaveBeenCalledWith("ctrl-c");

    fireEvent.click(screen.getByText("Ctrl"));
    fireEvent.click(screen.getByText("Tab"));
    expect(handleSendKey).toHaveBeenCalledWith("ctrl-tab");
  });

  it("retains legacy authentication without rendering old Orca branding", async () => {
    localStorage.setItem("rorca_remote_token", "legacy-token");
    vi.stubGlobal("fetch", ticketed(vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(confirmedNoFocusState))));

    render(<RemoteApp />);

    // Chat is the default surface and renders its own <header>, so the app shell's header is no
    // longer the only banner landmark. Every banner is checked, which is stricter than before.
    const banners = await screen.findAllByRole("banner");
    expect(banners.length).toBeGreaterThan(0);
    expect(banners.some((banner) => banner.textContent?.includes("Ferryx Remote"))).toBe(true);
    for (const banner of banners) {
      expect(banner.textContent?.toLowerCase()).not.toContain("orca");
    }
  });

  it("sets browser document.title to active tab or terminal title on initial authenticated load", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithTabs = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-2",
        terminalTabs: [
          { id: "tab-1", label: "Editor" },
          { id: "tab-2", label: "Dev Server" },
        ],
      },
    };
    vi.stubGlobal("fetch", ticketed(vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(stateWithTabs))));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    await waitFor(() => {
      expect(document.title).toBe("Dev Server - Ferryx");
    });
  });

  it("does not treat a worktree row id as a terminal tab id", () => {
    const parsed = normalizeRemoteWorkspaceState({
      projects: [
        {
          id: "workspace-1",
          worktrees: [{ id: "worktree-row-1", slug: "feature/fast-switch" }],
        },
      ],
    });

    expect(parsed.options).toContainEqual({
      workspaceId: "workspace-1",
      worktreeSlug: "feature/fast-switch",
      worktreeLabel: "feature/fast-switch",
    });
  });

  it("updates document.title on unsolicited desktop focus and active tab change and falls back to Ferryx", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const firstState = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor" },
          { id: "tab-2", label: "Dev Server" },
        ],
      },
    };
    const secondState = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-2",
        terminalTabs: [
          { id: "tab-1", label: "Editor" },
          { id: "tab-2", label: "Dev Server" },
        ],
      },
    };

    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(firstState))
      .mockResolvedValueOnce(jsonResponse(secondState))
      .mockResolvedValueOnce(jsonResponse(confirmedNoFocusState));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);

    const { unmount } = render(<RemoteApp />);

    await waitFor(() => {
      expect(document.title).toBe("Editor - Ferryx");
    });

    // Unsolicited desktop tab change
    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "ferryx-ui",
              worktreeSlug: "main",
              tabId: "tab-2",
            },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(document.title).toBe("Dev Server - Ferryx");
    });

    // Desktop unfocuses terminal/tab
    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: null,
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(document.title).toBe("Ferryx");
    });

    unmount();
    expect(document.title).toBe("Ferryx");
  });

  it("resets document.title to Ferryx when unpaired or disconnected", async () => {
    render(<RemoteApp />);
    expect(document.title).toBe("Ferryx");
  });

  it("allows sequential traversal using previous and next terminal tab controls and ordinal indicator", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const multiTabState1 = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-1",
        sessionId: "session-tab-1",
        activeTerminal: {
          sessionId: "session-tab-1",
          title: "Editor",
          running: true,
        },
        terminalTabs: [
          { id: "tab-1", label: "Editor" },
          { id: "tab-2", label: "Build" },
          { id: "tab-3", label: "Server" },
        ],
      },
      sessions: [
        { sessionId: "session-tab-1", title: "Editor", workspaceId: "ferryx-ui", worktreeLabel: "main", running: true },
        { sessionId: "session-tab-2", title: "Build", workspaceId: "ferryx-ui", worktreeLabel: "main", running: true },
        { sessionId: "session-tab-3", title: "Server", workspaceId: "ferryx-ui", worktreeLabel: "main", running: true },
      ],
    };
    const multiTabState2 = {
      ...multiTabState1,
      activeContext: {
        ...multiTabState1.activeContext,
        tabId: "tab-2",
        sessionId: "session-tab-2",
        activeTerminal: {
          sessionId: "session-tab-2",
          title: "Build",
          running: true,
        },
      },
    };
    const multiTabState3 = {
      ...multiTabState1,
      activeContext: {
        ...multiTabState1.activeContext,
        tabId: "tab-3",
        sessionId: "session-tab-3",
        activeTerminal: {
          sessionId: "session-tab-3",
          title: "Server",
          running: true,
        },
      },
    };

    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(multiTabState1))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(multiTabState2))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(multiTabState3))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(multiTabState2));

    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    const terminal = await screen.findByTestId("remote-terminal");
    expect(terminal).toHaveAttribute("data-session-id", "session-tab-1");

    // Ordinal indicator
    await openWorktreeSheet();
    expect(screen.getByText("1 / 3")).toBeInTheDocument();

    await openWorktreeSheet();
    const prevBtn = screen.getByRole("button", { name: /Previous terminal tab/i });
    await openWorktreeSheet();
    const nextBtn = screen.getByRole("button", { name: /Next terminal tab/i });
    expect(prevBtn).toBeDisabled();
    expect(nextBtn).toBeEnabled();

    // Traverse to next tab (tab-2)
    fireEvent.click(nextBtn);

    // The traversal's own selections, scoped to the terminal contract's endpoints so their
    // order is the order the test drove, not the order any other lane happened to request in.
    expect(selectCalls(fetchMock)[0]).toMatchObject({
      url: "/api/v1/workspace/select",
      init: expect.objectContaining({
        method: "POST",
        headers: { "Content-Type": "application/json", Authorization: "Bearer test-token" },
        body: JSON.stringify({
          workspaceId: "ferryx-ui",
          worktreeSlug: "main",
          tabId: "tab-2",
        }),
      }),
    });

    // Simulate desktop focus event
    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "ferryx-ui",
              worktreeSlug: "main",
              tabId: "tab-2",
              sessionId: "session-tab-2",
            },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
        "data-session-id",
        "session-tab-2",
      );
    });
    await openWorktreeSheet();
    expect(screen.getByText("2 / 3")).toBeInTheDocument();
    expect(prevBtn).toBeEnabled();
    expect(nextBtn).toBeEnabled();

    // Traverse to next tab (tab-3)
    fireEvent.click(nextBtn);

    expect(selectCalls(fetchMock)[1]).toMatchObject({
      url: "/api/v1/workspace/select",
      init: expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          workspaceId: "ferryx-ui",
          worktreeSlug: "main",
          tabId: "tab-3",
        }),
      }),
    });

    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "ferryx-ui",
              worktreeSlug: "main",
              tabId: "tab-3",
              sessionId: "session-tab-3",
            },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
        "data-session-id",
        "session-tab-3",
      );
    });
    expect(screen.getByText("3 / 3")).toBeInTheDocument();
    expect(prevBtn).toBeEnabled();
    expect(nextBtn).toBeDisabled();

    // Traverse back to previous tab (tab-2)
    fireEvent.click(prevBtn);

    expect(selectCalls(fetchMock)[2]).toMatchObject({
      url: "/api/v1/workspace/select",
      init: expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          workspaceId: "ferryx-ui",
          worktreeSlug: "main",
          tabId: "tab-2",
        }),
      }),
    });

    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "ferryx-ui",
              worktreeSlug: "main",
              tabId: "tab-2",
              sessionId: "session-tab-2",
            },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
        "data-session-id",
        "session-tab-2",
      );
    });
    await openWorktreeSheet();
    expect(screen.getByText("2 / 3")).toBeInTheDocument();
    expect(prevBtn).toBeEnabled();
    expect(nextBtn).toBeEnabled();
  });

  it("renders safe tab items under active worktree and dispatches tab switch request", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithTabs = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor" },
          { id: "tab-2", label: "Dev Server" },
          { id: "tab-3", label: "/Users/secret/path/run" },
        ],
      },
    };

    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(stateWithTabs));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    await screen.findByTestId("remote-terminal");

    const tablist = await openWorktreeSheet();
    expect(tablist).toBeInTheDocument();

    const editorTab = within(tablist).getByRole("tab", { name: /editor/i });
    const devServerTab = within(tablist).getByRole("tab", { name: /dev server/i });
    expect(editorTab).toHaveAttribute("aria-selected", "true");
    expect(devServerTab).toHaveAttribute("aria-selected", "false");

    expect(within(tablist).queryByText("/Users/secret/path/run")).not.toBeInTheDocument();
    expect(within(tablist).getByText("Terminal")).toBeInTheDocument();

    fireEvent.click(devServerTab);

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/workspace/select"),
        expect.objectContaining({
          method: "POST",
          body: JSON.stringify({
            workspaceId: "ferryx-ui",
            worktreeSlug: "main",
            tabId: "tab-2",
          }),
        }),
      );
    });
  });

  it("cycles published terminal tabs only after Desktop confirms the selected tab", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const editorState = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: {
          sessionId: "focused-terminal",
          title: "Editor",
          running: true,
        },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor" },
          { id: "tab-2", label: "Dev Server" },
          { id: "tab-3", label: "Tests" },
        ],
      },
    };
    const devServerState = {
      ...editorState,
      activeContext: {
        ...editorState.activeContext,
        activeTerminal: {
          sessionId: "dev-server-terminal",
          title: "Dev Server",
          running: true,
        },
        tabId: "tab-2",
      },
      sessions: [
        {
          sessionId: "dev-server-terminal",
          workspaceId: "ferryx-ui",
          worktreeLabel: "main",
          running: true,
        },
      ],
    };
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(editorState))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(devServerState))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(editorState));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    expect(await screen.findByTestId("remote-terminal")).toHaveAttribute(
      "data-session-id",
      "focused-terminal",
    );
    await openWorktreeSheet();
    expect(screen.getByLabelText("Terminal position: Tab 1 of 3")).toHaveTextContent("1 / 3");

    await openWorktreeSheet();
    fireEvent.click(screen.getByRole("button", { name: "Next terminal tab" }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/workspace/select"),
        expect.objectContaining({
          body: JSON.stringify({
            workspaceId: "ferryx-ui",
            worktreeSlug: "main",
            tabId: "tab-2",
          }),
        }),
      );
    });
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
      "data-session-id",
      "focused-terminal",
    );

    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: { workspaceId: "ferryx-ui", worktreeSlug: "main", tabId: "tab-2" },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
        "data-session-id",
        "dev-server-terminal",
      );
    });
    await openWorktreeSheet();
    expect(screen.getByLabelText("Terminal position: Tab 2 of 3")).toHaveTextContent("2 / 3");

    await openWorktreeSheet();
    fireEvent.click(screen.getByRole("button", { name: "Previous terminal tab" }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/workspace/select"),
        expect.objectContaining({
          body: JSON.stringify({
            workspaceId: "ferryx-ui",
            worktreeSlug: "main",
            tabId: "tab-1",
          }),
        }),
      );
    });

    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: { workspaceId: "ferryx-ui", worktreeSlug: "main", tabId: "tab-1" },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
        "data-session-id",
        "focused-terminal",
      );
    });
    await openWorktreeSheet();
    expect(screen.getByLabelText("Terminal position: Tab 1 of 3")).toHaveTextContent("1 / 3");
  });

  it("retains authorization across normal page reload when server returns transient error", async () => {
    localStorage.setItem("ferryx_remote_token", "paired-device-token");
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(jsonResponse({ error: "gateway busy" }, false));
    vi.stubGlobal("fetch", ticketed(fetchMock));

    render(<RemoteApp />);

    // Wait for the fetch attempt
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());

    // Authorization token must NOT be cleared from localStorage on non-401/403 failure
    expect(localStorage.getItem(`ferryx_remote_token_local:${window.location.origin}`)).toBe("paired-device-token");
    expect(screen.queryByPlaceholderText(/6-digit PIN/i)).not.toBeInTheDocument();
  });

  // Plan ordering constraint (.omo/plans/account-issued-remote-grants.md): browser account attach retired the PIN surface in favour of the account sign-in surface.
  it("returns to the account sign-in surface when the session token is revoked (401)", async () => {
    localStorage.setItem("ferryx_remote_token", "revoked-device-token");
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue({
      ok: false,
      status: 401,
      json: vi.fn(async () => ({ error: "Invalid or revoked token" })),
    } as unknown as Response);
    vi.stubGlobal("fetch", ticketed(fetchMock));

    render(<RemoteApp />);

    // Must navigate to account sign-in surface and clear token
    expect(await screen.findByRole("heading", { name: /Sign In to Ferryx/i })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("name@example.com")).toBeInTheDocument();
    expect(localStorage.getItem("ferryx_remote_token")).toBeNull();
  });

  it("normalizeRemoteWorkspaceState parses activityState, agentType, and attention and drops invalid values defensively", () => {
    const rawState = {
      activeWorkspaceId: "project-1",
      activeContext: {
        workspaceId: "project-1",
        worktreeSlug: "wt-main",
        worktreeLabel: "main",
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Claude Agent", activityState: "working", agentType: "claude" },
          { id: "tab-2", label: "Codex Agent", activityState: "waiting", agentType: "codex" },
          { id: "tab-3", label: "Omo Agent", activityState: "done", agentType: "omo" },
          { id: "tab-4", label: "Invalid State", activityState: "unknown_state", agentType: "/bin/sh" },
          { id: "tab-5", label: "Invalid State 2", activityState: "starting", agentType: "copilot" },
        ],
      },
      projects: [
        {
          workspaceId: "project-1",
          worktrees: [
            { worktreeSlug: "wt-main", worktreeLabel: "main", attention: "waiting" },
            { worktreeSlug: "wt-feature", worktreeLabel: "feature", attention: "working" },
            { worktreeSlug: "wt-done", worktreeLabel: "done-wt", attention: "done" },
            { worktreeSlug: "wt-invalid", worktreeLabel: "invalid-wt", attention: "unsupported" },
          ],
        },
      ],
      worktrees: [
        { worktreeSlug: "wt-main", worktreeLabel: "main", attention: "waiting" },
      ],
      sessions: [],
    };

    const model = normalizeRemoteWorkspaceState(rawState);
    const tabs = model.context.terminalTabs!;
    expect(tabs).toHaveLength(5);
    expect(tabs[0]).toEqual({ id: "tab-1", label: "Claude Agent", activityState: "working", agentType: "claude" });
    expect(tabs[1]).toEqual({ id: "tab-2", label: "Codex Agent", activityState: "waiting", agentType: "codex" });
    expect(tabs[2]).toEqual({ id: "tab-3", label: "Omo Agent", activityState: "done", agentType: "omo" });
    expect(tabs[3]).toEqual({ id: "tab-4", label: "Invalid State" });
    expect(tabs[4]).toEqual({ id: "tab-5", label: "Invalid State 2", agentType: "copilot" });

    const mainOpt = model.options.find((opt) => opt.worktreeSlug === "wt-main");
    expect(mainOpt?.attention).toBe("waiting");

    const featOpt = model.options.find((opt) => opt.worktreeSlug === "wt-feature");
    expect(featOpt?.attention).toBe("working");

    const doneOpt = model.options.find((opt) => opt.worktreeSlug === "wt-done");
    expect(doneOpt?.attention).toBe("done");

    const invalidOpt = model.options.find((opt) => opt.worktreeSlug === "wt-invalid");
    expect(invalidOpt?.attention).toBeUndefined();
  });

  it("tab strip renders state indicators for waiting and working tabs discoverable by accessible name", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithActivity = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", activityState: "working" },
          { id: "tab-2", label: "Dev Server", activityState: "waiting" },
          { id: "tab-3", label: "Tests" },
        ],
      },
    };
    vi.stubGlobal("fetch", ticketed(vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(stateWithActivity))));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    await screen.findByTestId("remote-terminal");

    const tablist = await openWorktreeSheet();
    expect(tablist).toBeInTheDocument();

    const workingTab = within(tablist).getByRole("tab", { name: /editor.*working/i });
    expect(workingTab).toBeInTheDocument();
    expect(within(workingTab).getByTestId("tab-working-indicator")).toBeInTheDocument();

    const waitingTab = within(tablist).getByRole("tab", { name: /dev server.*waiting/i });
    expect(waitingTab).toBeInTheDocument();
    expect(within(waitingTab).getByTestId("tab-waiting-indicator")).toBeInTheDocument();
  });

  it("lists a published terminal pane even when the desktop has nothing focused", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const inventoryWithoutFocus = {
      ...focusedState,
      activeContext: {
        workspaceId: "ferryx-ui",
        worktreeSlug: "main",
        worktreeLabel: "main",
        terminalTabs: [{ id: "tab-1", label: "Editor", worktreeSlug: "main", worktreeLabel: "main" }],
      },
    };
    vi.stubGlobal("fetch", ticketed(vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(inventoryWithoutFocus))));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    const tablist = await openWorktreeSheet();
    // One entry is enough to render the list, and no mirrored terminal is required to browse it.
    expect(within(tablist).getAllByRole("tab")).toHaveLength(1);
    expect(within(tablist).getByRole("tab", { name: /editor/i })).toBeInTheDocument();
    expect(screen.queryByTestId("remote-terminal")).not.toBeInTheDocument();
    expect(screen.getByText("No focused terminal")).toBeInTheDocument();
  });

  it("selects a pane from another worktree using that pane's own worktree", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const crossWorktreeState = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", worktreeSlug: "main", worktreeLabel: "main" },
          {
            id: "tab-2::leaf-b",
            label: "Build (2)",
            worktreeSlug: "feature/remote-safe",
            worktreeLabel: "feature/remote-safe",
          },
        ],
      },
    };
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(crossWorktreeState));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);

    const tablist = await openWorktreeSheet();
    fireEvent.click(within(tablist).getByRole("tab", { name: /build.*feature\/remote-safe/i }));

    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        "/api/v1/workspace/select",
        expect.objectContaining({
          method: "POST",
          // The pane's own worktree travels with the request; the mirrored context is not assumed.
          body: JSON.stringify({
            workspaceId: "ferryx-ui",
            worktreeSlug: "feature/remote-safe",
            tabId: "tab-2::leaf-b",
          }),
        }),
      ),
    );
  });

  it("tab strip renders brand logo image for supported agentType and fallback terminal icon for unknown/missing agentType", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithAgents = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Claude Agent", agentType: "claude" },
          { id: "tab-2", label: "Unknown Agent", agentType: "unsupported-tool" },
          { id: "tab-3", label: "Plain Terminal" },
        ],
      },
    };
    vi.stubGlobal("fetch", ticketed(vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(stateWithAgents))));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    await screen.findByTestId("remote-terminal");

    const tablist = await openWorktreeSheet();
    const claudeTab = within(tablist).getByRole("tab", { name: /claude agent/i });
    const unknownTab = within(tablist).getByRole("tab", { name: /unknown agent/i });
    const plainTab = within(tablist).getByRole("tab", { name: /plain terminal/i });

    // Claude tab has img with claude logo
    const claudeImg = within(claudeTab).getByTestId("tab-agent-icon");
    expect(claudeImg).toHaveAttribute("src", resolveAgentLogo("claude")!);
    expect(claudeImg).toHaveAttribute("data-agent-type", "claude");
    expect(within(claudeTab).queryByTestId("tab-terminal-icon")).not.toBeInTheDocument();

    // Unknown agent and plain tab do not have agent logo img, they have terminal fallback icon
    const unknownIcon = within(unknownTab).getByTestId("tab-terminal-icon");
    expect(unknownIcon).toBeInTheDocument();
    expect(within(unknownTab).queryByTestId("tab-agent-icon")).not.toBeInTheDocument();

    const plainIcon = within(plainTab).getByTestId("tab-terminal-icon");
    expect(plainIcon).toBeInTheDocument();
    expect(within(plainTab).queryByTestId("tab-agent-icon")).not.toBeInTheDocument();
  });

  it("context selector exposes worktree attention in its accessible name", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithAttention = {
      ...focusedState,
      projects: [
        {
          workspaceId: "ferryx-ui",
          worktrees: [
            { worktreeSlug: "main", worktreeLabel: "main", attention: "waiting" },
          ],
        },
        {
          workspaceId: "api-service",
          worktrees: [
            { worktreeSlug: "feature/remote-safe", worktreeLabel: "feature/remote-safe", attention: "working" },
          ],
        },
      ],
    };
    vi.stubGlobal("fetch", ticketed(vi.fn<typeof fetch>().mockResolvedValue(jsonResponse(stateWithAttention))));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    await screen.findByTestId("remote-terminal");

    fireEvent.click(screen.getByRole("button", { name: /Change workspace context/i }));
    const selector = screen.getByRole("dialog", { name: /Workspace context/i });

    // The worktree with waiting attention exposes "waiting" in its button accessible name
    const waitingOption = within(selector).getByRole("button", {
      name: /ferryx-ui.*main.*waiting/i,
    });
    expect(waitingOption).toBeInTheDocument();

    const workingOption = within(selector).getByRole("button", {
      name: /api-service.*feature\/remote-safe.*working/i,
    });
    expect(workingOption).toBeInTheDocument();
  });

  it("immediately remounts RemoteTerminal to session when selecting a tab with sessionId before confirmation", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const selectionResponse = deferred<Response>();
    const stateWithSessions = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: {
          sessionId: "session-editor",
          title: "Editor",
          running: true,
        },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
        ],
      },
    };
    const switchedState = {
      ...stateWithSessions,
      activeContext: {
        ...stateWithSessions.activeContext,
        activeTerminal: {
          sessionId: "session-dev",
          title: "Dev Server",
          running: true,
        },
        tabId: "tab-2",
      },
    };
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithSessions))
      .mockImplementationOnce(() => selectionResponse.promise)
      .mockResolvedValueOnce(jsonResponse(switchedState));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    const terminal = await screen.findByTestId("remote-terminal");
    expect(terminal).toHaveAttribute("data-session-id", "session-editor");

    const tablist = await openWorktreeSheet();
    const devTab = within(tablist).getByRole("tab", { name: /dev server/i });

    // Click dev server tab
    fireEvent.click(devTab);

    const optimisticTerminal = screen.getByTestId("remote-terminal");
    expect(optimisticTerminal).toHaveAttribute("data-session-id", "session-dev");
    const optimisticInstanceId = optimisticTerminal.getAttribute("data-instance-id");
    fireEvent.doubleClick(optimisticTerminal);

    // Tab buttons should be disabled during pending selection
    expect(devTab).toBeDisabled();

    // Resolve POST request
    await act(async () => {
      selectionResponse.resolve(jsonResponse({ accepted: true }));
      await selectionResponse.promise;
    });

    // RemoteTerminal is still on session-dev
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-dev");

    // Desktop confirms selection via WebSocket event
    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "ferryx-ui",
              worktreeSlug: "main",
              tabId: "tab-2",
            },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(devTab).toBeEnabled();
    });
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-dev");
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
      "data-instance-id",
      optimisticInstanceId,
    );
  });

  it("remounts the optimistic terminal after confirmation when its socket closed before opening", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithSessions = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: { sessionId: "session-editor", running: true },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
        ],
      },
    };
    const switchedState = {
      ...stateWithSessions,
      activeContext: {
        ...stateWithSessions.activeContext,
        activeTerminal: { sessionId: "session-dev", running: true },
        tabId: "tab-2",
      },
    };
    vi.stubGlobal("fetch", ticketed(vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithSessions))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(switchedState))));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();
    await screen.findByTestId("remote-terminal");
    fireEvent.click(within(await openWorktreeSheet()).getByRole("tab", { name: /dev server/i }));

    const optimisticTerminal = screen.getByTestId("remote-terminal");
    const optimisticInstanceId = optimisticTerminal.getAttribute("data-instance-id");
    fireEvent.click(optimisticTerminal);

    act(() => {
      eventSocket().onmessage?.(new MessageEvent("message", {
        data: JSON.stringify({
          event: "remote_active_selection_changed",
          payload: { workspaceId: "ferryx-ui", worktreeSlug: "main", tabId: "tab-2" },
        }),
      }));
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).not.toHaveAttribute(
        "data-instance-id",
        optimisticInstanceId,
      );
    });
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-dev");
  });

  it("remounts on confirmation when the optimistic socket opened and then closed", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithSessions = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: { sessionId: "session-editor", running: true },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
        ],
      },
    };
    const switchedState = {
      ...stateWithSessions,
      activeContext: {
        ...stateWithSessions.activeContext,
        activeTerminal: { sessionId: "session-dev", running: true },
        tabId: "tab-2",
      },
    };
    vi.stubGlobal("fetch", ticketed(vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithSessions))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(switchedState))));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();
    await screen.findByTestId("remote-terminal");
    fireEvent.click(within(await openWorktreeSheet()).getByRole("tab", { name: /dev server/i }));

    const optimisticTerminal = screen.getByTestId("remote-terminal");
    const optimisticInstanceId = optimisticTerminal.getAttribute("data-instance-id");
    fireEvent.doubleClick(optimisticTerminal);
    fireEvent.click(optimisticTerminal);

    act(() => {
      eventSocket().onmessage?.(new MessageEvent("message", {
        data: JSON.stringify({
          event: "remote_active_selection_changed",
          payload: { workspaceId: "ferryx-ui", worktreeSlug: "main", tabId: "tab-2" },
        }),
      }));
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).not.toHaveAttribute(
        "data-instance-id",
        optimisticInstanceId,
      );
    });
  });

  it("remounts when the optimistic socket reports its failed handshake after confirmation", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithSessions = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: { sessionId: "session-editor", running: true },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
        ],
      },
    };
    const switchedState = {
      ...stateWithSessions,
      activeContext: {
        ...stateWithSessions.activeContext,
        activeTerminal: { sessionId: "session-dev", running: true },
        tabId: "tab-2",
      },
    };
    vi.stubGlobal("fetch", ticketed(vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithSessions))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(switchedState))));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();
    await screen.findByTestId("remote-terminal");
    fireEvent.click(within(await openWorktreeSheet()).getByRole("tab", { name: /dev server/i }));

    const optimisticTerminal = screen.getByTestId("remote-terminal");
    const optimisticInstanceId = optimisticTerminal.getAttribute("data-instance-id");
    act(() => {
      eventSocket().onmessage?.(new MessageEvent("message", {
        data: JSON.stringify({
          event: "remote_active_selection_changed",
          payload: { workspaceId: "ferryx-ui", worktreeSlug: "main", tabId: "tab-2" },
        }),
      }));
    });

    const devServerPane = within(await openWorktreeSheet()).getByRole("tab", { name: /dev server/i });
    await waitFor(() => {
      expect(devServerPane).toBeEnabled();
    });
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
      "data-instance-id",
      optimisticInstanceId,
    );

    fireEvent.click(screen.getByTestId("remote-terminal"));
    expect(screen.getByTestId("remote-terminal")).not.toHaveAttribute(
      "data-instance-id",
      optimisticInstanceId,
    );
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-dev");
  });

  it("does not retry again when the confirmed replacement socket also closes", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithSessions = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: { sessionId: "session-editor", running: true },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
        ],
      },
    };
    const switchedState = {
      ...stateWithSessions,
      activeContext: {
        ...stateWithSessions.activeContext,
        activeTerminal: { sessionId: "session-dev", running: true },
        tabId: "tab-2",
      },
    };
    vi.stubGlobal("fetch", ticketed(vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithSessions))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(switchedState))));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();
    await screen.findByTestId("remote-terminal");
    fireEvent.click(within(await openWorktreeSheet()).getByRole("tab", { name: /dev server/i }));
    const firstInstanceId = screen.getByTestId("remote-terminal").getAttribute("data-instance-id");

    act(() => {
      eventSocket().onmessage?.(new MessageEvent("message", {
        data: JSON.stringify({
          event: "remote_active_selection_changed",
          payload: { workspaceId: "ferryx-ui", worktreeSlug: "main", tabId: "tab-2" },
        }),
      }));
    });
    const devServerPaneInList = within(await openWorktreeSheet()).getByRole("tab", { name: /dev server/i });
    await waitFor(() => expect(devServerPaneInList).toBeEnabled());

    fireEvent.click(screen.getByTestId("remote-terminal"));
    const replacement = screen.getByTestId("remote-terminal");
    expect(replacement).not.toHaveAttribute("data-instance-id", firstInstanceId);
    const replacementInstanceId = replacement.getAttribute("data-instance-id");

    fireEvent.click(replacement);
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
      "data-instance-id",
      replacementInstanceId,
    );
  });

  it("does not perform immediate post-POST workspace/state refresh on selection", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithSessions = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
        ],
      },
    };
    const switchedState = {
      ...stateWithSessions,
      activeContext: {
        ...stateWithSessions.activeContext,
        tabId: "tab-2",
        activeTerminal: {
          sessionId: "session-dev",
          running: true,
        },
      },
    };
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithSessions))
      .mockResolvedValueOnce(jsonResponse({ accepted: true }))
      .mockResolvedValueOnce(jsonResponse(switchedState));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    await screen.findByTestId("remote-terminal");
    expect(stateReads(fetchMock)).toHaveLength(1);
    expect(selectCalls(fetchMock)).toHaveLength(0);

    const tablist = await openWorktreeSheet();
    fireEvent.click(within(tablist).getByRole("tab", { name: /dev server/i }));

    // Wait for POST to complete
    await waitFor(() => {
      expect(selectCalls(fetchMock)).toHaveLength(1);
    });
    expect(selectCalls(fetchMock)[0].url).toContain("/api/v1/workspace/select");

    // Crucial check: selection flow must NOT immediately call /api/v1/workspace/state. The
    // counter is scoped to that endpoint, so no other lane's request can mask a stray read.
    expect(stateReads(fetchMock)).toHaveLength(1);

    // Desktop confirms selection via WebSocket event
    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "ferryx-ui",
              worktreeSlug: "main",
              tabId: "tab-2",
            },
          }),
        }),
      );
    });

    // Now confirmation fetch is triggered
    await waitFor(() => {
      expect(stateReads(fetchMock)).toHaveLength(2);
    });
    // The confirmation refresh is authenticated, so it carries a bearer header.
    expect(stateReads(fetchMock)[1].init).toMatchObject({
      headers: { Authorization: "Bearer test-token" },
    });
  });

  it("clears optimistic session override and reverts when confirmation times out", async () => {
    vi.useFakeTimers();
    try {
      localStorage.setItem("ferryx_remote_token", "test-token");
      const stateWithSessions = {
        ...focusedState,
        activeContext: {
          ...focusedState.activeContext,
          activeTerminal: {
            sessionId: "session-editor",
            title: "Editor",
            running: true,
          },
          tabId: "tab-1",
          terminalTabs: [
            { id: "tab-1", label: "Editor", sessionId: "session-editor" },
            { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
          ],
        },
      };
      const fetchMock = vi
        .fn<typeof fetch>()
        .mockResolvedValueOnce(jsonResponse(stateWithSessions))
        .mockResolvedValueOnce(jsonResponse({ accepted: true }));
      vi.stubGlobal("fetch", ticketed(fetchMock));
      vi.stubGlobal("WebSocket", EventWebSocket);

      render(<RemoteApp />);
    await switchToTerminalMode();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });

      const tablist = await openWorktreeSheet();
      fireEvent.click(within(tablist).getByRole("tab", { name: /dev server/i }));

      // Optimistically shows session-dev
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-dev");

      // Advance past confirmation timeout (6000ms)
      await act(async () => {
        await vi.advanceTimersByTimeAsync(7000);
      });

      // Optimistic override should clear and revert to authoritative session-editor
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-editor");
      expect(within(tablist).getByRole("tab", { name: /dev server/i })).toBeEnabled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("clears optimistic session override when selection request fails", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const stateWithSessions = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: {
          sessionId: "session-editor",
          title: "Editor",
          running: true,
        },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
        ],
      },
    };
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithSessions))
      .mockResolvedValueOnce(jsonResponse({ error: "gateway busy" }, false));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    const terminal = await screen.findByTestId("remote-terminal");
    expect(terminal).toHaveAttribute("data-session-id", "session-editor");

    const tablist = await openWorktreeSheet();
    const devTab = within(tablist).getByRole("tab", { name: /dev server/i });

    fireEvent.click(devTab);

    // After failure resolves, optimistic override reverts and lock is released
    await waitFor(() => {
      expect(devTab).toBeEnabled();
    });
    expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-editor");
  });

  // Plan ordering constraint (.omo/plans/account-issued-remote-grants.md): browser account attach retired the PIN surface in favour of the account sign-in surface.
  it("clears optimistic session override and returns to account sign-in when user disconnects", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const selectionResponse = deferred<Response>();
    const stateWithSessions = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: {
          sessionId: "session-editor",
          title: "Editor",
          running: true,
        },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
        ],
      },
    };
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithSessions))
      .mockImplementationOnce(() => selectionResponse.promise);
    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    await screen.findByTestId("remote-terminal");

    const tablist = await openWorktreeSheet();
    fireEvent.click(within(tablist).getByRole("tab", { name: /dev server/i }));

    expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-dev");

    // Disconnect now lives in the Machines drawer, behind a confirmation step.
    fireEvent.click(screen.getByRole("button", { name: "Machines" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove pairing" }));

    expect(screen.queryByRole("heading", { name: /Sign In to Ferryx/i })).not.toBeInTheDocument();
    expect(screen.getByTestId("remote-terminal")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Confirm disconnect" }));

    expect(screen.queryByTestId("remote-terminal")).not.toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: /Sign In to Ferryx/i })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("name@example.com")).toBeInTheDocument();
  });

  it("requires confirmation before Disconnect removes the pairing", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(focusedState));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    await screen.findByTestId("remote-terminal");

    await openWorktreeSheet();
    fireEvent.click(screen.getByRole("button", { name: "Machines" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove pairing" }));

    expect(screen.queryByPlaceholderText(/6-digit PIN/i)).not.toBeInTheDocument();
    expect(screen.getByTestId("remote-terminal")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Confirm disconnect" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(screen.getByRole("button", { name: "Remove pairing" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Confirm disconnect" })).not.toBeInTheDocument();
    expect(screen.queryByPlaceholderText(/6-digit PIN/i)).not.toBeInTheDocument();
    expect(screen.getByTestId("remote-terminal")).toBeInTheDocument();
  });

  it("Cancel keeps the remote session paired", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(focusedState));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    await screen.findByTestId("remote-terminal");

    await openWorktreeSheet();
    fireEvent.click(screen.getByRole("button", { name: "Machines" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove pairing" }));
    expect(screen.getByRole("button", { name: "Confirm disconnect" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(screen.queryByPlaceholderText(/6-digit PIN/i)).not.toBeInTheDocument();
    expect(screen.getByTestId("remote-terminal")).toBeInTheDocument();
  });

  it("clears optimistic session override when a different authoritative state arrives", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    const selectionResponse = deferred<Response>();
    const stateWithThreeTabs = {
      ...focusedState,
      activeContext: {
        ...focusedState.activeContext,
        activeTerminal: {
          sessionId: "session-editor",
          title: "Editor",
          running: true,
        },
        tabId: "tab-1",
        terminalTabs: [
          { id: "tab-1", label: "Editor", sessionId: "session-editor" },
          { id: "tab-2", label: "Dev Server", sessionId: "session-dev" },
          { id: "tab-3", label: "Tests", sessionId: "session-tests" },
        ],
      },
    };
    const differentAuthoritativeState = {
      ...stateWithThreeTabs,
      activeContext: {
        ...stateWithThreeTabs.activeContext,
        activeTerminal: {
          sessionId: "session-tests",
          title: "Tests",
          running: true,
        },
        tabId: "tab-3",
      },
    };
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(jsonResponse(stateWithThreeTabs))
      .mockImplementationOnce(() => selectionResponse.promise)
      .mockResolvedValueOnce(jsonResponse(differentAuthoritativeState));
    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);

    render(<RemoteApp />);
    await switchToTerminalMode();

    await screen.findByTestId("remote-terminal");

    const tablist = await openWorktreeSheet();
    // User requested tab-2 (session-dev)
    fireEvent.click(within(tablist).getByRole("tab", { name: /dev server/i }));

    expect(screen.getByTestId("remote-terminal")).toHaveAttribute("data-session-id", "session-dev");

    // Desktop unexpectedly switches to tab-3 (session-tests) instead
    act(() => {
      eventSocket().onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            event: "remote_active_selection_changed",
            payload: {
              workspaceId: "ferryx-ui",
              worktreeSlug: "main",
              tabId: "tab-3",
            },
          }),
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByTestId("remote-terminal")).toHaveAttribute(
        "data-session-id",
        "session-tests",
      );
    });
  });

  it("prioritizes account magic-link consumption at /login?code=... over saved active host, device token, and stored account token", async () => {
    const pageOrigin = window.location.origin;
    const savedHostOrigin = "https://saved-machine.example.com";
    const savedHostId = "https://saved-machine.example.com/host/m-123";

    // Seed saved remote host store with an active host and device token on a distinct origin
    remoteHostStore.reset();
    remoteHostStore.upsertHost({
      hostId: savedHostId,
      name: "Saved Remote Machine",
      address: savedHostOrigin,
      relayOrigin: savedHostOrigin,
      machineId: "m-123",
      transport: "relay",
      authStatus: "paired",
      online: true,
      deviceToken: "saved-device-token-abc",
    });
    remoteHostStore.setActiveHost(savedHostId);

    // Seed device token and account token via storage helpers
    setRemoteAuthToken("saved-device-token-abc", savedHostId);
    storeAccountSessionToken("existing-account-token", pageOrigin);

    // Setup URL navigation to /login?code=<hex>
    const magicCode = "e1f2a3b4c5d6e7f8091a2b3c4d5e6f70";
    const originalLocation = window.location;
    delete (window as any).location;
    window.location = {
      ...originalLocation,
      pathname: "/login",
      search: `?code=${magicCode}`,
      hash: "",
    } as any;

    const consumeDeferred = deferred<Response>();
    const interceptedRequests: Array<{ url: string; method?: string; body?: any }> = [];

    const fetchMock = vi.fn<typeof fetch>(async (input, init) => {
      const url = String(input instanceof Request ? input.url : input);
      let body: any = null;
      if (init?.body && typeof init.body === "string") {
        try {
          body = JSON.parse(init.body);
        } catch {
          body = init.body;
        }
      }
      interceptedRequests.push({ url, method: init?.method, body });

      if (url.includes("/api/account/v1/login/consume")) {
        return consumeDeferred.promise;
      }
      if (url.includes("/api/account/v1/machines")) {
        return jsonResponse([]);
      }
      if (url.includes("/api/v1/workspace/state")) {
        return jsonResponse(focusedState);
      }
      return jsonResponse({});
    });

    vi.stubGlobal("fetch", ticketed(fetchMock));
    vi.stubGlobal("WebSocket", EventWebSocket);

    try {
      render(<RemoteApp />);
    await switchToTerminalMode();

      // RemoteApp must prioritize /login?code=... at page origin and mount AccountLoginPage
      // while consume promise is pending:
      // 1. AccountLoginPage sign-in surface is rendered, NOT terminal or device session
      expect(screen.queryByTestId("remote-terminal")).toBeNull();
      expect(screen.getByRole("heading", { name: /sign in to ferryx/i })).not.toBeNull();

      // 2. Consume request is sent to page origin with intercepted code
      await waitFor(() => {
        const consumeReq = interceptedRequests.find((r) =>
          r.url.includes("/api/account/v1/login/consume")
        );
        expect(consumeReq).toBeDefined();
        expect(consumeReq?.url).toContain(pageOrigin);
        expect(consumeReq?.body).toMatchObject({ code: magicCode });
      });

      // 3. No old-host workspace state request was issued while prioritizing magic-link consume
      const stateReq = interceptedRequests.find((r) => r.url.includes("/api/v1/workspace/state"));
      expect(stateReq).toBeUndefined();

      // 4. Deferred consume avoids premature terminal transition while still pending
      expect(screen.queryByTestId("remote-terminal")).toBeNull();

      // 5. Complete consumption: resolves token and transitions to AccountMachinesPage
      // without jumping back to saved host or permanently overriding host switching
      await act(async () => {
        consumeDeferred.resolve(
          jsonResponse({
            token: "jwt-session-token-new",
            accountId: "acc-new",
            email: "new-user@example.com",
          })
        );
      });

      // Verify top workspace context trigger is present and magic link override is released
      expect(
        await screen.findByRole("button", { name: /change workspace context/i })
      ).not.toBeNull();

      // 6. User switches to saved host: must now issue workspace state request to saved origin
      act(() => {
        remoteHostStore.setActiveHost(savedHostId);
      });

      await waitFor(() => {
        const savedStateReq = interceptedRequests.find(
          (r) => r.url.includes("/api/v1/workspace/state") && r.url.startsWith(savedHostOrigin)
        );
        expect(savedStateReq).toBeDefined();
      });
    } finally {
      remoteHostStore.reset();
      clearStoredAccountSessionToken();
      clearRemoteAuthToken(savedHostId);
      Object.defineProperty(window, "location", {
        configurable: true,
        value: originalLocation,
      });
    }
  });
});
