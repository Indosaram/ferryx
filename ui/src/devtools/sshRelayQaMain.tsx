import ReactDOM from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { SshHost, SystemSshConfig } from "../lib/sshHosts";
import type { DeviceInfo, DirectAutoTrustStatus, RemoteGatewayStatus } from "../lib/tauri";
import type { PairedHostReachabilityList } from "../lib/pairedHostReachability";
import type { PairedHostRouteList } from "../lib/pairedHostRoute";
import type { BranchDeletionPreview, Worktree } from "../lib/types";
import { createRemoteHostStore, remoteHostKey } from "../state/remoteHostStore";
import {
  createPairedHostInventory,
  nativePairedHostCommands,
  type HostView,
  type PairedHostCommands,
} from "../lib/pairedHostInventory";
import "../index.css";
import "../settings-runtime.css";

// Fixture-only entry: no relay, account server, SSH host, or daemon is contacted. Every answer
// comes from `mockIPC`, and every origin is a reserved `.invalid` name, so nothing here resolves.
const SCENARIOS = ["settings-sync", "add-machine", "desktop-scrim", "worktree-delete"] as const;
type Scenario = (typeof SCENARIOS)[number];
const params = new URLSearchParams(location.search);
const requestedScenario = params.get("scenario");
// An unknown scenario falls back to the relay settings page instead of a half-configured mount.
const mode: Scenario = SCENARIOS.find((scenario) => scenario === requestedScenario) ?? "settings-sync";
document.documentElement.dataset.theme = params.get("theme") === "light" ? "light" : "dark";
document.documentElement.dataset.fixtureScenario = mode;
Object.defineProperty(globalThis, "isTauri", { value: true, configurable: true });

// Fixed clock so repeated captures render the same relative times.
const FIXTURE_NOW_MS = 1_760_000_000_000;
const FIXTURE_RELAY_ORIGIN = "https://relay.invalid";
const FIXTURE_ACCOUNT_ORIGIN = "https://account.invalid";
const SSH_WORKSPACE_ID = "ssh:abcdef123456";
const PAIRED_HOST_ID = remoteHostKey(FIXTURE_RELAY_ORIGIN, "fixture-studio");

const sshHosts: SshHost[] = [
  { id: "manual:fixture-build", label: "Build server", hostname: "build.fixture.invalid", username: "ci", port: 22, source: "manual", authMethod: "agent" },
  { id: "config:fixture-nas", label: "Media NAS", hostname: "nas.fixture.invalid", username: "media", port: 22, source: "config", authMethod: "key" },
];

// Three importable hosts: enough for the selection list, its Select All control, and the count.
const systemConfig: SystemSshConfig = {
  path: "~/.ssh/config",
  exists: true,
  rawText: "Host build\n  HostName build.fixture.invalid\nHost nas\n  HostName nas.fixture.invalid\nHost staging\n  HostName staging.fixture.invalid",
  hosts: [
    { id: "config:fixture-build", label: "build", hostname: "build.fixture.invalid", username: "ci", port: 22, source: "config", authMethod: "agent" },
    { id: "config:fixture-nas", label: "nas", hostname: "nas.fixture.invalid", username: "media", port: 22, source: "config", authMethod: "key" },
    { id: "config:fixture-staging", label: "staging", hostname: "staging.fixture.invalid", username: "deploy", port: 22, source: "config", authMethod: "agent" },
  ],
};

// Relay on, relay link down: the page must say "not connected" without hiding the bound address.
const relayStatus: RemoteGatewayStatus = {
  enabled: true, mode: "relay", port: 8899, boundAddress: "127.0.0.1:8899", localIp: null,
  relayUrl: FIXTURE_RELAY_ORIGIN, relayConnected: false, controlChannelConnected: false,
  insecureModes: ["relay"], tokenConfigured: true, bindScope: "relay", publicExposure: true,
  gateStatus: null, gateReason: null,
};

