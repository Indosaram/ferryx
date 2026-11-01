// Real-browser QA for supervisor setup (no daemon): the onboarding SupervisorStep, the desktop
// setup banner over a main area, and the phone Supervisor home, each with a fixture SetupStatus.
//   ?view=step|banner|phone  &kind=enable_engine|fix_engine|adopt|none|...  &theme=dark|light
//   &fixture=upgrade-done|imported-adopt overrides kind with a named end state.
import React from "react";
import ReactDOM from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";

import "../index.css";
import "../settings-runtime.css";
import { WelcomeWizard } from "../components/onboarding/WelcomeWizard";
import { SupervisorSetupBanner } from "../components/supervisor/SupervisorSetupBanner";
import type { ResolvedAgent } from "../lib/agentsSettings";
import { SETUP_NEXT_ACTIONS, type SetupActions, type SetupNextAction, type SetupStatus } from "../lib/supervisorSetupModel";
import { fixtureLane, fixtureMachine, fixtureSnapshot } from "../remote/home/phoneHomeFixtures";
import { SupervisorHome } from "../remote/home/SupervisorHome";

const params = new URLSearchParams(window.location.search);
const view = params.get("view") ?? "step";
const kindParam = params.get("kind") ?? "enable_engine";
const kind: SetupNextAction = (SETUP_NEXT_ACTIONS as readonly string[]).includes(kindParam) ? (kindParam as SetupNextAction) : "enable_engine";
const viewOnly = params.get("viewOnly") === "1";
const theme = params.get("theme") ?? "dark";
document.documentElement.classList.remove(theme === "light" ? "dark" : "light");
document.documentElement.classList.add(theme === "light" ? "light" : "dark");
document.documentElement.dataset.theme = theme === "light" ? "light" : "dark";

function fixtureStatus(next: SetupNextAction): SetupStatus {
  const ready = next === "none" || next === "adopt" || next === "import_memory";
  return {
    engineEnabled: next !== "enable_engine",
    engineInstalled: true,
    engineState: next === "fix_engine" ? "degraded" : ready ? "ready" : next === "enable_engine" ? "absent" : "starting",
    engineFault: next === "fix_engine" ? { code: "MHC_CAPABILITY_MISSING", message: "The engine refused to start the supervisor profile." } : null,
    missingCapabilities: next === "fix_engine" ? ["memory_receipts", "memory_forget", "supervisor_work_tools", "effect_tools_deny"] : [],
    restartRequired: next === "restart_required",
    owner: ready ? { supervisorId: "sup-01", ownerPrincipal: "slack:T024BE7LD:U0G9QF9C6" } : null,
    projects: ready ? 3 : 0,
    intake: { unifiedOpen: ready, legacy: ready ? "stopped" : "open", origin: ready ? "cutover" : "upgrade", legacyHistory: true },
    autoCutover: true,
    memoryImport: { enabled: true, sessions: ready ? 42 : 0, turns: ready ? 318 : 0, lastAt: ready ? "2026-11-01T09:12:00Z" : null },
    adoptable: next === "adopt" ? 3 : 0,
    nextAction: next,
  };
}

/** Named end states: an upgrade that cut over and imported with nothing to adopt; the same with jobs to adopt. */
function namedFixture(name: string | null): SetupStatus | null {
  const upgraded: SetupStatus = {
    ...fixtureStatus("none"),
    intake: { unifiedOpen: true, legacy: "stopped", origin: "cutover", legacyHistory: true },
    memoryImport: { enabled: true, sessions: 42, turns: 318, lastAt: "2026-11-01T09:12:00Z" },
    adoptable: 0,
  };
  if (name === "upgrade-done") return upgraded;
  if (name === "imported-adopt") return { ...upgraded, adoptable: 3, nextAction: "adopt" };
  return null;
}

const status = namedFixture(params.get("fixture")) ?? fixtureStatus(kind);
const projects = [
  { name: "ferryx", path: "/Users/me/code/ferryx" },
  { name: "mhc", path: "/Users/me/code/mhc" },
  { name: "an-unusually-long-project-folder-name-that-must-truncate", path: "/Users/me/code/clients/acme/an-unusually-long-project-folder-name-that-must-truncate" },
];

mockIPC(
  (command) => {
    switch (command) {
      case "cmd_supervisor_snapshot":
        return { hub: { available: true, running: true }, lanes: [], watched: [], actions: [], setup: status };
      case "cmd_supervisor_setup_status":
      case "cmd_supervisor_setup_run":
        return status;
      case "cmd_supervisor_adopt_all":
        return { adopted: ["lane-a", "lane-b"], refused: [{ lane: "lane-c", code: "LEGACY_GOAL_OFF", message: "goal is off" }] };
      case "plugin:event|listen":
        return 1;
      default:
        return null;
    }
  },
  { shouldMockEvents: true },
);

const agents: ResolvedAgent[] = [{ name: "claude", available: true, enabled: true, command: "claude", args: "", custom: false }];

const qaActions: SetupActions = {
  run: async () => ({ kind: "ok", status }),
  adopt: async () => ({ kind: "ok", adopted: ["lane-a", "lane-b"], refused: [] }),
};

function QaApp() {
  if (view === "banner") {
    return (
      <div className="flex min-h-screen w-full flex-col bg-background">
        <SupervisorSetupBanner surface="desktop" status={status} actions={qaActions} projects={projects} dismissible className="mx-3 mt-2" />
        <main className="m-3 flex flex-1 items-center justify-center rounded-md border border-dashed border-border text-[12px] text-muted-foreground">
          Terminal area
        </main>
      </div>
    );
  }
  if (view === "phone") {
    const machines = [
      fixtureMachine({
        machineId: "studio",
        label: "Studio Mac",
        snapshot: { ...fixtureSnapshot({ lanes: [fixtureLane({ key: "a" })] }), setup: status },
      }),
    ];
    return (
      <div className="flex h-screen w-full flex-col">
        <SupervisorHome
          machines={machines}
          now={Date.now()}
          needsYou={new Set()}
          onSetPaused={async (_m, laneKey, paused) => ({ kind: "ok", laneKey, paused, changed: true })}
          onOpenLane={() => undefined}
          setupActions={() => (viewOnly ? null : qaActions)}
        />
      </div>
    );
  }
  return (
    <div className="min-h-screen w-full bg-background">
      <WelcomeWizard
        steps={["intro", "features", "agents", "project", "supervisor", "try", "phone", "account"]}
        initialStepIndex={4}
        agents={agents}
        isMac
        supervisorProjects={projects}
        onAddProject={() => undefined}
        onConnectMachine={() => undefined}
        onStepCompleted={(step) => console.info(`[supervisor-setup-qa] completed ${step}`)}
        onFinish={() => undefined}
        onSkip={() => undefined}
        onRemindLater={() => undefined}
      />
    </div>
  );
}

const rootEl = document.getElementById("root");
if (rootEl) {
  ReactDOM.createRoot(rootEl).render(
    <React.StrictMode>
      <QaApp />
    </React.StrictMode>,
  );
}
