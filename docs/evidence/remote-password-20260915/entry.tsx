import React, { useState, useEffect, useRef } from "react";
import { createRoot } from "react-dom/client";
import "/ui/src/index.css";
import "/ui/src/settings-runtime.css";

import { RemoteSection } from "/ui/src/components/settings/RemoteSection";
import { AddMachineModal } from "/ui/src/components/settings/AddMachineModal";
import {
  createRemoteHostStore,
  remoteHostKey,
  remoteHostStore,
  type RemoteHostStore,
} from "/ui/src/state/remoteHostStore";
import {
  createPairedHostInventory,
  DEFAULT_RELAY_ORIGIN,
  type HostView,
  type PairedHostCommands,
} from "/ui/src/lib/pairedHostInventory";
import { resetSshHostsCache, type SshHost } from "/ui/src/lib/sshHosts";
import type { MachineProjectTarget } from "/ui/src/lib/machineNavigation";

// Labeled fixture datasets
const MOCK_PAIRED_HOSTS: HostView[] = [
  {
    hostId: remoteHostKey(DEFAULT_RELAY_ORIGIN, "box-linux-builder"),
    machineId: "box-linux-builder",
    relayOrigin: DEFAULT_RELAY_ORIGIN,
    displayLabel: "Linux Build Box",
    grantScope: "machine",
    generation: "1",
    authStatus: "paired",
    online: true,
  },
  {
    hostId: remoteHostKey(DEFAULT_RELAY_ORIGIN, "box-mac-mini"),
    machineId: "box-mac-mini",
    relayOrigin: DEFAULT_RELAY_ORIGIN,
    displayLabel: "M3 Mac Mini",
    grantScope: "mirror",
    generation: "2",
    authStatus: "needsMachineGrant",
    online: true,
  },
];

const MOCK_SSH_HOSTS: SshHost[] = [
  {
    id: "ssh-dev-server",
    label: "Dev Server",
    hostname: "192.168.1.50",
    username: "ubuntu",
    port: 22,
    source: "manual",
    authMethod: "password",
    disabled: false,
  },
  {
    id: "ssh-bastion",
    label: "Bastion Gateway",
    hostname: "bastion.corp.internal",
    username: "ec2-user",
    port: 2222,
    source: "config",
    authMethod: "key",
    identityFile: "~/.ssh/id_ed25519",
    jumpHost: "jump.corp.internal",
    disabled: false,
  },
];

// Active mock state
let currentScenario: "mixed" | "empty" = "mixed";
let activePairedHosts: HostView[] = [...MOCK_PAIRED_HOSTS];
let activeSshHosts: SshHost[] = [...MOCK_SSH_HOSTS];
const ipcCalls: Array<{ cmd: string; args?: any; time: number }> = [];

let parentEscapeCount = 0;
let windowEscapeCount = 0;
let openedProjectTarget: MachineProjectTarget | null = null;

