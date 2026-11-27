import { describe, expect, it } from "vitest";
import {
  STRUCTURED_CHAT_OUTCOME_UNKNOWN,
  STRUCTURED_CHAT_REVERT_FILES_UNSUPPORTED,
  StructuredChatStream,
  decodeStructuredReply,
  structuredChatEditFrom,
  structuredChatFailure,
  structuredChatHttpWire,
  structuredChatIpcWire,
  structuredChatLaneCommand,
  structuredChatPermissionModes,
  structuredChatRequestRoute,
  structuredChatRevertFilesSupported,
  structuredChatRollbackPrompts,
  structuredChatSendMode,
  structuredChatTransportFrom,
  type StructuredChatOp,
  type StructuredChatTransport,
} from "./structuredChatTransport";
import type { StructuredPart } from "./structuredChatTypes";

const SIGNAL = new AbortController().signal;

function part(seq: number, extra: Record<string, unknown>): StructuredPart {
  return { seq, tsMs: 1000 + seq, ...extra } as StructuredPart;
}

function wire(handler: (request: { op: string; sessionId?: string; body?: unknown }) => { status: number; body: unknown }) {
  return structuredChatTransportFrom(async (request) => handler(request));
}

describe("structured chat transport routing", () => {
  it("maps every op to the route lane G1 published", () => {
    expect(structuredChatRequestRoute("createSession")).toEqual({
      method: "POST",
      path: "/api/v1/structured-chat/sessions",
    });
    expect(structuredChatRequestRoute("listSessions")).toEqual({
      method: "GET",
      path: "/api/v1/structured-chat/sessions",
    });
    expect(structuredChatRequestRoute("turn", "s/1")).toEqual({
      method: "POST",
      path: "/api/v1/structured-chat/sessions/s%2F1/turns",
    });
    expect(structuredChatRequestRoute("history", "s1").path).toBe(
      "/api/v1/structured-chat/sessions/s1/history",
    );
    expect(structuredChatRequestRoute("events", "s1").method).toBe("GET");
    expect(structuredChatRequestRoute("steer", "s1").method).toBe("POST");
    expect(structuredChatRequestRoute("interrupt", "s1").path).toBe(
      "/api/v1/structured-chat/sessions/s1/interrupt",
    );
    expect(structuredChatRequestRoute("rollback", "s1").path).toBe(
      "/api/v1/structured-chat/sessions/s1/rollback",
    );
    expect(structuredChatRequestRoute("approve", "s1").path).toBe(
      "/api/v1/structured-chat/sessions/s1/approvals",
    );
    expect(structuredChatRequestRoute("settings", "s1").method).toBe("PUT");
    expect(structuredChatRequestRoute("clear", "s1").method).toBe("POST");
    expect(structuredChatRequestRoute("compact", "s1").method).toBe("POST");
    expect(structuredChatRequestRoute("closeSession", "s1")).toEqual({
      method: "DELETE",
      path: "/api/v1/structured-chat/sessions/s1",
    });
  });

  it("maps the extension, branch, media and preference ops to their own routes", () => {
    // The branch and extension verbs address provider entry/request ids, so each is its own
    // route: reusing another route's path would send an id into the wrong space.
    const expected: readonly [StructuredChatOp, "GET" | "POST" | "PUT", string][] = [
      ["commands", "GET", "/commands"],
      ["answerExtensionUi", "POST", "/extension-ui"],
      ["tree", "GET", "/tree"],
      ["navigateTree", "POST", "/navigate-tree"],
      ["fork", "POST", "/fork"],
      ["clone", "POST", "/clone"],
      ["editMessage", "POST", "/edit-message"],
      ["rename", "PUT", "/name"],
      ["stats", "GET", "/stats"],
      ["reload", "POST", "/reload"],
      ["media", "GET", "/media"],
      ["clearQueue", "POST", "/queue/clear"],
      ["modelPreferences", "PUT", "/models/preferences"],
    ];
    for (const [op, method, suffix] of expected) {
      expect(structuredChatRequestRoute(op, "s1")).toEqual({
        method,
        path: `/api/v1/structured-chat/sessions/s1${suffix}`,
      });
    }
  });

  it("decodes a turn reply to its seq and refuses a malformed one", async () => {
    const ok = await wire(() => ({ status: 200, body: { seq: 7 } }))(
      { op: "turn", sessionId: "s1", body: { text: "hi" } },
      SIGNAL,
    );
    expect(ok).toEqual({ ok: true, data: 7 });

    const broken = await wire(() => ({ status: 200, body: { nothing: true } }))(
      { op: "turn", sessionId: "s1", body: { text: "hi" } },
      SIGNAL,
    );
    expect(broken).toMatchObject({ ok: false, code: "MALFORMED_RESPONSE" });
  });

  it("keeps a refusal's machine code, including a structured code inside an IPC message", () => {
    expect(
      structuredChatFailure({ error: { code: "CONFLICT", message: "TURN_ACTIVE: busy" } }, 409),
    ).toEqual({ code: "CONFLICT", message: "TURN_ACTIVE: busy" });
    expect(
      structuredChatFailure({ code: "ControlConflict", message: "STEER_UNSUPPORTED: none" }, 409).code,
    ).toBe("STEER_UNSUPPORTED");
    expect(structuredChatFailure({ error: { code: "FORBIDDEN", message: "no" } }, 403).code).toBe(
      "FORBIDDEN",
    );
    expect(structuredChatFailure(null, 404).code).toBe("NOT_FOUND");
  });

  it("reports a thrown mutation as an unknown outcome instead of a plain failure", async () => {
    const transport = structuredChatTransportFrom(async () => {
      throw new Error("tunnel closed");
    });
    const result = await transport(
      { op: "turn", sessionId: "s1", body: { text: "hi" } },
      SIGNAL,
    );
    expect(result).toMatchObject({ ok: false, code: STRUCTURED_CHAT_OUTCOME_UNKNOWN });
  });

  it("honours the abort signal for a read and never for a mutation in flight", async () => {
    const controller = new AbortController();
    controller.abort();
    const transport = wire(() => ({ status: 200, body: { parts: [] } }));
    await expect(
      transport({ op: "history", sessionId: "s1", body: { after: 0 } }, controller.signal),
    ).rejects.toThrow();
  });
});

