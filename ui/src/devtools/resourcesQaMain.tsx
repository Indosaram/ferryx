import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import ReactDOM from "react-dom/client";

import "../index.css";
import "../settings-runtime.css";
import { SystemResourcesDialog } from "../components/SystemResourcesDialog";
import type { HostResourceSnapshot } from "../lib/systemResources";
import { remoteHostStore, type HostEndpoint } from "../state/remoteHostStore";

declare global {
  interface Window { __qaLog?: string[] }
}
window.__qaLog = [];
const log = (event: string) => window.__qaLog!.push(event);

const params = new URLSearchParams(window.location.search);
const theme = params.get("theme");
if (theme === "light" || theme === "dark") {
  document.documentElement.dataset.theme = theme;
}
const manyFleet = params.get("fleet") === "many";

mockWindows("main");
(window as unknown as { isTauri?: boolean }).isTauri = true;

const GiB = 1024 ** 3;
let tick = 0;
function sample(
  platform: string,
  cores: number,
  memoryUsed: number,
  memoryTotal: number,
  cpu: number,
  sessions: HostResourceSnapshot["sessions"],
): HostResourceSnapshot {
  const jitter = 0.04 * Math.sin(tick / 2 + cores);
  const util = Math.min(0.98, Math.max(0.02, cpu + jitter));
  return {
    sampledAtMs: Date.now(),
    platform,
    cpuCount: cores,
    cpuUtilization: util,
    loadAverage1m: util * cores * 1.15,
    memoryTotalBytes: memoryTotal * GiB,
    memoryUsedBytes: Math.min(memoryTotal, memoryUsed) * GiB,
    swapTotalBytes: 4 * GiB,
    swapUsedBytes: 1.67 * GiB,
    uptimeSeconds: 86400 * 1 + 7200,
    diskTotalBytes: 460 * GiB,
    diskFreeBytes: 363 * GiB,
    processCount: 400 + cores * 31,
    sessions,
    unavailable: [],
  };
}
const session = (sessionId: string, worktreePath: string | null, cpuPercent: number, mib: number, procs = 3) =>
  ({ sessionId, pid: 10000 + sessionId.length * 137, worktreePath, cpuPercent, residentBytes: mib * 1024 ** 2, processCount: procs });

const baseHosts: HostEndpoint[] = [
  { hostId: "h-omarchy", name: "omarchy", displayName: "omarchy", address: "https://relay.ferryx.dev", transport: "relay", authStatus: "paired", online: true, machineId: "m1", generation: "2", grantScope: "machine" },
  { hostId: "h-win", name: "maho-win", displayName: "maho-win", address: "https://relay.ferryx.dev", transport: "relay", authStatus: "paired", online: false, machineId: "m2", generation: "5", grantScope: "machine" },
  { hostId: "h-old", name: "old-mac", displayName: "old-mac", address: "https://relay.ferryx.dev", transport: "relay", authStatus: "paired", online: true, machineId: "m3", generation: "1", grantScope: "machine" },
];

const extraHosts: HostEndpoint[] = manyFleet
  ? [
      { hostId: "h-mac", name: "maho-mac", displayName: "maho-mac", address: "https://relay.ferryx.dev", transport: "relay", authStatus: "paired", online: true, machineId: "m4", generation: "3", grantScope: "machine" },
      { hostId: "h-noye", name: "noye-box", displayName: "noye-box", address: "https://relay.ferryx.dev", transport: "relay", authStatus: "paired", online: true, machineId: "m5", generation: "4", grantScope: "machine" },
      { hostId: "h-daas", name: "daas-prod", displayName: "daas-prod", address: "https://relay.ferryx.dev", transport: "relay", authStatus: "paired", online: true, machineId: "m6", generation: "2", grantScope: "machine" },
      { hostId: "h-gpu", name: "gpu-worker-01", displayName: "gpu-worker-01", address: "https://relay.ferryx.dev", transport: "relay", authStatus: "paired", online: false, machineId: "m7", generation: "1", grantScope: "machine" },
    ]
  : [];

const hosts = [...baseHosts, ...extraHosts];
remoteHostStore.setState((prev) => ({
  ...prev,
  nativeStatus: "ready",
  hosts: Object.fromEntries(hosts.map((host) => [host.hostId, host])),
}));

mockIPC((command, args) => {
  tick += 1;
  if (command === "cmd_system_resources") {
    log("local");
    return sample("macos", 10, 11.7, 16, 0.29, [
      session("10a9a649-7736-420f-9690-bbf83f9af14a", "/Users/indo/code/ferryx", 10.1, 238, 12),
      session("bb305239-1381-497d-9c5b-7e8d4f92e3af", "/Users/indo/code/ferryx", 0.6, 203, 3),
      session("dafed86b-90ef-4aa6-b34b-10ccbb40a563", "/Users/indo/code/ferryx", 0.5, 32.2, 2),
      session("codex-review", "/Users/indo/code/ferryx/.ferryx-worktrees/ws/resource-monitor", 8.1, 410, 4),
    ]);
  }
  if (command === "cmd_ssh_hosts_list") {
    return manyFleet
      ? [
          { id: "ssh-vultr", label: "vultr-build", hostname: "100.70.12.4", source: "manual", authMethod: "agent" },
          { id: "ssh-hark", label: "hark-runner", hostname: "100.70.12.9", source: "config", authMethod: "key" },
        ]
      : [];
  }
  if (command === "cmd_ssh_host_resources") {
    const hostId = (args as { hostId: string }).hostId;
    if (hostId === "ssh-vultr") {
      return sample("linux", 8, 14.4, 16, 0.92, [
        session("cargo-clippy", "/home/build/ferryx", 84.2, 3400, 18),
      ]);
    }
    throw new Error("ssh unreachable");
  }
  if (command === "paired_host_operation") {
    const request = (args as { request: { hostId: string; generation: string; operation: { kind: string } } }).request;
    log(`paired:${request.hostId}:${request.operation.kind}`);
    if (request.hostId === "h-win" || request.hostId === "h-gpu") {
      throw { code: "TRANSPORT", machineError: null, ambiguous: false, requestId: null };
    }
    if (request.hostId === "h-old") {
      throw { code: "PAIRED_HOST_CAPABILITY_UNAVAILABLE", machineError: null, ambiguous: false, requestId: null };
    }
    if (request.hostId === "h-mac") {
      return {
        hostId: request.hostId,
        generation: request.generation,
        result: {
          kind: "resources",
          data: sample("macos", 14, 18, 36, 0.44, [
            session("xcode-archive", "/Users/i552267/code/ferryx", 31.4, 1820, 9),
          ]),
        },
      };
    }
    return {
      hostId: request.hostId,
      generation: request.generation,
      result: {
        kind: "resources",
        data: sample("linux", 12, 24.5, 32, 0.76, [
          session("agent-build", "/home/indo/code/pty-integration", 96.0, 2300, 14),
          session("logs", null, 0.1, 6, 1),
        ]),
      },
    };
  }
  if (command === "cmd_terminal_close") { log(`close:${JSON.stringify(args)}`); return null; }
  throw new Error(`unmocked ${command}`);
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <div className="min-h-screen bg-background text-foreground">
    <SystemResourcesDialog onClose={() => log("close-dialog")} />
  </div>,
);
