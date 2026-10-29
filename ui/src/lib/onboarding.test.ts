import { describe, expect, it } from "vitest";

import {
  ONBOARDING_STEP_ORDER,
  ONBOARDING_VERSION,
  OPEN_ONBOARDING_EVENT,
  dismissOnboarding,
  initialWizardStepIndex,
  isOnboardingStepSatisfied,
  loadOnboardingState,
  markOnboardingStepsCompleted,
  pendingOnboardingSteps,
  resetOnboarding,
  satisfiedOnboardingSteps,
  saveOnboardingState,
  shouldAutoOpenOnboarding,
  visiblePermissionKeys,
  wizardSteps,
  type OnboardingContext,
  type OnboardingState,
} from "./onboarding";
import {
  ONBOARDING_STORAGE_KEY,
  PERMISSIONS_ONBOARDING_DISMISSED_STORAGE_KEY,
} from "./storageKeys";
import type { CliLauncherStatus, SystemPermissionsStatus } from "./types";

function createMockStorage(initial: Record<string, string> = {}): Storage {
  const store = new Map<string, string>(Object.entries(initial));
  return {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, val: string) => {
      store.set(key, val);
    },
    removeItem: (key: string) => {
      store.delete(key);
    },
    clear: () => store.clear(),
    key: () => null,
    length: store.size,
  };
}

const macosPermissionsStatus: SystemPermissionsStatus = {
  platform: "macos",
  allGranted: false,
  fullDiskAccess: {
    status: "denied",
    granted: false,
    canRequest: false,
    canOpenSettings: true,
    description: "Full disk access needed.",
  },
  accessibility: {
    status: "not_determined",
    granted: false,
    canRequest: true,
    canOpenSettings: true,
    description: "Accessibility needed.",
  },
  notifications: {
    status: "denied",
    granted: false,
    canRequest: true,
    canOpenSettings: true,
    description: "Notifications needed.",
  },
};

const macosAllGrantedStatus: SystemPermissionsStatus = {
  platform: "macos",
  allGranted: true,
  fullDiskAccess: {
    status: "granted",
    granted: true,
    canRequest: false,
    canOpenSettings: true,
    description: "Full disk access.",
  },
  accessibility: {
    status: "granted",
    granted: true,
    canRequest: false,
    canOpenSettings: true,
    description: "Accessibility.",
  },
  notifications: {
    status: "granted",
    granted: true,
    canRequest: false,
    canOpenSettings: true,
    description: "Notifications.",
  },
};

const windowsPermissionsStatus: SystemPermissionsStatus = {
  platform: "windows",
  allGranted: false,
  fullDiskAccess: {
    status: "unsupported",
    granted: false,
    canRequest: false,
    canOpenSettings: false,
    description: "Unsupported on Windows",
  },
  accessibility: {
    status: "unsupported",
    granted: false,
    canRequest: false,
    canOpenSettings: false,
    description: "Unsupported on Windows",
  },
  notifications: {
    status: "unknown",
    granted: false,
    canRequest: false,
    canOpenSettings: true,
    description: "Managed by Windows",
  },
};

const linuxPermissionsStatus: SystemPermissionsStatus = {
  platform: "linux",
  allGranted: false,
  fullDiskAccess: {
    status: "unsupported",
    granted: false,
    canRequest: false,
    canOpenSettings: false,
    description: "Unsupported on Linux",
  },
  accessibility: {
    status: "unsupported",
    granted: false,
    canRequest: false,
    canOpenSettings: false,
    description: "Unsupported on Linux",
  },
  notifications: {
    status: "unknown",
    granted: false,
    canRequest: false,
    canOpenSettings: false,
    description: "Managed by desktop environment",
  },
};

const webPermissionsStatus: SystemPermissionsStatus = {
  platform: "web",
  allGranted: false,
  fullDiskAccess: {
    status: "unsupported",
    granted: false,
    canRequest: false,
    canOpenSettings: false,
    description: "Unsupported",
  },
  accessibility: {
    status: "unsupported",
    granted: false,
    canRequest: false,
    canOpenSettings: false,
    description: "Unsupported",
  },
  notifications: {
    status: "unsupported",
    granted: false,
    canRequest: false,
    canOpenSettings: false,
    description: "Unsupported",
  },
};