const devices: DeviceInfo[] = [
  { id: "device-phone", name: "Pixel 9", permission: "control", accessScope: "machine", createdAt: FIXTURE_NOW_MS - 604_800_000, lastSeenAt: FIXTURE_NOW_MS - 60_000, revoked: false },
  { id: "device-tablet", name: "iPad (view only)", permission: "view", createdAt: FIXTURE_NOW_MS - 2_592_000_000, lastSeenAt: FIXTURE_NOW_MS - 172_800_000, revoked: false },
];

// The env switch outranks the persisted toggle, so the page must explain itself rather than
// offering a control that cannot take effect.
const autoTrust: DirectAutoTrustStatus = { enabled: false, disabledBySetting: false, disabledByEnv: true, pinnedAccountKey: null };

const pairedView: HostView = {
  hostId: PAIRED_HOST_ID, relayOrigin: FIXTURE_RELAY_ORIGIN, machineId: "fixture-studio",
  displayLabel: "Studio Mac", grantScope: "machine", generation: "1", authStatus: "paired", online: true,
};
const reachability: PairedHostReachabilityList = {
  supported: true,
  entries: [{ hostId: PAIRED_HOST_ID, generation: "1", state: "online", checkedAtMs: FIXTURE_NOW_MS, lastError: null }],
};
const routes: PairedHostRouteList = { supported: true, entries: [{ hostId: PAIRED_HOST_ID, state: "relay" }] };

const sshWorktree: Worktree = {
  workspaceId: SSH_WORKSPACE_ID, identity: { wsId: "ssh-abcdef", slug: "feat" }, hostLabel: "Build server",
  path: "/srv/repo/.ferryx/ssh-abcdef/feat", head: "deadbeef", branch: "ferryx/ssh-abcdef/feat",
  bare: false, detached: false, locked: null, prunable: null,
};

// Wire shape from `src-tauri/src/worktree/model.rs`: the field is `statusCode`, and the dialog
// renders it verbatim, so a `status` key would paint a "?" chip instead of "M"/"??".
const deletePreview: BranchDeletionPreview = {
  dirtyState: { isDirty: true, files: [{ statusCode: "M", path: "src/app.ts" }, { statusCode: "??", path: "notes 1.txt" }] },
  missing: false, branch: "ferryx/ssh-abcdef/feat", head: "deadbeef", upstream: "origin/main",
  merged: false, ahead: 2, behind: 0,
};

// A flat 320x180 white JPEG: bright enough that the dark scrim over it is measurable in both
// themes. Inline because the fixture must not fetch anything.
const WHITE_JPEG_BASE64 = "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAIBAQEBAQIBAQECAgICAgQDAgICAgUEBAMEBgUGBgYFBgYGBwkIBgcJBwYGCAsICQoKCgoKBggLDAsKDAkKCgr/2wBDAQICAgICAgUDAwUKBwYHCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgr/wAARCAC0AUADASIAAhEBAxEB/8QAFQABAQAAAAAAAAAAAAAAAAAAAAn/xAAUEAEAAAAAAAAAAAAAAAAAAAAA/8QAFAEBAAAAAAAAAAAAAAAAAAAAAP/EABQRAQAAAAAAAAAAAAAAAAAAAAD/2gAMAwEAAhEDEQA/AL+AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA//9k=";
const FRAME_WIDTH = 320, FRAME_HEIGHT = 180;
/** kind(1) + width(4) + height(4) + frameId(8), then the JPEG, per `parseRdFrameMessage`. */
const RD_VIDEO_HEADER_BYTES = 17;
const RD_VIDEO_KIND = 0x01, RD_STATUS_KIND = 0x02;

function videoFrame(): Uint8Array {
  const raw = atob(WHITE_JPEG_BASE64);
  const bytes = new Uint8Array(RD_VIDEO_HEADER_BYTES + raw.length);
  const view = new DataView(bytes.buffer);
  bytes[0] = RD_VIDEO_KIND;
  view.setUint32(1, FRAME_WIDTH, true);
  view.setUint32(5, FRAME_HEIGHT, true);
  view.setBigUint64(9, 1n, true);
  for (let index = 0; index < raw.length; index += 1) bytes[RD_VIDEO_HEADER_BYTES + index] = raw.charCodeAt(index);
  return bytes;
}

