import { describe, expect, it } from "vitest";

import { normalizeSnapshot } from "./supervisorModel";
import {
  engineStatus,
  goalControls,
  goalEnvelope,
  goalWaitLine,
  normalizeUnified,
  parseConversationPage,
  parseSubmitReceipt,
  parseTeachReceipt,
  submitEnvelope,
  type UnifiedGoal,
} from "./unifiedSupervisor";

const wireGoal = {
  goalId: "g-1",
  supervisorId: "s-1",
  projectId: "p-1",
  laneKey: "sv-abc",
  title: "Write result.txt",
  runState: "running",
  stateDetail: null,
  revision: 3,
  pendingWaits: [],
  nextTrigger: null,
  updatedAt: "2026-10-28T00:00:00Z",
  mode: "full",
  turnLimit: 20,
  turnsUsed: 2,
  grant: ["edit", "test"],
};

function goal(overrides: Partial<UnifiedGoal> = {}): UnifiedGoal {
  return { ...(normalizeUnified({ goals: [wireGoal] })!.goals[0] as UnifiedGoal), ...overrides };
}

describe("normalizeUnified", () => {
  it("reads identity, projects, goals and engine health from the snapshot's supervisor part", () => {
    const snapshot = normalizeSnapshot({
      hub: { available: true, running: true },
      supervisor: {
        identity: { supervisorId: "s-1", ownerPrincipal: "desktop:qa-owner" },
        projects: [{ projectId: "p-1", name: "qa-supervisor" }],
        goals: [wireGoal, { goalId: "broken" }],
        health: { state: "degraded", generation: 2, pid: null, capabilities: [], serverVersion: null,
          fault: { kind: "capability", missing: ["memory_forget"] }, detail: null, queuedDecisions: 0 },
      },
    });
    const unified = snapshot.supervisor!;
    expect(unified.identity).toEqual({ supervisorId: "s-1", ownerPrincipal: "desktop:qa-owner" });
    expect(unified.projects).toEqual([{ projectId: "p-1", name: "qa-supervisor" }]);
    expect(unified.goals.map((item) => [item.goalId, item.revision, item.grant])).toEqual([["g-1", 3, ["edit", "test"]]]);
    expect(unified.health?.fault).toEqual({ kind: "capability", missing: ["memory_forget"] });
  });

  it("is null for an older daemon or a closed hub", () => {
    expect(normalizeSnapshot({ hub: { available: true, running: true } }).supervisor).toBeNull();
    expect(normalizeSnapshot({ hub: { available: false, running: false }, supervisor: null }).supervisor).toBeNull();
  });
});

describe("engineStatus", () => {
  it("names the degraded cause with its wire code", () => {
    const status = engineStatus({ state: "degraded", generation: 1, pid: null, capabilities: [], serverVersion: null,
      fault: { kind: "quota", detail: "rate limited" }, detail: null, queuedDecisions: 0 });
    expect(status).toMatchObject({ degraded: true, code: "MHC_QUOTA", detail: "rate limited" });
    expect(status.label).toContain("quota");
    const capability = engineStatus({ state: "degraded", generation: 1, pid: null, capabilities: [], serverVersion: null,
      fault: { kind: "capability", missing: ["memory_receipts", "memory_forget"] }, detail: null, queuedDecisions: 0 });
    expect(capability).toMatchObject({ code: "MHC_CAPABILITY_MISSING", detail: "missing: memory_receipts, memory_forget" });
  });

  it("is healthy only when ready, and explicit when no engine runs", () => {
    expect(engineStatus({ state: "ready", generation: 1, pid: 7, capabilities: [], serverVersion: null, fault: null, detail: null, queuedDecisions: 0 }).degraded).toBe(false);
    expect(engineStatus(null)).toMatchObject({ state: "absent", degraded: true });
  });
});

