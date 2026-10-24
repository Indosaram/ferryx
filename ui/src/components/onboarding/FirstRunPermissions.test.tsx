import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { wizardSteps } from "../../lib/onboarding";
import type { CliLauncherStatus, SystemPermissionsStatus } from "../../lib/types";
import { WelcomeWizard } from "./WelcomeWizard";

const permissionCalls = vi.hoisted(() => ({
  getSystemPermissionsStatus: vi.fn(),
  openPermissionsSystemSettings: vi.fn(),
  requestAccessibilityPermission: vi.fn(),
  requestNotificationPermission: vi.fn(),
}));

vi.mock("../../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/tauri")>()),
  ...permissionCalls,
}));

const macStatusNothingGranted: SystemPermissionsStatus = {
  platform: "macos",
  allGranted: false,
  fullDiskAccess: {
    status: "denied",
    granted: false,
    canRequest: false,
    canOpenSettings: true,
    description: "Full disk access.",
  },
  accessibility: {
    status: "denied",
    granted: false,
    canRequest: true,
    canOpenSettings: true,
    description: "Accessibility.",
  },
  notifications: {
    status: "not_determined",
    granted: false,
    canRequest: true,
    canOpenSettings: true,
    description: "Notifications.",
  },
};

const cliNotInstalled: CliLauncherStatus = {
  launcherPath: "/Users/me/.local/bin/ferryx",
  isInstalled: false,
  isSymlink: false,
  currentTarget: null,
  activeExecutable: null,
  isSupported: true,
};

describe("first-run wizard and macOS permissions", () => {
  beforeEach(() => {
    for (const fn of Object.values(permissionCalls)) fn.mockReset();
  });

  afterEach(() => {
    cleanup();
  });

  it("walks every macOS first-run step without a single permission request", async () => {
    const steps = wizardSteps({
      permissions: macStatusNothingGranted,
      agents: null,
      cli: cliNotInstalled,
      projectCount: 0,
      accountLinked: true,
    }).filter((step) => step !== "account");
    expect(steps as readonly string[]).not.toContain("permissions");

    const onFinish = vi.fn();
    render(
      <WelcomeWizard
        steps={steps}
        agents={[]}
        isMac
        onAddProject={vi.fn()}
        onConnectMachine={vi.fn()}
        onStepCompleted={vi.fn()}
        onFinish={onFinish}
        onSkip={vi.fn()}
        onRemindLater={vi.fn()}
        loadCliStatus={async () => cliNotInstalled}
        installCli={async () => ({ ...cliNotInstalled, isInstalled: true })}
      />,
    );

    for (let index = 0; index < steps.length - 1; index += 1) {
      expect(screen.queryByText("Grant system access")).toBeNull();
      fireEvent.click(screen.getByTestId("onboarding-next"));
    }
    await screen.findByTestId("onboarding-finish");
    fireEvent.click(screen.getByTestId("onboarding-finish"));

    expect(onFinish).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId(/^onboarding-permission-card-/)).toBeNull();
    expect(permissionCalls.requestNotificationPermission).not.toHaveBeenCalled();
    expect(permissionCalls.requestAccessibilityPermission).not.toHaveBeenCalled();
    expect(permissionCalls.openPermissionsSystemSettings).not.toHaveBeenCalled();
    expect(permissionCalls.getSystemPermissionsStatus).not.toHaveBeenCalled();
  });
});