const cliInstalled: CliLauncherStatus = {
  launcherPath: "/usr/local/bin/ferryx",
  isInstalled: true,
  isSymlink: true,
  currentTarget: "/Applications/Ferryx.app/Contents/MacOS/ferryx",
  activeExecutable: "/Applications/Ferryx.app/Contents/MacOS/ferryx",
  isSupported: true,
};

const cliNotInstalled: CliLauncherStatus = {
  launcherPath: "/usr/local/bin/ferryx",
  isInstalled: false,
  isSymlink: false,
  currentTarget: null,
  activeExecutable: null,
  isSupported: true,
};

const cliUnsupported: CliLauncherStatus = {
  launcherPath: "",
  isInstalled: false,
  isSymlink: false,
  currentTarget: null,
  activeExecutable: null,
  isSupported: false,
};

describe("onboarding exports and constants", () => {
  it("exports expected constants and step order", () => {
    expect(ONBOARDING_VERSION).toBe(1);
    expect(ONBOARDING_STEP_ORDER).toEqual([
      "intro",
      "features",
      "agents",
      "cli",
      "project",
      "phone",
      "account",
    ]);
    expect(OPEN_ONBOARDING_EVENT).toBe("ferryx:open-onboarding");
  });

  it("has no permissions step in the first-run flow", () => {
    expect(ONBOARDING_STEP_ORDER as readonly string[]).not.toContain("permissions");
  });
});

describe("loadOnboardingState", () => {
  it("returns default state when storage is empty", () => {
    const storage = createMockStorage();
    expect(loadOnboardingState(storage)).toEqual({
      version: 1,
      completedSteps: [],
      dismissed: false,
    });
  });

  it("is null-safe when storage is null", () => {
    expect(loadOnboardingState(null)).toEqual({
      version: 1,
      completedSteps: [],
      dismissed: false,
    });
  });

  it("loads valid v1 state from storage", () => {
    const storage = createMockStorage({
      [ONBOARDING_STORAGE_KEY]: JSON.stringify({
        version: 1,
        completedSteps: ["intro", "agents"],
        dismissed: true,
      }),
    });
    expect(loadOnboardingState(storage)).toEqual({
      version: 1,
      completedSteps: ["intro", "agents"],
      dismissed: true,
    });
  });

  it("drops unknown step ids and removes duplicate completed steps", () => {
    const storage = createMockStorage({
      [ONBOARDING_STORAGE_KEY]: JSON.stringify({
        version: 1,
        completedSteps: ["intro", "future_step", "intro", "permissions", "agents"],
        dismissed: false,
      }),
    });
    expect(loadOnboardingState(storage)).toEqual({
      version: 1,
      completedSteps: ["intro", "agents"],
      dismissed: false,
    });
  });

  it("keeps the features step id in completedSteps", () => {
    const storage = createMockStorage({
      [ONBOARDING_STORAGE_KEY]: JSON.stringify({
        version: 1,
        completedSteps: ["features", "intro"],
        dismissed: false,
      }),
    });
    expect(loadOnboardingState(storage).completedSteps).toEqual([
      "features",
      "intro",
    ]);
  });

  it("treats malformed JSON as absent and returns default if no legacy key", () => {
    const storage = createMockStorage({
      [ONBOARDING_STORAGE_KEY]: "invalid-json{",
    });
    expect(loadOnboardingState(storage)).toEqual({
      version: 1,
      completedSteps: [],
      dismissed: false,
    });
  });

  it("treats wrong version as absent and returns default if no legacy key", () => {
    const storage = createMockStorage({
      [ONBOARDING_STORAGE_KEY]: JSON.stringify({
        version: 2,
        completedSteps: ["intro"],
        dismissed: true,
      }),
    });
    expect(loadOnboardingState(storage)).toEqual({
      version: 1,
      completedSteps: [],
      dismissed: false,
    });
  });

  it("migrates from legacy key when absent, saves v1, and removes legacy key", () => {
    const storage = createMockStorage({
      [PERMISSIONS_ONBOARDING_DISMISSED_STORAGE_KEY]: "true",
    });
    const loaded = loadOnboardingState(storage);
    expect(loaded).toEqual({
      version: 1,
      completedSteps: ["intro"],
      dismissed: false,
    });
    expect(storage.getItem(PERMISSIONS_ONBOARDING_DISMISSED_STORAGE_KEY)).toBeNull();
    expect(JSON.parse(storage.getItem(ONBOARDING_STORAGE_KEY) ?? "{}")).toEqual({
      version: 1,
      completedSteps: ["intro"],
      dismissed: false,
    });
  });

  it("migrates from legacy key when v1 JSON is malformed", () => {
    const storage = createMockStorage({
      [ONBOARDING_STORAGE_KEY]: "{not valid json",
      [PERMISSIONS_ONBOARDING_DISMISSED_STORAGE_KEY]: "true",
    });
    const loaded = loadOnboardingState(storage);
    expect(loaded).toEqual({
      version: 1,
      completedSteps: ["intro"],
      dismissed: false,
    });
    expect(storage.getItem(PERMISSIONS_ONBOARDING_DISMISSED_STORAGE_KEY)).toBeNull();
  });
});

