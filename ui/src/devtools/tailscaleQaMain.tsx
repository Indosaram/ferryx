import ReactDOM from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { SshHost, SshTargetSummary } from "../lib/sshHosts";
import { createRemoteHostStore } from "../state/remoteHostStore";
import { createPairedHostInventory, type PairedHostCommands } from "../lib/pairedHostInventory";
import "../index.css";
import "../settings-runtime.css";

// Fixture-only entry: no real daemon, SSH target, account, or terminal is contacted.
const mode = new URLSearchParams(location.search).get("scenario") ?? "ready";
document.documentElement.dataset.theme = new URLSearchParams(location.search).get("theme") ?? "dark";
Object.defineProperty(globalThis, "isTauri", { value: true, configurable: true });
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
const diagnostic = mode === "auth"
  ? { code: "SSH_AUTH_FAILED", message: "SSH authentication requires a configured user or key.", details: { stage: "authentication" } }
  : mode === "offline"
    ? { code: "REMOTE_UNAVAILABLE", message: "Tailnet machine is offline.", details: { stage: "offline" } }
    : mode === "provision"
      ? { code: "CLI_EXECUTABLE_NOT_FOUND", message: "Bundled helper for this target is unavailable.", details: { stage: "provisioning" } }
      : null;
const target: SshTargetSummary = {
  host,
  reachable: mode === "ready" || mode === "provision",
  checkedAt: 1,
  environment: mode === "ready" || mode === "provision" ? environment : null,
  helper: mode === "ready" ? "installed" : "unknown",
  diagnostic,
  lastError: diagnostic?.message,
};
mockIPC((command) => {
  switch (command) {
    case "cmd_ssh_list_hosts": return [host];
    case "cmd_ssh_sync_tailscale":
      if (mode === "connecting") return new Promise(() => {});
      return {
        hosts: [host],
        targets: mode === "discovery-error" ? [] : [target],
        discoveryError: mode === "discovery-error"
          ? { code: "CLI_EXECUTABLE_NOT_FOUND", message: "Tailscale CLI is not installed.", details: { stage: "discovery" } }
          : null,
      };
    case "cmd_remote_status": return { enabled: false, mode: "off", port: 43821 };
    case "cmd_remote_devices": return [];
    case "plugin:event|listen": return 1;
    default: return null;
  }
}, { shouldMockEvents: true });
const commands: PairedHostCommands = {
  list: async () => [],
  capabilities: async () => ({ pairedHostInventoryV1: true, pairedDaemonProxyV1: true }),
  pair: async () => { throw new Error("Fixture pairing is disabled"); },
  forget: async () => {},
  migrate: async () => { throw new Error("Fixture migration is disabled"); },
  read: async () => { throw new Error("Fixture credentials are unavailable"); },
};
const store = createRemoteHostStore();
const inventory = createPairedHostInventory(store, commands);
const { RemoteSection } = await import("../components/settings/RemoteSection");
const { startTailscaleDiscovery } = await import("../lib/tailscaleDiscovery");
const stopDiscovery = startTailscaleDiscovery();
import.meta.hot?.dispose(stopDiscovery);
const root = document.getElementById("root");
if (root) {
  ReactDOM.createRoot(root).render(
    <main className="mx-auto max-w-3xl p-4 sm:p-6">
      <RemoteSection store={store} inventory={inventory} accountOrigin="https://account.invalid" accountSessionToken={null} onOpenSshProject={() => { document.documentElement.dataset.fixtureProjectOpened = "true"; }} />
    </main>,
  );
}
