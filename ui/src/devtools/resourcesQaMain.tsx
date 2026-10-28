import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import ReactDOM from "react-dom/client";

import "../index.css";
import { SystemResourcesDialog } from "../components/SystemResourcesDialog";
import type { HostResourceSnapshot } from "../lib/systemResources";
import { remoteHostStore, type HostEndpoint } from "../state/remoteHostStore";

declare global {
  interface Window { __qaLog?: string[] }
}
window.__qaLog = [];
const log = (event: string) => window.__qaLog!.push(event);

mockWindows("main");
(window as unknown as { isTauri?: boolean }).isTauri = true;

const GiB = 1024 ** 3;
let tick = 0;
function sample(platform: string, cores: number, memoryUsed: number, sessions: HostResourceSnapshot["sessions"]): HostResourceSnapshot {
  const wave = 0.35 + 0.25 * Math.sin(tick / 2 + cores);
  return {
    sampledAtMs: Date.now(), platform, cpuCount: cores, cpuUtilization: wave, loadAverage1m: wave * cores,
    memoryTotalBytes: 32 * GiB, memoryUsedBytes: memoryUsed * GiB + wave * GiB, swapTotalBytes: 4 * GiB, swapUsedBytes: GiB / 2,
    uptimeSeconds: 86400 * 3 + 3600, diskTotalBytes: 1000 * GiB, diskFreeBytes: 230 * GiB, processCount: 400 + cores,
    sessions, unavailable: [],
  };
}
const session = (sessionId: string, worktreePath: string | null, cpuPercent: number, mib: number) =>
  ({ sessionId, pid: 1000 + sessionId.length * 7, worktreePath, cpuPercent, residentBytes: mib * 1024 ** 2, processCount: 3 });

const hosts: HostEndpoint[] = [
  { hostId: "h-omarchy", name: "omarchy", displayName: "omarchy", address: "https://relay.ferryx.dev", transport: "relay", authStatus: "paired", online: true, machineId: "m1", generation: "2", grantScope: "machine" },
  { hostId: "h-win", name: "maho-win", displayName: "maho-win", address: "https://relay.ferryx.dev", transport: "relay", authStatus: "paired", online: false, machineId: "m2", generation: "5", grantScope: "machine" },
  { hostId: "h-old", name: "old-mac", displayName: "old-mac", address: "https://relay.ferryx.dev", transport: "relay", authStatus: "paired", online: true, machineId: "m3", generation: "1", grantScope: "machine" },
];
remoteHostStore.setState((prev) => ({ ...prev, nativeStatus: "ready", hosts: Object.fromEntries(hosts.map((host) => [host.hostId, host])) }));

mockIPC((command, args) => {
  tick += 1;
  if (command === "cmd_system_resources") {
    log("local");
    return sample("macos", 16, 18, [
      session("claude-main", "/Users/me/code/ferryx", 42.5, 820),
      session("codex-review", "/Users/me/code/ferryx/.ferryx-worktrees/ws/resource-monitor", 8.1, 410),
      session("zsh", "/Users/me/code/ferryx/.ferryx-worktrees/ws/resource-monitor", 0.2, 12),
    ]);
  }
  if (command === "paired_host_operation") {
    const request = (args as { request: { hostId: string; generation: string; operation: { kind: string } } }).request;
    log(`paired:${request.hostId}:${request.operation.kind}`);
    if (request.hostId === "h-win") throw { code: "TRANSPORT", machineError: null, ambiguous: false, requestId: null };
    if (request.hostId === "h-old") throw { code: "PAIRED_HOST_CAPABILITY_UNAVAILABLE", machineError: null, ambiguous: false, requestId: null };
    return { hostId: request.hostId, generation: request.generation, result: { kind: "resources", data: sample("linux", 12, 9, [
      session("agent-build", "pty-integration", 96.0, 2300),
      session("logs", null, 0.1, 6),
    ]) } };
  }
  if (command === "cmd_terminal_close") { log(`close:${JSON.stringify(args)}`); return null; }
  throw new Error(`unmocked ${command}`);
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <div className="dark min-h-screen bg-background">
    <SystemResourcesDialog onClose={() => log("close-dialog")} />
  </div>,
);
