import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  normalizeUnified,
  type SupervisorCallResult,
  type SupervisorEnvelope,
  type UnifiedSupervisorClient,
  type UnifiedSupervisorView,
} from "../../lib/unifiedSupervisor";
import { UnifiedSupervisorSection, type UnifiedSupervisorSectionProps } from "./UnifiedSupervisorSection";

afterEach(cleanup);

const GOAL = {
  goalId: "g-1",
  supervisorId: "s-1",
  projectId: "p-1",
  laneKey: "sv-1",
  title: "Write result.txt",
  runState: "running",
  stateDetail: null,
  revision: 3,
  pendingWaits: [],
  nextTrigger: null,
  updatedAt: "2026-10-28T00:00:00Z",
  mode: "full",
  turnLimit: 20,
  turnsUsed: 1,
  grant: ["edit", "test", "commit"],
};

function view(overrides: Record<string, unknown> = {}): UnifiedSupervisorView {
  return normalizeUnified({
    identity: { supervisorId: "s-1", ownerPrincipal: "desktop:qa-owner" },
    projects: [{ projectId: "p-1", name: "qa-supervisor" }],
    goals: [GOAL],
    health: { state: "ready", generation: 1, pid: 42, capabilities: [], serverVersion: null, fault: null, detail: null, queuedDecisions: 0 },
    ...overrides,
  })!;
}

type Deferred = { promise: Promise<SupervisorCallResult>; resolve: (value: SupervisorCallResult) => void };