describe("goal wait line and controls", () => {
  it("says concretely what a waiting goal waits on", () => {
    const waiting = goal({ runState: "waiting_children", pendingWaits: [{ waitId: "w1", kind: "child_goal", childGoalId: "child-7", eventKey: null }] });
    expect(goalWaitLine(waiting)).toContain("child-7");
    expect(goalWaitLine(waiting)).toContain("No model call");
    const event = goal({ runState: "waiting_event", pendingWaits: [{ waitId: "w2", kind: "external_event", childGoalId: null, eventKey: "ci:green" }] });
    expect(goalWaitLine(event)).toContain("ci:green");
    expect(goalWaitLine(goal({ runState: "awaiting_owner", stateDetail: "SCOPE_NEEDS_OWNER: push needs the owner's explicit approval" })))
      .toContain("SCOPE_NEEDS_OWNER");
    expect(goalWaitLine(goal({ runState: "delivery_uncertain" }))).toContain("never resent");
  });

  it("offers approve only for a releasable hold and resume only when paused", () => {
    expect(goalControls(goal({ runState: "awaiting_owner", stateDetail: "OWNER_APPROVAL_REQUIRED: x" })).approve).toBe(true);
    expect(goalControls(goal({ runState: "awaiting_owner", stateDetail: "GRANT_REVOKED: y" })).approve).toBe(false);
    expect(goalControls(goal({ runState: "awaiting_owner", stateDetail: "TURN_LIMIT: z" })).approve).toBe(false);
    expect(goalControls(goal({ runState: "paused" }))).toMatchObject({ pause: false, resume: true, cancel: true });
    expect(goalControls(goal({ runState: "completed" }))).toEqual({
      pause: false,
      resume: false,
      cancel: false,
      approve: false,
      correct: false,
      resolveDelivery: false,
    });
  });

  it("offers the delivery answer only while the delivery is uncertain", () => {
    expect(goalControls(goal({ runState: "delivery_uncertain" })).resolveDelivery).toBe(true);
    expect(goalControls(goal({ runState: "running" })).resolveDelivery).toBe(false);
    expect(goalControls(goal({ runState: "paused" })).resolveDelivery).toBe(false);
  });
});

describe("envelopes and receipts", () => {
  it("always sends the goal's revision with a goal action", () => {
    const envelope = goalEnvelope("phone", goal(), "pause");
    expect(envelope).toMatchObject({ goalId: "g-1", projectId: "p-1", expectedRevision: 3, action: "pause", payload: {} });
    expect(envelope.requestId).toMatch(/^phone-pause-/);
  });

  it("submits the owner's words verbatim with a baseline grant and one requirement", () => {
    expect(submitEnvelope("desktop-submit-1", "p-1", { text: "  Create result.txt containing READY.  ", doneWhen: "", verifier: "" })).toEqual({
      requestId: "desktop-submit-1",
      projectId: "p-1",
      action: "submit",
      payload: {
        request: "Create result.txt containing READY.",
        work: {
          title: "Create result.txt containing READY.",
          instruction: "Create result.txt containing READY.",
          requirements: [{ id: "r1", text: "Create result.txt containing READY.", verifier: "owner review" }],
          operations: ["edit", "test", "commit"],
        },
      },
    });
  });

  it("reads admission outcomes from their persisted receipt", () => {
    expect(parseSubmitReceipt({ outcome: "admitted", receipt: { goal_id: "g-9" }, revision: 1, runState: "ready", acknowledgment: "Goal g-9 is registered", duplicate: false }))
      .toEqual({ outcome: "admitted", goalId: "g-9", revision: 1, runState: "ready", acknowledgment: "Goal g-9 is registered", duplicate: false });
    expect(parseSubmitReceipt({ outcome: "conversational", reason: "a question" })).toEqual({ outcome: "conversational", reason: "a question" });
    // A model saying "registered" without a persisted receipt is not an admission.
    expect(parseSubmitReceipt({ outcome: "admitted", acknowledgment: "registered!" })).toEqual({ outcome: "unknown" });
  });

  it("parses conversation pages and teach receipts", () => {
    const page = parseConversationPage({ messages: [{ id: 4, sourceAdapter: "slack", sourceMessageId: "m", body: "Do it", goalId: "g-1", admittedRevision: 1, createdAt: "t" }], next: 4 }, 0);
    expect(page.next).toBe(4);
    expect(page.messages[0]).toMatchObject({ sourceAdapter: "slack", body: "Do it" });
    expect(parseConversationPage({ messages: [] }, 7).next).toBe(7);
    expect(parseTeachReceipt({ action: "teach", memory: { state: "accepted", sourceMessageId: "phone:r1", duplicate: false } }, "use the QA host"))
      .toEqual({ state: "accepted", sourceMessageId: "phone:r1", duplicate: false, text: "use the QA host" });
  });
});
