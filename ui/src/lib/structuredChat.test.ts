import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import {
  STRUCTURED_CHAT_QUEUED_FAILED,
  STRUCTURED_CHAT_REVERT_FILES_UNSUPPORTED,
  STRUCTURED_CHAT_SEND_FAILED,
  STRUCTURED_CHAT_SESSION_ENDED,
  structuredMediaCachePut,
  useStructuredChat,
  structuredUserTextCount,
} from "./structuredChat";
import type {
  StructuredChatOp,
  StructuredChatResult,
  StructuredChatTransport,
} from "./structuredChatTransport";
import type {
  StructuredCapabilities,
  StructuredPart,
  StructuredSessionSettings,
  StructuredSessionSummary,
} from "./structuredChatTypes";

afterEach(() => {
  cleanup();
});

function at(seq: number, extra: Record<string, unknown>): StructuredPart {
  return { seq, tsMs: 1000 + seq, ...extra } as StructuredPart;
}

interface FakeCalls {
  readonly op: StructuredChatOp;
  readonly body: unknown;
}

interface FakeOptions {
  readonly parts?: StructuredPart[];
  readonly capabilities?: StructuredCapabilities;
  readonly settings?: StructuredSessionSettings;
  readonly failTurn?: { code: string; message: string };
  readonly failRollback?: boolean;
  readonly state?: StructuredSessionSummary["state"];
}

/**
 * A transport that answers from an in-memory journal. `turn` appends the user part the way the
 * store does (the echo test relies on that), so the hook is exercised against real seq flow.
 */
function fakeTransport(options: FakeOptions = {}) {
  const calls: FakeCalls[] = [];
  let parts = [...(options.parts ?? [])];
  const capabilities: StructuredCapabilities = options.capabilities ?? {
    steer: true,
    rollback: true,
    permissionModes: ["supervised", "full-access"],
    models: [{ id: "gpt-5", label: "GPT-5", efforts: ["low"] }],
    slashCommands: [],
  };
  const summary: StructuredSessionSummary = {
    sessionId: "s1",
    provider: "codex",
    cwd: "/repo",
    state: options.state ?? "running",
    settings: options.settings ?? {},
  };
  let seq = parts.reduce((highest, part) => Math.max(highest, part.seq), 0);

  const transport = (async (request: { op: StructuredChatOp; sessionId?: string; body: unknown }) => {
    calls.push({ op: request.op, body: request.body });
    const after = (request.body as { after?: number }).after ?? 0;
    switch (request.op) {
      case "createSession":
        return { ok: true, data: summary } as StructuredChatResult<StructuredSessionSummary>;
      case "listSessions":
        return { ok: true, data: [summary] } as StructuredChatResult<readonly StructuredSessionSummary[]>;
      case "capabilities":
        return { ok: true, data: capabilities } as StructuredChatResult<StructuredCapabilities>;
      case "history":
        return { ok: true, data: { parts, nextSeq: seq } };
      case "events":
        return { ok: true, data: { parts: parts.filter((part) => part.seq > after), nextSeq: seq } };
      case "turn": {
        if (options.failTurn !== undefined) {
          return { ok: false, ...options.failTurn } as StructuredChatResult<number>;
        }
        seq += 1;
        const text = (request.body as { text: string }).text;
        parts = [...parts, at(seq, { turnId: `t${seq}`, type: "userMessage", text })];
        return { ok: true, data: seq } as StructuredChatResult<number>;
      }
      case "steer": {
        seq += 1;
        const text = (request.body as { text: string }).text;
        parts = [...parts, at(seq, { turnId: "t1", type: "userMessage", text })];
        return { ok: true, data: seq } as StructuredChatResult<number>;
      }
      case "rollback": {
        if (options.failRollback === true) {
          return { ok: false, code: "CONFLICT", message: "no" } as StructuredChatResult<void>;
        }
        const turns = (request.body as { turns: number }).turns;
        const groups = new Set(parts.filter((part) => part.type === "userMessage").map((part) => part.turnId));
        const drop = new Set([...groups].slice(-turns));
        parts = parts.filter((part) => !drop.has(part.turnId));
        return { ok: true, data: undefined } as StructuredChatResult<void>;
      }
      case "approve":
        // The store rewrites the part in place at the seq it already has, which is why an
        // `after` poll cannot carry the answer and the hook must re-read the whole history.
        parts = parts.map((part) =>
          part.type === "approvalRequest" && part.requestId === (request.body as { requestId: string }).requestId
            ? ({ ...part, answered: true } as StructuredPart)
            : part,
        );
        return { ok: true, data: undefined } as StructuredChatResult<void>;
      case "settings":
        return { ok: true, data: request.body as StructuredSessionSettings } as StructuredChatResult<StructuredSessionSettings>;
      case "interrupt":
      case "compact":
      case "closeSession":
        return { ok: true, data: undefined } as StructuredChatResult<void>;
      case "clear":
        seq += 1;
        parts = [...parts, at(seq, { type: "newSession" })];
        return { ok: true, data: seq } as StructuredChatResult<number>;
      default:
        return { ok: false, code: "INVALID_REQUEST", message: "unknown op" };
    }
  }) as unknown as StructuredChatTransport;

  return {
    transport,
    calls,
    setParts: (next: StructuredPart[]) => {
      parts = next;
    },
    readParts: () => parts,
  };
}

