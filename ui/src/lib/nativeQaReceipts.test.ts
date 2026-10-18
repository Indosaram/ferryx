import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { appendNativeRecoveryEvent, recordNativeBindingPersisted, resetNativeQaCapabilityForTest, writeNativeRecoverySnapshot, registerNativeSnapshotPane, observeRequestedNativeSnapshot, installNativeSnapshotObserver } from "./nativeQaReceipts";
import type { NativeRecoverySnapshot, NativeSnapshotRequest } from "./nativeQaReceipts";

const api = vi.hoisted(() => ({ invoke: vi.fn(), isTauri: vi.fn() }));
const events = vi.hoisted(() => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => api);
vi.mock("@tauri-apps/api/event", () => events);
const tuple = { backendSessionId: "backend", incarnation: "incarnation", daemonEpoch: "epoch",
  frontendSessionId: "frontend", paneIdentity: "pane", bindingKey: "binding", attemptGeneration: 2 };
const snapshot = { producerComponent: "native-frontend-snapshot", attachTuple: tuple, target: { inputPoint: { x: 20, y: 30 } },
  focus: { focused: true, inputPoint: { x: 20, y: 30 } },
  paneBounds: { x: 0, y: 0, width: 80, height: 24, scaleFactor: 2,
    inputPoint: { x: 20, y: 30 }, focusedInputPoint: { x: 18, y: 28 } } };
beforeEach(() => { api.invoke.mockReset(); api.isTauri.mockReturnValue(true); resetNativeQaCapabilityForTest(); });
describe("native persistence QA receipts", () => {
  it("does not invoke desktop commands on the web", async () => {
    api.isTauri.mockReturnValue(false);
    await recordNativeBindingPersisted(tuple);
    expect(api.invoke).not.toHaveBeenCalled();
  });
  it("does not invoke the feature-gated command when capability is disabled", async () => {
    api.invoke.mockResolvedValue(false);
    await recordNativeBindingPersisted(tuple);
    expect(api.invoke.mock.calls).toEqual([["cmd_native_qa_enabled"]]);
  });
  it("awaits the actual append acknowledgment with the exact tuple", async () => {
    let acknowledge: (() => void) | undefined;
    let entered: (() => void) | undefined;
    const appended = new Promise<void>((resolve) => { entered = resolve; });
    api.invoke.mockResolvedValueOnce(true).mockImplementationOnce(() => {
      entered?.(); return new Promise<void>((resolve) => { acknowledge = resolve; });
    });
    let completed = false;
    const recording = recordNativeBindingPersisted(tuple).then(() => { completed = true; });
    await appended;
    expect(completed).toBe(false);
    expect(api.invoke.mock.calls[1]).toEqual(["cmd_qa_native_binding_persisted", { attachTuple: tuple }]);
    acknowledge?.(); await recording;
    expect(completed).toBe(true);
  });
  it("checks the feature gate only once for repeated output observations", async () => {
    let acknowledge: (() => void) | undefined;
    const appended = new Promise<void>(resolve => { acknowledge = resolve; });
    let count = 0;
    api.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_native_qa_enabled") return true;
      if (command === "cmd_qa_native_recovery_event" && ++count === 4) acknowledge?.();
    });
    const { observeNativeOutput } = await import("./nativeQaReceipts");
    observeNativeOutput(tuple, new Uint8Array([1]), "1", "epoch");
    observeNativeOutput(tuple, new Uint8Array([2]), "2", "epoch");
    await appended;
    expect(api.invoke.mock.calls.filter(([command]) => command === "cmd_qa_native_recovery_event")).toHaveLength(4);
    expect(api.invoke.mock.calls.filter(([command]) => command === "cmd_native_qa_enabled")).toHaveLength(1);
  });
  it("propagates append failure", async () => {
    api.invoke.mockResolvedValueOnce(true).mockRejectedValueOnce(new Error("disk full"));
    await expect(recordNativeBindingPersisted(tuple)).rejects.toThrow("disk full");
  });
  it("writes a measured current snapshot through the private command", async () => {
    api.invoke.mockResolvedValueOnce(true).mockResolvedValueOnce(undefined);
    await writeNativeRecoverySnapshot(snapshot);
    expect(api.invoke.mock.calls).toEqual([
      ["cmd_native_qa_enabled"],
      ["cmd_qa_native_recovery_snapshot", { snapshot }],
    ]);
  });
  it("refuses snapshots without a separately measured active pane point", async () => {
    api.invoke.mockResolvedValueOnce(true);
    const { paneBounds: _bounds, ...invalid } = snapshot;
    await expect(writeNativeRecoverySnapshot({ ...invalid, paneBounds: { x: 0, y: 0, width: 80, height: 24, scaleFactor: 2 } })).rejects.toThrow("measured active and previously focused pane points");
    expect(api.invoke).not.toHaveBeenCalled();
  });
  it("returns the physical JSONL append acknowledgment with event provenance", async () => {
    const event = { kind: "attach-complete", attachTuple: tuple, producerComponent: "native-frontend-attach", payload: { sessionId: "backend" } };
    const acknowledgment = { path: "/profile/receipts/native-events.jsonl", lineIndex: 3, event: { ...event, runId: "run", operationId: "op", producerPid: 7, lineIndex: 3 } };
    api.invoke.mockResolvedValueOnce(true).mockResolvedValueOnce(acknowledgment);
    await expect(appendNativeRecoveryEvent(event)).resolves.toEqual(acknowledgment);
    expect(api.invoke.mock.calls).toEqual([
      ["cmd_native_qa_enabled"],
      ["cmd_qa_native_recovery_event", { event }],
    ]);
  });
});