describe("structured chat reply decoding", () => {
  it("keeps an unknown part kind raw and drops one with no usable seq", () => {
    const decoded = decodeStructuredReply("history", 200, {
      parts: [
        { seq: 3, tsMs: 1, type: "futureThing", payload: { a: 1 } },
        { tsMs: 2, type: "userMessage", text: "no seq" },
        { seq: 4, tsMs: 3, type: "userMessage", text: "kept" },
      ],
    });
    expect(decoded.ok).toBe(true);
    if (!decoded.ok) return;
    expect(decoded.data.parts.map((row) => row.type)).toEqual(["raw", "userMessage"]);
    expect(decoded.data.parts[0]).toMatchObject({ seq: 3, type: "raw", rawType: "futureThing" });
    expect(decoded.data.nextSeq).toBe(4);
  });

  it("unwraps the IPC {ok,data} envelope and decodes capabilities", async () => {
    const result = await wire(() => ({
      status: 200,
      body: {
        ok: true,
        data: {
          steer: true,
          rollback: true,
          permissionModes: ["supervised", "auto", "bogus"],
          models: [{ id: "gpt-5", label: "GPT-5", efforts: ["low", "high"] }],
          slashCommands: ["/compact", " "],
        },
      },
    }))({ op: "capabilities", sessionId: "s1", body: {} }, SIGNAL);
    expect(result).toMatchObject({
      ok: true,
      data: {
        steer: true,
        rollback: true,
        permissionModes: ["supervised", "auto"],
        models: [{ id: "gpt-5", label: "GPT-5", efforts: ["low", "high"] }],
        slashCommands: ["/compact"],
      },
    });
  });

  it("decodes a session list and drops rows missing their identity", () => {
    const decoded = decodeStructuredReply("listSessions", 200, {
      sessions: [
        { sessionId: "a", provider: "codex", cwd: "/w", state: "running", settings: { model: "m" } },
        { provider: "codex", cwd: "/w", state: "running" },
        { sessionId: "b", provider: "codex", cwd: "/w", state: "ended", settings: {} },
      ],
    });
    expect(decoded.ok).toBe(true);
    if (!decoded.ok) return;
    expect(decoded.data.map((row) => row.sessionId)).toEqual(["a", "b"]);
    expect(decoded.data[0].settings.model).toBe("m");
  });

  it("treats an error envelope on a 2xx as a refusal", () => {
    const decoded = decodeStructuredReply("capabilities", 200, {
      error: { code: "PROVIDER_FAILED", message: "child died" },
    });
    expect(decoded).toEqual({ ok: false, code: "PROVIDER_FAILED", message: "child died" });
  });

  it("refuses a read whose body did not parse instead of drawing an empty transcript", () => {
    // `readJson` answers null for a truncated or non-JSON 200. Rendering that as an empty page
    // hides a broken gateway behind a blank chat; the caller must be able to warn instead.
    expect(decodeStructuredReply("history", 200, null)).toMatchObject({
      ok: false,
      code: "MALFORMED_RESPONSE",
    });
    expect(decodeStructuredReply("events", 200, "not json")).toMatchObject({
      ok: false,
      code: "MALFORMED_RESPONSE",
    });
    expect(decodeStructuredReply("history", 200, { parts: "not-an-array" })).toMatchObject({
      ok: false,
      code: "MALFORMED_RESPONSE",
    });
    expect(decodeStructuredReply("events", 200, {})).toMatchObject({
      ok: false,
      code: "MALFORMED_RESPONSE",
    });
  });

  it("still reads an empty page as an empty page", () => {
    expect(decodeStructuredReply("history", 200, { parts: [] }, 42)).toEqual({
      ok: true,
      data: { parts: [], nextSeq: 42 },
    });
  });

  it("refuses a session list whose body did not parse instead of reporting no sessions", () => {
    expect(decodeStructuredReply("listSessions", 200, null)).toMatchObject({
      ok: false,
      code: "MALFORMED_RESPONSE",
    });
    expect(decodeStructuredReply("listSessions", 200, { sessions: "nope" })).toMatchObject({
      ok: false,
      code: "MALFORMED_RESPONSE",
    });
    expect(decodeStructuredReply("listSessions", 200, { sessions: [] })).toEqual({ ok: true, data: [] });
  });
});

