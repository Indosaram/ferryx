import { afterEach, describe, expect, it, vi } from "vitest";
import {
  LocalSplitLifecycle,
  type LocalSplitIntent,
  type LocalSplitSession,
} from "./localSplitLifecycle";
import {
  emitNativeTerminalPresentation,
  resetNativeTerminalLifecycleForTest,
} from "./nativeTerminalLifecycle";
import type { AttachTerminalResponse, PreparedLocalSplit, SplitOperationResponse } from "./types";
import { switchDebug } from "./switchDebug";

vi.mock("./switchDebug", () => ({ switchDebug: vi.fn() }));

function deferred<T>() {
  let resolve: (value: T) => void = () => {
    throw new Error("Deferred not initialized");
  };
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

const prepared: PreparedLocalSplit = {
  identity: {
    requestId: "27be1fd6-7182-4fba-b42c-27c0288fb460",
    originEpoch: "7",
    expiresAtUnixMs: 600_000,
  },
  workspaceId: "default",
  worktree: null,
  cwd: "/repo",
  shell: "/bin/sh",
  cols: 80,
  rows: 24,
};

function fixture(restored?: Partial<LocalSplitIntent>, visible = true) {
  const intent: LocalSplitIntent = {
    requestId: prepared.identity.requestId,
    prepared: null,
    generation: 0,
    createSent: false,
    cancelRequested: false,
    ...restored,
  };
  let session: LocalSplitSession | undefined = {
    id: "front",
    workspaceId: "default",
    worktree: null,
    cwd: "/repo",
    backendSessionId: null,
    lifecycle: "starting",
    spawnIntent: intent,
  };
  const attached = deferred<void>();
  const removed = deferred<void>();
  const operation = vi.fn(async (): Promise<SplitOperationResponse> => ({
    action: "prepare",
    prepared,
  }));
  const create = vi.fn(async () => ({
    sessionId: "back",
    daemonEpoch: "7",
    session: {
      sessionId: "back",
      cwd: "/repo",
      cols: 80,
      rows: 24,
      running: true,
    },
  }));
  const attach = vi.fn(async (): Promise<AttachTerminalResponse> => {
    attached.resolve();
    return {
      sessionId: "back",
      daemonEpoch: "7",
      history: "",
      historyStartSequence: null,
      historyEndSequence: null,
      gap: null,
      attachTuple: {
        backendSessionId: session?.backendSessionId ?? "back", incarnation: null,
        daemonEpoch: "7", frontendSessionId: "front", paneIdentity: "front",
        bindingKey: `${session?.backendSessionId ?? "back"}:7:0:`,
        attemptGeneration: session?.spawnIntent?.generation ?? 0,
      },
    };
  });
  const publish = vi.fn((next: LocalSplitSession) => {
    session = next;
  });
  const ensureEvents = vi.fn(async () => {});
  const persisted: LocalSplitSession[] = [];
  const persist = vi.fn(async () => { if (session) persisted.push(structuredClone(session)); });
  const controller = new LocalSplitLifecycle(
    {
      operation,
      create,
      attach,
      ensureEvents,
      persist,
      visible: () => visible,
      read: () => session,
      publish,
      remove: () => {
        session = undefined;
        removed.resolve();
      },
    },
    intent,
  );
  const present = (generation = session?.spawnIntent?.generation ?? 0) =>
    emitNativeTerminalPresentation({
      frontendSessionId: "front",
      paneIdentity: "front",
      backendSessionId: "back",
      bindingKey: "back:7:0:",
      attemptGeneration: generation,
      incarnation: null,
      daemonEpoch: "7",
    });
  return {
    controller,
    operation,
    create,
    attach,
    ensureEvents,
    publish,
    attached,
    removed,
    present,
    persist,
    persisted,
    replace: (next: LocalSplitSession) => { session = next; },
    read: () => session,
  };
}

afterEach(() => {
  resetNativeTerminalLifecycleForTest();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("localSplitLifecycle monotonic budget and phase progression", () => {
  it("correlates monotonic stage timings in execution order", async () => {
    vi.mocked(switchDebug).mockClear();
    const presentationEntered = deferred<void>();
    vi.mocked(switchDebug).mockImplementation((name, fields) => {
      if (
        name === "terminal.localSplit.attempt" &&
        fields?.stage === "presentation" &&
        fields.phase === "begin"
      ) {
        presentationEntered.resolve();
      }
      return null;
    });
    const f = fixture();
    const run = f.controller.run({ workspaceId: "default", worktree: null });
    await presentationEntered.promise;
    f.present(1);
    await run;

    const records = vi
      .mocked(switchDebug)
      .mock.calls.filter(([name]) => name === "terminal.localSplit.attempt")
      .map(([, fields]) => fields);
    expect(records.map((fields) => [fields?.stage, fields?.phase])).toEqual(
      ["prepare", "persistPreparation", "persistCreateIntent", "create", "persistBinding", "persistAttempt", "listeners", "attach", "presentation"].flatMap((stage) => [
        [stage, "begin"],
        [stage, "end"],
      ]),
    );

    let previousElapsed = 0;
    for (const fields of records) {
      expect(fields).toMatchObject({
        requestId: prepared.identity.requestId,
        generation: 1,
        elapsedMs: expect.any(Number),
        stageElapsedMs: expect.any(Number),
        remainingMs: expect.any(Number),
      });
      const elapsed = fields?.elapsedMs as number;
      expect(elapsed).toBeGreaterThanOrEqual(previousElapsed);
      previousElapsed = elapsed;
    }
    expect(records.at(-1)).toMatchObject({
      backendSessionId: "back",
      epoch: "7",
      delivery: "confirmed",
    });
  });

  it("retains actionable failed state on phase timeout rather than permanent Reconnecting", async () => {
    vi.useFakeTimers();
    const f = fixture();
    f.operation.mockImplementation(
      () => new Promise<SplitOperationResponse>(() => {}),
    );
    const run = f.controller.run({ workspaceId: "default", worktree: null });
    await vi.advanceTimersByTimeAsync(9_001);
    await run;
    expect(f.read()?.reconnectLifecycle).toBe("failed");
    expect(f.read()?.reconnectError?.code).toBe("SPAWN_ATTEMPT_TIMEOUT");
  });

  it("retries via status-first on the same durable request without minting duplicate PTY", async () => {
    const f = fixture({ prepared, createSent: true });
    f.operation.mockResolvedValueOnce({
      action: "status",
      operation: {
        state: "created",
        sessionId: "back-existing",
        daemonEpoch: "7",
        ownership: "created",
        session: {
          sessionId: "back-existing",
          cwd: "/repo",
          cols: 80,
          rows: 24,
          running: true,
        },
      },
    });

    const run = f.controller.run();
    await f.attached.promise;
    expect(f.create).not.toHaveBeenCalled();
    expect(f.operation).toHaveBeenCalledWith(
      expect.objectContaining({
        action: "status",
        identity: prepared.identity,
      }),
    );
    emitNativeTerminalPresentation({
      frontendSessionId: "front",
      paneIdentity: "front",
      backendSessionId: "back-existing",
      incarnation: null,
      daemonEpoch: "7",
      bindingKey: "back-existing:7:0:",
      attemptGeneration: 1,
    });
    await run;
    expect(f.read()?.lifecycle).toBe("running");
    expect(f.read()?.backendSessionId).toBe("back-existing");
  });

  it("fences cancel-before-create and closes only request-owned created target", async () => {
    const f = fixture({ prepared });
    f.operation.mockResolvedValueOnce({
      action: "cancel",
      operation: { state: "cancelled" },
    });
    await f.controller.cancel();
    expect(f.operation).toHaveBeenCalledWith({
      action: "cancel",
      identity: prepared.identity,
      remainingMs: 3_000,
    });
    expect(f.read()).toBeUndefined();
  });

  it("drops stale presentation receipt from older generation", async () => {
    const f = fixture();
    const presentationEntered = deferred<void>();
    vi.mocked(switchDebug).mockImplementation((name, fields) => {
      if (
        name === "terminal.localSplit.attempt" &&
        fields?.stage === "presentation" &&
        fields.phase === "begin"
      ) {
        presentationEntered.resolve();
      }
      return null;
    });
    const run = f.controller.run({ workspaceId: "default", worktree: null });
    await presentationEntered.promise;
    f.present(999);
    expect(f.read()?.lifecycle).not.toBe("running");
    f.present(1);
    await run;
    expect(f.read()?.lifecycle).toBe("running");
  });

  it("isolates local split operations from unrelated remote RPC states", async () => {
    const f = fixture();
    const run = f.controller.run({ workspaceId: "default", worktree: null });
    await f.attached.promise;
    f.present(1);
    await run;
    expect(f.read()?.lifecycle).toBe("running");
    expect(f.read()?.workspaceId).toBe("default");
  });
});

describe("durable split races", () => {
  it.each([
    "backendSessionId", "incarnation", "daemonEpoch", "frontendSessionId",
    "paneIdentity", "bindingKey", "attemptGeneration",
  ] as const)("rejects an attach response with stale %s without changing the binding", async (field) => {
    const f = fixture(undefined, false);
    const attach = f.attach.getMockImplementation()!;
    f.attach.mockImplementation(async () => {
      const response = await attach();
      return { ...response, attachTuple: {
        ...response.attachTuple!,
        [field]: field === "attemptGeneration" ? 0 : "stale",
      } };
    });
    await f.controller.run({ workspaceId: "default", worktree: null });
    expect(f.read()?.backendSessionId).toBe("back");
    expect(f.read()?.daemonEpoch).toBe("7");
    expect(f.read()?.spawnIntent?.attachTuple?.attemptGeneration).toBe(1);
    expect(f.read()?.reconnectLifecycle).toBe("failed");
    expect(f.read()?.spawnIntent?.ready).not.toBe(true);
  });
  it("hidden creation binds durably without claiming visible readiness", async () => {
    const f = fixture(undefined, false);
    await f.controller.run({ workspaceId: "default", worktree: null });
    expect(f.read()?.backendSessionId).toBe("back");
    expect(f.read()?.spawnIntent?.bindingPersisted).toBe(true);
    expect(f.read()?.spawnIntent?.ready).not.toBe(true);
    expect(f.read()?.reconnectLifecycle).toBe("idle");
    expect(f.persisted.some((saved) => saved.backendSessionId === "back")).toBe(true);
  });
  it("retry durably advances the attempt before reattaching the same PTY", async () => {
    const f = fixture(undefined, false);
    await f.controller.run({ workspaceId: "default", worktree: null });
    const first = f.read()?.spawnIntent?.attachTuple;
    f.operation.mockResolvedValue({ action: "status", operation: {
      state: "created", sessionId: "back", daemonEpoch: "7", ownership: "created",
      session: { sessionId: "back", cwd: "/repo", cols: 80, rows: 24, running: true },
    } });
    await f.controller.run();
    expect(f.read()?.spawnIntent?.attachTuple).toEqual({ ...first, attemptGeneration: 2 });
    expect(f.persisted.some((saved) => saved.spawnIntent?.attachTuple?.attemptGeneration === 2)).toBe(true);
    expect(f.read()?.backendSessionId).toBe("back");
    expect(f.read()?.spawnIntent?.ready).not.toBe(true);
  });
  it("holds attachment until backend identity has been durably saved", async () => {
    const f = fixture();
    const saving = deferred<void>();
    const entered = deferred<void>();
    f.persist.mockImplementation(async () => {
      if (f.read()?.backendSessionId) { entered.resolve(); await saving.promise; }
    });
    const run = f.controller.run({ workspaceId: "default", worktree: null });
    await entered.promise;
    expect(f.read()?.backendSessionId).toBe("back");
    expect(f.read()?.reconnectLifecycle).toBe("spawning");
    expect(f.attach).not.toHaveBeenCalled();
    saving.resolve();
    await f.attached.promise;
    await Promise.resolve();
    f.present();
    await run;
    expect(f.read()?.lifecycle).toBe("running");
  });

  it("late creation cannot replace a newer frontend binding", async () => {
    const f = fixture();
    const held = deferred<Awaited<ReturnType<typeof f.create>>>();
    const entered = deferred<void>();
    f.create.mockImplementation(async () => { entered.resolve(); return held.promise; });
    const run = f.controller.run({ workspaceId: "default", worktree: null });
    await entered.promise;
    const current = f.read();
    if (!current?.spawnIntent) throw new Error("Missing split fixture");
    f.replace({ ...current, backendSessionId: "newer", spawnIntent: { ...current.spawnIntent, generation: 9 } });
    held.resolve({ sessionId: "late", daemonEpoch: "7", session: { sessionId: "late", cwd: "/repo", cols: 80, rows: 24, running: true } });
    await run;
    expect(f.read()?.backendSessionId).toBe("newer");
    expect(f.read()?.spawnIntent?.generation).toBe(9);
    expect(f.attach).not.toHaveBeenCalled();
  });

  it("unknown epoch retry retains the original identity without creating", async () => {
    const f = fixture({ prepared, createSent: true, backendSessionId: "back", daemonEpoch: "7" });
    f.operation.mockResolvedValue({ action: "status", operation: { state: "unknown", reason: "epochChanged" } });
    await f.controller.run();
    expect(f.read()?.spawnIntent?.requestId).toBe(prepared.identity.requestId);
    expect(f.read()?.spawnIntent?.generation).toBe(1);
    expect(f.read()?.reconnectLifecycle).toBe("failed");
    expect(f.create).not.toHaveBeenCalled();
  });

  it("cancel during held preparation retains cleanup until its authoritative reply", async () => {
    const f = fixture();
    const held = deferred<SplitOperationResponse>();
    const entered = deferred<void>();
    f.operation.mockImplementation(async () => { entered.resolve(); return held.promise; });
    const run = f.controller.run({ workspaceId: "default", worktree: null });
    await entered.promise;
    await f.controller.cancel();
    expect(f.read()?.spawnIntent?.cancelRequested).toBe(true);
    expect(f.read()?.reconnectLifecycle).toBe("failed");
    f.operation.mockImplementation(async () => {
      return { action: "cancel", operation: { state: "cancelled" } };
    });
    held.resolve({ action: "prepare", prepared });
    await f.removed.promise;
    await run;
    await Promise.resolve();
    expect(f.create).not.toHaveBeenCalled();
    expect(f.read()).toBeUndefined();
    expect(f.persisted.some((saved) => saved.spawnIntent?.cancelRequested)).toBe(true);
  });
});