describe("saveOnboardingState", () => {
  it("persists state to storage", () => {
    const storage = createMockStorage();
    const state: OnboardingState = {
      version: 1,
      completedSteps: ["intro", "agents"],
      dismissed: false,
    };
    saveOnboardingState(state, storage);
    expect(JSON.parse(storage.getItem(ONBOARDING_STORAGE_KEY) ?? "{}")).toEqual(state);
  });

  it("is null-safe", () => {
    const state: OnboardingState = {
      version: 1,
      completedSteps: [],
      dismissed: false,
    };
    expect(() => saveOnboardingState(state, null)).not.toThrow();
  });
});

describe("markOnboardingStepsCompleted", () => {
  it("unions completed steps in ONBOARDING_STEP_ORDER without duplicates", () => {
    const storage = createMockStorage();
    const result1 = markOnboardingStepsCompleted(["agents", "intro"], storage);
    expect(result1.completedSteps).toEqual(["intro", "agents"]);

    const result2 = markOnboardingStepsCompleted(["features", "intro"], storage);
    expect(result2.completedSteps).toEqual(["intro", "features", "agents"]);

    const stored = JSON.parse(storage.getItem(ONBOARDING_STORAGE_KEY) ?? "{}");
    expect(stored.completedSteps).toEqual(["intro", "features", "agents"]);
  });
});

describe("dismissOnboarding", () => {
  it("sets dismissed: true while keeping completed steps", () => {
    const storage = createMockStorage();
    markOnboardingStepsCompleted(["intro"], storage);

    const dismissedState = dismissOnboarding(storage);
    expect(dismissedState).toEqual({
      version: 1,
      completedSteps: ["intro"],
      dismissed: true,
    });
    expect(loadOnboardingState(storage).dismissed).toBe(true);
  });
});

describe("resetOnboarding", () => {
  it("removes both v1 storage key and legacy permissions dismissed key", () => {
    const storage = createMockStorage({
      [ONBOARDING_STORAGE_KEY]: JSON.stringify({
        version: 1,
        completedSteps: ["intro"],
        dismissed: true,
      }),
      [PERMISSIONS_ONBOARDING_DISMISSED_STORAGE_KEY]: "true",
    });

    resetOnboarding(storage);
    expect(storage.getItem(ONBOARDING_STORAGE_KEY)).toBeNull();
    expect(storage.getItem(PERMISSIONS_ONBOARDING_DISMISSED_STORAGE_KEY)).toBeNull();
  });

  it("is null-safe", () => {
    expect(() => resetOnboarding(null)).not.toThrow();
  });
});

describe("visiblePermissionKeys", () => {
  it("returns empty array when status is null", () => {
    expect(visiblePermissionKeys(null)).toEqual([]);
  });

  it("returns empty array when platform is web", () => {
    expect(visiblePermissionKeys(webPermissionsStatus)).toEqual([]);
  });

  it("returns all three keys on macOS", () => {
    expect(visiblePermissionKeys(macosPermissionsStatus)).toEqual([
      "fullDiskAccess",
      "accessibility",
      "notifications",
    ]);
  });

  it("returns only notifications on Windows", () => {
    expect(visiblePermissionKeys(windowsPermissionsStatus)).toEqual([
      "notifications",
    ]);
  });

  it("returns empty array on Linux", () => {
    expect(visiblePermissionKeys(linuxPermissionsStatus)).toEqual([]);
  });
});