function deferred(): Deferred {
  let resolve!: (value: SupervisorCallResult) => void;
  const promise = new Promise<SupervisorCallResult>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

/** A client whose calls resolve only when the test says so; every call is recorded. */
function fakeClient() {
  const requests: Array<{ envelope: SupervisorEnvelope; answer: Deferred }> = [];
  const remembers: Array<{ envelope: SupervisorEnvelope; answer: Deferred }> = [];
  const conversation = vi.fn(async (): Promise<SupervisorCallResult> => ({
    kind: "ok",
    value: { projectId: "p-1", messages: [{ id: 1, sourceAdapter: "slack", sourceMessageId: "m1", body: "Create result.txt", goalId: "g-1", admittedRevision: 1, createdAt: "t" }], next: 1 },
  }));
  const memory = vi.fn(async (): Promise<SupervisorCallResult> => ({ kind: "error", code: "MEMORY_RECEIPTS_UNAVAILABLE", message: "not yet", status: 503 }));
  const refresh = vi.fn();
  const client: UnifiedSupervisorClient = {
    request: (envelope) => {
      const answer = deferred();
      requests.push({ envelope, answer });
      return answer.promise;
    },
    remember: (envelope) => {
      const answer = deferred();
      remembers.push({ envelope, answer });
      return answer.promise;
    },
    conversation,
    memory,
    refresh,
  };
  return { client, requests, remembers, conversation, memory, refresh };
}

async function renderSection(overrides: Partial<UnifiedSupervisorSectionProps> = {}) {
  const fake = fakeClient();
  const props: UnifiedSupervisorSectionProps = {
    surface: "desktop",
    hostLabel: "studio",
    connection: "live",
    view: view(),
    client: fake.client,
    ...overrides,
  };
  const utils = render(<UnifiedSupervisorSection {...props} />);
  // Settle the mount reads: their continuations were queued before these awaits.
  await act(async () => {
    await fake.conversation.mock.results[0]?.value;
    await fake.memory.mock.results[0]?.value;
  });
  return { ...fake, ...utils, props };
}

describe("UnifiedSupervisorSection", () => {
  it("shows the goal, its revision, run state and concrete wait, and the cross-channel conversation", async () => {
    const { conversation } = await renderSection();
    const card = screen.getByTestId("supervisor-goal");
    expect(card.dataset.revision).toBe("3");
    expect(within(card).getByTestId("supervisor-goal-id").textContent).toBe("g-1");
    expect(within(card).getByTestId("supervisor-run-state").dataset.state).toBe("running");
    expect(within(card).getByTestId("supervisor-goal-wait").textContent).toContain("waits for its turn to settle");
    expect(within(card).getByTestId("supervisor-goal-budget").textContent).toBe("1 / 20 turns");
    expect(within(card).getByTestId("supervisor-goal-grant").textContent).toBe("Allowed: edit, test, commit");
    expect(conversation).toHaveBeenCalledWith("p-1", 0);
    expect(screen.getByTestId("supervisor-message").dataset.adapter).toBe("slack");
    expect(screen.getByTestId("supervisor-connection").dataset.state).toBe("live");
  });

  it("pauses with the goal's revision, once, while the request is in flight", async () => {
    const { requests, refresh } = await renderSection({ surface: "phone" });
    const pause = screen.getByTestId("supervisor-pause") as HTMLButtonElement;
    fireEvent.click(pause);
    fireEvent.click(pause);
    expect(requests).toHaveLength(1);
    expect(requests[0]!.envelope).toMatchObject({ goalId: "g-1", expectedRevision: 3, action: "pause", payload: {} });
    expect(requests[0]!.envelope.requestId).toMatch(/^phone-pause-/);
    expect(pause.disabled).toBe(true);
    await act(async () => {
      requests[0]!.answer.resolve({ kind: "ok", value: { goalId: "g-1", runState: "paused", revision: 4 } });
      await requests[0]!.answer.promise;
    });
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("supervisor-conflict")).toBeNull();
  });

  it("refreshes and shows a visible conflict on a stale revision, without retrying", async () => {
    const { requests, refresh } = await renderSection();
    fireEvent.click(screen.getByTestId("supervisor-pause"));
    await act(async () => {
      requests[0]!.answer.resolve({ kind: "error", code: "REVISION_CONFLICT", message: "goal g-1 revision changed", status: 409 });
      await requests[0]!.answer.promise;
    });
    expect(requests).toHaveLength(1);
    expect(refresh).toHaveBeenCalledTimes(1);
    const conflict = within(screen.getByTestId("supervisor-goal")).getByTestId("supervisor-conflict");
    expect(conflict.dataset.code).toBe("REVISION_CONFLICT");
    expect(conflict.getAttribute("role")).toBe("alert");
  });

  it("submits the composer text and shows the persisted goal receipt", async () => {
    const { requests } = await renderSection();
    fireEvent.change(screen.getByTestId("supervisor-composer"), { target: { value: "Create result.txt containing READY." } });
    fireEvent.click(screen.getByTestId("supervisor-submit"));
    expect(requests).toHaveLength(1);
    const envelope = requests[0]!.envelope;
    expect(envelope).toMatchObject({ projectId: "p-1", action: "submit", payload: { request: "Create result.txt containing READY." } });
    expect(envelope.expectedRevision).toBeUndefined();
    await act(async () => {
      requests[0]!.answer.resolve({
        kind: "ok",
        value: { outcome: "admitted", receipt: { goal_id: "g-new" }, revision: 1, runState: "ready", duplicate: false, acknowledgment: "Goal g-new is registered (lane sv-x, revision 1)." },
      });
      await requests[0]!.answer.promise;
    });
    const receipt = screen.getByTestId("supervisor-receipt");
    expect(receipt.dataset.goalId).toBe("g-new");
    expect(receipt.textContent).toContain("Goal g-new is registered");
    expect((screen.getByTestId("supervisor-composer") as HTMLTextAreaElement).value).toBe("");
  });

  it("reuses the request id when the same words are resent after a lost answer", async () => {
    const { requests } = await renderSection();
    fireEvent.change(screen.getByTestId("supervisor-composer"), { target: { value: "Do the thing." } });
    fireEvent.click(screen.getByTestId("supervisor-submit"));
    await act(async () => {
      requests[0]!.answer.resolve({ kind: "error", code: "OFFLINE", message: "tunnel closed", status: null });
      await requests[0]!.answer.promise;
    });
    fireEvent.click(screen.getByTestId("supervisor-submit"));
    expect(requests).toHaveLength(2);
    expect(requests[1]!.envelope.requestId).toBe(requests[0]!.envelope.requestId);
  });

  it("sends a correction with the goal's revision", async () => {
    const { requests } = await renderSection();
    fireEvent.click(screen.getByTestId("supervisor-correct"));
    fireEvent.change(screen.getByTestId("supervisor-correction-input"), { target: { value: "Use DONE instead of READY" } });
    fireEvent.click(screen.getByTestId("supervisor-correction-send"));
    expect(requests[0]!.envelope).toMatchObject({ action: "correct", goalId: "g-1", expectedRevision: 3, payload: { text: "Use DONE instead of READY" } });
  });

  it("asks before cancelling, and approve names the scoped grant", async () => {
    const held = view({ goals: [{ ...GOAL, runState: "awaiting_owner", stateDetail: "OWNER_APPROVAL_REQUIRED: supervised legacy lane" }] });
    const { requests } = await renderSection({ view: held });
    expect(screen.getByTestId("supervisor-goal-wait").textContent).toContain("Needs your decision");
    expect(screen.getByTestId("supervisor-approve").textContent).toBe("Approve (edit, test, commit)");
    fireEvent.click(screen.getByTestId("supervisor-cancel"));
    expect(requests).toHaveLength(0);
    fireEvent.click(screen.getByTestId("supervisor-cancel-confirm"));
    expect(requests[0]!.envelope).toMatchObject({ action: "cancel", expectedRevision: 3 });
  });

  it("shows a paused goal with resume", async () => {
    const { requests } = await renderSection({ view: view({ goals: [{ ...GOAL, runState: "paused", stateDetail: "OWNER_PAUSED: by phone", revision: 4 }] }) });
    expect(screen.queryByTestId("supervisor-pause")).toBeNull();
    fireEvent.click(screen.getByTestId("supervisor-goal-resume"));
    expect(requests[0]!.envelope).toMatchObject({ action: "resume", expectedRevision: 4 });
  });

  it("names the engine's degraded cause", async () => {
    await renderSection({
      view: view({ health: { state: "degraded", generation: 1, pid: null, capabilities: [], serverVersion: null, fault: { kind: "auth", detail: "401 from provider" }, detail: null, queuedDecisions: 0 } }),
    });
    const engine = screen.getByTestId("supervisor-engine");
    expect(engine.dataset.degraded).toBe("true");
    expect(engine.dataset.code).toBe("MHC_AUTH");
    expect(engine.textContent).toContain("401 from provider");
  });

  it("keeps the last goals visible but sends nothing while the host is disconnected", async () => {
    const { conversation, memory } = await renderSection({ connection: "disconnected", client: null, connectionError: { code: "OFFLINE", message: "" } });
    const banner = screen.getByTestId("supervisor-connection");
    expect(banner.dataset.state).toBe("disconnected");
    expect(banner.textContent).toContain("no other supervisor is started");
    expect(screen.getByTestId("supervisor-goal-id").textContent).toBe("g-1");
    expect((screen.getByTestId("supervisor-pause") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("supervisor-composer") as HTMLTextAreaElement).disabled).toBe(true);
    expect(conversation).not.toHaveBeenCalled();
    expect(memory).not.toHaveBeenCalled();
  });

  it("teaches with a visible receipt and its provenance, and reports memory refusals by code", async () => {
    const { remembers } = await renderSection();
    expect(screen.getByTestId("supervisor-memory-unavailable").dataset.code).toBe("MEMORY_RECEIPTS_UNAVAILABLE");
    fireEvent.change(screen.getByTestId("supervisor-teach-input"), { target: { value: "use the QA remote host" } });
    fireEvent.click(screen.getByTestId("supervisor-teach"));
    expect(remembers[0]!.envelope).toMatchObject({ action: "teach", payload: { text: "use the QA remote host" } });
    await act(async () => {
      remembers[0]!.answer.resolve({ kind: "ok", value: { action: "teach", memory: { state: "accepted", sourceMessageId: "desktop:desktop-teach-1", duplicate: false } } });
      await remembers[0]!.answer.promise;
    });
    const receipt = screen.getByTestId("supervisor-memory-receipt");
    expect(receipt.dataset.state).toBe("accepted");
    expect(receipt.textContent).toContain("desktop:desktop-teach-1");

    fireEvent.change(screen.getByTestId("supervisor-forget-input"), { target: { value: "m-1" } });
    fireEvent.click(screen.getByTestId("supervisor-forget"));
    expect(remembers[1]!.envelope).toMatchObject({ action: "forget", payload: { memoryId: "m-1" } });
    await act(async () => {
      remembers[1]!.answer.resolve({ kind: "error", code: "MEMORY_FORGET_UNAVAILABLE", message: "nothing was forgotten", status: 503 });
      await remembers[1]!.answer.promise;
    });
    expect(screen.getByTestId("supervisor-memory-notice").dataset.code).toBe("MEMORY_FORGET_UNAVAILABLE");
  });

  it("corrects a memory as forget then teach, and teaches nothing when the forget is refused", async () => {
    const { remembers } = await renderSection();
    fireEvent.change(screen.getByTestId("supervisor-forget-input"), { target: { value: "m-2" } });
    fireEvent.change(screen.getByTestId("supervisor-memory-correct-input"), { target: { value: "use omarchy for QA" } });
    fireEvent.click(screen.getByTestId("supervisor-memory-correct"));
    expect(remembers.map((call) => call.envelope.action)).toEqual(["forget"]);
    await act(async () => {
      remembers[0]!.answer.resolve({ kind: "error", code: "MEMORY_FORGET_UNAVAILABLE", message: "no", status: 503 });
      await remembers[0]!.answer.promise;
    });
    expect(remembers.map((call) => call.envelope.action)).toEqual(["forget"]);
  });

  it("offers explicit adoption of a legacy lane", async () => {
    const { requests } = await renderSection({ adoptable: [{ key: "ledger", title: "Ledger" }] });
    fireEvent.click(screen.getByRole("button", { name: "Adopt Ledger" }));
    expect(requests[0]!.envelope).toMatchObject({ action: "adopt", projectId: "p-1", payload: { laneKey: "ledger" } });
  });

  it("says when the hub has no single owner and accepts nothing", async () => {
    await renderSection({ view: view({ identity: null, goals: [] }) });
    expect(screen.getByTestId("supervisor-no-identity")).toBeTruthy();
    expect((screen.getByTestId("supervisor-composer") as HTMLTextAreaElement).disabled).toBe(true);
  });
});
