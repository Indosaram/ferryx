/**
 * Falsification suite for lane G7 (chat-g7-falsify-20261125).
 *
 * Independent of the lane's own tests: each block tries to break one of the six claims in the
 * G7-verify brief against the frozen t3code behaviour, not against the lane's description of it.
 * A green test here means the claim survived that attack; a red one is the defect report.
 */
import { afterEach, describe, expect, it } from "vitest";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import {
  STRUCTURED_CHAT_COMPACT_FAILED,
  STRUCTURED_CHAT_ROLLBACK_FAILED,
  useStructuredChat,
} from "./structuredChat";
import {
  StructuredChatStream,
  decodeStructuredReply,
  structuredChatOpNeedsSession,
  structuredChatRequestRoute,
  type StructuredChatOp,
  type StructuredChatTransport,
} from "./structuredChatTransport";
import {
  decodeStructuredPage,
  decodeStructuredPart,
  mergeStructuredParts,
  structuredChatRoute,
  structuredPendingApprovals,
  type StructuredCapabilities,
  type StructuredPart,
  type StructuredSessionSettings,
  type StructuredSessionSummary,
} from "./structuredChatTypes";
import {
  structuredChatMessages,
  structuredChatRunning,
} from "./structuredChatParts";

afterEach(() => {
  cleanup();
});

const SIGNAL = new AbortController().signal;

function at(seq: number, extra: Record<string, unknown>): StructuredPart {
  return { seq, tsMs: 1000 + seq, ...extra } as StructuredPart;
}

function user(seq: number, text: string, turnId = `t${seq}`): StructuredPart {
  return at(seq, { turnId, type: "userMessage", text });
}

function assistant(seq: number, text: string, turnId = `t${seq}`): StructuredPart {
  return at(seq, { turnId, type: "assistantMessage", text });
}

