import React from "react";
import ReactDOM from "react-dom/client";

import "../index.css";
import "../settings-runtime.css";
import { AddMachineModal } from "../components/settings/AddMachineModal";
import { MachineApprovalCard } from "../components/settings/remote/MachineApprovalCard";
import { PairedDaemonProjectForm } from "../components/PairedDaemonProjectForm";
import { remoteHostStore, remoteHostKey, type HostEndpoint } from "../state/remoteHostStore";
import discoveryFixtureJson from "./discoveryQaFixture.json";
import contractFixtures from "../../../docs/evidence/paired-daemon/fixtures/contracts.json";
import type { DiscoveredRepositories } from "../lib/pairedDaemonContracts";

// Recorded from the real discovery route on the QA host (see the C5 transcript).
const discoveryFixture = discoveryFixtureJson as unknown as Omit<DiscoveredRepositories, "repos"> & {
  repos: Array<Omit<DiscoveredRepositories["repos"][number], "target">>;
};
import type {
  AccountMachineView,
  IssueInstallKeyResponse,
  MachineRequestView,
} from "../remote/accountSession";

// Static QA harness for screenshots: ?view=install | approve | card | card-error | discovery, ?theme=light.
// Every account call is a local fixture; nothing reaches a server.

declare global {
  interface Window {
    __qaLog?: string[];
  }
}

function logQa(event: string) {
  if (!window.__qaLog) window.__qaLog = [];
  window.__qaLog.push(event);
  console.info(`[approval-qa] ${event}`);
}

const params = new URLSearchParams(window.location.search);
const viewParam = params.get("view") ?? "install";
const themeParam = params.get("theme") ?? "dark";
document.documentElement.classList.toggle("dark", themeParam !== "light");
document.documentElement.classList.toggle("light", themeParam === "light");
document.documentElement.dataset.theme = themeParam === "light" ? "light" : "dark";

const ORIGIN = "https://relay.ferryx.dev";
const FIXTURE_KEY = "fxk_" + "3f".repeat(32);
const nowSecs = Math.floor(Date.now() / 1000);

const installKeyFixture: IssueInstallKeyResponse = {
  keyId: "key_qa_01",
  key: FIXTURE_KEY,
  expiresAt: nowSecs + 3600,
  installCommand: {
    sh: `curl -fsSL ${ORIGIN}/install.sh | sh -s -- --key ${FIXTURE_KEY}`,
    ps1: `& ([scriptblock]::Create((irm ${ORIGIN}/install.ps1))) -Key ${FIXTURE_KEY}`,
  },
};

const requestFixture: MachineRequestView = {
  requestId: "req_qa_01",
  machineId: "c0ffee00-1111-4222-8333-444455556666",
  displayName: "noye",
  platform: "linux (Debian 13, x86_64)",
  appVersion: "2026.1023.1",
  requesterIp: "203.0.113.42",
  createdAt: nowSecs - 30,
  expiresAt: nowSecs + 870,
};

const machinesFixture: AccountMachineView[] = [];

// The discovery view replays a recorded daemon answer through the real native adapter seam.
const discoveryHost: HostEndpoint = { hostId: remoteHostKey(ORIGIN, "omarchy-qa"), machineId: "omarchy-qa", generation: "1", name: "omarchy (QA fixture)",
  address: ORIGIN, transport: "relay", authStatus: "paired", grantScope: "machine", online: true };
if (viewParam === "discovery") {
  (window as unknown as { isTauri: boolean }).isTauri = true;
  (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: async (command: string, args: { request: { hostId: string; generation: string; operation: { kind: string } } }) => {
      const { request } = args;
      logQa(`${command}:${request.operation.kind}`);
      const data = request.operation.kind === "capabilities"
        ? { ...contractFixtures.find(row => row.kind === "capabilities")!.value as object, machineId: discoveryHost.machineId,
          capabilities: ["directoryBrowseV1", "machineWorkspaceV1", "repoDiscoveryV1"] }
        : request.operation.kind === "discoverRepos"
          ? { ...discoveryFixture, repos: discoveryFixture.repos.map(repo => ({ ...repo, target: { kind: "pairedDaemon", hostId: discoveryHost.hostId } })) }
          : { ...contractFixtures.find(row => row.kind === "directories")!.value as object, path: discoveryFixture.path, homePath: discoveryFixture.path };
      return { hostId: request.hostId, generation: request.generation, result: { kind: request.operation.kind, data } };
    },
  };
  remoteHostStore.setState(s => ({ ...s, hosts: { [discoveryHost.hostId]: discoveryHost }, nativeStatus: "ready", machineFeaturesEnabled: true }));
}

function ApprovalQaApp() {
  if (viewParam === "discovery") {
    return (
      <div className="min-h-screen w-full bg-background">
        <PairedDaemonProjectForm onBack={() => logQa("back")} onClose={() => logQa("close")} onRegistered={project => logQa(`registered:${project.repoRoot}`)} />
      </div>
    );
  }
  if (viewParam === "card" || viewParam === "card-error") {
    return (
      <div className="flex min-h-screen w-full items-start justify-center bg-background p-4 sm:items-center">
        <div className="w-full max-w-sm">
          <MachineApprovalCard
            request={requestFixture}
            onApprove={async () => {
              logQa("approve");
              if (viewParam === "card-error") throw { code: "MACHINE_REQUEST_DECIDED", message: "decided" };
            }}
            onDeny={async () => {
              logQa("deny");
            }}
          />
        </div>
      </div>
    );
  }
  return (
    <div className="min-h-screen w-full bg-background">
      <AddMachineModal
        isOpen
        onClose={() => logQa("close")}
        onSuccess={() => logQa("success")}
        initialMethod={viewParam === "approve" ? "approve" : "install"}
        accountOrigin={ORIGIN}
        accountMachines={machinesFixture}
        onIssueEnrollmentCode={async () => "ABCD-1234"}
        onIssueInstallKey={async () => {
          logQa("issueInstallKey");
          return installKeyFixture;
        }}
        onLookupMachineRequest={async (code) => {
          logQa(`lookup:${code}`);
          return requestFixture;
        }}
        onApproveMachineRequest={async () => {
          logQa("approve");
          return { machineId: requestFixture.machineId };
        }}
        onDenyMachineRequest={async () => {
          logQa("deny");
        }}
      />
    </div>
  );
}

const rootEl = document.getElementById("root");
if (rootEl) {
  ReactDOM.createRoot(rootEl).render(
    <React.StrictMode>
      <ApprovalQaApp />
    </React.StrictMode>,
  );
}
