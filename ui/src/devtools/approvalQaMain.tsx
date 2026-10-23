import React from "react";
import ReactDOM from "react-dom/client";

import "../index.css";
import "../settings-runtime.css";
import { AddMachineModal } from "../components/settings/AddMachineModal";
import { MachineApprovalCard } from "../components/settings/remote/MachineApprovalCard";
import type {
  AccountMachineView,
  IssueInstallKeyResponse,
  MachineRequestView,
} from "../remote/accountSession";

// Static QA harness for screenshots: ?view=install | approve | card | card-error, ?theme=light.
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

function ApprovalQaApp() {
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
