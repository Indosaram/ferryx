import { describe, expect, it, vi } from "vitest";
import {
  desktopChatMessagesFromTurns,
  desktopChatMutate,
  desktopChatRestoreDraft,
  desktopChatStopCapability,
  desktopChatTargetFor,
  desktopChatTransportFrom,
  type DesktopChatHandlers,
} from "./desktopChat";
import type { TerminalSession } from "./types";

function session(overrides: Partial<TerminalSession> = {}): TerminalSession {
  return {
    id: "pane-1",
    cwd: "/repo",
    workspaceId: "ws",
    worktree: null,
    backendSessionId: "be-1",
    lifecycle: "running",
    ownerId: "owner-1",
    daemonEpoch: "7",
    ...overrides,
  };
}

const target = { target: { hostId: "local", ownerId: "o", epoch: "1", backendSessionId: "b" } };

describe("desktopChat", () => {
  it("dispatches each request kind to its own handler", async () => {
    const page = { source: "scrollback" as const, availability: "scrollback" as const, turns: [], hasMore: false, generation: "g" };
    const handlers: DesktopChatHandlers = {
      history: vi.fn(async () => ({ ok: true as const, data: page })),
      prompt: vi.fn(async () => ({ ok: true as const, data: { prompt: null, screenRevision: null } })),
      submit: vi.fn(async () => ({ ok: true as const, data: null })),
      stop: vi.fn(async () => ({ ok: true as const, data: null })),
      answer: vi.fn(async () => ({ ok: true as const, data: null })),
    };
    const transport = desktopChatTransportFrom(handlers);
    const signal = new AbortController().signal;
    const history = await transport({ kind: "history", requestId: "r1", target, registryId: null, params: { cursor: null } }, signal);
    expect(history).toEqual({ ok: true, data: page });
    await transport(
      { kind: "stop", requestId: "r2", target, registryId: "claude", params: { capability: "providerInterrupt" } },
      signal,
    );
    expect(handlers.history).toHaveBeenCalledTimes(1);
    expect(handlers.stop).toHaveBeenCalledWith(expect.objectContaining({ kind: "stop", requestId: "r2" }), signal);
    expect(handlers.submit).not.toHaveBeenCalled();
  });

  it("addresses a pane by host, owner, epoch and backend session, and only when all are known", () => {
    expect(desktopChatTargetFor(session(), { hostId: "local" })).toEqual({
      target: { hostId: "local", ownerId: "owner-1", epoch: "7", backendSessionId: "be-1" },
      providerSessionId: null,
    });
    expect(desktopChatTargetFor(session({ daemonEpoch: null }), { hostId: "local" })).toBeNull();
    expect(desktopChatTargetFor(session({ backendSessionId: null }), { hostId: "local" })).toBeNull();
    expect(desktopChatTargetFor(session(), { hostId: "local", ownerId: "new-owner", epoch: "8" })?.target)
      .toEqual({ hostId: "local", ownerId: "owner-1", epoch: "7", backendSessionId: "be-1" });
    const exited = session({ backendSessionId: null, lastBackendSessionId: "be-old", lifecycle: "exited" });
    expect(desktopChatTargetFor(exited, { hostId: "local" })?.target.backendSessionId).toBe("be-old");
  });

  it("treats a thrown mutation as an unknown outcome", async () => {
    const outcome = await desktopChatMutate(async () => {
      throw new Error("socket closed");
    });
    expect(outcome).toEqual({ ok: false, code: "OPERATION_OUTCOME_UNKNOWN", message: "socket closed" });
  });

  it("refuses stop for an unknown agent and restores unsent text ahead of new typing", () => {
    expect(desktopChatStopCapability(null)).toBe("refused");
    expect(desktopChatStopCapability("claude")).toBe("providerInterrupt");
    expect(desktopChatRestoreDraft("", "hi")).toBe("hi");
    expect(desktopChatRestoreDraft("hi there", "hi")).toBe("hi there");
    expect(desktopChatRestoreDraft("later", "hi")).toBe("hi\nlater");
  });

  it("maps turns to messages without inventing ids or assistant text", () => {
    const messages = desktopChatMessagesFromTurns([
      { role: "user", id: "t1", parts: [{ kind: "text", text: "hello" }] },
      { role: "assistant", parts: [{ kind: "tool", name: "Read", summary: "", input: "", output: "" }] },
    ]);
    expect(messages[0]).toMatchObject({ id: "t1", role: "user", content: "hello" });
    expect(messages[1].id).toMatch(/^turn-/);
    expect(messages[1].content).toBe("");
  });
});