// Mock Tauri IPC bridge
(window as any).isTauri = true;
(window as any).__TAURI_INTERNALS__ = {
  invoke: async (cmd: string, args: any) => {
    ipcCalls.push({ cmd, args, time: Date.now() });

    // Paired Host Commands
    if (cmd === "paired_host_list") {
      return activePairedHosts;
    }
    if (cmd === "paired_host_capabilities") {
      return { pairedHostInventoryV1: true, pairedDaemonProxyV1: true };
    }
    if (cmd === "paired_host_pair") {
      const pin = args?.request?.pin ?? "";
      if (pin === "000000" || pin === "bad-pin" || pin.toLowerCase().includes("fail")) {
        throw new Error("PAIR_FAILED");
      }
      if (pin === "888888") {
        const mirrorHost: HostView = {
          hostId: remoteHostKey(DEFAULT_RELAY_ORIGIN, "box-mirror-only"),
          machineId: "box-mirror-only",
          relayOrigin: DEFAULT_RELAY_ORIGIN,
          displayLabel: "Mirror Only Machine",
          grantScope: "mirror",
          generation: "1",
          authStatus: "needsMachineGrant",
          online: true,
        };
        activePairedHosts = [...activePairedHosts, mirrorHost];
        return mirrorHost;
      }
      const newHost: HostView = {
        hostId: remoteHostKey(DEFAULT_RELAY_ORIGIN, "box-paired-success"),
        machineId: "box-paired-success",
        relayOrigin: DEFAULT_RELAY_ORIGIN,
        displayLabel: args?.request?.displayLabel || "New Paired Machine",
        grantScope: "machine",
        generation: "1",
        authStatus: "paired",
        online: true,
      };
      activePairedHosts = [...activePairedHosts, newHost];
      return newHost;
    }
    if (cmd === "paired_host_forget") {
      const hostId = args?.request?.hostId;
      activePairedHosts = activePairedHosts.filter(h => h.hostId !== hostId);
      return;
    }
    if (cmd === "paired_host_operation") {
      const hostId = args?.request?.hostId;
      const generation = args?.request?.generation;
      const operation = args?.request?.operation;
      const machineId = hostId?.split("/").pop() || "box-paired-success";
      return {
        hostId,
        generation,
        result: {
          kind: operation?.kind || "capabilities",
          data: {
            apiVersion: 1,
            machineId,
            daemonEpoch: "1",
            platform: "linux",
            accessScope: "machine",
            permission: "control",
            capabilities: ["directoryBrowseV1", "machineWorkspaceV1"],
            limits: { directoryEntries: 1000, terminalSessions: 10 },
          },
        },
      };
    }

    // Browser-only password fixture: never contacts SSH or stores secrets on disk.
    if (cmd === "cmd_ssh_set_password") {
      if (args.password === "fixture-setter-reject") throw new Error("FIXTURE_PASSWORD_REJECTED");
      return;
    }

    // SSH Commands
    if (cmd === "cmd_ssh_list_hosts") {
      return activeSshHosts;
    }
    if (cmd === "cmd_ssh_test_connection") {
      const host = args?.host;
      if (host?.hostname?.includes("fail") || host?.hostname === "unreachable.corp") {
        return {
          host,
          reachable: false,
          lastError: "Host unreachable: Connection timed out",
          checkedAt: Date.now(),
        };
      }
      return {
        host,
        reachable: true,
        checkedAt: Date.now(),
        environment: {
          platform: "posix",
          executor: "bash",
          version: "5.2.15",
          home: `/home/${host?.username || "user"}`,
          temp: "/tmp",
          git: true,
        },
      };
    }
    if (cmd === "cmd_ssh_update_host") {
      const host = args?.host;
      const index = activeSshHosts.findIndex(h => h.id === host.id);
      if (index >= 0) {
        activeSshHosts[index] = host;
      } else {
        activeSshHosts = [...activeSshHosts, host];
      }
      return activeSshHosts;
    }
    if (cmd === "cmd_ssh_delete_host") {
      const id = args?.id;
      activeSshHosts = activeSshHosts.filter(h => h.id !== id);
      return activeSshHosts;
    }
    if (cmd === "cmd_ssh_read_system_config") {
      return {
        path: args?.configPath || "/Users/developer/.ssh/config",
        exists: true,
        rawText: "Host staging-east\n  HostName 10.0.1.20\n  User deploy\n\nHost prod-db\n  HostName 10.0.2.50\n  User admin\n",
        hosts: [
          {
            id: "sys-staging-east",
            label: "staging-east",
            hostname: "10.0.1.20",
            username: "deploy",
            port: 22,
            source: "config",
            authMethod: "agent",
            disabled: false,
          },
          {
            id: "sys-prod-db",
            label: "prod-db",
            hostname: "10.0.2.50",
            username: "admin",
            port: 22,
            source: "config",
            authMethod: "agent",
            disabled: false,
          },
        ],
      };
    }
    if (cmd === "cmd_ssh_import_config") {
      const text = args?.configText || "";
      const imported: SshHost = {
        id: `imported-${Date.now()}`,
        label: "imported-host",
        hostname: "imported.host.net",
        username: "root",
        port: 22,
        source: "config",
        authMethod: "agent",
        disabled: false,
      };
      activeSshHosts = [...activeSshHosts, imported];
      return activeSshHosts;
    }
    if (cmd === "cmd_ssh_prepare_integration") {
      return;
    }

    // Dialog Commands
    if (cmd === "plugin:dialog|open") {
      return "/Users/developer/.ssh/custom_config";
    }

    // Remote Access Commands
    if (cmd === "cmd_remote_status") {
      return { enabled: false, port: 43821, relayOrigin: DEFAULT_RELAY_ORIGIN };
    }
    if (cmd === "cmd_remote_devices") {
      return [];
    }
    if (cmd === "cmd_remote_pairing_create") {
      return { code: "987-654", expiresAt: Date.now() + 60000, relayOrigin: DEFAULT_RELAY_ORIGIN };
    }

    console.warn("[QA Mock IPC] Unhandled cmd:", cmd, args);
    throw new Error(`Unhandled fixture IPC: ${cmd}`);
  },
};

// Global escape listener to observe window-level escape leaks
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    windowEscapeCount++;
  }
});

