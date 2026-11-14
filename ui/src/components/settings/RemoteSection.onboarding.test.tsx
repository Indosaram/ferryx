import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ACCOUNT_ONBOARDING_POLL_MS, RemoteSection } from "./RemoteSection";
import { createRemoteHostStore } from "../../state/remoteHostStore";
import { createPairedHostInventory, DEFAULT_RELAY_ORIGIN, type PairedHostCommands } from "../../lib/pairedHostInventory";
import { resetSshHostsCache } from "../../lib/sshHosts";
import { resetTailscaleDiscoveryForTests } from "../../lib/tailscaleDiscovery";

const { isTauriMock, invokeMock } = vi.hoisted(() => ({
  isTauriMock: vi.fn(() => true),
  invokeMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => isTauriMock(),
  // Desktop account HTTP runs through cmd_account_http_request; replay it as the stubbed fetch.
  invoke: async (...args: unknown[]) =>
    args[0] === "cmd_account_http_request"
      ? (await import("../../test/accountIpcFetchBridge")).accountIpcViaFetch(args[1])
      : invokeMock(...args),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("./RemoteAccessSection", () => ({
  RemoteAccessSection: () => <div data-testid="remote-access-section" />,
}));
vi.mock("../../lib/pairedHostReachability", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/pairedHostReachability")>()),
  usePairedHostReachability: () => ({ supported: false, entry: null }),
}));

const jsonResponse = (payload: unknown, status = 200) =>
  new Response(JSON.stringify(payload), { status, headers: { "Content-Type": "application/json" } });

const nowSecs = () => Math.floor(Date.now() / 1000);
const pendingRequest = () => ({
  requestId: "req-1",
  userCode: "ABCD-EFGH",
  machineId: "mach-new",
  displayName: "noye",
  platform: "linux",
  appVersion: "2026.1023.1",
  requesterIp: "203.0.113.42",
  createdAt: nowSecs(),
  expiresAt: nowSecs() + 900,
});

function createInventory() {
  const store = createRemoteHostStore();
  const commands: PairedHostCommands = {
    list: vi.fn().mockResolvedValue([]),
    capabilities: vi.fn().mockResolvedValue({ pairedHostInventoryV1: true, pairedDaemonProxyV1: true }),
    pair: vi.fn(),
    forget: vi.fn().mockResolvedValue(true),
    migrate: vi.fn(),
    read: vi.fn(),
  };
  return { store, inventory: createPairedHostInventory(store, commands) };
}

type Route = (url: string, init?: RequestInit) => Response | undefined;

function stubAccountFetch(route: Route = () => undefined) {
  const fetchMock = vi.fn(async (input: string, init?: RequestInit) => {
    const url = String(input);
    const routed = route(url, init);
    if (routed) return routed;
    if (url.endsWith("/api/account/v1/machines")) return jsonResponse([]);
    if (url.endsWith("/api/account/v1/machine-requests")) return jsonResponse([]);
    return jsonResponse({ code: "NOT_FOUND", message: "not found" }, 404);
  });
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

const callsTo = (fetchMock: ReturnType<typeof stubAccountFetch>, suffix: string) =>
  fetchMock.mock.calls.filter(([url]) => String(url).endsWith(suffix)).length;

async function renderSignedIn() {
  const { store, inventory } = createInventory();
  await inventory.refresh();
  // Fake timers before mount, so intervals created by the first render are controllable.
  // Wall-clock time never advances them; tick() below is the only clock.
  vi.useFakeTimers();
  await act(async () => {
    render(<RemoteSection store={store} inventory={inventory} accountOrigin={DEFAULT_RELAY_ORIGIN} accountSessionToken="tok" />);
  });
}

async function tick(ms: number) {
  await act(async () => { await vi.advanceTimersByTimeAsync(ms); });
}

describe("RemoteSection machine onboarding", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetSshHostsCache();
    resetTailscaleDiscoveryForTests();
    localStorage.clear();
    isTauriMock.mockReturnValue(true);
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "cmd_ssh_list_hosts") return [];
      if (command === "cmd_agent_integration_get") return { enabled: true };
      if (command === "cmd_ssh_read_system_config") return { path: "", exists: false, rawText: "", hosts: [] };
      return undefined;
    });
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.unstubAllGlobals();
    resetTailscaleDiscoveryForTests();
  });

  it("polls machines and pending requests every 5 s only while the Add Machine wizard is open", async () => {
    const fetchMock = stubAccountFetch((url, init) =>
      url.endsWith("/api/account/v1/install-keys") && init?.method === "POST"
        ? jsonResponse({ keyId: "k", key: "fxk_1", expiresAt: nowSecs() + 3600, installCommand: { sh: "sh-cmd", ps1: "ps1-cmd" } })
        : undefined,
    );
    await renderSignedIn();

    // Nothing pending and the wizard closed: no polling.
    const idleMachines = callsTo(fetchMock, "/machines");
    await tick(ACCOUNT_ONBOARDING_POLL_MS * 3);
    expect(callsTo(fetchMock, "/machines")).toBe(idleMachines);

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add Machine" })); });
    // Signed in: the wizard defaults to the install command.
    expect(screen.getByRole("radio", { name: "Install command" })).toHaveAttribute("aria-checked", "true");

    const machinesBefore = callsTo(fetchMock, "/machines");
    const requestsBefore = callsTo(fetchMock, "/machine-requests");
    await tick(ACCOUNT_ONBOARDING_POLL_MS);
    expect(callsTo(fetchMock, "/machines")).toBe(machinesBefore + 1);
    expect(callsTo(fetchMock, "/machine-requests")).toBe(requestsBefore + 1);
    await tick(ACCOUNT_ONBOARDING_POLL_MS);
    expect(callsTo(fetchMock, "/machines")).toBe(machinesBefore + 2);

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Cancel" })); });
    const machinesAfterClose = callsTo(fetchMock, "/machines");
    await tick(ACCOUNT_ONBOARDING_POLL_MS * 3);
    expect(callsTo(fetchMock, "/machines")).toBe(machinesAfterClose);
  });

  it("stops polling on unmount", async () => {
    const fetchMock = stubAccountFetch((url) =>
      url.endsWith("/api/account/v1/machine-requests") ? jsonResponse([pendingRequest()]) : undefined,
    );
    await renderSignedIn();
    await tick(0);
    expect(screen.getByTestId("machine-approval-card")).toBeInTheDocument();
    await tick(ACCOUNT_ONBOARDING_POLL_MS);
    const before = callsTo(fetchMock, "/machines");
    cleanup();
    await vi.advanceTimersByTimeAsync(ACCOUNT_ONBOARDING_POLL_MS * 3);
    expect(callsTo(fetchMock, "/machines")).toBe(before);
  });

  it("shows a pending request as an approval card that does not steal focus, and approves it", async () => {
    let pending = [pendingRequest()];
    const fetchMock = stubAccountFetch((url, init) => {
      if (url.endsWith("/api/account/v1/machine-requests")) return jsonResponse(pending);
      if (url.endsWith("/machine-requests/req-1/approve") && init?.method === "POST") {
        pending = [];
        return jsonResponse({ machineId: "mach-new" });
      }
      return undefined;
    });
    await renderSignedIn();

    await tick(0);
    const card = screen.getByTestId("machine-approval-card");
    expect(card).toBeInTheDocument();
    expect(card).toHaveTextContent("noye");
    expect(screen.getByRole("button", { name: "Deny" })).not.toHaveFocus();

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Approve" })); });
    await tick(0);
    const approveCall = fetchMock.mock.calls.find(([url]) => String(url).endsWith("/req-1/approve"));
    expect(JSON.parse(String(approveCall?.[1]?.body))).toEqual({ userCode: "ABCD-EFGH" });
    expect(new Headers(approveCall?.[1]?.headers).get("Authorization")).toBe("Bearer tok");
    expect(screen.queryByTestId("machine-approval-card")).toBeNull();
  });

  it("polls while a request is pending even with the wizard closed, and stops once it is gone", async () => {
    let pending = [pendingRequest()];
    const fetchMock = stubAccountFetch((url) =>
      url.endsWith("/api/account/v1/machine-requests") ? jsonResponse(pending) : undefined,
    );
    await renderSignedIn();
    await tick(0);
    expect(screen.getByTestId("machine-approval-card")).toBeInTheDocument();
    const before = callsTo(fetchMock, "/machines");
    await tick(ACCOUNT_ONBOARDING_POLL_MS);
    expect(callsTo(fetchMock, "/machines")).toBe(before + 1);

    pending = [];
    await tick(ACCOUNT_ONBOARDING_POLL_MS);
    expect(screen.queryByTestId("machine-approval-card")).toBeNull();
    const settled = callsTo(fetchMock, "/machines");
    await tick(ACCOUNT_ONBOARDING_POLL_MS * 3);
    expect(callsTo(fetchMock, "/machines")).toBe(settled);
  });

  it("ignores an account server without the machine-requests endpoint", async () => {
    stubAccountFetch();
    await renderSignedIn();
    await tick(0);
    expect(screen.queryByTestId("machine-approval-card")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("does not poll or list requests while signed out", async () => {
    const fetchMock = stubAccountFetch();
    const { store, inventory } = createInventory();
    await inventory.refresh();
    vi.useFakeTimers();
    await act(async () => { render(<RemoteSection store={store} inventory={inventory} accountOrigin={DEFAULT_RELAY_ORIGIN} accountSessionToken={null} />); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add Machine" })); });
    await tick(ACCOUNT_ONBOARDING_POLL_MS * 2);
    expect(callsTo(fetchMock, "/machine-requests")).toBe(0);
    expect(callsTo(fetchMock, "/machines")).toBe(0);
  });
});

