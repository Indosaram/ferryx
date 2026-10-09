import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearAgentReconnectInflightForTests, reconnectAgentSession } from "./agentReconnect";
import type { TerminalSession } from "./types";

function coldSession(): TerminalSession {
  return {
    id: "local-1", workspaceId: "ws", worktree: null, cwd: "/repo",
    backendSessionId: null, lifecycle: "exited", agentType: "claude",
    providerSession: { key: "session_id", id: "provider-1" }, reconnectLifecycle: "idle",
  };
}

function result() {
  return { sessionId: "backend-new", daemonEpoch: "42", session: { sessionId: "backend-new", workspaceId: "ws", worktree: null, cwd: "/repo", cols: 80, rows: 24, running: true } };
}

describe("reconnectAgentSession", () => {
  beforeEach(clearAgentReconnectInflightForTests);

  it("spawns, attaches, and commits the same local session transactionally", async () => {
    const session = coldSession(); const actions: unknown[] = [];
    const spawn = vi.fn(async () => result()); const attach = vi.fn(async () => undefined); const persist = vi.fn(async () => undefined);
    await reconnectAgentSession(session.id, { getSessions: () => ({ [session.id]: session }), dispatch: (a) => actions.push(a), spawn, attach, persist, createRequestId: () => "request-1" });
    expect(spawn).toHaveBeenCalledWith(expect.objectContaining({ clientRequestId: "request-1", startup: { kind: "agentResume", agentType: "claude", providerSession: session.providerSession } }));
    expect(attach).toHaveBeenCalledOnce();
    expect(persist).toHaveBeenCalledWith(expect.objectContaining({ sessionId: "backend-new" }), session);
    expect(actions.map((a: any) => a.lifecycle ?? a.type)).toEqual(["validating", "spawning", "binding", "REBIND_SESSION_BACKEND"]);
  });

  it("deduplicates duplicate clicks into one spawn", async () => {
    const session = coldSession(); let resolve!: (value: ReturnType<typeof result>) => void;
    const spawn = vi.fn(() => new Promise<ReturnType<typeof result>>((r) => { resolve = r; }));
    const deps = { getSessions: () => ({ [session.id]: session }), dispatch: vi.fn(), spawn, attach: vi.fn(async () => undefined), createRequestId: () => "same" };
    const a = reconnectAgentSession(session.id, deps); const b = reconnectAgentSession(session.id, deps);
    expect(a).toBe(b); expect(spawn).toHaveBeenCalledOnce(); resolve(result()); await a;
  });

  it("reuses the same request identity after an ambiguous response loss", async () => {
    let session = coldSession();
    const requestIds: string[] = [];
    const dispatch = (action: any) => {
      if (action.type === "SET_RECONNECT_LIFECYCLE") {
        session = { ...session, reconnectLifecycle: action.lifecycle, reconnectError: action.error ?? null, reconnectRequestId: action.requestId ?? session.reconnectRequestId ?? null };
      }
    };
    const spawn = vi.fn(async (request) => {
      requestIds.push(request.clientRequestId ?? "");
      if (requestIds.length === 1) throw new Error("response lost after delivery");
      return result();
    });
    const createRequestId = vi.fn(() => "stable-across-retry");

    await expect(reconnectAgentSession(session.id, { getSessions: () => ({ [session.id]: session }), dispatch, spawn, attach: vi.fn(), createRequestId })).rejects.toBeTruthy();
    await expect(reconnectAgentSession(session.id, { getSessions: () => ({ [session.id]: session }), dispatch, spawn, attach: vi.fn(async () => undefined), createRequestId })).resolves.toMatchObject({ sessionId: "backend-new" });

    expect(requestIds).toEqual(["stable-across-retry", "stable-across-retry"]);
    expect(createRequestId).toHaveBeenCalledOnce();
  });

  it("does not spawn invalid cold state", async () => {
    const session = { ...coldSession(), providerSession: null }; const spawn = vi.fn();
    await expect(reconnectAgentSession(session.id, { getSessions: () => ({ [session.id]: session }), dispatch: vi.fn(), spawn, attach: vi.fn() })).rejects.toMatchObject({ code: "AGENT_RESUME_INVALID" });
    expect(spawn).not.toHaveBeenCalled();
  });

  it("completes reconnect for a legacy session normalized to providerSession", async () => {
    const session = { ...coldSession(), agentSessionId: "provider-1" };
    const dispatch = vi.fn();
    await expect(reconnectAgentSession(session.id, {
      getSessions: () => ({ [session.id]: session }),
      dispatch,
      spawn: vi.fn(async () => result()),
      attach: vi.fn(async () => undefined),
    })).resolves.toMatchObject({ sessionId: "backend-new" });
    expect(dispatch).toHaveBeenCalledWith(expect.objectContaining({ type: "REBIND_SESSION_BACKEND" }));
  });

  it("rolls back a spawned backend when attach fails", async () => {
    const session = coldSession(); const close = vi.fn(async () => undefined); const dispatch = vi.fn();
    await expect(reconnectAgentSession(session.id, { getSessions: () => ({ [session.id]: session }), dispatch, spawn: vi.fn(async () => result()), attach: vi.fn(async () => { throw new Error("attach failed"); }), close })).rejects.toBeTruthy();
    expect(close).toHaveBeenCalledWith("backend-new");
    expect(dispatch).not.toHaveBeenCalledWith(expect.objectContaining({ type: "REBIND_SESSION_BACKEND" }));
    expect(dispatch).toHaveBeenLastCalledWith(expect.objectContaining({ lifecycle: "failed" }));
  });

  it("closes a late spawn and never attaches after the local session leaves the workspace", async () => {
    const session = coldSession();
    let sessions: Record<string, TerminalSession> = { [session.id]: session };
    let resolve!: (value: ReturnType<typeof result>) => void;
    const spawn = vi.fn(() => new Promise<ReturnType<typeof result>>((done) => { resolve = done; }));
    const attach = vi.fn(async () => undefined);
    const persist = vi.fn(async () => undefined);
    const close = vi.fn(async () => undefined);
    const dispatch = vi.fn();

    const reconnect = reconnectAgentSession(session.id, {
      getSessions: () => sessions,
      dispatch,
      spawn,
      attach,
      persist,
      close,
      createRequestId: () => "late-workspace-result",
    });
    sessions = {};
    resolve(result());

    await expect(reconnect).rejects.toMatchObject({ code: "AGENT_RESUME_INVALID" });
    expect(close).toHaveBeenCalledWith("backend-new");
    expect(attach).not.toHaveBeenCalled();
    expect(persist).not.toHaveBeenCalled();
    expect(dispatch).not.toHaveBeenCalledWith(expect.objectContaining({ type: "REBIND_SESSION_BACKEND" }));
  });

  it("closes the backend when identity changes while attach is pending", async () => {
    const session = coldSession();
    let sessions: Record<string, TerminalSession> = { [session.id]: session };
    let resolveAttach!: () => void;
    const attach = vi.fn(() => new Promise<void>((done) => { resolveAttach = done; }));
    const persist = vi.fn(async () => undefined);
    const close = vi.fn(async () => undefined);
    const dispatch = vi.fn();

    const reconnect = reconnectAgentSession(session.id, {
      getSessions: () => sessions,
      dispatch,
      spawn: vi.fn(async () => result()),
      attach,
      persist,
      close,
    });
    await vi.waitFor(() => expect(attach).toHaveBeenCalledOnce());
    sessions = {};
    resolveAttach();

    await expect(reconnect).rejects.toMatchObject({ code: "AGENT_RESUME_INVALID" });
    expect(close).toHaveBeenCalledWith("backend-new");
    expect(persist).not.toHaveBeenCalled();
    expect(dispatch).not.toHaveBeenCalledWith(expect.objectContaining({ type: "REBIND_SESSION_BACKEND" }));
  });

  it("closes the backend when identity changes while persistence is pending", async () => {
    const session = coldSession();
    let sessions: Record<string, TerminalSession> = { [session.id]: session };
    let resolvePersist!: () => void;
    const persist = vi.fn(() => new Promise<void>((done) => { resolvePersist = done; }));
    const close = vi.fn(async () => undefined);
    const dispatch = vi.fn();

    const reconnect = reconnectAgentSession(session.id, {
      getSessions: () => sessions,
      dispatch,
      spawn: vi.fn(async () => result()),
      attach: vi.fn(async () => undefined),
      persist,
      close,
    });
    await vi.waitFor(() => expect(persist).toHaveBeenCalledOnce());
    sessions = {};
    resolvePersist();

    await expect(reconnect).rejects.toMatchObject({ code: "AGENT_RESUME_INVALID" });
    expect(close).toHaveBeenCalledWith("backend-new");
    expect(dispatch).not.toHaveBeenCalledWith(expect.objectContaining({ type: "REBIND_SESSION_BACKEND" }));
  });
});

