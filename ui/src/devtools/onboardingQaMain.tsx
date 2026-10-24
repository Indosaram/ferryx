import React from "react";
import ReactDOM from "react-dom/client";

import "../index.css";
import "../settings-runtime.css";
import { WelcomeWizard } from "../components/onboarding/WelcomeWizard";
import { GettingStartedChecklist } from "../components/onboarding/GettingStartedChecklist";
import { DaemonConnectionBanner } from "../components/DaemonConnectionBanner";
import { WhatsNewDialog } from "../components/onboarding/WhatsNewDialog";
import {
  ONBOARDING_STEP_ORDER,
  visiblePermissionKeys,
  wizardSteps,
  type OnboardingContext,
  type OnboardingStepId,
} from "../lib/onboarding";
import type {
  CliLauncherStatus,
  SystemPermissionsStatus,
} from "../lib/types";
import type { ResolvedAgent } from "../lib/agentsSettings";

declare global {
  interface Window {
    __qaSteps?: readonly OnboardingStepId[];
    __qaLog?: string[];
  }
}

if (!window.__qaLog) {
  window.__qaLog = [];
}

function logQa(event: string) {
  if (!window.__qaLog) {
    window.__qaLog = [];
  }
  window.__qaLog.push(event);
  console.info(`[onboarding-qa] ${event}`);
}

const params = new URLSearchParams(window.location.search);
const viewParam = params.get("view") ?? "wizard";
const platformParam = (params.get("platform") ?? "macos").toLowerCase();
const stepParam = parseInt(params.get("step") ?? "0", 10);
const initialStepIndex = Number.isNaN(stepParam) ? 0 : stepParam;
const codeParam = params.get("code") ?? "DAEMON_UNAVAILABLE";
const grantedParam = params.get("granted") === "1";
// ?cli=installed | unsupported; anything else is the not-installed fixture.
const cliParam = params.get("cli");
const doneParam: OnboardingStepId[] = (params.get("done") ?? "")
  .split(",")
  .map((step) => step.trim())
  .filter((step): step is OnboardingStepId =>
    (ONBOARDING_STEP_ORDER as readonly string[]).includes(step),
  );
const themeParam =
  params.get("theme") ?? (viewParam === "agents-light" ? "light" : "dark");

if (themeParam === "light") {
  document.documentElement.classList.remove("dark");
  document.documentElement.classList.add("light");
  document.documentElement.dataset.theme = "light";
} else {
  document.documentElement.classList.remove("light");
  document.documentElement.classList.add("dark");
  document.documentElement.dataset.theme = "dark";
}

function getPermissionsFixture(platform: string): SystemPermissionsStatus {
  if (platform === "windows") {
    return {
      platform: "windows",
      fullDiskAccess: {
        status: "unsupported",
        granted: false,
        canRequest: false,
        canOpenSettings: false,
        description: "",
      },
      accessibility: {
        status: "unsupported",
        granted: false,
        canRequest: false,
        canOpenSettings: false,
        description: "",
      },
      notifications: {
        status: "unknown",
        granted: false,
        canRequest: false,
        canOpenSettings: true,
        description: "Windows manages per-app notification access in Settings > System > Notifications.",
      },
      allGranted: false,
    };
  }

  if (platform === "linux") {
    return {
      platform: "linux",
      fullDiskAccess: {
        status: "unsupported",
        granted: false,
        canRequest: false,
        canOpenSettings: false,
        description: "",
      },
      accessibility: {
        status: "unsupported",
        granted: false,
        canRequest: false,
        canOpenSettings: false,
        description: "",
      },
      notifications: {
        status: "unknown",
        granted: false,
        canRequest: false,
        canOpenSettings: false,
        description: "Desktop notifications are managed by the desktop environment; most setups need no per-app grant.",
      },
      allGranted: false,
    };
  }

  return {
    platform: "macos",
    fullDiskAccess: {
      status: "denied",
      granted: false,
      canRequest: false,
      canOpenSettings: true,
      description: "Allows terminal subagents, worktrees, and git tools to read project files without macOS Photo Library or folder access prompts.",
    },
    accessibility: {
      status: "denied",
      granted: false,
      canRequest: true,
      canOpenSettings: true,
      description: "Allows global keyboard shortcuts, native terminal focus management, and automation.",
    },
    notifications: {
      status: "not_determined",
      granted: false,
      canRequest: true,
      canOpenSettings: true,
      description: "Allows desktop alerts for agent task completions, background builds, and version updates.",
    },
    allGranted: false,
  };
}

function grantAllVisible(status: SystemPermissionsStatus): SystemPermissionsStatus {
  const next: SystemPermissionsStatus = { ...status, allGranted: true };
  for (const key of visiblePermissionKeys(status)) {
    next[key] = { ...status[key], status: "granted", granted: true, canRequest: false };
  }
  return next;
}