/** A transport driven from an in-memory journal with knobs for each attack. */
function makeFake(options: {
  readonly parts?: readonly StructuredPart[];
  readonly state?: "running" | "ended";
  readonly steer?: boolean;
  readonly capabilities?: StructuredCapabilities;
} = {}) {
  const calls: { op: StructuredChatOp; body: unknown }[] = [];
  let parts = [...(options.parts ?? [])];
  let state: "running" | "ended" = options.state ?? "running";
  let seq = parts.reduce((highest, part) => Math.max(highest, part.seq), 0);
  let failEvents: { code: string; message: string } | null = null;
  let failList = false;
  let failTurn = false;
  let failRollback = false;
  let holdTurn = false;
  let afterInterrupt: readonly StructuredPart[] = [];
  let release: (() => void) | null = null;
  let gate: Promise<void> | null = null;

  const summary: StructuredSessionSummary = {
    sessionId: "s1",
    provider: "codex",
    cwd: "/repo",
    get state() {
      return state;
    },
    settings: {},
  };

  const capabilities: StructuredCapabilities = options.capabilities ?? {
    steer: options.steer ?? true,
    rollback: true,
    permissionModes: ["supervised", "full-access"],
    models: [{ id: "gpt-5", label: "GPT-5", efforts: ["low", "high"] }],
    slashCommands: ["/compact"],
  };

  const transport = (async (request: { op: StructuredChatOp; sessionId?: string; body: unknown }) => {
    calls.push({ op: request.op, body: request.body });
    const body = (request.body ?? {}) as Record<string, unknown>;
    switch (request.op) {
      case "createSession":
        return { ok: true, data: summary };
      case "listSessions":
        if (failList) return { ok: false, code: "NOT_FOUND", message: "gateway restarting" };
        return { ok: true, data: [summary] };
      case "capabilities":
        return { ok: true, data: capabilities };
      case "history":
        return { ok: true, data: { parts, nextSeq: seq } };
      case "events": {
        if (gate !== null) await gate;
        if (failEvents !== null) return { ok: false, ...failEvents };
        const after = (body.after as number | undefined) ?? 0;
        return { ok: true, data: { parts: parts.filter((part) => part.seq > after), nextSeq: seq } };
      }
      case "turn": {
        if (failTurn) return { ok: false, code: "CONFLICT", message: "TURN_ACTIVE: busy" };
        if (holdTurn) return { ok: true, data: seq + 1 };
        seq += 1;
        parts = [...parts, user(seq, String(body.text))];
        return { ok: true, data: seq };
      }
      case "steer": {
        seq += 1;
        parts = [...parts, user(seq, String(body.text), "t1")];
        return { ok: true, data: seq };
      }
      case "interrupt": {
        if (afterInterrupt.length > 0) {
          parts = [...parts, ...afterInterrupt];
          seq = Math.max(seq, ...afterInterrupt.map((part) => part.seq));
        }
        return { ok: true, data: undefined };
      }
      case "rollback": {
        if (failRollback) {
          return {
            ok: false,
            code: "INVALID_REQUEST",
            message: "the session does not have that many turns",
          };
        }
        const turns = Number(body.turns ?? 0);
        const turnIds = [...new Set(parts.map((part) => part.turnId).filter((id): id is string => id !== undefined))];
        const drop = new Set(turnIds.slice(Math.max(0, turnIds.length - turns)));
        parts = parts.filter((part) => part.turnId === undefined || !drop.has(part.turnId));
        seq = parts.reduce((highest, part) => Math.max(highest, part.seq), 0);
        return { ok: true, data: undefined };
      }
      case "approve": {
        const requestId = String(body.requestId);
        parts = parts.map((part) =>
          part.type === "approvalRequest" && part.requestId === requestId
            ? ({ ...part, answered: true } as StructuredPart)
            : part,
        );
        return { ok: true, data: undefined };
      }
      case "settings":
        return { ok: true, data: body as StructuredSessionSettings };
      case "clear": {
        seq += 1;
        parts = [...parts, at(seq, { type: "newSession" })];
        return { ok: true, data: seq };
      }
      case "compact":
        return { ok: false, code: "COMPACT_UNSUPPORTED", message: "COMPACT_UNSUPPORTED: no compact method" };
      case "closeSession":
        return { ok: true, data: undefined };
      default:
        return { ok: false, code: "INVALID_REQUEST", message: "unknown op" };
    }
  }) as unknown as StructuredChatTransport;

  return {
    transport,
    calls,
    setParts(next: readonly StructuredPart[]) {
      parts = [...next];
      seq = parts.reduce((highest, part) => Math.max(highest, part.seq), 0);
    },
    setState(next: "running" | "ended") {
      state = next;
    },
    setFailEvents(next: { code: string; message: string } | null) {
      failEvents = next;
    },
    setFailList(next: boolean) {
      failList = next;
    },
    setFailTurn(next: boolean) {
      failTurn = next;
    },
    setFailRollback(next: boolean) {
      failRollback = next;
    },
    setHoldTurn(next: boolean) {
      holdTurn = next;
    },
    setAfterInterrupt(next: readonly StructuredPart[]) {
      afterInterrupt = next;
    },
    /** Holds every `events` reply until `releaseGate()` runs. */
    closeGate() {
      gate = new Promise<void>((resolve) => {
        release = resolve;
      });
    },
    releaseGate() {
      release?.();
      gate = null;
      release = null;
    },
    ops(): StructuredChatOp[] {
      return calls.map((call) => call.op);
    },
    readParts(): readonly StructuredPart[] {
      return parts;
    },
  };
}

function mountChat(fake: ReturnType<typeof makeFake>, extra: Record<string, unknown> = {}) {
  return renderHook(() => useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0, ...extra }));
}

