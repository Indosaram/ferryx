import {
  ONBOARDING_STORAGE_KEY,
  PERMISSIONS_ONBOARDING_DISMISSED_STORAGE_KEY,
} from "./storageKeys";
import type { CliLauncherStatus, SystemPermissionsStatus } from "./types";

export type OnboardingStepId =
  | "account"
  | "intro"
  | "features"
  | "import"
  | "agents"
  | "enroll"
  | "skills"
  | "cli"
  | "project"
  | "supervisor"
  | "try"
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
  "import",
  "agents",
  "enroll",
  "skills",
  "cli",
  "project",
  "supervisor",
  "try",
  "phone",
  "account",
];

export const OPEN_ONBOARDING_EVENT = "ferryx:open-onboarding";

export type OnboardingState = {
  version: 1;
  completedSteps: OnboardingStepId[];
  dismissed: boolean;
  updatedAt?: number;
};

export type PermissionKey = "fullDiskAccess" | "accessibility" | "notifications";

export type OnboardingContext = {
  permissions: SystemPermissionsStatus | null;
  agents: ReadonlyArray<{ name: string; available: boolean }> | null;
  cli: CliLauncherStatus | null;
  projectCount: number;
  /** True once this computer is enrolled to the signed-in Ferryx account. */
  accountLinked?: boolean;
  enrollmentReady?: boolean;
  skillsReady?: boolean;
  /** Paired remote devices, when known; unknown never nags. */
  pairedDeviceCount?: number | null;
  /** True once supervisor setup is finished; null or absent (unknown, older daemon) never nags. */
  supervisorReady?: boolean | null;
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
          ...(Number.isSafeInteger(parsed.updatedAt) && parsed.updatedAt >= 0
            ? { updatedAt: parsed.updatedAt } : {}),
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
    if (storage === browserStorage()) {
      window.dispatchEvent(new Event("ferryx:onboarding-changed"));
    }
    if (storage === browserStorage() && typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
      persistence = persistence.then(async () => {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("cmd_onboarding_set", { state });
      }).catch((error) => {
        console.error("Could not persist onboarding", error);
        window.dispatchEvent(new CustomEvent("ferryx:onboarding-persistence-error", { detail: error }));
      });
    }
  } catch {
    void 0;
  }
}

let persistence: Promise<void> = Promise.resolve();

/** Hydrate before opening; localStorage is only a cache on desktop. */
export async function hydrateOnboardingState(): Promise<OnboardingState> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return loadOnboardingState();
  await persistence;
  const cache = loadOnboardingState();
  const { invoke } = await import("@tauri-apps/api/core");
  const state = await invoke<OnboardingState>("cmd_onboarding_get", { cache });
  browserStorage()?.setItem(ONBOARDING_STORAGE_KEY, JSON.stringify(state));
  return state;
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
    updatedAt: Math.floor(Date.now() / 1000),
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
    updatedAt: Math.floor(Date.now() / 1000),
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
  if (storage === browserStorage() && typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
    saveOnboardingState({ version: 1, completedSteps: [], dismissed: false }, storage);
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
    case "enroll":
      return ctx.enrollmentReady !== false;
    case "skills":
      return ctx.skillsReady !== false;
    case "account":
      return ctx.accountLinked === true;
    case "intro":
      return ctx.projectCount > 0;
    case "agents":
      return ctx.agents !== null && ctx.agents.some((a) => a.available);
    case "cli":
      return ctx.cli === null || !ctx.cli.isSupported || ctx.cli.isInstalled;
    case "features":
    case "import":
      // Informational step: only completedSteps marks it as done.
      return false;
    case "project":
      return ctx.projectCount > 0;
    case "try":
      // An offer, not a requirement: it is never pending, so it neither reopens the
      // wizard nor becomes the landing step. Only a created worktree records it.
      return true;
    case "phone":
      return ctx.pairedDeviceCount == null || ctx.pairedDeviceCount > 0;
    case "supervisor":
      return ctx.supervisorReady == null || ctx.supervisorReady;
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
    (step) => completed.has(step) || (step !== "try" && isOnboardingStepSatisfied(step, ctx))
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
  const target = pending.find((step) => step !== "features" && step !== "import") ?? pending[0];
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
      // "try", "account", "phone" and "supervisor" are optional for existing users too: none may
      // force the wizard open (already-onboarded users get the supervisor setup banner instead).
      (step) =>
        step !== "features" &&
        step !== "import" &&
        step !== "cli" &&
        step !== "try" &&
        step !== "account" &&
        step !== "phone" &&
        step !== "supervisor" && step !== "enroll" && step !== "skills"
    )
  );
}
