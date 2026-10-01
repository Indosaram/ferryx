import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { RemoteApp } from "./RemoteApp";
import {
  PLAN_LIMIT_REACHED,
  REMOTE_SUSPENDED,
  REMOTE_SUSPENDED_CLOSE_REASON,
  clearStoredAccountEntitlementSnapshot,
  storeAccountEntitlementSnapshot,
  storeAccountSessionToken,
} from "./accountSession";
import { useAccountWorktrees } from "./useAccountWorktrees";
import type {
  TunnelTransport,
  TunnelResponse,
  TunnelWebSocket,
  TunnelMessageEvent,
  TunnelCloseEvent,
  TunnelErrorEvent,
  FetchLikeInit,
} from "./attachTunnel";
import * as attachTunnelModule from "./attachTunnel";
import * as accountAttachModule from "./accountAttach";
import * as accountSessionModule from "./accountSession";
import { remoteHostStore } from "../state/remoteHostStore";

vi.mock("./RemoteTerminal", () => ({
  RemoteTerminal: ({ sessionId }: { sessionId: string }) => (
    <div data-testid="remote-session">{sessionId}</div>
  ),
}));

const AWAIT_DEADLINE_MS = 2000;
// Captured before any test installs fake timers so a subscription deadline still fires
// while the retry clock is faked; bare setTimeout here would hang those waits instead.
const realSetTimeout = globalThis.setTimeout;
const realClearTimeout = globalThis.clearTimeout;

interface MockTunnelWebSocket extends TunnelWebSocket {
  send: ReturnType<typeof vi.fn>;
  close: ReturnType<typeof vi.fn>;
}

function createSocket(onCloseArmed?: () => void): MockTunnelWebSocket {
  let userOnClose: ((event: TunnelCloseEvent) => void) | null = null;
  const socket: MockTunnelWebSocket = {
    readyState: 1,
    binaryType: "arraybuffer",
    send: vi.fn(),
    close: vi.fn(),
    onopen: null,
    get onclose() {
      return userOnClose;
    },
    set onclose(handler: ((event: TunnelCloseEvent) => void) | null) {
      userOnClose = handler;
      if (typeof socket.onclose === "function" && onCloseArmed) {
        onCloseArmed();
      }
    },
    onmessage: null,
    onerror: null,
  };
  return socket;
}

const GRACE_ENDS_AT = 1735689600;
const STOPPED_AT = 1735776000;

function machineFixture(index: number) {
  return {
    machineRecordId: `rec-phone-${index}`,
    machineId: `mach-phone-${index}`,
    displayName: `Phone ${index}`,
    publicKey: `phone-pub-${index}`,
    attachPublicKey: `phone-attach-pub-${index}`,
    relayOrigin: window.location.origin,
    platform: "darwin",
    online: true,
    enrollmentEpoch: "1",
    lastSeenAt: Date.now(),
  };
}

function worktreeSlugFor(machineId: string): string {
  return machineId === "mach-phone-1" ? "main" : "feature";
}

function grantBody(machineId: string, grantId: string) {
  return {
    grantId,
    machineId,
    relayOrigin: window.location.origin,
    pairingToken: `pair-${machineId}`,
    machineAttachPublicKey: `attach-key-${machineId}`,
    grantScope: "machine",
    expiresAt: Date.now() + 600000,
  };
}

