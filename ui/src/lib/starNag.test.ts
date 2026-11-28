import { describe, expect, it } from "vitest";
import {
  STAR_NAG_COOLDOWN_MS,
  STAR_NAG_INITIAL_THRESHOLD,
  consumeStarNagValueMoment,
  deferStarNag,
  deferStarNagAfterWebHandoff,
  disableStarNag,
  ensureStarNagBaseline,
  getStarNagValueMomentDecision,
  hasConsumedStarNagValueMoment,
  hasSuccessfulDoneTransition,
  isStarNagCooldownActive,
  loadStarNagNextThreshold,
  markStarNagCompleted,
  shouldShowStarNagThresholdPrompt,
} from "./starNag";

function createStorage(seed: Record<string, string> = {}) {
  const map = new Map(Object.entries(seed));
  return {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => void map.set(key, value),
    removeItem: (key: string) => void map.delete(key),
    dump: () => Object.fromEntries(map),
  };
}

const NOW = 1_700_000_000_000;

describe("star nag threshold", () => {
  it("seeds the baseline on first evaluation instead of prompting a long-time user", () => {
    const storage = createStorage();
    expect(
      shouldShowStarNagThresholdPrompt(storage, {
        totalAgentsSpawned: 500,
        promptVisible: false,
        evaluating: false,
        now: NOW,
      }),
    ).toBe(false);
    expect(storage.dump()["ferryx.starNag.baselineAgents"]).toBe("500");
    expect(storage.dump()["ferryx.starNag.nextThreshold"]).toBe(String(STAR_NAG_INITIAL_THRESHOLD));
  });

  it("prompts once 35 spawns have happened since the baseline", () => {
    const storage = createStorage();
    ensureStarNagBaseline(storage, 100);
    const base = { promptVisible: false, evaluating: false, now: NOW };
    expect(shouldShowStarNagThresholdPrompt(storage, { ...base, totalAgentsSpawned: 134 })).toBe(false);
    expect(shouldShowStarNagThresholdPrompt(storage, { ...base, totalAgentsSpawned: 135 })).toBe(true);
  });

  it("doubles the threshold, resets the baseline and starts a 3 day cooldown on defer", () => {
    const storage = createStorage();
    ensureStarNagBaseline(storage, 0);
    deferStarNag(storage, { totalAgentsSpawned: 40, now: NOW });
    expect(loadStarNagNextThreshold(storage)).toBe(70);
    expect(storage.dump()["ferryx.starNag.baselineAgents"]).toBe("40");
    expect(Number(storage.dump()["ferryx.starNag.deferredUntil"])).toBe(NOW + STAR_NAG_COOLDOWN_MS);
    expect(
      shouldShowStarNagThresholdPrompt(storage, {
        totalAgentsSpawned: 200,
        promptVisible: false,
        evaluating: false,
        now: NOW + 1,
      }),
    ).toBe(false);
    expect(
      shouldShowStarNagThresholdPrompt(storage, {
        totalAgentsSpawned: 110,
        promptVisible: false,
        evaluating: false,
        now: NOW + STAR_NAG_COOLDOWN_MS,
      }),
    ).toBe(true);
  });

  it("never prompts while visible, evaluating, completed or disabled", () => {
    const base = { totalAgentsSpawned: 1000, now: NOW };
    const visible = createStorage();
    ensureStarNagBaseline(visible, 0);
    expect(shouldShowStarNagThresholdPrompt(visible, { ...base, promptVisible: true, evaluating: false })).toBe(false);
    expect(shouldShowStarNagThresholdPrompt(visible, { ...base, promptVisible: false, evaluating: true })).toBe(false);

    const completed = createStorage();
    ensureStarNagBaseline(completed, 0);
    markStarNagCompleted(completed);
    expect(shouldShowStarNagThresholdPrompt(completed, { ...base, promptVisible: false, evaluating: false })).toBe(false);

    const disabled = createStorage();
    ensureStarNagBaseline(disabled, 0);
    disableStarNag(disabled);
    expect(shouldShowStarNagThresholdPrompt(disabled, { ...base, promptVisible: false, evaluating: false })).toBe(false);
  });

  it("treats a web handoff as cooldown only, not completion", () => {
    const storage = createStorage();
    ensureStarNagBaseline(storage, 0);
    deferStarNagAfterWebHandoff(storage, { totalAgentsSpawned: 35, now: NOW });
    expect(storage.dump()["ferryx.starNag.completed"]).toBeUndefined();
    expect(loadStarNagNextThreshold(storage)).toBe(70);
  });

  it("only treats a future numeric deadline as an active cooldown", () => {
    expect(isStarNagCooldownActive(NOW + 1, NOW)).toBe(true);
    expect(isStarNagCooldownActive(NOW, NOW)).toBe(false);
    expect(isStarNagCooldownActive(null, NOW)).toBe(false);
  });
});

describe("star nag value moment", () => {
  it("is available once per app version and skipped after completion", () => {
    const storage = createStorage();
    ensureStarNagBaseline(storage, 0);
    const args = { appVersion: "2026.1127.1", now: NOW, evaluating: false, promptVisible: false };
    expect(getStarNagValueMomentDecision(storage, args)).toBe("gh");
    consumeStarNagValueMoment(storage, "2026.1127.1");
    expect(hasConsumedStarNagValueMoment(storage)).toBe(true);
    expect(getStarNagValueMomentDecision(storage, args)).toBe(null);
    // Orca spends this moment once per install, not once per version.
    expect(getStarNagValueMomentDecision(storage, { ...args, appVersion: "2026.1127.2" })).toBe(null);

    const completed = createStorage();
    markStarNagCompleted(completed);
    expect(getStarNagValueMomentDecision(completed, args)).toBe(null);
  });

  it("counts only a real agent turn that newly reaches done", () => {
    const realTurn = { previousState: "working", state: "done", isAgent: true, hasMeaningfulPrompt: true };
    expect(hasSuccessfulDoneTransition(realTurn)).toBe(true);
    expect(hasSuccessfulDoneTransition({ ...realTurn, previousState: "done" })).toBe(false);
    expect(hasSuccessfulDoneTransition({ ...realTurn, isAgent: false })).toBe(false);
    expect(hasSuccessfulDoneTransition({ ...realTurn, sessionBoundary: true })).toBe(false);
    expect(hasSuccessfulDoneTransition({ ...realTurn, agentTurnEndedUncleaned: true })).toBe(false);
    // "Cannot tell whether the turn carried content" must not count; only an explicit yes does.
    expect(hasSuccessfulDoneTransition({ ...realTurn, hasMeaningfulPrompt: undefined })).toBe(false);
  });
});
