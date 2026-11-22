import {
  isOnboardingStepSatisfied,
  type OnboardingContext,
  type OnboardingStepId,
} from "./onboarding";

/** Ordered goals, each judged by the wizard's own predicate; "features" and "try" are informational and excluded. */
export const SETUP_PROGRESS_STEPS: readonly OnboardingStepId[] = [
  "project",
  "agents",
  "cli",
  "phone",
  "supervisor",
  "account",
];

export type SetupProgressItem = {
  id: OnboardingStepId;
  title: string;
  hint: string;
  done: boolean;
};

const COPY: Record<string, { title: string; hint: string }> = {
  project: { title: "First project", hint: "Add a project folder to open a terminal workspace." },
  agents: { title: "Agents", hint: "Pick the agent Ferryx should launch by default." },
  cli: { title: "Command line", hint: "Install the ferryx command to drive workspaces from a shell." },
  phone: { title: "Phone", hint: "Pair a device to keep working away from this computer." },
  supervisor: { title: "Supervisor", hint: "Let the supervisor run agents on your work." },
  account: { title: "Sign in", hint: "Sign in to sync machines and reach this computer remotely." },
};

/**
 * Mirrors `satisfiedOnboardingSteps` (onboarding.ts) for the six goals above: a step counts as
 * done when the wizard recorded it in `completedSteps`, or when its live predicate passes. The
 * wizard records non-optional steps the user advanced past without satisfying them, so ignoring
 * `completedSteps` would show goals as pending that the wizard itself considers done.
 */
export function buildSetupProgress(
  ctx: OnboardingContext,
  completedSteps: readonly OnboardingStepId[] = []
): SetupProgressItem[] {
  const completed = new Set<OnboardingStepId>(completedSteps);
  return SETUP_PROGRESS_STEPS.map((id) => {
    const copy = COPY[id];
    return {
      id,
      title: copy.title,
      hint: copy.hint,
      done: completed.has(id) || isOnboardingStepSatisfied(id, ctx),
    };
  });
}

export type SetupProgressSummary = {
  done: number;
  total: number;
  nextItem: SetupProgressItem | null;
};

export function summarizeSetupProgress(items: readonly SetupProgressItem[]): SetupProgressSummary {
  const done = items.filter((item) => item.done).length;
  return {
    done,
    total: items.length,
    nextItem: items.find((item) => !item.done) ?? null,
  };
}