function workspaceStateFor(machineId: string) {
  const slug = worktreeSlugFor(machineId);
  const sessionId = `term-${machineId}`;
  return {
    projects: [
      {
        workspaceId: "ws-ferryx",
        repoRoot: "/Users/dev/ferryx",
        worktrees: [{ slug, label: slug }],
      },
    ],
    activeContext: {
      workspaceId: "ws-ferryx",
      worktreeSlug: slug,
      worktreeLabel: slug,
      sessionId,
      activeTerminal: { sessionId, title: "zsh", running: true },
    },
    sessions: [{ sessionId, running: true, title: "zsh", workspaceId: "ws-ferryx" }],
  };
}

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (err: unknown) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (err: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

interface CounterSignal {
  bump(): void;
  count(): number;
  atLeast(n: number): Promise<void>;
}

function createCounterSignal(label: string): CounterSignal {
  let currentCount = 0;
  const subscribers: Array<() => void> = [];

  return {
    bump() {
      currentCount += 1;
      for (const subscriber of [...subscribers]) {
        subscriber();
      }
    },
    count() {
      return currentCount;
    },
    atLeast(n: number): Promise<void> {
      if (currentCount >= n) {
        return Promise.resolve();
      }

      return new Promise<void>((resolve, reject) => {
        let timer: ReturnType<typeof realSetTimeout> | null = null;

        const cleanup = () => {
          if (timer !== null) {
            realClearTimeout(timer);
            timer = null;
          }
          const index = subscribers.indexOf(onBump);
          if (index !== -1) {
            subscribers.splice(index, 1);
          }
        };

        const onBump = () => {
          if (currentCount >= n) {
            cleanup();
            resolve();
          }
        };

        subscribers.push(onBump);

        timer = realSetTimeout(() => {
          cleanup();
          reject(new Error(`Timed out waiting for counter "${label}" to reach at least ${n} (current: ${currentCount})`));
        }, AWAIT_DEADLINE_MS);
      });
    },
  };
}

function jsonBody(payload: unknown): TunnelResponse {
  return {
    status: 200,
    headers: { "content-type": "application/json" },
    body: new TextEncoder().encode(JSON.stringify(payload)),
  };
}

function awaitDomTransition<T>(
  query: () => T | null,
  description: string,
  timeoutMs = AWAIT_DEADLINE_MS,
): Promise<T> {
  const initial = query();
  if (initial !== null && initial !== undefined) {
    return Promise.resolve(initial);
  }

  return new Promise<T>((resolve, reject) => {
    let timer: ReturnType<typeof realSetTimeout> | null = null;
    let observer: MutationObserver | null = null;

    const cleanup = () => {
      if (timer !== null) {
        realClearTimeout(timer);
        timer = null;
      }
      if (observer !== null) {
        observer.disconnect();
        observer = null;
      }
    };

    const check = () => {
      try {
        const val = query();
        if (val !== null && val !== undefined) {
          cleanup();
          resolve(val);
          return true;
        }
      } catch (err) {
        cleanup();
        reject(err);
        return true;
      }
      return false;
    };

    observer = new MutationObserver(() => {
      check();
    });

    observer.observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      characterData: true,
    });

    timer = realSetTimeout(() => {
      cleanup();
      reject(new Error(`Timed out after ${timeoutMs}ms waiting for ${description}`));
    }, timeoutMs);
  });
}

interface HarnessOptions {
  machines?: ReturnType<typeof machineFixture>[];
  grantResponse?: (machineId: string) => Response | Promise<Response> | null;
  machinesResponse?: (callIndex: number) => Response | Promise<Response> | null;
}