const CAPABILITIES_NO_STEER: StructuredCapabilities = {
  steer: false,
  rollback: true,
  permissionModes: ["supervised"],
  models: [],
  slashCommands: [],
};

describe("useStructuredChat session lifecycle", () => {
  it("creates a session, seeds history and exposes the composer capabilities", async () => {
    const fake = fakeTransport({
      parts: [at(1, { turnId: "t1", type: "userMessage", text: "hello" })],
      settings: { model: "gpt-5" },
    });
    const { result } = renderHook(() =>
      useStructuredChat({
        transport: fake.transport,
        create: { provider: "codex", cwd: "/repo" },
        pollMs: 0,
      }),
    );
    await waitFor(() => expect(result.current.sessionId).toBe("s1"));
    await waitFor(() => expect(result.current.messages).toHaveLength(1));
    expect(result.current.messages[0]).toMatchObject({ role: "user", content: "hello" });
    expect(result.current.settings.model).toBe("gpt-5");
    expect(result.current.composerCapabilities?.models?.choices[0].id).toBe("gpt-5");
    expect(fake.calls.some((call) => call.op === "createSession")).toBe(true);
  });

  it("reports a failed create instead of a silent empty chat", async () => {
    const transport = (async () => ({ ok: false, code: "INVALID_REQUEST", message: "no such cwd" })) as unknown as StructuredChatTransport;
    const { result } = renderHook(() =>
      useStructuredChat({ transport, create: { provider: "codex", cwd: "/nope" }, pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.createError).toBe("no such cwd"));
    expect(result.current.sessionId).toBeNull();
    expect(result.current.messages).toEqual([]);
  });
});

describe("useStructuredChat sending", () => {
  it("sends a turn when idle and reconciles its echo into the confirmed part", async () => {
    const fake = fakeTransport();
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.composerCapabilities).not.toBeNull());
    act(() => result.current.setDraft("hello"));
    await act(async () => {
      expect(result.current.send("hello")).toBe(true);
    });
    await waitFor(() => expect(result.current.messages.map((message) => message.content)).toEqual(["hello"]));
    expect(result.current.messages[0].pending).toBeUndefined();
    expect(result.current.draft).toBe("");
    expect(structuredUserTextCount(result.current.parts, "hello")).toBe(1);
  });

  it("keeps the draft and warns when the send is refused", async () => {
    const fake = fakeTransport({ failTurn: { code: "CONFLICT", message: "TURN_ACTIVE: busy" } });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.composerCapabilities).not.toBeNull());
    await act(async () => {
      expect(result.current.send("hello")).toBe(true);
    });
    await waitFor(() => expect(result.current.composerWarning).toBe(STRUCTURED_CHAT_SEND_FAILED));
    expect(result.current.draft).toBe("hello");
    expect(result.current.messages).toEqual([]);
  });

  it("steers a running turn when the agent advertises it", async () => {
    const fake = fakeTransport();
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.composerCapabilities).not.toBeNull());
    await act(async () => {
      result.current.send("start");
    });
    await waitFor(() => expect(result.current.parts.length).toBeGreaterThan(0));
    // the fake leaves the turn open after the user part, so the lane counts as running
    await waitFor(() => expect(result.current.isRunning).toBe(true));
    await act(async () => {
      result.current.send("nudge");
    });
    await waitFor(() =>
      expect(fake.calls.filter((call) => call.op === "steer").map((call) => call.body)).toEqual([{ text: "nudge" }]),
    );
  });

  it("queues instead of steering when the agent cannot steer, and holds the row after a stop", async () => {
    // t3code composer.md:282 - a stop holds the queue; only Resume or a new send releases it.
    const fake = fakeTransport({ capabilities: CAPABILITIES_NO_STEER });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.composerCapabilities).not.toBeNull());
    await act(async () => {
      result.current.send("start");
    });
    await waitFor(() => expect(result.current.isRunning).toBe(true));
    await act(async () => {
      result.current.send("later");
    });
    expect(result.current.queuedMessages.map((row) => row.text)).toEqual(["later"]);
    expect(fake.calls.some((call) => call.op === "steer")).toBe(false);

    // the running turn ends: a usage part is the terminal marker the store writes
    fake.setParts([...fake.readParts(), at(99, { turnId: "t99", type: "usage", totalTokens: 1, inputTokens: 1, cachedInputTokens: 0, outputTokens: 0 })]);
    await act(async () => {
      await result.current.pollNow();
    });
    await waitFor(() => expect(result.current.queuedMessages).toHaveLength(0));
    await waitFor(() =>
      expect(fake.calls.filter((call) => call.op === "turn").map((call) => call.body)).toEqual([
        { text: "start" },
        { text: "later" },
      ]),
    );
  });

  it("holds the queue after an interrupt until resume, instead of draining it into a turn", async () => {
    const fake = fakeTransport({ capabilities: CAPABILITIES_NO_STEER });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.composerCapabilities).not.toBeNull());
    await act(async () => {
      result.current.send("start");
    });
    await waitFor(() => expect(result.current.isRunning).toBe(true));
    await act(async () => {
      result.current.send("later");
    });
    // The stop lands and leaves the stream idle; the held row must not become the next turn.
    fake.setParts([
      ...fake.readParts(),
      at(50, { turnId: "t9", type: "assistantMessage", text: "stopped" }),
    ]);
    await act(async () => {
      await result.current.interrupt();
    });
    await waitFor(() => expect(result.current.isRunning).toBe(false));
    expect(result.current.queueHeld).toBe(true);
    expect(result.current.queuedMessages.map((row) => row.text)).toEqual(["later"]);
    expect(fake.calls.filter((call) => call.op === "turn").map((call) => call.body)).toEqual([{ text: "start" }]);
    expect(result.current.composerWarningAction?.label).toBe("Resume");

    await act(async () => {
      result.current.composerWarningAction?.onAction();
    });
    await waitFor(() =>
      expect(fake.calls.filter((call) => call.op === "turn").map((call) => call.body)).toEqual([
        { text: "start" },
        { text: "later" },
      ]),
    );
    expect(result.current.queueHeld).toBe(false);
  });

  it("marks a refused queued delivery failed instead of losing it, and retries on demand", async () => {
    let refuse = false;
    const inner = fakeTransport({ capabilities: CAPABILITIES_NO_STEER });
    const transport = (async (request: { op: StructuredChatOp; sessionId?: string; body: unknown }, signal: AbortSignal) => {
      if (request.op === "turn" && refuse) {
        return { ok: false, code: "CONFLICT", message: "TURN_ACTIVE: busy" };
      }
      return inner.transport(request as never, signal);
    }) as unknown as StructuredChatTransport;
    const { result } = renderHook(() => useStructuredChat({ transport, sessionId: "s1", pollMs: 0 }));
    await waitFor(() => expect(result.current.composerCapabilities).not.toBeNull());
    await act(async () => {
      result.current.send("start");
    });
    await waitFor(() => expect(result.current.isRunning).toBe(true));
    await act(async () => {
      result.current.send("later");
    });
    refuse = true;
    inner.setParts([...inner.readParts(), at(60, { turnId: "t1", type: "assistantMessage", text: "done" })]);
    await act(async () => {
      await result.current.pollNow();
    });
    // The row survives the refusal, carries the failure, and says so.
    await waitFor(() => expect(result.current.queuedMessages[0]?.failed).toBe(true));
    expect(result.current.queuedMessages.map((row) => row.text)).toEqual(["later"]);
    expect(result.current.composerWarning).toBe(STRUCTURED_CHAT_QUEUED_FAILED);
    // It is not retried on its own: the text is still there, untouched.
    expect(result.current.draft).toBe("");
    expect(result.current.composerWarningAction?.label).toBe("Retry");

    refuse = false;
    await act(async () => {
      result.current.composerWarningAction?.onAction();
    });
    await waitFor(() => expect(result.current.queuedMessages).toHaveLength(0));
    expect(inner.calls.filter((call) => call.op === "turn").map((call) => (call.body as { text: string }).text)).toEqual([
      "start",
      "later",
    ]);
  });

  it("drops a queued row the user cancels", async () => {
    const fake = fakeTransport({ capabilities: CAPABILITIES_NO_STEER });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.composerCapabilities).not.toBeNull());
    await act(async () => {
      result.current.send("start");
    });
    await waitFor(() => expect(result.current.isRunning).toBe(true));
    await act(async () => {
      result.current.send("later");
    });
    const id = result.current.queuedMessages[0].id;
    act(() => result.current.removeQueued(id));
    expect(result.current.queuedMessages).toEqual([]);
    expect(fake.calls.filter((call) => call.op === "turn")).toHaveLength(1);
  });
});

