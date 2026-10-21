import ReactDOM from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { SshHost, SshTargetSummary } from "../lib/sshHosts";
import type { TailscaleSyncResult } from "../lib/tailscaleDiscovery";
import type { DeviceInfo, RemoteGatewayStatus } from "../lib/tauri";
import type { HostReachability, PairedHostReachabilityList } from "../lib/pairedHostReachability";
import type { RemoteContext } from "../lib/machineNavigation";
import type { AccountMachineView } from "../remote/accountSession";
import { createRemoteHostStore, remoteHostKey } from "../state/remoteHostStore";
import {
  createPairedHostInventory,
  DEFAULT_RELAY_ORIGIN,
  nativePairedHostCommands,
  type HostView,
  type PairedHostCommands,
} from "../lib/pairedHostInventory";
import "../index.css";
import "../settings-runtime.css";

// Fixture-only entry: no real daemon, SSH target, account, or terminal is contacted.
const SCENARIOS = [
  "ready",
  "connecting",
  "auth",
  "offline",
  "provision",
  "discovery-error",
  "machines",
  "empty",
  "account",
  "drawer",
] as const;
type Scenario = (typeof SCENARIOS)[number];
const params = new URLSearchParams(location.search);
const requestedScenario = params.get("scenario");
// An unknown scenario falls back to `ready` instead of rendering a half-configured fixture.
const mode: Scenario = SCENARIOS.find((scenario) => scenario === requestedScenario) ?? "ready";
const machineScenario = mode === "machines" || mode === "account" || mode === "drawer";
document.documentElement.dataset.theme = params.get("theme") ?? "dark";
document.documentElement.dataset.fixtureScenario = mode;
Object.defineProperty(globalThis, "isTauri", { value: true, configurable: true });

// Fixed clock so every capture renders the same relative times.
const FIXTURE_NOW_MS = 1_760_000_000_000;
const FIXTURE_RELAY_ORIGIN = "https://relay.invalid";
const FIXTURE_ACCOUNT_ORIGIN = "https://account.invalid";

const host: SshHost = {
  id: "tailscale:fixture-node",
  label: "Build workstation",
  hostname: "100.64.0.42",
  source: "tailscale",
  authMethod: "agent",
};
const environment = {
  platform: "posix",
  executor: "sh",
  version: "fixture",
  home: "/fixture/home",
  temp: "/fixture/tmp",
  git: true,
} as const;

function targetFor(target: SshHost, overrides: Partial<SshTargetSummary> = {}): SshTargetSummary {
  return { host: target, reachable: true, checkedAt: 1, environment, helper: "installed", diagnostic: null, ...overrides };
}

// Tailnet-only scenarios: one Tailscale machine in a single diagnostic state.
const diagnostic = mode === "auth"
  ? { code: "SSH_AUTH_FAILED", message: "SSH authentication requires a configured user or key.", details: { stage: "authentication" } }
  : mode === "offline"
    ? { code: "REMOTE_UNAVAILABLE", message: "Tailnet machine is offline.", details: { stage: "offline" } }
    : mode === "provision"
      ? { code: "CLI_EXECUTABLE_NOT_FOUND", message: "Bundled helper for this target is unavailable.", details: { stage: "provisioning" } }
      : null;
const target = targetFor(host, {
  reachable: mode === "ready" || mode === "provision",
  environment: mode === "ready" || mode === "provision" ? environment : null,
  helper: mode === "ready" ? "installed" : "unknown",
  diagnostic,
  lastError: diagnostic?.message,
});

// Redesign scenarios: paired machines in every reachability and access state, a manual SSH
// machine, a provisioned Tailscale machine, and an unprovisioned Tailscale peer (Discovered).
const manualSshHost: SshHost = {
  id: "manual:fixture-db",
  label: "Database VM",
  hostname: "db.fixture.invalid",
  username: "deploy",
  port: 22,
  source: "manual",
  authMethod: "agent",
};
const tailnetPeer: SshHost = {
  id: "tailscale:fixture-peer",
  label: "Media NAS",
  hostname: "100.64.0.77",
  source: "tailscale",
  authMethod: "agent",
};