describe("requested native snapshot observation", () => {
  const cleanup: Array<() => void> = [];
  const request: NativeSnapshotRequest = { runId: "run", operationId: "operation",
    requestId: "00000000-0000-4000-8000-000000000001", attachTuple: tuple, timeoutMs: 5000 };
  function pane(identity: string, x: number) {
    const input = document.createElement("textarea");
    document.body.append(input);
    vi.spyOn(input, "getBoundingClientRect").mockReturnValue(new DOMRect(x, 10, 20, 10));
    let live = true;
    const raw: NativeRecoverySnapshot = { ...snapshot, attachTuple: { ...tuple, paneIdentity: identity },
      paneBounds: { x, y: 0, width: 100, height: 50, scaleFactor: 1 } };
    const unregister = registerNativeSnapshotPane({ input, observe: () => live ? raw : null });
    cleanup.push(unregister, () => input.remove());
    return { input, raw, unregister, dispose: () => { live = false; } };
  }
  afterEach(() => { for (const release of cleanup.splice(0).reverse()) release(); });
  it("measures the target and actual active registered pane without a previous focus point", () => {
    const target = pane("pane", 100);
    const focused = pane("other", 300);
    focused.input.focus();
    const observed = observeRequestedNativeSnapshot(request);
    expect(observed?.attachTuple).toEqual(target.raw.attachTuple);
    expect(observed?.target.inputPoint).toEqual({ x: 110, y: 15 });
    expect(observed?.focus).toEqual({ focused: false, paneIdentity: "other", inputPoint: { x: 310, y: 15 } });
    expect(observed?.paneBounds.focusedInputPoint).toEqual({ x: 310, y: 15 });
  });
  it.each(["disconnected", "disposed", "foreign-tuple", "active-unregistered", "active-disposed", "active-disconnected", "unmounted"])("refuses %s observations", reason => {
    const target = pane("pane", 100);
    const focused = pane("other", 300);
    focused.input.focus();
    if (reason === "disconnected") target.input.remove();
    if (reason === "disposed") target.dispose();
    if (reason === "unmounted") target.unregister();
    if (reason === "active-unregistered") focused.unregister();
    if (reason === "active-disposed") focused.dispose();
    if (reason === "active-disconnected") focused.input.remove();
    const next = reason === "foreign-tuple" ? { ...request, attachTuple: { ...tuple, attemptGeneration: 9 } } : request;
    expect(observeRequestedNativeSnapshot(next)).toBeNull();
  });
  it.each(["backendSessionId", "incarnation", "daemonEpoch", "frontendSessionId", "paneIdentity", "bindingKey", "attemptGeneration"] as const)("requires the exact %s", field => {
    const target = pane("pane", 100);
    target.input.focus();
    const foreign = field === "attemptGeneration" ? { ...tuple, attemptGeneration: 99 } : { ...tuple, [field]: "foreign" };
    expect(observeRequestedNativeSnapshot({ ...request, attachTuple: foreign })).toBeNull();
  });
  it("keeps different target observations independent under interleaved requests", () => {
    pane("pane", 100);
    const other = pane("other", 300);
    other.input.focus();
    const first = observeRequestedNativeSnapshot(request);
    const second = observeRequestedNativeSnapshot({ ...request, requestId: "00000000-0000-4000-8000-000000000002", attachTuple: other.raw.attachTuple });
    expect(first?.target.inputPoint).toEqual({ x: 110, y: 15 });
    expect(second?.target.inputPoint).toEqual({ x: 310, y: 15 });
    expect(first?.attachTuple.paneIdentity).toBe("pane");
    expect(second?.attachTuple.paneIdentity).toBe("other");
  });
  it("installs the listener before readiness, correlates refusal, and removes it on cleanup", async () => {
    const unlisten = vi.fn();
    let deliver: ((event: { payload: NativeSnapshotRequest }) => void) | undefined;
    events.listen.mockImplementation(async (_name: string, handler: typeof deliver) => {
      deliver = handler;
      return unlisten;
    });
    api.invoke.mockImplementation(async (command: string, args?: { snapshot?: { qaSnapshotAction: string } }) => {
      if (command === "cmd_native_qa_enabled") return true;
      if (args?.snapshot?.qaSnapshotAction === "ready") expect(deliver).toBeTypeOf("function");
    });
    const stop = await installNativeSnapshotObserver();
    deliver?.({ payload: request });
    expect(api.invoke).toHaveBeenCalledWith("cmd_qa_native_recovery_snapshot", { snapshot: {
      ...request, qaSnapshotAction: "response", status: "refused", code: "SNAPSHOT_TARGET_OR_FOCUS_UNAVAILABLE",
    } });
    stop();
    expect(unlisten).toHaveBeenCalledOnce();
    const count = api.invoke.mock.calls.length;
    deliver?.({ payload: request });
    expect(api.invoke.mock.calls).toHaveLength(count);
  });

  it("emits timeline events with required runner fields", async () => {
    const acknowledgment = { path: "/profile/receipts/native-events.jsonl", lineIndex: 1, event: {} };
    api.invoke.mockResolvedValueOnce(true).mockResolvedValueOnce(acknowledgment);
    const event = {
      kind: "status",
      attachTuple: tuple,
      outcome: "ready",
      producerComponent: "native-frontend-status",
      payload: { outcome: "ready" },
    };
    await expect(appendNativeRecoveryEvent(event)).resolves.toEqual(acknowledgment);
    expect(api.invoke).toHaveBeenCalledWith("cmd_qa_native_recovery_event", { event });
  });

  it("observes native output with decoded text bytes string and timeline events", async () => {
    const calls: unknown[] = [];
    api.invoke.mockImplementation(async (command: string, args?: unknown) => {
      if (command === "cmd_native_qa_enabled") return true;
      if (command === "cmd_qa_native_recovery_event") {
        calls.push(args);
        return { path: "/tmp/events.jsonl", lineIndex: 1, event: {} };
      }
    });
    const { observeNativeOutput } = await import("./nativeQaReceipts");
    const chunk = new TextEncoder().encode("hello world");
    observeNativeOutput(tuple, chunk, "seq-1", "epoch-1");
    // Allow microtasks to settle
    await Promise.resolve();
    await Promise.resolve();
    expect(calls.length).toBeGreaterThanOrEqual(2);
    const outputPublished = calls.find((c: any) => c?.event?.kind === "output-published") as any;
    const output = calls.find((c: any) => c?.event?.kind === "output") as any;
    expect(outputPublished?.event?.bytes).toBe("hello world");
    expect(outputPublished?.event?.payload?.bytes).toBe("hello world");
    expect(output?.event?.bytes).toBe("hello world");
    expect(output?.event?.payload?.bytes).toBe("hello world");
  });

  it("preserves top-level fields for timeline events required by QA runner", async () => {
    api.invoke.mockResolvedValueOnce(true).mockResolvedValueOnce({ path: "/tmp/e.jsonl", lineIndex: 1, event: {} });
    const listenerEvent = {
      kind: "listener",
      attachTuple: tuple,
      active: true,
      producerComponent: "native-frontend-lifecycle",
      payload: { active: true },
    };
    await appendNativeRecoveryEvent(listenerEvent);
    expect(api.invoke).toHaveBeenCalledWith("cmd_qa_native_recovery_event", { event: listenerEvent });

    api.invoke.mockResolvedValueOnce({ path: "/tmp/e.jsonl", lineIndex: 2, event: {} });
    const attemptEvent = {
      kind: "attempt",
      attachTuple: tuple,
      outcome: "retryable",
      attempt: 1,
      producerComponent: "native-frontend-recovery-controller",
      payload: { attempt: 1, outcome: "retryable" },
    };
    await appendNativeRecoveryEvent(attemptEvent);
    expect(api.invoke).toHaveBeenCalledWith("cmd_qa_native_recovery_event", { event: attemptEvent });
  });

  it("holds output observations after binding-persisted until attached completes to preserve attached < output ordering", async () => {
    const kinds: string[] = [];
    let releaseAttached: (() => void) | undefined;
    const attachedGate = new Promise<void>((resolve) => { releaseAttached = resolve; });
    api.invoke.mockImplementation(async (command: string, args?: any) => {
      if (command === "cmd_native_qa_enabled") return true;
      if (command === "cmd_qa_native_recovery_event") {
        if (args?.event?.kind === "attached") await attachedGate;
        kinds.push(args?.event?.kind);
        return { path: "/tmp/e.jsonl", lineIndex: kinds.length, event: args?.event ?? {} };
      }
    });
    const { observeNativeOutput } = await import("./nativeQaReceipts");
    await appendNativeRecoveryEvent({ kind: "binding-persisted", attachTuple: tuple, producerComponent: "native-frontend-persistence" });
    const attachedPromise = appendNativeRecoveryEvent({ kind: "attached", attachTuple: tuple, producerComponent: "native-frontend-attach" });
    observeNativeOutput(tuple, new TextEncoder().encode("marker-text"), "seq-9", "epoch-1");
    await Promise.resolve();
    expect(kinds).toEqual(["binding-persisted"]);
    releaseAttached?.();
    await attachedPromise;
    await Promise.resolve();
    await Promise.resolve();
    expect(kinds).toEqual(["binding-persisted", "attached", "output-published", "output"]);
  });
});