describe("useStructuredChat approvals, rollback and settings", () => {
  const approvalParts = [
    at(1, { turnId: "t0", type: "userMessage", text: "clean up" }),
    at(2, {
      turnId: "t0",
      type: "approvalRequest",
      requestId: "req-1",
      kind: "command",
      summary: "rm -rf build",
      answered: false,
    }),
  ];

  it("draws a pending approval as a prompt card and answers it by decision", async () => {
    const fake = fakeTransport({ parts: approvalParts });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.prompt?.id).toBe("req-1"));
    expect(result.current.prompt?.question).toBe("rm -rf build");
    await act(async () => {
      expect(await result.current.answerPrompt({ promptId: "req-1", screenRevision: "req-1", answer: { optionIndex: 1 } })).toBe(true);
    });
    expect(fake.calls.filter((call) => call.op === "approve").map((call) => call.body)).toEqual([
      { requestId: "req-1", decision: "acceptForSession" },
    ]);
    // The answer mutates the part in place, so only a whole-history read can show it answered.
    expect(fake.calls.filter((call) => call.op === "history").length).toBeGreaterThanOrEqual(2);
    await waitFor(() => expect(result.current.prompt).toBeNull());
  });

  it("refuses an option the card did not offer instead of guessing a decision", async () => {
    const fake = fakeTransport({ parts: approvalParts });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.prompt?.id).toBe("req-1"));
    await act(async () => {
      expect(await result.current.answerPrompt({ promptId: "req-1", screenRevision: "req-1", answer: { optionIndex: 7 } })).toBe(false);
    });
    expect(fake.calls.some((call) => call.op === "approve")).toBe(false);
  });

  it("rolls back whole turns and returns the prompt text it took back", async () => {
    const fake = fakeTransport({
      parts: [
        at(1, { turnId: "t1", type: "userMessage", text: "first" }),
        at(2, { turnId: "t1", type: "assistantMessage", text: "a" }),
        at(3, { turnId: "t2", type: "userMessage", text: "second" }),
        at(4, { turnId: "t2", type: "assistantMessage", text: "b" }),
      ],
    });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.parts).toHaveLength(4));
    let removed: readonly string[] = [];
    await act(async () => {
      removed = await result.current.rollback(1);
    });
    expect(removed).toEqual(["second"]);
    await waitFor(() =>
      expect(result.current.parts.map((part) => (part.type === "assistantMessage" ? part.text : part.type))).toEqual([
        "userMessage",
        "a",
      ]),
    );
  });

  it("restores the prompt text for an edit-from-here and leaves the stream alone when it fails", async () => {
    const parts = [
      at(1, { turnId: "t1", type: "userMessage", text: "first" }),
      at(2, { turnId: "t1", type: "assistantMessage", text: "a" }),
      at(3, { turnId: "t2", type: "userMessage", text: "second" }),
    ];
    const fake = fakeTransport({ parts });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.parts).toHaveLength(3));
    await act(async () => {
      expect(await result.current.editFrom(3)).toBe("second");
    });
    expect(result.current.draft).toBe("second");

    const failing = fakeTransport({ parts, failRollback: true });
    const second = renderHook(() =>
      useStructuredChat({ transport: failing.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(second.result.current.parts).toHaveLength(3));
    await act(async () => {
      expect(await second.result.current.editFrom(3)).toBeNull();
    });
    expect(second.result.current.draft).toBe("");
  });

  it("applies a composer control as session settings and says what happened", async () => {
    const fake = fakeTransport();
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.composerCapabilities).not.toBeNull());
    await act(async () => {
      await result.current.applyControl({ controlId: "model", valueId: "gpt-5", label: "GPT-5", apply: "command" });
    });
    expect(fake.calls.filter((call) => call.op === "settings").map((call) => call.body)).toEqual([{ model: "gpt-5" }]);
    expect(result.current.settings.model).toBe("gpt-5");
    expect(result.current.controlNotice?.status).toBe("applied");
  });

  it("reports file reverting as unsupported with one stable notice", async () => {
    const fake = fakeTransport();
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.capabilities).not.toBeNull());
    expect(result.current.revertFilesSupported).toBe(false);
    expect(result.current.revertFilesNotice).toBe(STRUCTURED_CHAT_REVERT_FILES_UNSUPPORTED);
  });
});