function hostView(
  machineId: string,
  displayLabel: string,
  authStatus: HostView["authStatus"],
  grantScope: HostView["grantScope"],
  generation: string,
): HostView {
  return {
    hostId: remoteHostKey(FIXTURE_RELAY_ORIGIN, machineId),
    relayOrigin: FIXTURE_RELAY_ORIGIN,
    machineId,
    displayLabel,
    grantScope,
    generation,
    authStatus,
    // Wire contract: the legacy flag is true only for a host probed online.
    online: machineId === "fixture-studio",
  };
}

const studio = hostView("fixture-studio", "Studio Mac", "paired", "machine", "3");
const render = hostView("fixture-render", "Render box", "paired", "machine", "2");
const laptop = hostView("fixture-laptop", "Travel laptop", "paired", "machine", "5");
const mirror = hostView("fixture-mirror", "Shared mirror", "needsMachineGrant", "mirror", "1");
const retired = hostView("fixture-retired", "Retired server", "revoked", "machine", "4");
const pairedViews: HostView[] = machineScenario ? [studio, render, laptop, mirror, retired] : [];

function reachability(view: HostView, state: HostReachability["state"], lastError: string | null = null): HostReachability {
  return { hostId: view.hostId, generation: view.generation, state, checkedAtMs: FIXTURE_NOW_MS, lastError };
}

const reachabilityList: PairedHostReachabilityList = {
  supported: true,
  entries: machineScenario
    ? [reachability(studio, "online"), reachability(render, "offline", "HOST_UNAVAILABLE"), reachability(laptop, "checking")]
    : [],
};

const sshHosts: SshHost[] = mode === "empty" ? [] : machineScenario ? [manualSshHost, host, tailnetPeer] : [host];

function tailscaleSync(): TailscaleSyncResult {
  if (mode === "empty") return { hosts: [], targets: [], discoveryError: null };
  if (machineScenario) {
    return {
      hosts: [host, tailnetPeer],
      targets: [targetFor(host), targetFor(tailnetPeer, { helper: "missing" })],
      discoveryError: null,
    };
  }
  return {
    hosts: [host],
    targets: mode === "discovery-error" ? [] : [target],
    discoveryError: mode === "discovery-error"
      ? { code: "CLI_EXECUTABLE_NOT_FOUND", message: "Tailscale CLI is not installed.", details: { stage: "discovery" } }
      : null,
  };
}

const gatewayStatus: RemoteGatewayStatus = machineScenario
  ? {
    enabled: true,
    mode: "relay",
    port: 43821,
    boundAddress: "127.0.0.1",
    localIp: null,
    relayUrl: DEFAULT_RELAY_ORIGIN,
    gateStatus: "loopbackOnly",
    gateReason: null,
  }
  : { enabled: false, mode: "off", port: 43821, boundAddress: null, localIp: null, relayUrl: null };

const DAY_MS = 86_400_000;
const devices: DeviceInfo[] = machineScenario
  ? [
    { id: "device-phone", name: "Pixel 9", permission: "control", createdAt: FIXTURE_NOW_MS - 7 * DAY_MS, lastSeenAt: FIXTURE_NOW_MS - 60_000, revoked: false },
    { id: "device-tablet", name: "iPad (view only)", permission: "view", createdAt: FIXTURE_NOW_MS - 30 * DAY_MS, lastSeenAt: FIXTURE_NOW_MS - 2 * DAY_MS, revoked: false },
  ]
  : [];

mockIPC((command) => {
  switch (command) {
    case "cmd_ssh_list_hosts": return sshHosts;
    case "cmd_ssh_sync_tailscale":
      if (mode === "connecting") return new Promise(() => {});
      return tailscaleSync();
    case "paired_host_list": return pairedViews;
    case "paired_host_capabilities": return { pairedHostInventoryV1: true, pairedDaemonProxyV1: true };
    case "paired_host_forget": return null;
    case "paired_host_reachability_list": return reachabilityList;
    case "cmd_agent_integration_get": return { enabled: true };
    case "cmd_remote_status": return gatewayStatus;
    case "cmd_remote_devices": return devices;
    case "plugin:event|listen": return 1;
    default: return null;
  }
}, { shouldMockEvents: true });

