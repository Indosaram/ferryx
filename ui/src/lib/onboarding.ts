import {
  ONBOARDING_STORAGE_KEY,
  PERMISSIONS_ONBOARDING_DISMISSED_STORAGE_KEY,
} from "./storageKeys";
import type { CliLauncherStatus, SystemPermissionsStatus } from "./types";

export type OnboardingStepId =
  | "account"
  | "intro"
  | "features"
  | "agents"
  | "cli"
  | "project"
  | "phone";

export const ONBOARDING_VERSION = 1;

/**
 * macOS permissions are not part of the first-run flow. Each one is requested
 * when the feature that needs it is first used (notifications on the first
 * notify, Full Disk Access when a read hits a TCC-protected path); Settings >
 * Permissions remains the place to review all of them.
 *
 * The product comes first; pairing a phone and signing in close the flow and
 * can both be skipped.
 */
export const ONBOARDING_STEP_ORDER: readonly OnboardingStepId[] = [
  "intro",
  "features",
  "agents",
  "cli",
  "project",
  "phone",
  "account",
];

export const OPEN_ONBOARDING_EVENT = "ferryx:open-onboarding";

export type OnboardingState = {
  version: 1;
  completedSteps: OnboardingStepId[];
  dismissed: boolean;
};

export type PermissionKey = "fullDiskAccess" | "accessibility" | "notifications";

export type OnboardingContext = {
  permissions: SystemPermissionsStatus | null;
  agents: ReadonlyArray<{ name: string; available: boolean }> | null;
  cli: CliLauncherStatus | null;
  projectCount: number;
  /** True once this computer is enrolled to the signed-in Ferryx account. */
  accountLinked?: boolean;
  /** Paired remote devices, when known; unknown never nags. */
  pairedDeviceCount?: number | null;
};

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

const PERMISSION_KEYS_IN_ORDER: readonly PermissionKey[] = [
  "fullDiskAccess",
  "accessibility",
  "notifications",
];

export function loadOnboardingState(
  storage: Storage | null = browserStorage()
): OnboardingState {
  const defaultState: OnboardingState = {
    version: 1,
    completedSteps: [],
    dismissed: false,
  };

  if (!storage) {
    return defaultState;
  }

  let isAbsent = false;
  try {
    const raw = storage.getItem(ONBOARDING_STORAGE_KEY);
    if (raw === null) {
      isAbsent = true;
    } else {
      const parsed = JSON.parse(raw);
      if (
        parsed &&
        typeof parsed === "object" &&
        parsed.version === 1 &&
        Array.isArray(parsed.completedSteps)
      ) {
        const knownSet = new Set<string>(ONBOARDING_STEP_ORDER);
        const filteredSteps: OnboardingStepId[] = [];
        const seen = new Set<OnboardingStepId>();
        for (const item of parsed.completedSteps) {
          if (typeof item === "string" && knownSet.has(item)) {
            const stepId = item as OnboardingStepId;
            if (!seen.has(stepId)) {
              seen.add(stepId);
              filteredSteps.push(stepId);
            }
          }
        }
        return {
          version: 1,
          completedSteps: filteredSteps,
          dismissed: Boolean(parsed.dismissed),
        };
      } else {
        isAbsent = true;
      }
    }
  } catch {
    isAbsent = true;
  }

  if (isAbsent) {
    try {
      const legacy = storage.getItem(PERMISSIONS_ONBOARDING_DISMISSED_STORAGE_KEY);
      if (legacy === "true") {
        const migrated: OnboardingState = {
          version: 1,
          completedSteps: ["intro"],
          dismissed: false,
        };
        saveOnboardingState(migrated, storage);
        try {
          storage.removeItem(PERMISSIONS_ONBOARDING_DISMISSED_STORAGE_KEY);
        } catch {
          void 0;
        }
        return migrated;
      }
    } catch {
      void 0;
    }
  }

  return defaultState;
}

export function saveOnboardingState(
  state: OnboardingState,
  storage: Storage | null = browserStorage()
): void {
  if (!storage) return;
  try {
    storage.setItem(ONBOARDING_STORAGE_KEY, JSON.stringify(state));
  } catch {
    void 0;
  }
}

export function markOnboardingStepsCompleted(
  steps: readonly OnboardingStepId[],
  storage: Storage | null = browserStorage()
): OnboardingState {
  const current = loadOnboardingState(storage);
  const currentSet = new Set<OnboardingStepId>(current.completedSteps);
  for (const step of steps) {
    currentSet.add(step);
  }
  const completedSteps = ONBOARDING_STEP_ORDER.filter((id) => currentSet.has(id));
  const next: OnboardingState = {
    ...current,
    completedSteps,
  };
  saveOnboardingState(next, storage);
  return next;
}