describe("useStructuredChat resume and cursors", () => {  it("sends a turn rather than steering when the summary says the session ended", async () => {
    // The stream's last part is still an unfinished tool call from the turn that died with the
    // provider, so only the summary can say there is no turn left to steer.
    const fake = fakeTransport({
      state: "ended",
      parts: [
        at(1, { turnId: "t1", type: "userMessage", text: "go" }),
        at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" }),
      ],
    });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.session?.state).toBe("ended"));
    expect(result.current.isRunning).toBe(false);
    expect(result.current.activityState).toBe("idle");
    expect(result.current.sessionEndedNotice).toBe(STRUCTURED_CHAT_SESSION_ENDED);
    await act(async () => {
      expect(result.current.send("again")).toBe(true);
    });
    await waitFor(() => expect(fake.calls.some((call) => call.op === "turn")).toBe(true));
    expect(fake.calls.some((call) => call.op === "steer")).toBe(false);
  });

  it("lowers the cursor when a history read comes back empty, so reused seqs are not skipped", async () => {
    const fake = fakeTransport({
      parts: [
        at(1, { turnId: "t1", type: "userMessage", text: "go" }),
        at(2, { turnId: "t1", type: "assistantMessage", text: "ok" }),
      ],
    });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.parts).toHaveLength(2));
    // A rollback empties the journal; the store reuses seqs from 1 on the next turn.
    fake.setParts([]);
    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.parts).toHaveLength(0);
    fake.setParts([at(1, { turnId: "t2", type: "userMessage", text: "fresh" })]);
    await act(async () => {
      await result.current.pollNow();
    });
    expect(result.current.parts.map((part) => (part.type === "userMessage" ? part.text : part.type))).toEqual(["fresh"]);
    expect(fake.calls.filter((call) => call.op === "events").at(-1)?.body).toEqual({ after: 0 });
  });
});