describe("isOnboardingStepSatisfied", () => {
  it("satisfies intro when projectCount > 0", () => {
    const baseCtx: OnboardingContext = {
      permissions: null,
      agents: null,
      cli: null,
      projectCount: 0,
      accountLinked: true,
    };
    expect(isOnboardingStepSatisfied("intro", baseCtx)).toBe(false);
    expect(
      isOnboardingStepSatisfied("intro", { ...baseCtx, projectCount: 1 })
    ).toBe(true);
  });

  it("satisfies project when projectCount > 0", () => {
    const baseCtx: OnboardingContext = {
      permissions: null,
      agents: null,
      cli: null,
      projectCount: 0,
      accountLinked: true,
    };
    expect(isOnboardingStepSatisfied("project", baseCtx)).toBe(false);
    expect(
      isOnboardingStepSatisfied("project", { ...baseCtx, projectCount: 2 })
    ).toBe(true);
  });

  it("satisfies agents only when an available agent exists, regardless of the cli", () => {
    expect(
      isOnboardingStepSatisfied("agents", {
        permissions: null,
        agents: null,
        cli: cliInstalled,
        projectCount: 0,
        accountLinked: true,
      })
    ).toBe(false);

    expect(
      isOnboardingStepSatisfied("agents", {
        permissions: null,
        agents: [{ name: "claude", available: false }],
        cli: cliInstalled,
        projectCount: 0,
        accountLinked: true,
      })
    ).toBe(false);

    expect(
      isOnboardingStepSatisfied("agents", {
        permissions: null,
        agents: [{ name: "claude", available: true }],
        cli: cliNotInstalled,
        projectCount: 0,
        accountLinked: true,
      })
    ).toBe(true);

    expect(
      isOnboardingStepSatisfied("agents", {
        permissions: null,
        agents: [{ name: "claude", available: true }],
        cli: cliInstalled,
        projectCount: 0,
        accountLinked: true,
      })
    ).toBe(true);

    expect(
      isOnboardingStepSatisfied("agents", {
        permissions: null,
        agents: [{ name: "claude", available: true }],
        cli: null,
        projectCount: 0,
        accountLinked: true,
      })
    ).toBe(true);

    expect(
      isOnboardingStepSatisfied("agents", {
        permissions: null,
        agents: [{ name: "claude", available: true }],
        cli: cliUnsupported,
        projectCount: 0,
        accountLinked: true,
      })
    ).toBe(true);
  });

  it("satisfies cli when installed, unsupported, or status is unknown", () => {
    const baseCtx: OnboardingContext = {
      permissions: null,
      agents: null,
      cli: null,
      projectCount: 0,
      accountLinked: true,
    };
    expect(isOnboardingStepSatisfied("cli", baseCtx)).toBe(true);
    expect(isOnboardingStepSatisfied("cli", { ...baseCtx, cli: cliInstalled })).toBe(true);
    expect(isOnboardingStepSatisfied("cli", { ...baseCtx, cli: cliUnsupported })).toBe(true);
    expect(isOnboardingStepSatisfied("cli", { ...baseCtx, cli: cliNotInstalled })).toBe(false);
  });
});

