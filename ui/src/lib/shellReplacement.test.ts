import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearShellReplacementInflightForTests, replaceExitedShellSession } from "./shellReplacement";
import type { TerminalSession } from "./types";

const session = (): TerminalSession => ({
  id: "local-shell", workspaceId: "ws", worktree: null, cwd: "/repo",
  backendSessionId: null, lifecycle: "exited", agentType: null, providerSession: null,
});
const result = () => ({ sessionId: "backend-shell", daemonEpoch: "new", session: { sessionId: "backend-shell", workspaceId: "ws", worktree: null, cwd: "/repo", cols: 80, rows: 24, running: true } });

describe("replaceExitedShellSession", () => {
  beforeEach(clearShellReplacementInflightForTests);

  it("spawns startup null, persists, and rebinds the same local shell", async () => {
    const local = session(); const spawn = vi.fn(async () => result()); const persist = vi.fn(async () => undefined); const dispatch = vi.fn();
    await replaceExitedShellSession(local.id, { getSessions: () => ({ [local.id]: local }), dispatch, spawn, persist, createRequestId: () => "shell-request" });
    expect(spawn).toHaveBeenCalledWith(expect.objectContaining({ clientRequestId: "shell-request", startup: null }));
    expect(persist).toHaveBeenCalledOnce();
    expect(dispatch).toHaveBeenCalledWith(expect.objectContaining({ type: "REBIND_SESSION_BACKEND", sessionId: local.id, backendSessionId: "backend-shell" }));
  });

  it("deduplicates activation into one normal shell spawn", async () => {
    const local = session(); let resolve!: (value: ReturnType<typeof result>) => void;
    const spawn = vi.fn(() => new Promise<ReturnType<typeof result>>((done) => { resolve = done; }));
    const deps = { getSessions: () => ({ [local.id]: local }), dispatch: vi.fn(), spawn };
    const first = replaceExitedShellSession(local.id, deps); const second = replaceExitedShellSession(local.id, deps);
    expect(first).toBe(second); expect(spawn).toHaveBeenCalledOnce(); resolve(result()); await first;
  });

  it("closes a late backend and never rebinds after the local shell changes", async () => {
    const local = session(); let sessions: Record<string, TerminalSession> = { [local.id]: local };
    const close = vi.fn(async () => undefined); const dispatch = vi.fn();
    const spawn = vi.fn(async () => { sessions = {}; return result(); });
    await expect(replaceExitedShellSession(local.id, { getSessions: () => sessions, dispatch, spawn, close })).rejects.toMatchObject({ code: "AGENT_RESUME_INVALID" });
    expect(close).toHaveBeenCalledWith("backend-shell"); expect(dispatch).not.toHaveBeenCalled();
  });

  it("rolls back when proposed-binding persistence fails", async () => {
    const local = session(); const close = vi.fn(async () => undefined); const dispatch = vi.fn();
    await expect(replaceExitedShellSession(local.id, { getSessions: () => ({ [local.id]: local }), dispatch, spawn: vi.fn(async () => result()), persist: vi.fn(async () => { throw new Error("persist failed"); }), close })).rejects.toBeTruthy();
    expect(close).toHaveBeenCalledWith("backend-shell"); expect(dispatch).not.toHaveBeenCalled();
  });
});