describe("useStructuredChat lane commands", () => {
  it("runs /clear as the lane's own clear, not as text sent to the agent", async () => {
    const fake = fakeTransport({
      parts: [
        at(1, { turnId: "t1", type: "userMessage", text: "go" }),
        at(2, { turnId: "t1", type: "assistantMessage", text: "done" }),
      ],
    });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.parts).toHaveLength(2));
    await act(async () => {
      expect(result.current.send("/clear")).toBe(true);
    });
    await waitFor(() => expect(fake.calls.some((call) => call.op === "clear")).toBe(true));
    expect(fake.calls.some((call) => call.op === "turn")).toBe(false);
    // the store appends a `newSession` part, which the lane draws as the divider
    await waitFor(() => expect(result.current.parts.some((part) => part.type === "newSession")).toBe(true));
    expect(result.current.draft).toBe("");
  });

  it("moves a command to the front of the queue when it is steered into a running turn", async () => {
    const fake = fakeTransport({
      parts: [at(1, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" })],
    });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.isRunning).toBe(true));
    await act(async () => {
      expect(await result.current.steer("/clear")).toBe(true);
    });
    expect(fake.calls.some((call) => call.op === "steer")).toBe(false);
    expect(result.current.queuedMessages.map((row) => row.text)).toEqual(["/clear"]);
  });

  it("keeps both rows visible when the same text is sent twice and only one is confirmed", async () => {
    // Both echoes are stamped with the same confirmed seqs, so a count-based retire drops both when
    // the first confirmed part lands. Each echo must be retired by at most one confirmed part.
    let hold = true;
    const inner = fakeTransport();
    const transport = (async (request: { op: StructuredChatOp; sessionId?: string; body: unknown }, signal: AbortSignal) => {
      if (request.op === "turn" && hold) return { ok: true, data: 1 };
      return inner.transport(request as never, signal);
    }) as unknown as StructuredChatTransport;
    const { result } = renderHook(() => useStructuredChat({ transport, sessionId: "s1", pollMs: 0 }));
    await waitFor(() => expect(result.current.composerCapabilities).not.toBeNull());
    await act(async () => {
      result.current.send("hello");
      result.current.send("hello");
    });
    expect(result.current.messages.filter((message) => message.content === "hello")).toHaveLength(2);
    // Only the first send's part has landed; one echo retires and the other stays.
    inner.setParts([at(1, { turnId: "t1", type: "userMessage", text: "hello" })]);
    await act(async () => {
      await result.current.pollNow();
    });
    expect(result.current.messages.filter((message) => message.content === "hello")).toHaveLength(2);
    expect(result.current.messages.filter((message) => message.content === "hello" && message.pending)).toHaveLength(1);
    // The second part retires the other echo: both sends confirmed, nothing left pending.
    hold = false;
    inner.setParts([
      at(1, { turnId: "t1", type: "userMessage", text: "hello" }),
      at(2, { turnId: "t2", type: "userMessage", text: "hello" }),
    ]);
    await act(async () => {
      await result.current.pollNow();
    });
    await waitFor(() => expect(result.current.messages.filter((message) => message.pending)).toEqual([]));
    // Two confirmed user parts, drawn as the one run they are: no duplicate, no lost row.
    expect(structuredUserTextCount(result.current.parts, "hello")).toBe(2);
    expect(result.current.messages.filter((message) => message.role === "user").map((message) => message.content)).toEqual([
      "hello\n\nhello",
    ]);
  });

  it("treats an unshaped commands reply as no commands instead of throwing on render", async () => {
    // The transport is a wire boundary: a gateway that answers `commands` without the list (or
    // with a shape this client does not know) must cost the chip menu, not the pane. A cast at
    // this seam once let `undefined` reach `structuredChipSources` and take the render down.
    const inner = fakeTransport({
      capabilities: { ...CAPABILITIES_NO_STEER, slashCommands: ["/context"] },
    });
    const transport = (async (request: { op: StructuredChatOp; sessionId?: string; body: unknown }, signal: AbortSignal) => {
      if (request.op === "commands") return { ok: true, data: undefined };
      return inner.transport(request as never, signal);
    }) as unknown as StructuredChatTransport;

    const { result } = renderHook(() => useStructuredChat({ transport, sessionId: "s1", pollMs: 0 }));
    await waitFor(() => expect(result.current.capabilities).not.toBeNull());
    await act(async () => {
      expect(await result.current.fetchCommands()).toEqual([]);
    });
    // The lane keeps its own two commands and draws no skills from the unreadable reply.
    expect(result.current.commands).toEqual([]);
    expect(result.current.chipSources?.skills ?? []).toEqual([]);
  });

  it("queues an advertised slash command instead of steering it into the running turn", async () => {
    // composer.md:72-74 - a slash command such as `/context` waits for the current turn to
    // finish; only the lane's own two commands were recognised before.
    const fake = fakeTransport({
      capabilities: { ...CAPABILITIES_NO_STEER, steer: true, slashCommands: ["/context", "/compact"] },
      parts: [
        at(1, { turnId: "t1", type: "userMessage", text: "go" }),
        at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" }),
      ],
    });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.isRunning).toBe(true));
    await act(async () => {
      expect(result.current.send("/context")).toBe(true);
    });
    expect(fake.calls.some((call) => call.op === "steer")).toBe(false);
    expect(result.current.queuedMessages.map((row) => row.text)).toEqual(["/context"]);
  });

  it("sends prose that merely starts with a slash as ordinary text", async () => {
    const fake = fakeTransport({
      capabilities: { ...CAPABILITIES_NO_STEER, steer: true, slashCommands: ["/context"] },
      parts: [
        at(1, { turnId: "t1", type: "userMessage", text: "go" }),
        at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" }),
      ],
    });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.isRunning).toBe(true));
    await act(async () => {
      result.current.send("/contextual help");
    });
    await waitFor(() =>
      expect(fake.calls.filter((call) => call.op === "steer").map((call) => call.body)).toEqual([
        { text: "/contextual help" },
      ]),
    );
  });

  it("holds a command behind a running turn and runs it after, in front of the queue", async () => {
    // t3code: a command steered into a turn would replace the agent's task, so it queues.
    const fake = fakeTransport({
      capabilities: CAPABILITIES_NO_STEER,
      parts: [
        at(1, { turnId: "t1", type: "userMessage", text: "go" }),
        at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" }),
      ],
    });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.isRunning).toBe(true));
    await act(async () => {
      expect(result.current.send("later")).toBe(true);
      expect(result.current.send("/compact")).toBe(true);
    });
    expect(result.current.queuedMessages.map((row) => row.text)).toEqual(["/compact", "later"]);
    expect(fake.calls.some((call) => call.op === "compact")).toBe(false);

    fake.setParts([...fake.readParts(), at(3, { turnId: "t1", type: "assistantMessage", text: "done" })]);
    await act(async () => {
      await result.current.pollNow();
    });
    await waitFor(() => expect(fake.calls.some((call) => call.op === "compact")).toBe(true));
    // The command is a lane operation, so the turn that follows it carries the prose only.
    await waitFor(() => expect(fake.calls.some((call) => call.op === "turn")).toBe(true));
    const turns = fake.calls.filter((call) => call.op === "turn").map((call) => (call.body as { text: string }).text);
    expect(turns).toEqual(["later"]);
  });
});