describe("pendingOnboardingSteps", () => {
  it("excludes completed and satisfied steps in ONBOARDING_STEP_ORDER", () => {
    const state: OnboardingState = {
      version: 1,
      completedSteps: ["intro"],
      dismissed: false,
    };
    const ctx: OnboardingContext = {
      permissions: macosPermissionsStatus,
      agents: [{ name: "claude", available: true }],
      cli: cliInstalled,
      projectCount: 0,
      accountLinked: true,
    };
    expect(pendingOnboardingSteps(state, ctx)).toEqual([
      "features",
      "project",
    ]);
  });

  it("lists cli as pending when the launcher is supported but not installed", () => {
    const state: OnboardingState = {
      version: 1,
      completedSteps: ["intro", "features", "agents", "project"],
      dismissed: false,
    };
    const ctx: OnboardingContext = {
      permissions: macosAllGrantedStatus,
      agents: [{ name: "claude", available: true }],
      cli: cliNotInstalled,
      projectCount: 1,
      accountLinked: true,
    };
    expect(pendingOnboardingSteps(state, ctx)).toEqual(["cli"]);
  });

  it("returns empty when all steps completed or satisfied", () => {
    const state: OnboardingState = {
      version: 1,
      completedSteps: ["intro", "features", "agents", "project"],
      dismissed: false,
    };
    const ctx: OnboardingContext = {
      permissions: macosPermissionsStatus,
      agents: [{ name: "claude", available: true }],
      cli: cliInstalled,
      projectCount: 1,
      accountLinked: true,
    };
    expect(pendingOnboardingSteps(state, ctx)).toEqual([]);
  });
});

describe("initialWizardStepIndex", () => {
  const steps = ["intro", "features", "agents", "project"] as const;

  it("skips the informational features step and lands on the first actionable one", () => {
    const state: OnboardingState = {
      version: 1,
      completedSteps: ["intro"],
      dismissed: false,
    };
    const ctx: OnboardingContext = {
      permissions: macosPermissionsStatus,
      agents: null,
      cli: cliInstalled,
      projectCount: 1,
      accountLinked: true,
    };
    expect(initialWizardStepIndex(steps, state, ctx)).toBe(steps.indexOf("agents"));
  });

  it("lands on features when it is the only pending step", () => {
    const state: OnboardingState = {
      version: 1,
      completedSteps: ["intro", "agents", "project"],
      dismissed: false,
    };
    const ctx: OnboardingContext = {
      permissions: macosPermissionsStatus,
      agents: null,
      cli: null,
      projectCount: 0,
      accountLinked: true,
    };
    expect(initialWizardStepIndex(steps, state, ctx)).toBe(steps.indexOf("features"));
  });

  it("returns 0 when nothing is pending", () => {
    const state: OnboardingState = {
      version: 1,
      completedSteps: [...steps],
      dismissed: false,
    };
    const ctx: OnboardingContext = {
      permissions: macosPermissionsStatus,
      agents: null,
      cli: null,
      projectCount: 0,
      accountLinked: true,
    };
    expect(initialWizardStepIndex(steps, state, ctx)).toBe(0);
  });
});

describe("wizardSteps", () => {
  it("includes features but never a permissions step on macOS", () => {
    const ctx: OnboardingContext = {
      permissions: macosPermissionsStatus,
      agents: null,
      cli: null,
      projectCount: 0,
      accountLinked: true,
    };
    expect(wizardSteps(ctx)).toEqual([
      "intro",
      "features",
      "agents",
      "cli",
      "project",
      "phone",
      "account",
    ]);
  });

  it("omits cli when the launcher is unsupported and keeps it when status is unknown", () => {
    const ctx: OnboardingContext = {
      permissions: macosPermissionsStatus,
      agents: null,
      cli: cliUnsupported,
      projectCount: 0,
      accountLinked: true,
    };
    expect(wizardSteps(ctx)).not.toContain("cli");
    expect(wizardSteps({ ...ctx, cli: null })).toContain("cli");
    expect(wizardSteps({ ...ctx, cli: cliNotInstalled })).toContain("cli");
    expect(wizardSteps({ ...ctx, cli: cliInstalled })).toContain("cli");
  });

  it("uses the same steps on Linux", () => {
    const ctx: OnboardingContext = {
      permissions: linuxPermissionsStatus,
      agents: null,
      cli: null,
      projectCount: 0,
      accountLinked: true,
    };
    expect(wizardSteps(ctx)).toEqual([
      "intro",
      "features",
      "agents",
      "cli",
      "project",
      "phone",
      "account",
    ]);
  });
});