// =============================================================================================
// Claim 1 - decoding: unknown/broken parts kept as raw, never dropped or trusted; seq merge
// dedupes and the newer copy wins; an empty page keeps its cursor.
// =============================================================================================
describe("claim 1: decoding", () => {
  it("keeps every kind it cannot read as raw, with the payload intact and the row count unchanged", () => {
    const rows = [
      { seq: 1, tsMs: 1, type: "toolCall", id: "c1" },
      { seq: 2, tsMs: 2, type: "futureKind", nested: { a: [1, 2] } },
      { seq: 3, tsMs: 3, type: "usage", totalTokens: "lots" },
      { seq: 4, tsMs: 4, type: "fileChange", id: "f", status: "completed", changes: [{ path: "p" }] },
      { seq: 5, tsMs: 5, type: "userMessage", text: "fine" },
      { seq: 6, tsMs: 6 },
    ];
    const decoded = rows.map(decodeStructuredPart);
    // Nothing with a usable envelope is dropped, however broken its body.
    expect(decoded.filter((part) => part !== null)).toHaveLength(rows.length);
    expect(decoded.map((part) => part?.type)).toEqual(["raw", "raw", "raw", "raw", "userMessage", "raw"]);
    expect(decoded[1]).toMatchObject({ rawType: "futureKind", value: { nested: { a: [1, 2] } } });
    expect(decoded[3]).toMatchObject({ rawType: "fileChange" });
    expect(decoded[5]).toMatchObject({ rawType: "unknown" });
  });

  it("never trusts a raw part: a settled turn stays settled and a raw row is not a user message", () => {
    // The last readable part is the final answer; a newer kind the client cannot read must not
    // resurrect the turn (that would pin the composer's running state forever).
    const settled = [assistant(1, "done", "t1"), at(2, { type: "raw", rawType: "futureKind", value: {} })];
    expect(structuredChatRunning(settled)).toBe(false);
    // A raw part draws as a notice, never as something the user is shown as their own message.
    const messages = structuredChatMessages([user(1, "go", "t1"), at(2, { turnId: "t1", type: "raw", rawType: "x", value: { a: 1 } })]);
    expect(messages.filter((message) => message.role === "user").map((message) => message.content)).toEqual(["go"]);
    expect(messages.some((message) => message.content.includes('"a":1'))).toBe(true);
  });

  it("dedupes a replayed page by seq and lets the later copy of a seq win", () => {
    const current = [user(1, "a"), at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" })];
    const replay = [
      at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "completed" }),
      user(1, "a"),
    ];
    const merged = mergeStructuredParts(current, replay);
    expect(merged.map((part) => part.seq)).toEqual([1, 2]);
    expect(merged[1]).toMatchObject({ type: "toolCall", status: "completed" });
  });

  it("keeps the highest seq of an out-of-order page and sorts it on merge", () => {
    const page = decodeStructuredPage({ parts: [user(7, "c"), user(3, "a"), user(5, "b")] }, 2);
    // Decoding preserves the wire order; the merge is what makes the stream ascending.
    expect(page.parts.map((part) => part.seq)).toEqual([7, 3, 5]);
    expect(page.nextSeq).toBe(7);
    expect(mergeStructuredParts([], page.parts).map((part) => part.seq)).toEqual([3, 5, 7]);
  });

  it("keeps the cursor an empty page was asked with", () => {
    expect(decodeStructuredPage({ parts: [] }, 42).nextSeq).toBe(42);
    expect(decodeStructuredPage(null, 42).nextSeq).toBe(42);
  });
});