describe("reconnectAgentSession deadline", () => {
  const deadlineMs = 1_000;
  const never = <T,>() => new Promise<T>(() => undefined);

  beforeEach(() => {
    clearAgentReconnectInflightForTests();
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  function deps(overrides: Partial<Parameters<typeof reconnectAgentSession>[1]>) {
    const session = coldSession();
    return {
      session,
      dependencies: {
        getSessions: () => ({ [session.id]: session }),
        dispatch: vi.fn(),
        spawn: vi.fn(async () => result()),
        attach: vi.fn(async () => undefined),
        persist: vi.fn(async () => undefined),
        close: vi.fn(async () => undefined),
        deadlineMs,
        ...overrides,
      },
    };
  }

  it("fails a hung spawn at the deadline and lets a retry start a new attempt", async () => {
    const spawn = vi.fn(() => never<ReturnType<typeof result>>());
    const { session, dependencies } = deps({ spawn });
    const first = reconnectAgentSession(session.id, dependencies);
    const assertion = expect(first).rejects.toMatchObject({ code: "SPAWN_ATTEMPT_TIMEOUT", details: { stage: "spawn", delivery: "ambiguous" } });
    await vi.advanceTimersByTimeAsync(deadlineMs);
    await assertion;
    expect(dependencies.dispatch).toHaveBeenLastCalledWith(expect.objectContaining({
      lifecycle: "failed", error: expect.objectContaining({ code: "SPAWN_ATTEMPT_TIMEOUT", details: expect.objectContaining({ stage: "spawn" }) }),
    }));
    expect(dependencies.close).not.toHaveBeenCalled();

    const second = reconnectAgentSession(session.id, dependencies);
    expect(second).not.toBe(first);
    expect(spawn).toHaveBeenCalledTimes(2);
    const secondAssertion = expect(second).rejects.toMatchObject({ code: "SPAWN_ATTEMPT_TIMEOUT" });
    await vi.advanceTimersByTimeAsync(deadlineMs);
    await secondAssertion;
  });

  it("fails a hung attach at the deadline and closes the spawned backend", async () => {
    const { session, dependencies } = deps({ attach: vi.fn(() => never<void>()) });
    const attempt = reconnectAgentSession(session.id, dependencies);
    const assertion = expect(attempt).rejects.toMatchObject({ code: "SPAWN_ATTEMPT_TIMEOUT", details: { stage: "attach", delivery: "confirmed" } });
    await vi.advanceTimersByTimeAsync(deadlineMs);
    await assertion;
    expect(dependencies.close).toHaveBeenCalledWith("backend-new");
    expect(dependencies.persist).not.toHaveBeenCalled();
    expect(dependencies.dispatch).toHaveBeenLastCalledWith(expect.objectContaining({ lifecycle: "failed" }));
  });

  it("fails a hung persist at the deadline without rebinding", async () => {
    const { session, dependencies } = deps({ persist: vi.fn(() => never<void>()) });
    const attempt = reconnectAgentSession(session.id, dependencies);
    const assertion = expect(attempt).rejects.toMatchObject({ code: "SPAWN_ATTEMPT_TIMEOUT", details: { stage: "persist" } });
    await vi.advanceTimersByTimeAsync(deadlineMs);
    await assertion;
    expect(dependencies.close).toHaveBeenCalledWith("backend-new");
    expect(dependencies.dispatch).not.toHaveBeenCalledWith(expect.objectContaining({ type: "REBIND_SESSION_BACKEND" }));
    expect(dependencies.dispatch).toHaveBeenLastCalledWith(expect.objectContaining({ lifecycle: "failed" }));
  });

  it("closes a spawn that resolves after the deadline and never rebinds it", async () => {
    let resolveSpawn!: (value: ReturnType<typeof result>) => void;
    const spawn = vi.fn(() => new Promise<ReturnType<typeof result>>((done) => { resolveSpawn = done; }));
    const { session, dependencies } = deps({ spawn });
    const attempt = reconnectAgentSession(session.id, dependencies);
    const assertion = expect(attempt).rejects.toMatchObject({ code: "SPAWN_ATTEMPT_TIMEOUT", details: { stage: "spawn" } });
    await vi.advanceTimersByTimeAsync(deadlineMs);
    await assertion;
    expect(dependencies.close).not.toHaveBeenCalled();

    resolveSpawn({ ...result(), sessionId: "backend-late" });
    await vi.advanceTimersByTimeAsync(0);
    expect(dependencies.close).toHaveBeenCalledWith("backend-late");
    expect(dependencies.attach).not.toHaveBeenCalled();
    expect(dependencies.dispatch).not.toHaveBeenCalledWith(expect.objectContaining({ type: "REBIND_SESSION_BACKEND" }));
  });

  it("completes within the deadline without firing the timeout", async () => {
    const { session, dependencies } = deps({});
    await expect(reconnectAgentSession(session.id, dependencies)).resolves.toMatchObject({ sessionId: "backend-new" });
    await vi.advanceTimersByTimeAsync(deadlineMs * 2);
    expect(dependencies.close).not.toHaveBeenCalled();
    expect(dependencies.dispatch).toHaveBeenLastCalledWith(expect.objectContaining({ type: "REBIND_SESSION_BACKEND" }));
  });
});