function setupHarness(options: HarnessOptions = {}) {
  const machines = options.machines ?? [machineFixture(1)];
  const tunnels: Array<{
    machineId: string;
    close: ReturnType<typeof vi.fn>;
    websocketPaths: string[];
  }> = [];
  const eventSockets: MockTunnelWebSocket[] = [];
  const armedEventSockets: MockTunnelWebSocket[] = [];
  const selectBodies: string[] = [];
  const tunnelOpens = createCounterSignal("openAccountTunnel");
  const attachSessions = createCounterSignal("/api/v1/attach/session");
  const eventsSockets = createCounterSignal("/api/v1/events socket");
  const armedEventsSockets = createCounterSignal("/api/v1/events socket onclose armed");
  const machinesLists = createCounterSignal("/api/account/v1/machines");
  const selectPosts = createCounterSignal("/api/v1/workspace/select");

  globalThis.fetch = vi.fn().mockImplementation(async (input: RequestInfo | URL) => {
    const url = typeof input === "string" ? input : input.toString();
    if (url.endsWith("/api/account/v1/machines")) {
      machinesLists.bump();
      const custom = options.machinesResponse?.(machinesLists.count());
      if (custom) return custom;
      return new Response(JSON.stringify(machines), { status: 200 });
    }
    if (url.includes("/grants")) {
      const machineId =
        machines.find((machine) => url.includes(machine.machineRecordId))?.machineId ??
        machines[0].machineId;
      const refusal = options.grantResponse?.(machineId);
      if (refusal) return refusal;
      return new Response(JSON.stringify(grantBody(machineId, `grant-${machineId}`)), {
        status: 200,
      });
    }
    if (url.endsWith("/api/v1/attach/session")) {
      attachSessions.bump();
      return new Response(JSON.stringify({ sessionId: `sess-alloc-${attachSessions.count()}` }), {
        status: 200,
      });
    }
    return new Response("Not Found", { status: 404 });
  });

  const makeTunnel = (machineId: string) => {
    const websocketPaths: string[] = [];
    const state = workspaceStateFor(machineId);
    const transport: TunnelTransport = {
      fetchLike: vi.fn(async (path: string, init?: FetchLikeInit): Promise<TunnelResponse> => {
        if (path.startsWith("/api/v1/pair/exchange")) {
          return jsonBody({
            token: `device-token-${machineId}`,
            device: { id: `dev-${machineId}`, name: "Phone" },
            machineId,
            displayName: `Machine ${machineId}`,
          });
        }
        if (path.startsWith("/api/v1/workspace/state")) return jsonBody(state);
        if (path.startsWith("/api/v1/sessions")) {
          const slug = worktreeSlugFor(machineId);
          const sessionId = `term-${machineId}`;
          return jsonBody({
            revision: "1",
            completeness: "complete",
            sessions: [
              {
                sessionId,
                title: "zsh",
                workspaceId: "ws-ferryx",
                worktree: { wsId: "ws-ferryx", slug },
                target: { machineId, sessionId, daemonEpoch: "1790742255752" },
                running: true,
              },
            ],
          });
        }
        if (path.startsWith("/api/v1/workspace/select")) {
          selectBodies.push(typeof init?.body === "string" ? init.body : "");
          selectPosts.bump();
          return jsonBody({ ok: true });
        }
        return { status: 404, headers: {}, body: new Uint8Array(0) };
      }),
      openWebSocket: vi.fn(async (path: string): Promise<TunnelWebSocket> => {
        websocketPaths.push(path);
        const socket = createSocket(() => {
          if (path.startsWith("/api/v1/events")) {
            armedEventSockets.push(socket);
            armedEventsSockets.bump();
          }
        });
        if (path.startsWith("/api/v1/events")) {
          eventSockets.push(socket);
          eventsSockets.bump();
        }
        return socket;
      }),
      close: vi.fn(),
    };
    const close = vi.fn();
    tunnels.push({ machineId, close, websocketPaths });
    return { transport, close };
  };

  vi.spyOn(attachTunnelModule, "openAccountTunnel").mockImplementation(async (params) => {
    tunnelOpens.bump();
    return makeTunnel(params.machineId);
  });
  vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
    publicKey: "phone-initiator-pub-key-base64",
    privateKey: "phone-initiator-priv-key-base64",
  });
  vi.spyOn(accountSessionModule, "openAccountWebSocket").mockImplementation(async (params) => {
    const socket = createSocket(() => {
      if (params.pathAndQuery.startsWith("/api/v1/events")) {
        armedEventSockets.push(socket);
        armedEventsSockets.bump();
      }
    });
    if (params.pathAndQuery.startsWith("/api/v1/events")) {
      eventSockets.push(socket);
      eventsSockets.bump();
    }
    return socket;
  });

  return {
    machines,
    tunnels,
    eventSockets,
    armedEventSockets,
    selectBodies,
    tunnelOpens,
    attachSessions,
    eventsSockets,
    armedEventsSockets,
    machinesLists,
    selectPosts,
  };
}

async function awaitWorktreeOption(): Promise<HTMLElement> {
  const query = () => screen.queryByRole("button", { name: /main/i });
  const existing = query();
  if (existing) return existing;

  const ensureExpanded = () => {
    const trigger = screen.queryByRole("button", { name: /Change workspace context/i });
    if (trigger && trigger.getAttribute("aria-expanded") !== "true") {
      act(() => {
        fireEvent.click(trigger);
      });
    }
  };
  ensureExpanded();

  return awaitDomTransition(() => {
    const button = query();
    if (button) return button;
    ensureExpanded();
    return query();
  }, "worktree option button");
}

function awaitPlanLimitNotice(): Promise<HTMLElement> {
  return awaitDomTransition(
    () => screen.queryByTestId("remote-plan-limit-notice"),
    "plan-limit notice",
  );
}

function fireCloseEvent(
  harness: ReturnType<typeof setupHarness>,
  event: { code: number; reason: string; wasClean: boolean },
) {
  const armed = harness.eventSockets.filter((socket) => typeof socket.onclose === "function");
  expect(armed.length).toBeGreaterThan(0);
  act(() => {
    for (const socket of armed) socket.onclose?.(event);
  });
}

const machineDiscovery = {
  retryMachine: null as null | ((machineId: string) => Promise<void>),
};

