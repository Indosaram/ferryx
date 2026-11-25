import { describe, expect, it } from "vitest";

import type { OnboardingContext } from "./onboarding";
import { buildSetupProgress, SETUP_PROGRESS_STEPS, summarizeSetupProgress } from "./setupProgress";
import type { CliLauncherStatus } from "./types";

const cliInstalled: CliLauncherStatus = {
  launcherPath: "/usr/local/bin/ferryx",
  isInstalled: true,
  isSymlink: true,
  currentTarget: "/Applications/Ferryx.app/Contents/MacOS/ferryx",
  activeExecutable: "/Applications/Ferryx.app/Contents/MacOS/ferryx",
  isSupported: true,
};

const cliNotInstalled: CliLauncherStatus = { ...cliInstalled, isInstalled: false, isSymlink: false, currentTarget: null, activeExecutable: null };

const satisfied: OnboardingContext = {
  permissions: null,
  agents: [{ name: "codex", available: true }],
  cli: cliInstalled,
  projectCount: 1,
  accountLinked: true,
  pairedDeviceCount: 1,
  supervisorReady: true,
};

const empty: OnboardingContext = {
  permissions: null,
  agents: [{ name: "codex", available: false }],
  cli: cliNotInstalled,
  projectCount: 0,
  accountLinked: false,
  pairedDeviceCount: 0,
  supervisorReady: false,
};

describe("buildSetupProgress", () => {
  it("marks nothing done and points at the first project for a fresh install", () => {
    const summary = summarizeSetupProgress(buildSetupProgress(empty));

    expect(summary.done).toBe(0);
    expect(summary.total).toBe(6);
    expect(summary.nextItem?.id).toBe("project");
  });

  it("reports complete setup as complete with no next item", () => {
    const summary = summarizeSetupProgress(buildSetupProgress(satisfied));

    expect(summary.done).toBe(summary.total);
    expect(summary.nextItem).toBeNull();
  });

  it("picks the earliest incomplete goal, not a later one", () => {
    const summary = summarizeSetupProgress(
      buildSetupProgress({ ...satisfied, projectCount: 0, pairedDeviceCount: 0 })
    );

    expect(summary.nextItem?.id).toBe("project");
    expect(summary.done).toBe(summary.total - 2);
  });

  it("treats unknown optional inputs as done so it never nags", () => {
    const summary = summarizeSetupProgress(
      buildSetupProgress({ ...satisfied, pairedDeviceCount: null, supervisorReady: null })
    );

    expect(summary.nextItem).toBeNull();
  });

  it("counts a goal the wizard recorded even when the capability is still missing", () => {
    // The wizard marks non-optional steps completed when the user advances past them, so a
    // pending capability it already recorded must not resurface here as a chore.
    const summary = summarizeSetupProgress(buildSetupProgress(empty, ["project", "cli"]));

    expect(summary.nextItem?.id).toBe("agents");
    expect(summary.done).toBe(2);
    expect(buildSetupProgress(empty, ["cli"]).find((item) => item.id === "cli")?.done).toBe(true);
  });

  it("keeps a recorded goal excluded from the next-item pointer", () => {
    const summary = summarizeSetupProgress(buildSetupProgress(empty, SETUP_PROGRESS_STEPS));

    expect(summary.nextItem).toBeNull();
    expect(summary.done).toBe(summary.total);
  });
});
