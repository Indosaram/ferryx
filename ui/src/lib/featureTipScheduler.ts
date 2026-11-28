import {
  type CompletedFeatureTipState,
  type FeatureTipId,
  getCompletedFeatureTipIds,
  getOrderedUnseenFeatureTips,
} from "./featureTips";

/**
 * Startup decision for the feature-tip dialog, ported 1:1 from Orca's
 * `getFeatureTipsAppOpenDecision`.
 *
 * Order matters and is load-bearing:
 * 1. onboarding that still wants to show suppresses tips for the whole session,
 * 2. a long list of "not yet" conditions skips silently (no tip, no re-check this session),
 * 3. otherwise the first pending tip opens.
 */
export type FeatureTipsAppOpenDecision =
  | { kind: "suppress-for-onboarding" }
  | { kind: "skip" }
  | { kind: "open"; tipId: FeatureTipId };

export type FeatureTipsAppOpenArgs = {
  /** Onboarding still has steps to walk the user through. */
  onboardingPending: boolean;
  /** A tip was already decided this session. */
  promptedThisSession: boolean;
  /** Onboarding already took over this session. */
  suppressedByOnboardingThisSession: boolean;
  /** Persisted UI state (projects, layout) has loaded. */
  persistedUIReady: boolean;
  /** No modal/dialog currently owns the screen. */
  activeModalIsNone: boolean;
  /** User turned feature tips off in settings. */
  tipsDisabled: boolean;
  /** `null` while the CLI launcher probe is still running. */
  cliInstalled: boolean | null;
  seenTipIds: readonly string[];
  completed: CompletedFeatureTipState;
};

export function getPendingFeatureTips(args: FeatureTipsAppOpenArgs) {
  return getOrderedUnseenFeatureTips({
    seenTipIds: new Set(args.seenTipIds),
    completedTipIds: new Set(getCompletedFeatureTipIds(args.completed)),
  });
}

export function getFeatureTipsAppOpenDecision(
  args: FeatureTipsAppOpenArgs,
): FeatureTipsAppOpenDecision {
  if (args.onboardingPending) return { kind: "suppress-for-onboarding" };
  if (
    args.promptedThisSession ||
    args.suppressedByOnboardingThisSession ||
    !args.persistedUIReady ||
    !args.activeModalIsNone ||
    args.tipsDisabled ||
    args.cliInstalled === null
  ) {
    return { kind: "skip" };
  }
  const tip = getPendingFeatureTips(args)[0];
  return tip ? { kind: "open", tipId: tip.id } : { kind: "skip" };
}