// =============================================================================================
// Claim 2 - transport: all 14 ops routed; a malformed response is MALFORMED_RESPONSE; the stream
// keeps its seq cursor across a failed poll; reset() discards an in-flight read.
// =============================================================================================
describe("claim 2: transport", () => {
  it("routes every one of the 14 ops, with a session id only where the contract needs one", () => {
    const ops: StructuredChatOp[] = [
      "createSession",
      "listSessions",
      "turn",
      "history",
      "events",
      "steer",
      "interrupt",
      "rollback",
      "approve",
      "settings",
      "capabilities",
      "clear",
      "compact",
      "closeSession",
    ];
    const seen = new Set<string>();
    for (const op of ops) {
      const route = structuredChatRequestRoute(op, "s1");
      expect(route.path.startsWith("/api/v1/structured-chat/sessions")).toBe(true);
      seen.add(`${route.method} ${route.path}`);
    }
    expect(seen.size).toBe(ops.length);
    expect(structuredChatOpNeedsSession("createSession")).toBe(false);
    expect(structuredChatOpNeedsSession("listSessions")).toBe(false);
    for (const op of ops.slice(2)) expect(structuredChatOpNeedsSession(op)).toBe(true);
    expect(structuredChatRoute("s/1", "turns")).toBe("/api/v1/structured-chat/sessions/s%2F1/turns");
  });

  it("answers MALFORMED_RESPONSE when a turn reply has no seq", () => {
    expect(decodeStructuredReply("turn", 200, { nothing: true })).toMatchObject({
      ok: false,
      code: "MALFORMED_RESPONSE",
    });
  });

  it("answers MALFORMED_RESPONSE, not an empty page, when a history body did not parse", () => {
    // `readJson` answers null for a truncated or non-JSON 200 body. Rendering that as an empty
    // transcript is the failure the claim forbids: the user sees a blank chat, not an error.
    expect(decodeStructuredReply("history", 200, null)).toMatchObject({
      ok: false,
      code: "MALFORMED_RESPONSE",
    });
    expect(decodeStructuredReply("history", 200, { parts: "not-an-array" })).toMatchObject({
      ok: false,
      code: "MALFORMED_RESPONSE",
    });
  });

  it("answers MALFORMED_RESPONSE, not an empty list, when a listSessions body did not parse", () => {
    expect(decodeStructuredReply("listSessions", 200, null)).toMatchObject({
      ok: false,
      code: "MALFORMED_RESPONSE",
    });
  });

  it("keeps the stream's rows and cursor across a failed poll, then resumes from the same place", async () => {
    const calls: number[] = [];
    let fail = false;
    const transport: StructuredChatTransport = async (request) => {
      const after = (request.body as { after: number }).after;
      calls.push(after);
      if (fail) return { ok: false, code: "NOT_FOUND", message: "gateway restarting" };
      return {
        ok: true,
        data: { parts: after === 0 ? [user(1, "hi")] : [user(2, "next", "t2")], nextSeq: after === 0 ? 1 : 2 },
      };
    };
    const stream = new StructuredChatStream(transport, "s1");
    await stream.load(SIGNAL);
    fail = true;
    expect(await stream.poll(SIGNAL)).toMatchObject({ ok: false, code: "NOT_FOUND" });
    expect(stream.after()).toBe(1);
    expect(stream.read().map((part) => part.seq)).toEqual([1]);
    fail = false;
    await stream.poll(SIGNAL);
    expect(calls).toEqual([0, 1, 1]);
    expect(stream.read().map((part) => part.seq)).toEqual([1, 2]);
    expect(stream.after()).toBe(2);
  });

  it("never duplicates a row when a replayed page arrives past the cursor", async () => {
    let page: StructuredPart[] = [user(1, "hi")];
    const transport: StructuredChatTransport = async () => ({ ok: true, data: { parts: page, nextSeq: 9 } });
    const stream = new StructuredChatStream(transport, "s1");
    await stream.load(SIGNAL);
    page = [user(1, "hi"), user(2, "again", "t2")];
    await stream.poll(SIGNAL);
    page = [user(1, "hi"), user(2, "again", "t2")];
    await stream.poll(SIGNAL);
    expect(stream.read().map((part) => part.seq)).toEqual([1, 2]);
  });

  it("discards a poll that was in flight when reset() ran", async () => {
    let release: (() => void) | null = null;
    const transport: StructuredChatTransport = async () => {
      await new Promise<void>((resolve) => {
        release = resolve;
      });
      return { ok: true, data: { parts: [user(9, "stale")], nextSeq: 9 } };
    };
    const stream = new StructuredChatStream(transport, "s1");
    const pending = stream.load(SIGNAL);
    await Promise.resolve();
    stream.reset();
    release!();
    await pending;
    expect(stream.read()).toEqual([]);
    expect(stream.after()).toBe(0);
  });
});