export function dismissOnboarding(
  storage: Storage | null = browserStorage()
): OnboardingState {
  const current = loadOnboardingState(storage);
  const next: OnboardingState = {
    ...current,
    dismissed: true,
  };
  saveOnboardingState(next, storage);
  return next;
}

export function resetOnboarding(
  storage: Storage | null = browserStorage()
): void {
  if (!storage) return;
  try {
    storage.removeItem(ONBOARDING_STORAGE_KEY);
  } catch {
    void 0;
  }
  try {
    storage.removeItem(PERMISSIONS_ONBOARDING_DISMISSED_STORAGE_KEY);
  } catch {
    void 0;
  }
}

export function visiblePermissionKeys(
  status: SystemPermissionsStatus | null
): PermissionKey[] {
  if (!status || status.platform === "web") {
    return [];
  }
  const result: PermissionKey[] = [];
  for (const key of PERMISSION_KEYS_IN_ORDER) {
    const item = status[key];
    if (!item) continue;
    if (
      item.status !== "unsupported" &&
      (item.granted || item.canRequest || item.canOpenSettings || item.status !== "unknown")
    ) {
      result.push(key);
    }
  }
  return result;
}

export function isOnboardingStepSatisfied(
  step: OnboardingStepId,
  ctx: OnboardingContext
): boolean {
  switch (step) {
    case "account":
      return ctx.accountLinked === true;
    case "intro":
      return ctx.projectCount > 0;
    case "agents":
      return ctx.agents !== null && ctx.agents.some((a) => a.available);
    case "cli":
      return ctx.cli === null || !ctx.cli.isSupported || ctx.cli.isInstalled;
    case "features":
      // Informational step: only completedSteps marks it as done.
      return false;
    case "project":
      return ctx.projectCount > 0;
    case "phone":
      return ctx.pairedDeviceCount == null || ctx.pairedDeviceCount > 0;
  }
}

export function wizardSteps(ctx: OnboardingContext): OnboardingStepId[] {
  return ONBOARDING_STEP_ORDER.filter((step) => {
    // Unknown CLI status (null) keeps the step so it can load its own status.
    if (step === "cli" && ctx.cli !== null && !ctx.cli.isSupported) {
      return false;
    }
    return true;
  });
}

export function satisfiedOnboardingSteps(
  state: OnboardingState,
  ctx: OnboardingContext
): OnboardingStepId[] {
  const completed = new Set<OnboardingStepId>(state.completedSteps);
  return wizardSteps(ctx).filter(
    (step) => completed.has(step) || isOnboardingStepSatisfied(step, ctx)
  );
}

export function pendingOnboardingSteps(
  state: OnboardingState,
  ctx: OnboardingContext
): OnboardingStepId[] {
  const completed = new Set<OnboardingStepId>(state.completedSteps);
  return wizardSteps(ctx).filter(
    (step) => !completed.has(step) && !isOnboardingStepSatisfied(step, ctx)
  );
}

/**
 * Where an auto-opened wizard should land: the first pending step that needs
 * action. "features" is informational and never auto-satisfied, so it is skipped
 * unless it is the only pending step; with nothing pending, the first step.
 */
export function initialWizardStepIndex(
  steps: readonly OnboardingStepId[],
  state: OnboardingState,
  ctx: OnboardingContext
): number {
  const pending = pendingOnboardingSteps(state, ctx);
  const target = pending.find((step) => step !== "features") ?? pending[0];
  if (!target) return 0;
  return Math.max(0, steps.indexOf(target));
}

export function shouldAutoOpenOnboarding(
  state: OnboardingState,
  ctx: OnboardingContext
): boolean {
  return (
    !state.dismissed &&
    ctx.permissions !== null &&
    ctx.permissions.platform !== "web" &&
    // "features" is informational and never auto-satisfied, and "cli" is optional
    // (users who finished the flow before it existed have no "cli" entry), so
    // neither may be the sole reason the wizard pops back open after an upgrade.
    pendingOnboardingSteps(state, ctx).some(
      // "account" and "phone" are optional for existing users too: neither may force the wizard open.
      (step) => step !== "features" && step !== "cli" && step !== "account" && step !== "phone"
    )
  );
}