function QAApp() {
  const [scenario, setScenario] = useState<"mixed" | "empty">("mixed");
  const [storeInstance, setStoreInstance] = useState(() => createRemoteHostStore());
  const [inventoryInstance, setInventoryInstance] = useState(() => {
    const commands: PairedHostCommands = {
      list: () => (window as any).__TAURI_INTERNALS__.invoke("paired_host_list"),
      capabilities: () => (window as any).__TAURI_INTERNALS__.invoke("paired_host_capabilities"),
      pair: (req) => (window as any).__TAURI_INTERNALS__.invoke("paired_host_pair", { request: req }),
      forget: (req) => (window as any).__TAURI_INTERNALS__.invoke("paired_host_forget", { request: req }),
      migrate: () => Promise.reject(new Error("not implemented")),
      read: () => Promise.reject(new Error("not implemented")),
    };
    return createPairedHostInventory(storeInstance, commands);
  });
  const [version, setVersion] = useState(0);

  const resetState = (nextScenario: "mixed" | "empty") => {
    currentScenario = nextScenario;
    if (nextScenario === "empty") {
      activePairedHosts = [];
      activeSshHosts = [];
    } else {
      activePairedHosts = [...MOCK_PAIRED_HOSTS];
      activeSshHosts = [...MOCK_SSH_HOSTS];
    }
    resetSshHostsCache();
    const newStore = createRemoteHostStore();
    newStore.setState(s => ({ ...s, nativeStatus: "ready", machineFeaturesEnabled: true }));
    remoteHostStore.setState(s => ({ ...s, nativeStatus: "ready", machineFeaturesEnabled: true }));
    const commands: PairedHostCommands = {
      list: () => (window as any).__TAURI_INTERNALS__.invoke("paired_host_list"),
      capabilities: () => (window as any).__TAURI_INTERNALS__.invoke("paired_host_capabilities"),
      pair: (req) => (window as any).__TAURI_INTERNALS__.invoke("paired_host_pair", { request: req }),
      forget: (req) => (window as any).__TAURI_INTERNALS__.invoke("paired_host_forget", { request: req }),
      migrate: () => Promise.reject(new Error("not implemented")),
      read: () => Promise.reject(new Error("not implemented")),
    };
    const newInventory = createPairedHostInventory(newStore, commands);
    void newInventory.refresh();
    newStore.subscribe(state => {
      remoteHostStore.setState(() => state);
    });
    setStoreInstance(newStore);
    setInventoryInstance(newInventory);
    setScenario(nextScenario);
    setVersion(v => v + 1);
  };

  useEffect(() => {
    storeInstance.setState(s => ({ ...s, nativeStatus: "ready", machineFeaturesEnabled: true }));
    remoteHostStore.setState(s => ({ ...s, nativeStatus: "ready", machineFeaturesEnabled: true }));
    void inventoryInstance.refresh();
    const unsub = storeInstance.subscribe(state => {
      remoteHostStore.setState(() => state);
    });
    return unsub;
  }, [storeInstance, inventoryInstance]);

  useEffect(() => {
    // Expose control API for automated runner
    (window as any).__qa = {
      setScenario: (sc: "mixed" | "empty") => resetState(sc),
      getScenario: () => scenario,
      getParentEscapeCount: () => parentEscapeCount,
      getWindowEscapeCount: () => windowEscapeCount,
      resetEscapeCounts: () => {
        parentEscapeCount = 0;
        windowEscapeCount = 0;
      },
      getIpcCalls: () => [...ipcCalls],
      getPersistedHosts: () => structuredClone(activeSshHosts),
      clearIpcCalls: () => {
        ipcCalls.length = 0;
      },
      getLastOpenedProject: () => openedProjectTarget,
      resetLastOpenedProject: () => {
        openedProjectTarget = null;
      },
      refresh: () => {
        setVersion(v => v + 1);
      },
    };
  }, [scenario]);

  return (
    <div className="min-h-screen bg-background text-foreground p-2 sm:p-6 overflow-x-hidden">
      {/* Mock Parent Settings Dialog wrapper to test Escape isolation */}
      <div
        id="mock-parent-settings-dialog"
        data-testid="mock-parent-settings-dialog"
        className="w-full max-w-4xl mx-auto rounded-lg border border-border bg-card/60 p-3 sm:p-6 shadow-sm overflow-x-hidden"
        tabIndex={-1}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            parentEscapeCount++;
          }
        }}
      >
        <header className="flex flex-wrap items-center justify-between gap-1 border-b border-border pb-2 mb-3 text-[11px] text-muted-foreground">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="font-semibold text-foreground">QA Harness:</span>
            <span>Remote Password - IPC FIXTURES ONLY</span>
            <span className="rounded bg-muted px-1 py-0.5 font-mono text-[9px]">
              {scenario}
            </span>
          </div>
          <div className="flex items-center gap-2 text-[10px]">
            <span id="qa-parent-escape-indicator">Parent Esc: {parentEscapeCount}</span>
            <span id="qa-window-escape-indicator">Window Esc: {windowEscapeCount}</span>
          </div>
        </header>

        <RemoteSection
          key={`remote-section-${version}-${scenario}`}
          store={storeInstance}
          inventory={inventoryInstance}
          onOpenProject={(target) => {
            openedProjectTarget = target;
          }}
        />
      </div>
    </div>
  );
}

const rootEl = document.getElementById("root");
if (rootEl) {
  createRoot(rootEl).render(<QAApp />);
}
