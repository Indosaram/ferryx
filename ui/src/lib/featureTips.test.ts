import { describe, expect, it } from "vitest";
import {
  FEATURE_TIPS,
  getCompletedFeatureTipIds,
  getOrderedUnseenFeatureTips,
  normalizeFeatureTipIds,
} from "./featureTips";
import { getFeatureTipsAppOpenDecision } from "./featureTipScheduler";

const NO_COMPLETION = {
  cliInstalled: false,
  voiceDictationEnabled: false,
  sessionSearchTipCompleted: false,
  chatUiLaunchTipCompleted: false,
  supervisorTipCompleted: false,
};

describe("feature tips", () => {
  it("keeps Orca's six-tip catalogue with new tips ahead of unseen ones", () => {
    expect(FEATURE_TIPS.map((tip) => tip.id)).toEqual([
      "chat-ui-here",
      "supervisor-teammate",
      "agent-session-search",
      "ferryx-cli",
      "cmd-j-palette",
      "voice-dictation",
    ]);
    expect(FEATURE_TIPS.filter((tip) => tip.priority === "new")).toHaveLength(5);
    expect(FEATURE_TIPS.at(-1)?.priority).toBe("unseen");
  });

  it("drops tips the user already uses", () => {
    expect(getCompletedFeatureTipIds({ ...NO_COMPLETION, cliInstalled: true })).toContain("ferryx-cli");
    expect(
      getCompletedFeatureTipIds({ ...NO_COMPLETION, featureInteractions: ["voice-dictation"] }),
    ).toContain("voice-dictation");
  });

  it("orders unseen tips new-first and skips seen and completed ids", () => {
    const ordered = getOrderedUnseenFeatureTips({
      seenTipIds: new Set(["chat-ui-here"]),
      completedTipIds: new Set(["ferryx-cli"]),
    });
    expect(ordered.map((tip) => tip.id)).toEqual([
      "supervisor-teammate",
      "agent-session-search",
      "cmd-j-palette",
      "voice-dictation",
    ]);
  });

  it("normalizes stored ids against the current catalogue", () => {
    expect(normalizeFeatureTipIds(["ferryx-cli", "ferryx-cli", "retired-tip", 7])).toEqual(["ferryx-cli"]);
    expect(normalizeFeatureTipIds(null)).toEqual([]);
  });
});

describe("feature tip startup decision", () => {
  const base = {
    onboardingPending: false,
    promptedThisSession: false,
    suppressedByOnboardingThisSession: false,
    persistedUIReady: true,
    activeModalIsNone: true,
    tipsDisabled: false,
    cliInstalled: false,
    seenTipIds: [] as string[],
    completed: NO_COMPLETION,
  };

  it("opens the first pending tip", () => {
    expect(getFeatureTipsAppOpenDecision(base)).toEqual({ kind: "open", tipId: "chat-ui-here" });
  });

  it("yields to onboarding for the whole session", () => {
    expect(getFeatureTipsAppOpenDecision({ ...base, onboardingPending: true })).toEqual({
      kind: "suppress-for-onboarding",
    });
  });

  it("skips while the CLI probe is pending, a modal is open, or a tip already ran", () => {
    expect(getFeatureTipsAppOpenDecision({ ...base, cliInstalled: null })).toEqual({ kind: "skip" });
    expect(getFeatureTipsAppOpenDecision({ ...base, activeModalIsNone: false })).toEqual({ kind: "skip" });
    expect(getFeatureTipsAppOpenDecision({ ...base, promptedThisSession: true })).toEqual({ kind: "skip" });
    expect(getFeatureTipsAppOpenDecision({ ...base, tipsDisabled: true })).toEqual({ kind: "skip" });
  });

  it("opens nothing when every tip is seen", () => {
    expect(
      getFeatureTipsAppOpenDecision({ ...base, seenTipIds: FEATURE_TIPS.map((tip) => tip.id) }),
    ).toEqual({ kind: "skip" });
  });
});