// Account rows: the account client talks to its origin through global fetch, so the `account`
// scenario answers the fixture origin locally. Every other URL keeps the real fetch.
const accountMachines: AccountMachineView[] = [
  {
    machineRecordId: "record-studio",
    machineId: "fixture-studio",
    displayName: "Studio Mac",
    publicKey: "fixture-public-key",
    attachPublicKey: "fixture-attach-key",
    relayOrigin: FIXTURE_RELAY_ORIGIN,
    platform: "macos",
    online: true,
    enrollmentEpoch: "3",
    lastSeenAt: FIXTURE_NOW_MS - 60_000,
  },
  {
    machineRecordId: "record-headless",
    machineId: "fixture-headless",
    displayName: "Headless runner",
    publicKey: "fixture-public-key",
    attachPublicKey: "fixture-attach-key",
    relayOrigin: FIXTURE_RELAY_ORIGIN,
    platform: "linux",
    online: true,
    enrollmentEpoch: "1",
    lastSeenAt: FIXTURE_NOW_MS - 5 * 60_000,
  },
];
if (mode === "account") {
  const realFetch = globalThis.fetch.bind(globalThis);
  const json = (body: unknown, status = 200) =>
    new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
  const fixtureFetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const url = new URL(input instanceof Request ? input.url : String(input));
    if (url.origin !== FIXTURE_ACCOUNT_ORIGIN) return realFetch(input, init);
    const method = (init?.method ?? (input instanceof Request ? input.method : "GET")).toUpperCase();
    if (method === "GET" && url.pathname === "/api/account/v1/machines") return json(accountMachines);
    if (method === "POST" && url.pathname === "/api/account/v1/enrollment-codes") {
      return json({ code: "FIXTURE-ENROLL-CODE", expiresAt: FIXTURE_NOW_MS + 10 * 60_000 });
    }
    if (method === "POST" && url.pathname === "/api/account/v1/logout") return new Response(null, { status: 204 });
    return json({ code: "FIXTURE_UNSUPPORTED", message: "This account route is not part of the QA fixture." }, 404);
  };
  Object.defineProperty(globalThis, "fetch", { value: fixtureFetch, configurable: true, writable: true });
}

const commands: PairedHostCommands = {
  ...nativePairedHostCommands,
  pair: async () => { throw new Error("Fixture pairing is disabled"); },
  migrate: async () => { throw new Error("Fixture migration is disabled"); },
  read: async () => { throw new Error("Fixture credentials are unavailable"); },
};
const store = createRemoteHostStore();
const inventory = createPairedHostInventory(store, commands);
const initialContext: RemoteContext | undefined = mode === "drawer"
  ? { page: "machines", filter: "all", machine: { kind: "pairedDaemon", hostId: studio.hostId, generation: studio.generation } }
  : undefined;
const { RemoteSection } = await import("../components/settings/RemoteSection");
const { startTailscaleDiscovery } = await import("../lib/tailscaleDiscovery");
const { bootstrapPairedHostReachability } = await import("../lib/pairedHostReachability");
await bootstrapPairedHostReachability();
const stopDiscovery = startTailscaleDiscovery();
import.meta.hot?.dispose(stopDiscovery);
const markProjectOpened = (target: string) => {
  document.documentElement.dataset.fixtureProjectOpened = "true";
  document.documentElement.dataset.fixtureProjectTarget = target;
};
const root = document.getElementById("root");
if (root) {
  ReactDOM.createRoot(root).render(
    <main className="mx-auto max-w-3xl p-4 sm:p-6">
      <RemoteSection
        store={store}
        inventory={inventory}
        initialContext={initialContext}
        accountOrigin={FIXTURE_ACCOUNT_ORIGIN}
        accountSessionToken={mode === "account" ? "fixture-account-session" : null}
        onOpenProject={(projectTarget) => markProjectOpened(`${projectTarget.kind}:${projectTarget.hostId}`)}
        onOpenSshProject={(hostId) => markProjectOpened(`ssh:${hostId}`)}
      />
    </main>,
  );
}