function statusFrame(state: "reconnecting", reason: string): Uint8Array {
  const body = new TextEncoder().encode(JSON.stringify({ state, reason }));
  const bytes = new Uint8Array(1 + body.length);
  bytes[0] = RD_STATUS_KIND;
  bytes.set(body, 1);
  return bytes;
}

/** Delivers one Channel message in the `{ index, message }` envelope the client decodes. */
function pushFrame(channelId: number, index: number, message: Uint8Array): void {
  const internals = (window as unknown as { __TAURI_INTERNALS__?: { runCallback?: (id: number, data: unknown) => void } }).__TAURI_INTERNALS__;
  internals?.runCallback?.(channelId, { index, message });
}

/**
 * Runs `then` once the pane has painted a frame (it publishes `visibility: visible`). Watching
 * that attribute preserves the ordering a real backend gives without polling for it.
 */
function whenCanvasPainted(then: () => void): void {
  const canvas = document.querySelector('[data-testid="remote-desktop-canvas"]');
  if (!(canvas instanceof HTMLElement) || canvas.style.visibility === "visible") return then();
  const observer = new MutationObserver(() => {
    if (canvas.style.visibility !== "visible") return;
    observer.disconnect();
    then();
  });
  observer.observe(canvas, { attributes: true, attributeFilter: ["style"] });
}

mockIPC((command, payload) => {
  switch (command) {
    case "cmd_remote_status":
    case "cmd_remote_enable":
      return relayStatus;
    case "cmd_remote_disable":
      return { ...relayStatus, enabled: false, mode: "off", boundAddress: null, relayConnected: false, publicExposure: false };
    // The settings section reads `cmd_remote_devices`; `cmd_remote_list_devices` is the brief's
    // name for the same list, so both answer identically.
    case "cmd_remote_devices":
    case "cmd_remote_list_devices":
      return devices;
    case "cmd_remote_auto_trust_status":
      return autoTrust;
    case "cmd_ssh_list_hosts":
      return sshHosts;
    case "cmd_ssh_sync_tailscale":
      return { hosts: [], targets: [], discoveryError: null };
    case "cmd_ssh_tunnel_list":
      return { views: [], definitions: [], hostRevisions: [], definitionsRevision: 0, storeStatus: { kind: "ok", path: "", invalidHosts: [], quarantinedPath: null } };
    case "paired_host_list":
      return [pairedView];
    case "paired_host_capabilities":
      return { pairedHostInventoryV1: true, pairedDaemonProxyV1: true };
    case "paired_host_reachability_list":
      return reachability;
    case "paired_host_route_list":
      return routes;
    case "cmd_agent_integration_get":
      return { enabled: true };
    // The import step reads the system config through `cmd_ssh_read_system_config`;
    // `cmd_ssh_import_system_config` is the brief's name for that contract, same three hosts.
    case "cmd_ssh_read_system_config":
    case "cmd_ssh_import_system_config":
      return systemConfig;
    case "cmd_ssh_import_config":
      return systemConfig.hosts;
    case "cmd_ssh_test_connection":
      return {
        host: systemConfig.hosts[0], reachable: true, checkedAt: 1, helper: "installed", diagnostic: null,
        environment: { platform: "posix", executor: "sh", version: "fixture", home: "/fixture/home", temp: "/fixture/tmp", git: true },
      };
    case "cmd_ssh_update_host":
      return sshHosts;
    case "cmd_ssh_set_password":
      return null;
    case "cmd_ssh_preview_remote_worktree_delete":
      return deletePreview;
    case "cmd_ssh_delete_remote_worktree":
      return null;
    case "cmd_rd_supported":
      return true;
    // Hosting is off here: this fixture is the viewer side of the desktop session.
    case "cmd_rd_host_state": return { enabled: false, running: false, allowInput: false, allowClipboard: false, sessions: [], pending: [], error: null };
    case "cmd_rd_connect": {
      // `mockIPC` hands the live `Channel` object through, so its callback id is right here.
      const payloadRecord = payload && typeof payload === "object" && !Array.isArray(payload)
        && !(payload instanceof ArrayBuffer) && !ArrayBuffer.isView(payload)
        ? (payload as Record<string, unknown>) : undefined;
      const channelId = (payloadRecord?.frames as { id?: unknown } | undefined)?.id;
      if (typeof channelId === "number") {
        // A bright frame first, then the reconnect status once it is painted: the pane keeps the
        // frozen canvas and covers it with the scrim, which is the contrast case under test.
        pushFrame(channelId, 0, videoFrame());
        whenCanvasPainted(() => pushFrame(channelId, 1, statusFrame("reconnecting", "Network interrupted")));
      }
      return { rdSessionId: "fixture-rd-session" };
    }
    case "cmd_rd_input":
    case "cmd_rd_disconnect":
      return null;
    default:
      return null;
  }
}, { shouldMockEvents: true });