describe("satisfiedOnboardingSteps", () => {
  it("reports completed and satisfied wizard steps in wizard order", () => {
    const storage = createMockStorage();
    saveOnboardingState(
      {
        version: 1,
        completedSteps: ["intro", "features"],
        dismissed: false,
      },
      storage,
    );
    const state = loadOnboardingState(storage);
    const ctx: OnboardingContext = {
      permissions: macosPermissionsStatus,
      agents: [{ name: "claude", available: true }],
      cli: cliInstalled,
      projectCount: 1,
      accountLinked: true,
    };
    expect(satisfiedOnboardingSteps(state, ctx)).toEqual([
      "intro",
      "features",
      "agents",
      "cli",
      "project",
      "phone",
      "account",
    ]);
  });

  it("stays empty when nothing is completed or satisfied", () => {
    const state: OnboardingState = {
      version: 1,
      completedSteps: [],
      dismissed: false,
    };
    const ctx: OnboardingContext = {
      permissions: macosPermissionsStatus,
      agents: null,
      cli: cliNotInstalled,
      projectCount: 0,
      accountLinked: true,
    };
    expect(satisfiedOnboardingSteps(state, ctx)).toEqual(["phone", "account"]);
  });

  it("reports only cli when its status is unknown and nothing else is done", () => {
    const state: OnboardingState = {
      version: 1,
      completedSteps: [],
      dismissed: false,
    };
    const ctx: OnboardingContext = {
      permissions: macosPermissionsStatus,
      agents: null,
      cli: null,
      projectCount: 0,
      accountLinked: true,
    };
    expect(satisfiedOnboardingSteps(state, ctx)).toEqual(["cli", "phone", "account"]);
  });
});

describe("shouldAutoOpenOnboarding", () => {
  const pendingCtx: OnboardingContext = {
    permissions: macosPermissionsStatus,
    agents: null,
    cli: null,
    projectCount: 0,
    accountLinked: true,
  };

  it("returns true when not dismissed with valid permissions and pending steps", () => {
    const state: OnboardingState = {
      version: 1,
      completedSteps: [],
      dismissed: false,
    };
    expect(shouldAutoOpenOnboarding(state, pendingCtx)).toBe(true);
  });

  it("returns false when dismissed", () => {
    const state: OnboardingState = {
      version: 1,
      completedSteps: [],
      dismissed: true,
    };
    expect(shouldAutoOpenOnboarding(state, pendingCtx)).toBe(false);
  });

  it("returns false when permissions is null", () => {
    const state: OnboardingState = {
      version: 1,
      completedSteps: [],
      dismissed: false,
    };
    expect(
      shouldAutoOpenOnboarding(state, { ...pendingCtx, permissions: null })
    ).toBe(false);
  });

  it("returns false on web platform", () => {
    const state: OnboardingState = {
      version: 1,
      completedSteps: [],
      dismissed: false,
    };
    expect(
      shouldAutoOpenOnboarding(state, {
        ...pendingCtx,
        permissions: webPermissionsStatus,
      })
    ).toBe(false);
  });

  it("returns false when no pending steps remain", () => {
    const state: OnboardingState = {
      version: 1,
      completedSteps: ["intro", "features", "agents", "project"],
      dismissed: false,
    };
    expect(shouldAutoOpenOnboarding(state, pendingCtx)).toBe(false);
  });

  it("does not reopen for users who finished the flow before the cli step existed", () => {
    const storage = createMockStorage({
      [ONBOARDING_STORAGE_KEY]: JSON.stringify({
        version: 1,
        completedSteps: ["intro", "features", "agents", "project"],
        dismissed: false,
      }),
    });
    const state = loadOnboardingState(storage);
    expect(state.completedSteps).not.toContain("cli");

    const satisfiedCtx: OnboardingContext = {
      permissions: macosAllGrantedStatus,
      agents: [{ name: "claude", available: true }],
      cli: cliInstalled,
      projectCount: 1,
      accountLinked: true,
    };
    expect(pendingOnboardingSteps(state, satisfiedCtx)).toEqual([]);
    expect(shouldAutoOpenOnboarding(state, satisfiedCtx)).toBe(false);

    const notInstalledCtx = { ...satisfiedCtx, cli: cliNotInstalled };
    expect(pendingOnboardingSteps(state, notInstalledCtx)).toEqual(["cli"]);
    expect(shouldAutoOpenOnboarding(state, notInstalledCtx)).toBe(false);
  });

  it("loads a persisted cli completion", () => {
    const storage = createMockStorage({
      [ONBOARDING_STORAGE_KEY]: JSON.stringify({
        version: 1,
        completedSteps: ["agents", "cli"],
        dismissed: false,
      }),
    });
    expect(loadOnboardingState(storage).completedSteps).toEqual(["agents", "cli"]);
  });
});