const agentsFixture: ReadonlyArray<ResolvedAgent> = [
  {
    name: "claude",
    available: true,
    enabled: true,
    command: "claude",
    args: "",
    custom: false,
  },
  {
    name: "codex",
    available: false,
    enabled: true,
    command: "codex",
    args: "",
    custom: false,
  },
];

const cliBaseFixture: CliLauncherStatus = {
  launcherPath: "/Users/me/.local/bin/ferryx",
  isInstalled: false,
  isSymlink: false,
  currentTarget: null,
  activeExecutable: null,
  isSupported: true,
};

const cliFixture: CliLauncherStatus =
  cliParam === "installed"
    ? { ...cliBaseFixture, isInstalled: true }
    : cliParam === "unsupported"
      ? { ...cliBaseFixture, isSupported: false }
      : cliBaseFixture;

function OnboardingQaApp(): JSX.Element {
  const permissionsFixture = React.useMemo(() => {
    const base = getPermissionsFixture(platformParam);
    return grantedParam ? grantAllVisible(base) : base;
  }, []);

  const steps = React.useMemo(() => {
    const context: OnboardingContext = {
      permissions: permissionsFixture,
      agents: agentsFixture,
      // view=cli keeps the step even when unsupported (status unknown to the
      // wizard) so the step's own unsupported line can be inspected; any other
      // view uses the real fixture, where an unsupported launcher drops the step.
      cli: viewParam === "cli" ? null : cliFixture,
      projectCount: 0,
    };
    const computedSteps = wizardSteps(context);
    window.__qaSteps = computedSteps;
    return computedSteps;
  }, [permissionsFixture]);

  if (viewParam === "empty") {
    return (
      <div className="min-h-screen w-full bg-background flex flex-col items-center justify-center p-4">
        <GettingStartedChecklist
          permissionsSummary={{ granted: 1, total: 3 }}
          onAddProject={() => logQa("onAddProject")}
          onConnectMachine={() => logQa("onConnectMachine")}
          onOpenWelcome={() => logQa("onOpenWelcome")}
        />
      </div>
    );
  }

  if (viewParam === "banner" || viewParam === "banner-mismatch") {
    const bannerCode =
      viewParam === "banner-mismatch" ? "DAEMON_PROTOCOL_MISMATCH" : codeParam;
    return (
      <div className="min-h-screen w-full bg-background p-4 flex flex-col gap-4">
        <DaemonConnectionBanner
          error={{ code: bannerCode, message: "daemon down" }}
          onRetry={() => logQa("onRetry")}
        />
        <main className="flex-1 rounded-md border border-dashed border-zinc-800 p-8 flex items-center justify-center text-zinc-500">
          Main content area placeholder
        </main>
      </div>
    );
  }

  if (viewParam === "whatsnew") {
    return (
      <div className="min-h-screen w-full bg-background">
        <WhatsNewDialog
          version="2026.928.7"
          notes={"## Highlights\n- Welcome setup on every platform\n- Daemon connection banner"}
          onClose={() => logQa("onClose")}
        />
      </div>
    );
  }

  const agentsView = viewParam === "agents" || viewParam === "agents-light";
  const landingStep: OnboardingStepId | null = agentsView
    ? "agents"
    : viewParam === "cli"
      ? "cli"
      : null;
  const startIndex =
    landingStep && !params.has("step")
      ? Math.max(0, steps.indexOf(landingStep))
      : initialStepIndex;

  if (steps.length === 0) {
    return (
      <div className="min-h-screen w-full bg-background p-8 flex items-center justify-center">
        <p data-testid="qa-no-steps">No pending onboarding steps</p>
      </div>
    );
  }

  return (
    <div className="min-h-screen w-full bg-background">
      <WelcomeWizard
        steps={steps}
        agents={agentsFixture}
        isMac={platformParam === "macos"}
        doneSteps={doneParam}
        initialStepIndex={startIndex}
        onAddProject={() => logQa("onAddProject")}
        onConnectMachine={() => logQa("onConnectMachine")}
        onStepCompleted={(step: OnboardingStepId) => logQa(`onStepCompleted:${step}`)}
        onFinish={() => logQa("onFinish")}
        onSkip={() => logQa("onSkip")}
        onRemindLater={() => logQa("onRemindLater")}
        loadCliStatus={async () => {
          logQa("loadCliStatus");
          return cliFixture;
        }}
        installCli={async () => {
          logQa("installCli");
          return { ...cliFixture, isInstalled: true };
        }}
      />
    </div>
  );
}

const rootEl = document.getElementById("root");
if (rootEl) {
  ReactDOM.createRoot(rootEl).render(
    <React.StrictMode>
      <OnboardingQaApp />
    </React.StrictMode>,
  );
}