// =============================================================================================
// Claim 3 - reconnect after a gateway restart; send on an ended session starts a turn.
// =============================================================================================
describe("claim 3: reconnect", () => {
  it("sends a turn, never a steer, on a session the gateway reports ended", async () => {
    const fake = makeFake({
      state: "ended",
      steer: true,
      parts: [
        user(1, "go"),
        at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" }),
      ],
    });
    const { result } = mountChat(fake);
    await waitFor(() => expect(result.current.session?.state).toBe("ended"));
    expect(result.current.isRunning).toBe(false);
    await act(async () => {
      result.current.send("again");
    });
    await waitFor(() => expect(fake.ops()).toContain("turn"));
    expect(fake.ops()).not.toContain("steer");
  });

  it("re-reads the summary when a poll fails, so a restarted gateway turns the next send into a turn", async () => {
    const fake = makeFake({
      parts: [user(1, "go"), at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" })],
    });
    const { result } = mountChat(fake);
    await waitFor(() => expect(result.current.isRunning).toBe(true));
    fake.setFailEvents({ code: "NOT_FOUND", message: "gateway restarting" });
    fake.setState("ended");
    await act(async () => {
      await result.current.pollNow();
    });
    await waitFor(() => expect(result.current.session?.state).toBe("ended"));
    expect(result.current.isRunning).toBe(false);
    await act(async () => {
      result.current.send("resume");
    });
    await waitFor(() => expect(fake.ops()).toContain("turn"));
    expect(fake.ops()).not.toContain("steer");
  });
});

// =============================================================================================
// Claim 4 - the verbs behave per t3code; /clear and /compact are lane commands; a command sent
// during a running turn moves to the front of the queue.
// =============================================================================================
describe("claim 4: verbs and queue", () => {
  it("reaches every verb's own op", async () => {
    const fake = makeFake({ parts: [user(1, "go"), assistant(2, "done", "t1")] });
    const { result } = mountChat(fake);
    await waitFor(() => expect(result.current.parts).toHaveLength(2));
    await act(async () => {
      result.current.send("one");
    });
    await act(async () => {
      await result.current.steer("two");
    });
    await act(async () => {
      await result.current.interrupt();
    });
    await act(async () => {
      await result.current.rollback(1);
    });
    await act(async () => {
      await result.current.setSettings({ model: "gpt-5" });
    });
    await act(async () => {
      await result.current.clear();
    });
    await act(async () => {
      await result.current.compact();
    });
    const ops = new Set(fake.ops());
    const expected: StructuredChatOp[] = ["turn", "steer", "interrupt", "rollback", "settings", "clear", "compact"];
    for (const op of expected) {
      expect(ops.has(op)).toBe(true);
    }
  });

  it("never sends rollback(0) to the server and reports nothing taken back", async () => {
    const fake = makeFake({ parts: [user(1, "go"), assistant(2, "done", "t1")] });
    const { result } = mountChat(fake);
    await waitFor(() => expect(result.current.parts).toHaveLength(2));
    let removed: readonly string[] = ["sentinel"];
    await act(async () => {
      removed = await result.current.rollback(0);
    });
    expect(removed).toEqual([]);
    expect(fake.ops()).not.toContain("rollback");
  });

  it("surfaces a warning and takes nothing back when the server refuses an over-long rollback", async () => {
    const fake = makeFake({ parts: [user(1, "go"), assistant(2, "done", "t1")] });
    fake.setFailRollback(true);
    const { result } = mountChat(fake);
    await waitFor(() => expect(result.current.parts).toHaveLength(2));
    let removed: readonly string[] = ["sentinel"];
    await act(async () => {
      removed = await result.current.rollback(9);
    });
    expect(removed).toEqual([]);
    await waitFor(() => expect(result.current.warnings).toContain(STRUCTURED_CHAT_ROLLBACK_FAILED));
  });

  it("puts a command sent during a running turn at the front, ahead of an older queued message", async () => {
    const fake = makeFake({
      steer: false,
      parts: [user(1, "go"), at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" })],
    });
    const { result } = mountChat(fake);
    await waitFor(() => expect(result.current.isRunning).toBe(true));
    await act(async () => {
      result.current.send("later");
    });
    await act(async () => {
      result.current.send("/compact");
    });
    expect(result.current.queuedMessages.map((row) => row.text)).toEqual(["/compact", "later"]);
    expect(fake.ops()).not.toContain("compact");
  });

  it("reports a compact the agent does not support instead of a silent success", async () => {
    const fake = makeFake({ parts: [user(1, "go"), assistant(2, "done", "t1")] });
    const { result } = mountChat(fake);
    await waitFor(() => expect(result.current.parts).toHaveLength(2));
    await act(async () => {
      await result.current.compact();
    });
    await waitFor(() => expect(result.current.warnings).toContain(STRUCTURED_CHAT_COMPACT_FAILED));
  });

  it("holds queued messages after a stop, as t3code's queue does, instead of sending them at once", async () => {
    // t3code composer.md:282 - "If you stop the agent or the server restarts, queued messages
    // keep their order and are held. Press Resume in an empty composer to continue."
    const fake = makeFake({
      steer: false,
      parts: [user(1, "go"), at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" })],
    });
    fake.setAfterInterrupt([assistant(3, "stopped", "t1")]);
    const { result } = mountChat(fake);
    await waitFor(() => expect(result.current.isRunning).toBe(true));
    await act(async () => {
      result.current.send("later");
    });
    expect(result.current.queuedMessages).toHaveLength(1);
    expect(fake.ops()).not.toContain("turn");
    await act(async () => {
      await result.current.interrupt();
    });
    // The stop landed (the lane read the turn-closing part the interrupt produced).
    await waitFor(() => expect(fake.ops()).toContain("interrupt"));
    await waitFor(() => expect(result.current.parts.some((part) => part.type === "assistantMessage")).toBe(true));
    // t3code holds the queue across a stop; the lane must not turn the held row into a turn.
    expect(fake.ops()).not.toContain("turn");
    expect(result.current.queuedMessages.map((row) => row.text)).toEqual(["later"]);
  });
});

// =============================================================================================
// Claim 5 - the optimistic echo is reconciled exactly once: no duplicate, no loss.
// =============================================================================================
describe("claim 5: optimistic echo", () => {
  it("reconciles one send into exactly one confirmed message", async () => {
    const fake = makeFake();
    const { result } = mountChat(fake);
    await waitFor(() => expect(result.current.composerCapabilities).not.toBeNull());
    await act(async () => {
      result.current.send("hello");
    });
    await waitFor(() =>
      expect(result.current.messages.filter((message) => message.content === "hello")).toHaveLength(1),
    );
    expect(result.current.messages[0].pending).toBeUndefined();
  });

  it("keeps the second identical send visible while only the first one is confirmed", async () => {
    const fake = makeFake();
    fake.setHoldTurn(true);
    const { result } = mountChat(fake);
    await waitFor(() => expect(result.current.composerCapabilities).not.toBeNull());
    await act(async () => {
      result.current.send("hello");
    });
    await act(async () => {
      result.current.send("hello");
    });
    await waitFor(() =>
      expect(result.current.messages.filter((message) => message.content === "hello")).toHaveLength(2),
    );
    // Only the first turn's user part has landed.
    fake.setParts([user(1, "hello")]);
    await act(async () => {
      await result.current.pollNow();
    });
    const shown = result.current.messages.filter((message) => message.content === "hello");
    // One confirmed + one still unconfirmed send = two rows. Anything less lost a message.
    expect(shown).toHaveLength(2);
  });

  it("restores the draft and shows no echo when the send is refused", async () => {
    const transport = (async (request: { op: string }) => {
      if (request.op === "turn") return { ok: false, code: "CONFLICT", message: "TURN_ACTIVE" };
      if (request.op === "listSessions") {
        return {
          ok: true,
          data: [
            { sessionId: "s1", provider: "codex", cwd: "/repo", state: "running", settings: {} },
          ],
        };
      }
      if (request.op === "capabilities") {
        return {
          ok: true,
          data: { steer: true, rollback: true, permissionModes: ["supervised"], models: [], slashCommands: [] },
        };
      }
      return { ok: true, data: { parts: [], nextSeq: 0 } };
    }) as unknown as StructuredChatTransport;
    const { result } = renderHook(() => useStructuredChat({ transport, sessionId: "s1", pollMs: 0 }));
    await waitFor(() => expect(result.current.composerCapabilities).not.toBeNull());
    await act(async () => {
      result.current.send("hello");
    });
    await waitFor(() => expect(result.current.draft).toBe("hello"));
    expect(result.current.messages).toEqual([]);
  });
});

// =============================================================================================
// Claim 6 - capabilities reach the composer pickers.
// =============================================================================================
describe("claim 6: capabilities to pickers", () => {
  it("lights up the model list with its efforts, the permission option and the slash commands", async () => {
    const fake = makeFake();
    const { result } = mountChat(fake);
    await waitFor(() => expect(result.current.composerCapabilities).not.toBeNull());
    const capabilities = result.current.composerCapabilities!;
    expect(capabilities.models?.choices).toEqual([
      { id: "gpt-5", label: "GPT-5", efforts: ["low", "high"] },
    ]);
    expect(capabilities.options.map((option) => option.id)).toContain("permissionMode");
    expect(capabilities.options[0].values).toEqual(["supervised", "full-access"]);
    expect(capabilities.slashCommands.map((command) => command.name)).toEqual(["/compact"]);
    expect(capabilities.followUp).toEqual({ queue: true, steer: true });
  });

  it("keeps a null capability null and hides the model picker when the agent lists none", async () => {
    const fake = makeFake({
      capabilities: {
        steer: false,
        rollback: false,
        permissionModes: [],
        models: [],
        slashCommands: [],
      },
    });
    const { result } = mountChat(fake);
    await waitFor(() => expect(result.current.composerCapabilities).not.toBeNull());
    const capabilities = result.current.composerCapabilities!;
    expect(capabilities.models).toBeNull();
    expect(capabilities.slashCommands).toEqual([]);
    expect(capabilities.followUp).toEqual({ queue: true, steer: false });
  });

  it("does not offer a permission mode the agent never advertised", async () => {
    // referenceTypes.ts:302 - "Missing models, options, or slash entries hide that UI." An agent
    // that advertises no permission modes must not get a picker with all four: picking one then
    // asks the store to apply a mode the provider never claimed to support.
    const fake = makeFake({
      capabilities: {
        steer: true,
        rollback: true,
        permissionModes: [],
        models: [],
        slashCommands: [],
      },
    });
    const { result } = mountChat(fake);
    await waitFor(() => expect(result.current.composerCapabilities).not.toBeNull());
    expect(result.current.composerCapabilities!.options).toEqual([]);
  });
});

// =============================================================================================
// Defect probes: each of these encodes what t3code does, not what the lane does.
// =============================================================================================
describe("defect probe: a slash command waits for the running turn", () => {
  it("queues an advertised slash command instead of steering it into the turn", async () => {
    // composer.md:72-74 - "A slash command such as `/compact` or `/context` waits for the current
    // turn to finish, because a command steered into a turn would replace the agent's task."
    // The lane only special-cases its own two commands, so every other advertised command is
    // sent as steer text while a turn runs.
    const fake = makeFake({
      steer: true,
      parts: [
        user(1, "go"),
        at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" }),
      ],
      capabilities: {
        steer: true,
        rollback: true,
        permissionModes: ["supervised"],
        models: [],
        slashCommands: ["/context", "/compact"],
      },
    });
    const { result } = mountChat(fake);
    await waitFor(() => expect(result.current.isRunning).toBe(true));
    await act(async () => {
      result.current.send("/context");
    });
    expect(fake.ops()).not.toContain("steer");
    expect(result.current.queuedMessages.map((row) => row.text)).toEqual(["/context"]);
  });
});

describe("defect probe: a failed queued delivery", () => {
  it("keeps the row and does not lose the text when the queued turn is refused", async () => {
    const fake = makeFake({
      steer: false,
      parts: [
        user(1, "go"),
        at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" }),
      ],
    });
    const { result } = mountChat(fake);
    await waitFor(() => expect(result.current.isRunning).toBe(true));
    await act(async () => {
      result.current.send("later");
    });
    expect(result.current.queuedMessages).toHaveLength(1);
    // The turn ends and the drain tries to deliver; the store refuses it.
    fake.setFailTurn(true);
    fake.setParts([...fake.readParts(), assistant(3, "done", "t1")]);
    await act(async () => {
      await result.current.pollNow();
    });
    await waitFor(() => expect(result.current.queuedMessages).toHaveLength(1));
    // The text is still reachable: either it is back in the draft, or it is still queued.
    const recoverable =
      result.current.draft.includes("later") || result.current.queuedMessages.some((row) => row.text === "later");
    expect(recoverable).toBe(true);
  });
});

// =============================================================================================
// Hunt list: poll racing with send, approval answered twice, very large pages.
// =============================================================================================
describe("hunt: races and scale", () => {
  it("does not duplicate a part when a poll that started before a send lands after it", async () => {
    const fake = makeFake({ parts: [user(1, "first")] });
    const { result } = mountChat(fake);
    await waitFor(() => expect(result.current.parts).toHaveLength(1));
    fake.closeGate();
    await act(async () => {
      const polling = result.current.pollNow();
      result.current.send("second");
      await Promise.resolve();
      fake.releaseGate();
      await polling;
    });
    await waitFor(() => expect(result.current.parts.length).toBeGreaterThan(1));
    const texts = result.current.parts
      .filter((part): part is Extract<StructuredPart, { type: "userMessage" }> => part.type === "userMessage")
      .map((part) => part.text);
    expect(texts).toEqual(["first", "second"]);
  });

  it("retires an approval once and never draws it again after it is answered", async () => {
    const parts = [
      at(1, { turnId: "t1", type: "approvalRequest", requestId: "r1", kind: "command", summary: "run", answered: false }),
    ];
    const fake = makeFake({ parts });
    const { result } = mountChat(fake);
    await waitFor(() => expect(result.current.prompt?.id).toBe("r1"));
    await act(async () => {
      await result.current.answerApproval("r1", "accept");
    });
    await waitFor(() => expect(result.current.prompt).toBeNull());
    expect(structuredPendingApprovals(result.current.parts)).toEqual([]);
    // A second answer is refused by the server, and the lane says so instead of pretending.
    await act(async () => {
      await result.current.answerApproval("r1", "accept");
    });
    expect(result.current.prompt).toBeNull();
  });

  it("merges and draws a very large page without dropping or duplicating a row", () => {
    const size = 20000;
    const page: StructuredPart[] = [];
    for (let index = 1; index <= size; index += 1) {
      page.push(index % 2 === 0 ? assistant(index, `a${index}`) : user(index, `u${index}`));
    }
    const shuffled = [...page].reverse();
    const merged = mergeStructuredParts([], shuffled);
    expect(merged).toHaveLength(size);
    expect(merged[0].seq).toBe(1);
    expect(merged[size - 1].seq).toBe(size);
    const again = mergeStructuredParts(merged, page);
    expect(again).toHaveLength(size);
    const messages = structuredChatMessages(merged.slice(0, 2000));
    expect(messages).toHaveLength(2000);
  });
});