function MachineDiscoveryProbe({ enabled }: { enabled: boolean }) {
  const discovery = useAccountWorktrees(
    window.location.origin,
    "account-session-token-xyz",
    enabled,
  );
  machineDiscovery.retryMachine = discovery.retryMachine;
  const status = discovery.machineStatuses["mach-phone-1"]?.status ?? "none";
  return (
    <div
      data-testid="machine-discovery"
      data-status={status}
      data-status-count={Object.keys(discovery.machineStatuses).length}
    />
  );
}

function awaitMachineStatus(expected: string): Promise<HTMLElement> {
  return awaitDomTransition(() => {
    const node = screen.queryByTestId("machine-discovery");
    return node && node.getAttribute("data-status") === expected ? node : null;
  }, `machine discovery status ${expected}`);
}

describe("RemoteApp plan limit and suspension flow", () => {
  beforeEach(() => {
    clearStoredAccountEntitlementSnapshot(window.location.origin);
    storeAccountSessionToken("account-session-token-xyz", window.location.origin);
    remoteHostStore.reset();
  });

  afterEach(() => {
    vi.useRealTimers();
    cleanup();
    vi.restoreAllMocks();
    clearStoredAccountEntitlementSnapshot(window.location.origin);
    localStorage.clear();
    remoteHostStore.reset();
  });

  it("renders the structured PLAN_LIMIT_REACHED notice and recovers into worktree-first selection", async () => {
    let refusal: Response | null = new Response(
      JSON.stringify({
        code: PLAN_LIMIT_REACHED,
        message: "Machine limit reached",
        details: { plan: "free", limit: 1, used: 2 },
      }),
      { status: 402 },
    );
    const harness = setupHarness({ grantResponse: () => refusal });

    render(<RemoteApp />);

    const notice = await awaitPlanLimitNotice();
    expect(notice.getAttribute("data-code")).toBe(PLAN_LIMIT_REACHED);
    expect(notice.getAttribute("data-plan")).toBe("free");
    expect(screen.getByTestId("remote-plan-limit-usage").getAttribute("data-limit")).toBe("1");
    expect(screen.getByTestId("remote-plan-limit-usage").getAttribute("data-used")).toBe("2");
    expect(screen.getByTestId("remote-plan-limit-upgrade").getAttribute("href")).toContain(
      "/pricing",
    );
    expect(screen.getByRole("button", { name: /Change workspace context/i })).toBeDefined();
    expect(screen.queryByRole("heading", { name: /Account Machines/i })).toBeNull();

    refusal = null;
    const rediscovery = harness.machinesLists.atLeast(2);
    const noticeRemoved = awaitDomTransition(
      () => (screen.queryByTestId("remote-plan-limit-notice") === null ? true : null),
      "plan-limit notice removed",
    );
    act(() => {
      fireEvent.click(screen.getByTestId("remote-plan-limit-retry"));
    });
    await rediscovery;
    await noticeRemoved;
    expect(screen.queryByTestId("remote-plan-limit-notice")).toBeNull();

    const option = await awaitWorktreeOption();
    const selectPost = harness.selectPosts.atLeast(1);
    act(() => {
      fireEvent.click(option);
    });
    await selectPost;
    expect(harness.selectBodies.some((body) => body.includes('"worktreeSlug":"main"'))).toBe(true);
  });

  it("renders the REMOTE_SUSPENDED HTTP attach refusal with its structured details", async () => {
    const refusal = {
      code: REMOTE_SUSPENDED,
      message: "Remote access is suspended",
      details: {
        plan: "pro_monthly",
        status: "stopped",
        graceEndsAt: GRACE_ENDS_AT,
        stoppedAt: STOPPED_AT,
      },
    };
    setupHarness({ grantResponse: () => new Response(JSON.stringify(refusal), { status: 402 }) });

    render(<RemoteApp />);

    const notice = await awaitPlanLimitNotice();
    expect(notice.getAttribute("data-code")).toBe(REMOTE_SUSPENDED);
    expect(notice.getAttribute("data-plan")).toBe("pro_monthly");
    expect(screen.getByTestId("remote-plan-limit-grace-ends").getAttribute("datetime")).toBe(
      new Date(GRACE_ENDS_AT * 1000).toISOString(),
    );
    expect(screen.getByTestId("remote-plan-limit-stopped").getAttribute("datetime")).toBe(
      new Date(STOPPED_AT * 1000).toISOString(),
    );
    expect(screen.getByRole("button", { name: /Change workspace context/i })).toBeDefined();
  });

  it("rediscovers on Retry when the suspension carried no machine", async () => {
    const harness = setupHarness({
      machinesResponse: (callIndex) =>
        callIndex === 1
          ? new Response(
              JSON.stringify({
                code: PLAN_LIMIT_REACHED,
                message: "Machine limit reached",
                details: { plan: "free", limit: 1, used: 2 },
              }),
              { status: 402 },
            )
          : new Response(JSON.stringify([machineFixture(1)]), { status: 200 }),
    });

    render(<RemoteApp />);

    const notice = await awaitPlanLimitNotice();
    expect(notice.getAttribute("data-code")).toBe(PLAN_LIMIT_REACHED);
    expect(harness.machinesLists.count()).toBe(1);

    const secondList = harness.machinesLists.atLeast(2);
    const noticeRemoved = awaitDomTransition(
      () => (screen.queryByTestId("remote-plan-limit-notice") === null ? true : null),
      "plan-limit notice removed",
    );
    act(() => {
      fireEvent.click(screen.getByTestId("remote-plan-limit-retry"));
    });
    await secondList;
    await noticeRemoved;
    expect(screen.queryByTestId("remote-plan-limit-notice")).toBeNull();

    const option = await awaitWorktreeOption();
    expect(option).toBeDefined();
  });

  it("closes every retained account tunnel and opens no new attach session once suspended", async () => {
    const harness = setupHarness({ machines: [machineFixture(1), machineFixture(2)] });

    render(<RemoteApp />);

    const option = await awaitWorktreeOption();
    const thirdTunnel = harness.tunnelOpens.atLeast(3);
    const firstEventsArmed = harness.armedEventsSockets.atLeast(1);
    act(() => {
      fireEvent.click(option);
    });
    await thirdTunnel;
    await firstEventsArmed;

    fireCloseEvent(harness, {
      code: 1012,
      reason: REMOTE_SUSPENDED_CLOSE_REASON,
      wasClean: true,
    });
    vi.useFakeTimers();

    expect(screen.getByTestId("remote-plan-limit-notice")).toBeDefined();
    const retainedTunnels = [...harness.tunnels];
    for (const tunnel of retainedTunnels) {
      expect(tunnel.close).toHaveBeenCalledTimes(1);
    }

    const attachBefore = harness.attachSessions.count();
    const tunnelsBefore = harness.tunnelOpens.count();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60000);
    });
    expect(harness.attachSessions.count()).toBe(attachBefore);
    expect(harness.tunnelOpens.count()).toBe(tunnelsBefore);
    expect(screen.getByTestId("remote-plan-limit-notice")).toBeDefined();
    vi.useRealTimers();
  });

  it("drops a probe that resolves after suspension instead of resurrecting state", async () => {
    const lateGrant = deferred<Response>();
    const harness = setupHarness({
      machines: [machineFixture(1), machineFixture(2)],
      grantResponse: (machineId) => (machineId === "mach-phone-2" ? lateGrant.promise : null),
    });

    render(<RemoteApp />);

    const option = await awaitWorktreeOption();
    const firstEventsArmed = harness.armedEventsSockets.atLeast(1);
    act(() => {
      fireEvent.click(option);
    });
    await firstEventsArmed;

    fireCloseEvent(harness, {
      code: 1012,
      reason: REMOTE_SUSPENDED_CLOSE_REASON,
      wasClean: true,
    });
    vi.useFakeTimers();
    expect(screen.getByTestId("remote-plan-limit-notice")).toBeDefined();

    const attachBefore = harness.attachSessions.count();
    const tunnelsBefore = harness.tunnelOpens.count();

    await act(async () => {
      lateGrant.resolve(
        new Response(JSON.stringify(grantBody("mach-phone-2", "late-grant")), { status: 200 }),
      );
    });

    expect(harness.tunnelOpens.count()).toBe(tunnelsBefore);
    expect(harness.attachSessions.count()).toBe(attachBefore);
    expect(screen.getByTestId("remote-plan-limit-notice")).toBeDefined();
    vi.useRealTimers();
  });

  it("stops automatic retries on the exact REMOTE_SUSPENDED close and allows explicit recovery", async () => {
    const harness = setupHarness();
    storeAccountEntitlementSnapshot(window.location.origin, {
      plan: "pro_monthly",
      status: "stopped",
      graceEndsAt: GRACE_ENDS_AT,
      stoppedAt: STOPPED_AT,
    });

    render(<RemoteApp />);

    const option = await awaitWorktreeOption();
    const firstEventsArmed = harness.armedEventsSockets.atLeast(1);
    act(() => {
      fireEvent.click(option);
    });
    await firstEventsArmed;

    fireCloseEvent(harness, {
      code: 1012,
      reason: REMOTE_SUSPENDED_CLOSE_REASON,
      wasClean: true,
    });
    vi.useFakeTimers();

    const notice = screen.getByTestId("remote-plan-limit-notice");
    expect(notice.getAttribute("data-code")).toBe(REMOTE_SUSPENDED);
    expect(notice.getAttribute("data-plan")).toBe("pro_monthly");
    expect(screen.getByTestId("remote-plan-limit-grace-ends").getAttribute("datetime")).toBe(
      new Date(GRACE_ENDS_AT * 1000).toISOString(),
    );
    expect(screen.getByTestId("remote-plan-limit-stopped").getAttribute("datetime")).toBe(
      new Date(STOPPED_AT * 1000).toISOString(),
    );

    const eventsBefore = harness.eventsSockets.count();
    const attachBefore = harness.attachSessions.count();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60000);
    });
    expect(harness.eventsSockets.count()).toBe(eventsBefore);
    expect(harness.attachSessions.count()).toBe(attachBefore);
    expect(screen.getByTestId("remote-plan-limit-notice")).toBeDefined();

    vi.useRealTimers();

    const rediscovery = harness.machinesLists.atLeast(2);
    const noticeRemoved = awaitDomTransition(
      () => (screen.queryByTestId("remote-plan-limit-notice") === null ? true : null),
      "plan-limit notice removed",
    );
    act(() => {
      fireEvent.click(screen.getByTestId("remote-plan-limit-retry"));
    });
    await rediscovery;
    await noticeRemoved;
    expect(screen.queryByTestId("remote-plan-limit-notice")).toBeNull();

    const recoveredOption = await awaitWorktreeOption();
    const selectPost = harness.selectPosts.atLeast(1);
    act(() => {
      fireEvent.click(recoveredOption);
    });
    await selectPost;
    expect(harness.selectBodies.some((body) => body.includes('"worktreeSlug":"main"'))).toBe(true);
  });

  it("ignores a close whose reason is not the exact suspension reason", async () => {
    const harness = setupHarness();

    render(<RemoteApp />);

    const option = await awaitWorktreeOption();
    const firstEventsArmed = harness.armedEventsSockets.atLeast(1);
    act(() => {
      fireEvent.click(option);
    });
    await firstEventsArmed;

    vi.useFakeTimers();
    const reconnect = harness.eventsSockets.atLeast(2);
    fireCloseEvent(harness, { code: 1006, reason: "MAINTENANCE", wasClean: false });

    expect(screen.queryByTestId("remote-plan-limit-notice")).toBeNull();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
      await vi.runAllTicks();
    });
    vi.useRealTimers();
    await reconnect;
    expect(harness.eventsSockets.count()).toBeGreaterThan(1);
  });

  it("ignores a retry probe whose grant resolves after discovery is disabled", async () => {
    const lateRetryGrant = deferred<Response>();
    let grantCalls = 0;
    const harness = setupHarness({
      machines: [machineFixture(1)],
      grantResponse: () => {
        grantCalls += 1;
        return grantCalls === 2 ? lateRetryGrant.promise : null;
      },
    });

    const statusReady = awaitMachineStatus("ready");
    const view = render(<MachineDiscoveryProbe enabled />);
    await statusReady;

    let retryPromise: Promise<void> = Promise.resolve();
    act(() => {
      retryPromise = machineDiscovery.retryMachine!("mach-phone-1");
    });

    act(() => {
      view.rerender(<MachineDiscoveryProbe enabled={false} />);
    });
    expect(screen.getByTestId("machine-discovery").getAttribute("data-status-count")).toBe("0");

    await act(async () => {
      lateRetryGrant.resolve(
        new Response(JSON.stringify(grantBody("mach-phone-1", "late-retry")), { status: 200 }),
      );
    });
    await act(async () => {
      await retryPromise;
    });

    const node = screen.getByTestId("machine-discovery");
    expect(node.getAttribute("data-status")).toBe("none");
    expect(node.getAttribute("data-status-count")).toBe("0");
    expect(harness.tunnelOpens.count()).toBe(1);
  });
});