describe("features step", () => {
  const satisfiedCtx: OnboardingContext = {
    permissions: macosAllGrantedStatus,
    agents: [{ name: "claude", available: true }],
    cli: cliInstalled,
    projectCount: 5,
    accountLinked: true,
  };

  it("is never auto-satisfied by context", () => {
    expect(isOnboardingStepSatisfied("features", satisfiedCtx)).toBe(false);
    expect(
      isOnboardingStepSatisfied("features", { ...satisfiedCtx, projectCount: 0 })
    ).toBe(false);
  });

  it("stays pending until it is recorded in completedSteps", () => {
    const upgraded: OnboardingState = {
      version: 1,
      completedSteps: ["intro", "agents", "project"],
      dismissed: false,
    };
    expect(pendingOnboardingSteps(upgraded, satisfiedCtx)).toEqual(["features"]);
    expect(satisfiedOnboardingSteps(upgraded, satisfiedCtx)).toEqual([
      "intro",
      "agents",
      "cli",
      "project",
      "phone",
      "account",
    ]);

    const doneState = markOnboardingStepsCompleted(
      ["features"],
      createMockStorage(),
    );
    expect(pendingOnboardingSteps(doneState, satisfiedCtx)).toEqual([]);
    expect(satisfiedOnboardingSteps(doneState, satisfiedCtx)).toContain(
      "features",
    );
  });

  it("never triggers auto-open on its own after upgrade", () => {
    const upgraded: OnboardingState = {
      version: 1,
      completedSteps: ["intro", "agents", "project"],
      dismissed: false,
    };
    expect(pendingOnboardingSteps(upgraded, satisfiedCtx)).toEqual(["features"]);
    expect(shouldAutoOpenOnboarding(upgraded, satisfiedCtx)).toBe(false);

    const fresh: OnboardingState = {
      version: 1,
      completedSteps: [],
      dismissed: false,
    };
    const openCtx: OnboardingContext = {
      permissions: macosPermissionsStatus,
      agents: null,
      cli: null,
      projectCount: 0,
      accountLinked: true,
    };
    expect(shouldAutoOpenOnboarding(fresh, openCtx)).toBe(true);
  });
});

describe("account step", () => {
  it("is pending until this computer is linked, without forcing auto-open", () => {
    const ctx = { permissions: null, agents: null, cli: null, projectCount: 1, accountLinked: false };
    expect(isOnboardingStepSatisfied("account", ctx)).toBe(false);
    expect(isOnboardingStepSatisfied("account", { ...ctx, accountLinked: true })).toBe(true);
    const steps = wizardSteps(ctx);
    expect(steps[0]).toBe("intro");
    expect(steps[steps.length - 1]).toBe("account");
  });
});

describe("phone step", () => {
  const ctx: OnboardingContext = {
    permissions: macosPermissionsStatus,
    agents: [{ name: "claude", available: true }],
    cli: cliInstalled,
    projectCount: 1,
    accountLinked: true,
  };

  it("sits right before the account step", () => {
    const steps = wizardSteps(ctx);
    expect(steps.slice(-2)).toEqual(["phone", "account"]);
  });

  it("is pending only when no device is paired and never forces auto-open", () => {
    expect(isOnboardingStepSatisfied("phone", { ...ctx, pairedDeviceCount: 0 })).toBe(false);
    expect(isOnboardingStepSatisfied("phone", { ...ctx, pairedDeviceCount: 1 })).toBe(true);
    expect(isOnboardingStepSatisfied("phone", { ...ctx, pairedDeviceCount: null })).toBe(true);
    const state: OnboardingState = {
      version: 1,
      completedSteps: ["intro", "features", "agents", "cli", "project"],
      dismissed: false,
    };
    const unpaired = { ...ctx, pairedDeviceCount: 0 };
    expect(pendingOnboardingSteps(state, unpaired)).toEqual(["phone"]);
    expect(shouldAutoOpenOnboarding(state, unpaired)).toBe(false);
  });
});