/**
 * Gap lane chat-gaps-20261126: the lane's verbs that had no caller and no test - a bounded
 * `history` window with an older cursor, the summary's `activeTurnId`, editing a queued row in
 * place, and a rewind that asks the provider to restore files too.
 */
describe("useStructuredChat gap lane", () => {
  /** A transport over one journal that serves a bounded newest window and names the active turn. */
  function pagingTransport(options: {
    readonly parts: readonly StructuredPart[];
    /** How many parts the newest `history` window carries; the whole journal by default. */
    readonly windowSize?: number;
    readonly activeTurnId?: string | null;
  }) {
    const calls: FakeCalls[] = [];
    const parts = [...options.parts];
    const summary: StructuredSessionSummary = {
      sessionId: "s1",
      provider: "codex",
      cwd: "/repo",
      state: "running",
      settings: {},
      ...(options.activeTurnId === undefined ? {} : { activeTurnId: options.activeTurnId }),
    };
    const transport = (async (request: { op: StructuredChatOp; body: unknown }) => {
      calls.push({ op: request.op, body: request.body });
      switch (request.op) {
        case "listSessions":
          return { ok: true, data: [summary] };
        case "capabilities":
          return { ok: true, data: { ...CAPABILITIES_NO_STEER, slashCommands: [] } };
        case "history": {
          const before = (request.body as { before?: number }).before;
          const limit = options.windowSize ?? parts.length;
          if (before !== undefined) {
            const below = parts.filter((part) => part.seq < before);
            return { ok: true, data: { parts: below, nextSeq: before } };
          }
          // The store serves the newest `limit` parts of the window; `hasOlder` says whether the
          // journal holds anything under it. `nextBefore` is the oldest seq of the page.
          const window = parts.slice(Math.max(0, parts.length - limit));
          const oldest = window.length === 0 ? undefined : window[0].seq;
          const holdsOlder = oldest !== undefined && parts.some((part) => part.seq < oldest);
          return {
            ok: true,
            data: {
              parts: window,
              nextSeq: window.reduce((highest, part) => Math.max(highest, part.seq), 0),
              ...(holdsOlder ? { hasOlder: true, nextBefore: oldest } : {}),
            },
          };
        }
        case "events":
          return { ok: true, data: { parts: [], nextSeq: 0 } };
        default:
          return { ok: true, data: undefined };
      }
    }) as unknown as StructuredChatTransport;
    return { transport, calls };
  }

  it("walks one older page at a time and stops when the window has no more", async () => {
    const fake = pagingTransport({
      parts: [
        at(1, { turnId: "t1", type: "userMessage", text: "OLDEST" }),
        at(2, { turnId: "t1", type: "assistantMessage", text: "older reply" }),
        at(3, { turnId: "t2", type: "userMessage", text: "NEWEST" }),
      ],
      windowSize: 1,
    });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );

    await waitFor(() => expect(result.current.hasOlderPage).toBe(true));
    expect(result.current.messages.map((message) => message.content)).toEqual(["NEWEST"]);
    expect(result.current.loadedOlder).toBe(false);

    await act(async () => {
      result.current.loadOlder();
    });

    // The page below the cursor arrives, and the read said it was the first.
    await waitFor(() => expect(result.current.messages).toHaveLength(3));
    expect(fake.calls.some((call) => call.op === "history" && (call.body as { before?: number }).before === 3)).toBe(
      true,
    );
    await waitFor(() => expect(result.current.hasOlderPage).toBe(false));
    expect(result.current.loadedOlder).toBe(true);
    expect(result.current.olderState).toBe("idle");
  });

  it("settles the older control instead of spinning when the window has no cursor", async () => {
    const fake = pagingTransport({
      parts: [at(1, { turnId: "t1", type: "userMessage", text: "only" })],
    });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.hasOlderPage).toBe(false));
    await act(async () => {
      result.current.loadOlder();
    });
    expect(result.current.loadedOlder).toBe(true);
    expect(result.current.olderState).toBe("idle");
    expect(fake.calls.some((call) => call.op === "history" && (call.body as { before?: number }).before !== undefined)).toBe(
      false,
    );
  });

  it("treats a summary activeTurnId as a running turn, and an absent one as unknown", async () => {
    // composer.md: the composer shows Stop while the agent works. The gateway naming the turn is
    // the earliest evidence; a stream that has not caught up must not read as idle.
    const busy = pagingTransport({
      parts: [at(1, { turnId: "t1", type: "userMessage", text: "go" })],
      activeTurnId: "t1",
    });
    const busyLane = renderHook(() =>
      useStructuredChat({ transport: busy.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(busyLane.result.current.isRunning).toBe(true));
    busyLane.unmount();

    // No activeTurnId and a settled tail: the lane's own journal decides, which is idle here.
    const idle = pagingTransport({
      parts: [
        at(1, { turnId: "t1", type: "userMessage", text: "go" }),
        at(2, { turnId: "t1", type: "assistantMessage", text: "done" }),
      ],
    });
    const idleLane = renderHook(() =>
      useStructuredChat({ transport: idle.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(idleLane.result.current.messages).toHaveLength(2));
    expect(idleLane.result.current.isRunning).toBe(false);
  });

  it("saves a queued row's new text in place, keeping its position", async () => {
    const fake = fakeTransport({
      capabilities: CAPABILITIES_NO_STEER,
      parts: [
        at(1, { turnId: "t1", type: "userMessage", text: "go" }),
        at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" }),
      ],
    });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.isRunning).toBe(true));
    await act(async () => {
      result.current.send("first queued");
      result.current.send("second queued");
    });
    const [first] = result.current.queuedMessages;

    await act(async () => {
      result.current.editQueued(first.id, "edited queued");
    });
    expect(result.current.queuedMessages.map((row) => row.text)).toEqual(["edited queued", "second queued"]);
    expect(result.current.queuedMessages[0].id).toBe(first.id);
  });

  it("drops a queued row when its edited text is emptied", async () => {
    const fake = fakeTransport({
      capabilities: CAPABILITIES_NO_STEER,
      parts: [
        at(1, { turnId: "t1", type: "userMessage", text: "go" }),
        at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" }),
      ],
    });
    const { result } = renderHook(() =>
      useStructuredChat({ transport: fake.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(result.current.isRunning).toBe(true));
    await act(async () => {
      result.current.send("only queued");
    });
    await act(async () => {
      result.current.editQueued(result.current.queuedMessages[0].id, "");
    });
    expect(result.current.queuedMessages).toEqual([]);
  });

  it("sends revertFiles only when the agent advertises it", async () => {
    // composer.md:191-194: "Revert files too" is a distinct choice, so a lane that cannot restore
    // files must not put the bit on the wire as if it could.
    const unsupported = fakeTransport({
      parts: [at(1, { turnId: "t1", type: "userMessage", text: "go" })],
    });
    const plain = renderHook(() =>
      useStructuredChat({ transport: unsupported.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(plain.result.current.turnCount).toBe(1));
    await act(async () => {
      await plain.result.current.rollbackTurns(1, true);
    });
    expect(
      unsupported.calls.filter((call) => call.op === "rollback").map((call) => call.body),
    ).toEqual([{ turns: 1 }]);
    plain.unmount();

    const supported = fakeTransport({
      parts: [at(1, { turnId: "t1", type: "userMessage", text: "go" })],
      capabilities: { ...CAPABILITIES_NO_STEER, revertFiles: true },
    });
    const reverting = renderHook(() =>
      useStructuredChat({ transport: supported.transport, sessionId: "s1", pollMs: 0 }),
    );
    await waitFor(() => expect(reverting.result.current.revertFilesSupported).toBe(true));
    await act(async () => {
      await reverting.result.current.rollbackTurns(1, true);
    });
    expect(
      supported.calls.filter((call) => call.op === "rollback").map((call) => call.body),
    ).toEqual([{ turns: 1, revertFiles: true }]);
  });

  it("bounds media cache by LRU and revokes any evicted objectUrl", () => {
    const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
    try {
      const blobWithUrl = {
        mimeType: "image/png",
        dataBase64: "AA==",
        objectUrl: "blob:evicted-1",
      } as unknown as { readonly mimeType: string; readonly dataBase64: string };
      let cache = structuredMediaCachePut({}, "k1", blobWithUrl, 2);
      cache = structuredMediaCachePut(cache, "k2", { mimeType: "image/png", dataBase64: "BB==" }, 2);
      cache = structuredMediaCachePut(cache, "k3", { mimeType: "image/png", dataBase64: "CC==" }, 2);
      expect(Object.keys(cache)).toEqual(["k2", "k3"]);
      expect(revoke).toHaveBeenCalledWith("blob:evicted-1");
    } finally {
      revoke.mockRestore();
    }
  });

  it("skips history and events polling while visible is false", async () => {
    const fake = fakeTransport({
      parts: [at(1, { turnId: "t1", type: "userMessage", text: "hidden" })],
    });
    const { rerender } = renderHook(
      ({ visible }: { visible: boolean }) =>
        useStructuredChat({ transport: fake.transport, sessionId: "s1", visible, pollMs: 500 }),
      { initialProps: { visible: false } },
    );
    await waitFor(() => expect(fake.calls.some((call) => call.op === "capabilities")).toBe(true));
    expect(fake.calls.filter((call) => call.op === "history" || call.op === "events")).toHaveLength(0);

    rerender({ visible: true });
    await waitFor(() => expect(fake.calls.some((call) => call.op === "history")).toBe(true));
  });
});
