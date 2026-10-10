import { describe, expect, it } from "vitest";
import {
  ATTEMPT_TOTAL_BUDGET_MS,
  CANCEL_ACK_MAX_MS,
  clipStageBudget,
  createAttemptBudget,
  DAEMON_CANCEL_CLEANUP_MAX_MS,
  hasLocalSplitCapability,
  isRecoverableLegacySession,
  LOCAL_SPLIT_LIFECYCLE_CAPABILITY,
  LOCAL_SPLIT_VALIDITY_MS,
  matchesAttachTuple,
  matchesIncarnation,
  type PaneAttachTuple,
  STAGE_ATTACH_OR_LISTENER_MAX_MS,
  STAGE_CREATE_OR_STATUS_MAX_MS,
  STAGE_CWD_PROBE_MAX_MS,
  STAGE_PRESENTATION_MAX_MS,
  WARM_NATIVE_READY_TARGET_MS,
} from "./localSplitContract";

describe("localSplitContract", () => {
  it("exports correct budget and timing constants", () => {
    expect(ATTEMPT_TOTAL_BUDGET_MS).toBe(15_000);
    expect(STAGE_CREATE_OR_STATUS_MAX_MS).toBe(9_000);
    expect(STAGE_ATTACH_OR_LISTENER_MAX_MS).toBe(4_000);
    expect(STAGE_PRESENTATION_MAX_MS).toBe(2_000);
    expect(STAGE_CWD_PROBE_MAX_MS).toBe(500);
    expect(CANCEL_ACK_MAX_MS).toBe(3_000);
    expect(DAEMON_CANCEL_CLEANUP_MAX_MS).toBe(2_500);
    expect(WARM_NATIVE_READY_TARGET_MS).toBe(2_000);
    expect(LOCAL_SPLIT_VALIDITY_MS).toBe(600_000);
    expect(LOCAL_SPLIT_LIFECYCLE_CAPABILITY).toBe("localSplitLifecycleV1");
  });

  it("clips stage budgets strictly to remaining budget and stage cap", () => {
    expect(clipStageBudget(15_000, STAGE_CREATE_OR_STATUS_MAX_MS)).toBe(9_000);
    expect(clipStageBudget(15_000, STAGE_ATTACH_OR_LISTENER_MAX_MS)).toBe(4_000);
    expect(clipStageBudget(15_000, STAGE_PRESENTATION_MAX_MS)).toBe(2_000);

    expect(clipStageBudget(6_000, STAGE_CREATE_OR_STATUS_MAX_MS)).toBe(6_000);
    expect(clipStageBudget(2_500, STAGE_ATTACH_OR_LISTENER_MAX_MS)).toBe(2_500);
    expect(clipStageBudget(1_200, STAGE_PRESENTATION_MAX_MS)).toBe(1_200);

    expect(clipStageBudget(0, STAGE_PRESENTATION_MAX_MS)).toBe(0);
    expect(clipStageBudget(-500, STAGE_PRESENTATION_MAX_MS)).toBe(0);
  });

  it("tracks attempt budget with monotonic elapsed time", () => {
    const start = 1_000_000;
    const budget = createAttemptBudget(start, 15_000);

    expect(budget.remainingMs(start)).toBe(15_000);
    expect(budget.stageBudget(STAGE_CREATE_OR_STATUS_MAX_MS, start)).toBe(9_000);

    expect(budget.remainingMs(start + 8_000)).toBe(7_000);
    expect(budget.stageBudget(STAGE_ATTACH_OR_LISTENER_MAX_MS, start + 8_000)).toBe(4_000);

    expect(budget.remainingMs(start + 14_000)).toBe(1_000);
    expect(budget.stageBudget(STAGE_PRESENTATION_MAX_MS, start + 14_000)).toBe(1_000);

    expect(budget.remainingMs(start + 15_000)).toBe(0);
    expect(budget.isExpired(start + 15_000)).toBe(true);

    expect(budget.remainingMs(start + 20_000)).toBe(0);
    expect(budget.isExpired(start + 20_000)).toBe(true);
  });

  it("evaluates local split capability correctly", () => {
    expect(hasLocalSplitCapability([LOCAL_SPLIT_LIFECYCLE_CAPABILITY])).toBe(true);
    expect(hasLocalSplitCapability(["otherCapability", LOCAL_SPLIT_LIFECYCLE_CAPABILITY])).toBe(true);
    expect(hasLocalSplitCapability([])).toBe(false);
    expect(hasLocalSplitCapability(["otherCapability"])).toBe(false);
    expect(hasLocalSplitCapability(null)).toBe(false);
    expect(hasLocalSplitCapability(undefined)).toBe(false);
  });

  it("distinguishes recoverable legacy sessions from proven incarnations", () => {
    expect(isRecoverableLegacySession(null)).toBe(true);
    expect(isRecoverableLegacySession({})).toBe(true);
    expect(isRecoverableLegacySession({ incarnation: null })).toBe(true);
    expect(isRecoverableLegacySession({ incarnation: undefined })).toBe(true);
    expect(isRecoverableLegacySession({ incarnation: "uuid-123" })).toBe(false);

    expect(matchesIncarnation({ incarnation: "uuid-123" }, "uuid-123")).toBe(true);
    expect(matchesIncarnation({ incarnation: "uuid-123" }, "uuid-456")).toBe(false);
    expect(matchesIncarnation({ incarnation: null }, "uuid-123")).toBe(false);
  });

  it("matches all 7 fields of PaneAttachTuple strictly", () => {
    const tuple: PaneAttachTuple = {
      backendSessionId: "backend-1",
      incarnation: "inc-1",
      daemonEpoch: "42",
      frontendSessionId: "fe-1",
      paneIdentity: "pane-1",
      bindingKey: "key-1",
      attemptGeneration: 1,
    };

    expect(matchesAttachTuple(tuple, { ...tuple })).toBe(true);

    expect(matchesAttachTuple(tuple, { ...tuple, backendSessionId: "backend-2" })).toBe(false);
    expect(matchesAttachTuple(tuple, { ...tuple, incarnation: "inc-2" })).toBe(false);
    expect(matchesAttachTuple(tuple, { ...tuple, daemonEpoch: "43" })).toBe(false);
    expect(matchesAttachTuple(tuple, { ...tuple, frontendSessionId: "fe-2" })).toBe(false);
    expect(matchesAttachTuple(tuple, { ...tuple, paneIdentity: "pane-2" })).toBe(false);
    expect(matchesAttachTuple(tuple, { ...tuple, bindingKey: "key-2" })).toBe(false);
    expect(matchesAttachTuple(tuple, { ...tuple, attemptGeneration: 2 })).toBe(false);
  });
});