const commands: PairedHostCommands = {
  ...nativePairedHostCommands,
  pair: async () => { throw new Error("Fixture pairing is disabled"); },
  migrate: async () => { throw new Error("Fixture migration is disabled"); },
  read: async () => { throw new Error("Fixture credentials are unavailable"); },
};
const store = createRemoteHostStore();
const inventory = createPairedHostInventory(store, commands);
/** Records an action on `<html>` instead of performing it, so a capture can assert the click. */
const mark = (event: string, value = "true") =>
  Object.assign(document.documentElement.dataset, { fixtureEvent: event, fixtureEventValue: value });

function mount(element: JSX.Element): void {
  const root = document.getElementById("root");
  if (!root) return;
  ReactDOM.createRoot(root).render(element);
  document.documentElement.dataset.qaReady = "true";
}

/* The fixtures resolve their component modules lazily; `es2020` has no top-level await, so the
   whole dispatch lives inside one async boot function instead. */
async function boot(): Promise<void> {
  if (mode === "settings-sync") {
    const { RemoteSection } = await import("../components/settings/RemoteSection");
    const { bootstrapPairedHostReachability } = await import("../lib/pairedHostReachability");
    const { bootstrapPairedHostRoute } = await import("../lib/pairedHostRoute");
    await bootstrapPairedHostReachability();
    await bootstrapPairedHostRoute();
    mount(
      <main className="mx-auto max-w-3xl p-4 sm:p-6">
        <RemoteSection
          store={store}
          inventory={inventory}
          accountOrigin={FIXTURE_ACCOUNT_ORIGIN}
          accountSessionToken={null}
          onOpenProject={(target) => mark("project-opened", `${target.kind}:${target.hostId}`)}
          onOpenSshProject={(hostId) => mark("project-opened", `ssh:${hostId}`)}
        />
      </main>,
    );
  } else if (mode === "add-machine") {
    const { AddMachineModal } = await import("../components/settings/AddMachineModal");
    mount(
      <main className="mx-auto max-w-3xl p-4 sm:p-6">
        <AddMachineModal
          isOpen
          initialMethod="import"
          inventory={inventory}
          accountOrigin={FIXTURE_ACCOUNT_ORIGIN}
          onClose={() => mark("add-machine-closed")}
          onSuccess={(result) => mark("add-machine-success", `${result.kind}:${result.hostId}`)}
          onImportDone={(hostId) => mark("add-machine-imported", hostId)}
        />
      </main>,
    );
  } else if (mode === "desktop-scrim") {
    const { RemoteDesktopPane } = await import("../components/RemoteDesktopPane");
    mount(
      <main className="flex min-h-screen flex-col gap-4 bg-background p-4 text-foreground sm:p-6">
        <h1 className="text-[13px] font-semibold">Frozen bright frame under the reconnect scrim</h1>
        <div className="h-[360px] w-full max-w-[640px] overflow-hidden rounded-lg border border-border">
          <RemoteDesktopPane hostId={PAIRED_HOST_ID} label="Studio Mac" visible />
        </div>
      </main>,
    );
  } else {
    const { WorktreeDeleteDialog } = await import("../components/WorktreeDeleteDialog");
    mount(
      <WorktreeDeleteDialog
        workspaceId={SSH_WORKSPACE_ID}
        worktree={sshWorktree}
        onClose={() => mark("worktree-delete-closed")}
        onDeleted={() => mark("worktree-delete-deleted", sshWorktree.path)}
      />,
    );
  }
}
void boot();