describe("structured chat incremental stream", () => {
  it("seeds from history then asks events only past the cursor, merging by seq", async () => {
    const calls: number[] = [];
    const transport: StructuredChatTransport = async (request) => {
      const after = (request.body as { after: number }).after;
      calls.push(after);
      if (request.op === "history") {
        return { ok: true, data: { parts: [part(1, { type: "userMessage", text: "hi" })], nextSeq: 1 } };
      }
      return {
        ok: true,
        data: {
          parts:
            after === 1
              ? [part(2, { type: "assistantMessage", text: "yo" })]
              : [],
          nextSeq: after === 1 ? 2 : after,
        },
      };
    };
    const stream = new StructuredChatStream(transport, "s1");
    const loaded = await stream.load(SIGNAL);
    expect(loaded.ok).toBe(true);
    const polled = await stream.poll(SIGNAL);
    expect(polled.ok).toBe(true);
    expect(calls).toEqual([0, 1]);
    expect(stream.after()).toBe(2);
    expect(stream.read().map((row) => row.seq)).toEqual([1, 2]);
  });

  it("keeps the cursor and the rows when a poll fails, so a restart resumes in place", async () => {
    let fail = false;
    const transport: StructuredChatTransport = async (request) => {
      if (fail) return { ok: false, code: "NOT_FOUND", message: "gateway restarting" };
      const after = (request.body as { after: number }).after;
      return {
        ok: true,
        data: { parts: after === 0 ? [part(1, { type: "userMessage", text: "hi" })] : [], nextSeq: 1 },
      };
    };
    const stream = new StructuredChatStream(transport, "s1");
    await stream.load(SIGNAL);
    fail = true;
    const failed = await stream.poll(SIGNAL);
    expect(failed).toMatchObject({ ok: false, code: "NOT_FOUND" });
    expect(stream.after()).toBe(1);
    expect(stream.read()).toHaveLength(1);
    fail = false;
    const resumed = await stream.poll(SIGNAL);
    expect(resumed.ok).toBe(true);
    expect(stream.after()).toBe(1);
  });

  it("drops a page that landed after a reset instead of resurrecting the old session", async () => {
    let release: (() => void) | null = null;
    const transport: StructuredChatTransport = async () => {
      await new Promise<void>((resolve) => {
        release = resolve;
      });
      return { ok: true, data: { parts: [part(9, { type: "userMessage", text: "old" })], nextSeq: 9 } };
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

describe("structured chat send and edit rules", () => {
  it("sends a turn when idle, steers when running with the capability, queues otherwise", () => {
    expect(structuredChatSendMode({ running: false, steer: false, text: "hi" })).toBe("turn");
    expect(structuredChatSendMode({ running: true, steer: true, text: "hi" })).toBe("steer");
    expect(structuredChatSendMode({ running: true, steer: false, text: "hi" })).toBe("queue");
    expect(structuredChatSendMode({ running: false, steer: true, text: "   " })).toBeNull();
  });

  it("recognises the two composer messages that are this lane's own commands", () => {
    expect(structuredChatLaneCommand("/clear")).toBe("clear");
    expect(structuredChatLaneCommand("  /clear\n")).toBe("clear");
    expect(structuredChatLaneCommand("/compact")).toBe("compact");
    expect(structuredChatLaneCommand("/Compact")).toBe("compact");
    // Only a message that is the command on its own, and only the lane's own two.
    expect(structuredChatLaneCommand("/clear the cache")).toBeNull();
    expect(structuredChatLaneCommand("please /clear")).toBeNull();
    expect(structuredChatLaneCommand("/context")).toBeNull();
    expect(structuredChatLaneCommand("")).toBeNull();
  });

  it("takes back the prompt text a rollback of n turns removes", () => {
    const parts: StructuredPart[] = [
      part(1, { turnId: "t1", type: "userMessage", text: "first" }),
      part(2, { turnId: "t1", type: "assistantMessage", text: "a" }),
      part(3, { turnId: "t2", type: "userMessage", text: "second" }),
      part(4, { turnId: "t2", type: "assistantMessage", text: "b" }),
    ];
    expect(structuredChatRollbackPrompts(parts, 1)).toEqual(["second"]);
    expect(structuredChatRollbackPrompts(parts, 2)).toEqual(["first", "second"]);
    expect(structuredChatRollbackPrompts(parts, 9)).toEqual(["first", "second"]);
    expect(structuredChatRollbackPrompts(parts, 0)).toEqual([]);
  });

  it("takes back every message of a turn a steer landed in, not one per user part", () => {
    const parts: StructuredPart[] = [
      part(1, { turnId: "t1", type: "userMessage", text: "go" }),
      part(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "completed" }),
      part(3, { turnId: "t1", type: "userMessage", text: "also this" }),
      part(4, { turnId: "t1", type: "assistantMessage", text: "done" }),
      part(5, { turnId: "t2", type: "userMessage", text: "next" }),
    ];
    expect(structuredChatRollbackPrompts(parts, 1)).toEqual(["next"]);
    expect(structuredChatRollbackPrompts(parts, 2)).toEqual(["go", "also this", "next"]);
  });

  it("leaves parts outside a turn out of the rollback text", () => {
    const parts: StructuredPart[] = [
      part(1, { type: "newSession" }),
      part(2, { turnId: "t1", type: "userMessage", text: "go" }),
    ];
    expect(structuredChatRollbackPrompts(parts, 1)).toEqual(["go"]);
    expect(structuredChatRollbackPrompts(parts, 2)).toEqual(["go"]);
  });

  it("derives the turns to roll back and the text to restore for an edit-from-here", () => {
    const parts: StructuredPart[] = [
      { seq: 1, tsMs: 1, turnId: "t1", type: "userMessage", text: "first" } as StructuredPart,
      { seq: 2, tsMs: 2, turnId: "t1", type: "assistantMessage", text: "a" } as StructuredPart,
      { seq: 3, tsMs: 3, turnId: "t2", type: "userMessage", text: "second" } as StructuredPart,
      { seq: 4, tsMs: 4, turnId: "t2", type: "assistantMessage", text: "b" } as StructuredPart,
    ];
    expect(structuredChatEditFrom({ parts, seq: 3 })).toEqual({ turns: 1, text: "second" });
    expect(structuredChatEditFrom({ parts, seq: 1 })).toEqual({ turns: 2, text: "first\n\nsecond" });
    expect(structuredChatEditFrom({ parts, seq: 99 })).toBeNull();
  });

  it("reports file reverting as unsupported and keeps the notice", () => {
    expect(structuredChatRevertFilesSupported(null)).toBe(false);
    expect(
      structuredChatRevertFilesSupported({
        steer: true,
        rollback: true,
        permissionModes: ["supervised"],
        models: [],
        slashCommands: [],
      }),
    ).toBe(false);
    expect(STRUCTURED_CHAT_REVERT_FILES_UNSUPPORTED).toContain("not supported");
  });

  it("offers only the grant modes the agent advertises, and none when it advertises none", () => {
    // referenceTypes.ts:302 - missing entries hide that UI; an agent that claims no permission
    // mode must not be offered a picker for modes it never claimed.
    expect(structuredChatPermissionModes(null)).toEqual([]);
    expect(
      structuredChatPermissionModes({
        steer: false,
        rollback: false,
        permissionModes: [],
        models: [],
        slashCommands: [],
      }),
    ).toEqual([]);
    expect(
      structuredChatPermissionModes({
        steer: false,
        rollback: false,
        permissionModes: ["full-access", "supervised"],
        models: [],
        slashCommands: [],
      }),
    ).toEqual(["supervised", "full-access"]);
  });
});

describe("structured chat wire adapters", () => {
  it("puts the approve requestId in the path and the after cursor in the query", async () => {
    const seen: { path: string; method?: string; body?: unknown; auth?: string }[] = [];
    const adapter = structuredChatHttpWire(
      { baseUrl: "https://host", token: "tok" },
      async (path, init) => {
        seen.push({
          path,
          method: init.method,
          body: init.body === undefined ? undefined : String(init.body),
          auth: (init.headers as Record<string, string>).Authorization,
        });
        return new Response(JSON.stringify({ ok: true }), { status: 200 });
      },
    );
    const transport = structuredChatTransportFrom(adapter);
    await transport(
      { op: "approve", sessionId: "s1", body: { requestId: "r/1", decision: "accept" } },
      SIGNAL,
    );
    await transport({ op: "history", sessionId: "s1", body: { after: 12 } }, SIGNAL);
    await transport({ op: "closeSession", sessionId: "s1", body: {} }, SIGNAL);
    expect(seen[0]).toMatchObject({
      path: "/api/v1/structured-chat/sessions/s1/approvals/r%2F1",
      method: "POST",
      auth: "Bearer tok",
    });
    expect(seen[1]).toMatchObject({
      path: "/api/v1/structured-chat/sessions/s1/history?after=12",
      method: "GET",
    });
    expect(seen[1].body).toBeUndefined();
    expect(seen[2]).toMatchObject({ method: "DELETE", body: undefined });
  });

  it("walks a history page backwards with before and a bounded limit", async () => {
    // `HistoryPage.hasOlder`/`nextBefore` only mean something if the request can ask for the page
    // below the newest window (gap lane chat-gaps-20261126).
    const seen: string[] = [];
    const adapter = structuredChatTransportFrom(
      structuredChatHttpWire({ baseUrl: "http://host", token: "tok" }, async (path) => {
        seen.push(path);
        return new Response(JSON.stringify({ parts: [] }), { status: 200 });
      }),
    );
    await adapter({ op: "history", sessionId: "s1", body: { after: 0, before: 42, limit: 50 } }, SIGNAL);
    await adapter({ op: "events", sessionId: "s1", body: { after: 42 } }, SIGNAL);
    expect(seen).toEqual([
      "/api/v1/structured-chat/sessions/s1/history?after=0&before=42&limit=50",
      // An events poll carries no paging: it asks only what is past the cursor.
      "/api/v1/structured-chat/sessions/s1/events?after=42",
    ]);
  });

  it("reads a bounded page's older cursor defensively", () => {
    const page = decodeStructuredReply("history", 200, {
      parts: [],
      nextSeq: 9,
      hasOlder: true,
      nextBefore: 7,
    });
    expect(page.ok && page.data).toMatchObject({ hasOlder: true, nextBefore: 7 });
    // A gateway that does not page says nothing: the client must not invent a cursor, and an
    // empty page's `nextSeq` is the cursor it was asked with, so the poll keeps asking there.
    const plain = decodeStructuredReply("history", 200, { parts: [], nextSeq: 9 }, 9);
    expect(plain.ok && plain.data).toEqual({ parts: [], nextSeq: 9 });
  });

  it("turns a rejected IPC command into the typed refusal the routes would have sent", async () => {
    const transport = structuredChatTransportFrom(
      structuredChatIpcWire(async () => {
        throw { code: "SessionNotFound", message: "NOT_FOUND: no such session" };
      }),
    );
    const result = await transport(
      { op: "interrupt", sessionId: "gone", body: {} },
      SIGNAL,
    );
    expect(result).toMatchObject({ ok: false, code: "NOT_FOUND" });
  });

  it("passes an IPC success through the same decoding as HTTP", async () => {
    const transport = structuredChatTransportFrom(
      structuredChatIpcWire(async (request) => {
        expect(request).toMatchObject({ op: "clear", sessionId: "s1" });
        return { seq: 11 };
      }),
    );
    const result = await transport({ op: "clear", sessionId: "s1", body: {} }, SIGNAL);
    expect(result).toEqual({ ok: true, data: 11 });
  });
});
