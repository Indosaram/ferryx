import { describe, expect, it } from "vitest";
import {
  STRUCTURED_PERMISSION_MODES,
  decodeStructuredCapabilities,
  decodeStructuredPage,
  decodeStructuredPart,
  decodeStructuredParts,
  decodeStructuredSession,
  decodeStructuredSettings,
  groupStructuredTurns,
  mergeStructuredParts,
  structuredChatRoute,
  structuredHighestSeq,
  structuredLatestUsage,
  structuredPartsAfter,
  structuredPendingApprovals,
  structuredTurnsBackTo,
  type StructuredPart,
} from "./structuredChatTypes";

/** One wire row for every `PartKind` in `src-tauri/src/structured_chat/types.rs`. */
const EVERY_KIND: readonly unknown[] = [
  { seq: 1, tsMs: 10, type: "userMessage", text: "hi" },
  { seq: 2, tsMs: 11, type: "assistantMessage", text: "yo" },
  { seq: 3, tsMs: 12, type: "reasoning", text: "thinking" },
  {
    seq: 4,
    tsMs: 13,
    type: "toolCall",
    id: "c1",
    name: "shell",
    status: "completed",
    output: "ok",
    exitCode: 0,
    durationMs: 42,
  },
  { seq: 5, tsMs: 14, type: "toolCall", id: "c2", name: "shell", status: "inProgress" },
  {
    seq: 6,
    tsMs: 15,
    type: "fileChange",
    id: "f1",
    status: "failed",
    changes: [{ path: "a.ts", kind: "update", diff: "@@" }],
  },
  {
    seq: 7,
    tsMs: 16,
    type: "approvalRequest",
    requestId: "r1",
    kind: "command",
    summary: "run it",
    reason: "why",
    answered: false,
  },
  { seq: 8, tsMs: 17, type: "plan", text: "1. plan" },
  { seq: 9, tsMs: 18, type: "error", message: "bad" },
  {
    seq: 10,
    tsMs: 19,
    type: "usage",
    totalTokens: 100,
    inputTokens: 60,
    cachedInputTokens: 10,
    outputTokens: 30,
    contextWindow: 8000,
  },
  { seq: 11, tsMs: 20, type: "newSession" },
];

describe("structured part decoding", () => {
  it("decodes one row of every PartKind the frozen contract defines", () => {
    const decoded = decodeStructuredParts({ parts: EVERY_KIND });
    expect(decoded.map((part) => part.type)).toEqual([
      "userMessage",
      "assistantMessage",
      "reasoning",
      "toolCall",
      "toolCall",
      "fileChange",
      "approvalRequest",
      "plan",
      "error",
      "usage",
      "newSession",
    ]);
    expect(decoded.every((part) => part.type !== "raw")).toBe(true);
  });

  it("keeps every optional toolCall field and omits the ones the wire omitted", () => {
    const [full, bare] = decodeStructuredParts({ parts: EVERY_KIND.slice(3, 5) });
    expect(full).toMatchObject({ type: "toolCall", exitCode: 0, durationMs: 42, output: "ok" });
    expect(bare).toEqual({
      seq: 5,
      tsMs: 14,
      type: "toolCall",
      id: "c2",
      name: "shell",
      status: "inProgress",
    });
  });

  it("keeps an unknown kind raw with its type named and its payload intact", () => {
    const part = decodeStructuredPart({ seq: 12, tsMs: 21, turnId: "t1", type: "futurePart", nested: { a: [1] } });
    expect(part).toEqual({
      seq: 12,
      tsMs: 21,
      turnId: "t1",
      type: "raw",
      rawType: "futurePart",
      value: { nested: { a: [1] } },
    });
  });

  it("keeps a known kind with broken fields raw rather than dropping it", () => {
    const part = decodeStructuredPart({ seq: 13, tsMs: 22, type: "toolCall", id: "only-an-id" });
    expect(part).toMatchObject({ type: "raw", rawType: "toolCall", seq: 13 });
    const usage = decodeStructuredPart({ seq: 14, tsMs: 23, type: "usage", totalTokens: "many" });
    expect(usage).toMatchObject({ type: "raw", rawType: "usage" });
  });

  it("drops a row whose envelope has no usable seq", () => {
    expect(decodeStructuredPart({ tsMs: 1, type: "userMessage", text: "x" })).toBeNull();
    expect(decodeStructuredPart({ seq: -1, tsMs: 1, type: "userMessage", text: "x" })).toBeNull();
    expect(decodeStructuredPart({ seq: 1.5, tsMs: 1, type: "userMessage", text: "x" })).toBeNull();
    expect(decodeStructuredPart(null)).toBeNull();
    expect(decodeStructuredPart([{ seq: 1 }])).toBeNull();
  });

  it("reads the {parts} envelope and a bare array alike", () => {
    expect(decodeStructuredParts({ parts: [EVERY_KIND[0]] })).toHaveLength(1);
    expect(decodeStructuredParts([EVERY_KIND[0]])).toHaveLength(1);
    expect(decodeStructuredParts({ nope: true })).toEqual([]);
  });
});

describe("structured page cursors", () => {
  it("reports the highest seq in the page, or the requested cursor when empty", () => {
    const page = decodeStructuredPage({ parts: EVERY_KIND }, 0);
    expect(page.nextSeq).toBe(11);
    expect(decodeStructuredPage({ parts: [] }, 7).nextSeq).toBe(7);
    expect(structuredHighestSeq([], 3)).toBe(3);
  });

  it("filters past a cursor without reordering", () => {
    const parts = decodeStructuredParts({ parts: EVERY_KIND });
    expect(structuredPartsAfter(parts, 9).map((part) => part.seq)).toEqual([10, 11]);
  });
});

describe("structured page merging", () => {
  it("de-duplicates by seq and lets the later copy of a seq win", () => {
    const first = decodeStructuredParts({ parts: [EVERY_KIND[0], EVERY_KIND[1]] });
    const replay = [
      { seq: 2, tsMs: 11, type: "assistantMessage", text: "yo (final)" } as StructuredPart,
      { seq: 3, tsMs: 12, type: "reasoning", text: "r" } as StructuredPart,
    ];
    const merged = mergeStructuredParts(first, replay);
    expect(merged.map((part) => part.seq)).toEqual([1, 2, 3]);
    expect(merged[1]).toMatchObject({ text: "yo (final)" });
  });

  it("sorts an out-of-order page ascending and is a no-op for an empty one", () => {
    const parts = decodeStructuredParts({ parts: EVERY_KIND });
    const merged = mergeStructuredParts([], [parts[3], parts[1], parts[0]]);
    expect(merged.map((part) => part.seq)).toEqual([1, 2, 4]);
    expect(mergeStructuredParts(merged, [])).toBe(merged);
  });
});

describe("structured turn grouping", () => {
  const parts = decodeStructuredParts({
    parts: [
      { seq: 1, tsMs: 1, turnId: "t1", type: "userMessage", text: "one" },
      { seq: 2, tsMs: 2, turnId: "t1", type: "assistantMessage", text: "a" },
      { seq: 3, tsMs: 3, type: "newSession" },
      { seq: 4, tsMs: 4, turnId: "t2", type: "userMessage", text: "two" },
      { seq: 5, tsMs: 5, type: "error", message: "session level" },
    ],
  });

  it("groups by turnId and gives an envelope without one its own group", () => {
    const groups = groupStructuredTurns(parts);
    expect(groups.map((group) => group.key)).toEqual(["t1", "seq:3", "t2", "seq:5"]);
    expect(groups[0].parts.map((part) => part.seq)).toEqual([1, 2]);
  });

  it("counts the whole turns a seq sits back from the end, skipping parts that belong to none", () => {
    // `rollback(n)` counts provider turns: `seq:3` and `seq:5` sit in no turn, so they never add one.
    expect(structuredTurnsBackTo(parts, 4)).toBe(1);
    expect(structuredTurnsBackTo(parts, 1)).toBe(2);
    expect(structuredTurnsBackTo(parts, 3)).toBe(1);
    expect(structuredTurnsBackTo(parts, 999)).toBe(0);
  });
});

describe("structured approvals and usage", () => {
  it("retires an approval a later part marked answered, even out of order", () => {
    const parts = decodeStructuredParts({
      parts: [
        { seq: 1, tsMs: 1, type: "approvalRequest", requestId: "r1", kind: "command", summary: "a", answered: false },
        { seq: 2, tsMs: 2, type: "approvalRequest", requestId: "r2", kind: "fileChange", summary: "b", answered: false },
        { seq: 3, tsMs: 3, type: "approvalRequest", requestId: "r1", kind: "command", summary: "a", answered: true },
      ],
    });
    expect(structuredPendingApprovals(parts).map((row) => row.requestId)).toEqual(["r2"]);
  });

  it("reads the most recent usage total and reports none when the agent never sent one", () => {
    const parts = decodeStructuredParts({
      parts: [
        { seq: 1, tsMs: 1, type: "usage", totalTokens: 5, inputTokens: 3, cachedInputTokens: 1, outputTokens: 1 },
        { seq: 2, tsMs: 2, type: "usage", totalTokens: 9, inputTokens: 6, cachedInputTokens: 2, outputTokens: 1, contextWindow: 4000 },
      ],
    });
    expect(structuredLatestUsage(parts)).toEqual({
      totalTokens: 9,
      inputTokens: 6,
      cachedInputTokens: 2,
      outputTokens: 1,
      contextWindow: 4000,
    });
    expect(structuredLatestUsage(decodeStructuredParts({ parts: [] }))).toBeNull();
  });
});

describe("structured contracts", () => {
  it("keeps the four grant modes in order", () => {
    expect(STRUCTURED_PERMISSION_MODES).toEqual([
      "supervised",
      "auto-accept-edits",
      "auto",
      "full-access",
    ]);
  });

  it("decodes capabilities and drops entries the contract does not allow", () => {
    const caps = decodeStructuredCapabilities({
      steer: true,
      rollback: false,
      permissionModes: ["auto", "nope", "supervised"],
      models: [{ id: "m", label: "M", efforts: ["low", "", 3] }, { id: "" }, "junk"],
      slashCommands: ["/clear", "", "  "],
    });
    expect(caps).toEqual({
      steer: true,
      rollback: false,
      permissionModes: ["auto", "supervised"],
      models: [{ id: "m", label: "M", efforts: ["low"] }],
      slashCommands: ["/clear"],
    });
    expect(decodeStructuredCapabilities({ steer: "yes", rollback: true })).toBeNull();
  });

  it("decodes a session summary and refuses one without its identity", () => {
    expect(
      decodeStructuredSession({
        sessionId: "s1",
        provider: "codex",
        cwd: "/repo",
        state: "ended",
        settings: { model: "m", effort: "high", permissionMode: "auto", extra: 1 },
      }),
    ).toEqual({
      sessionId: "s1",
      provider: "codex",
      cwd: "/repo",
      state: "ended",
      settings: { model: "m", effort: "high", permissionMode: "auto" },
    });
    expect(decodeStructuredSession({ provider: "codex", cwd: "/repo", state: "running" })).toBeNull();
    expect(decodeStructuredSession({ sessionId: "s", provider: "p", cwd: "/", state: "gone" })).toBeNull();
  });

  it("decodes partial settings without inventing the fields it did not get", () => {
    expect(decodeStructuredSettings({ model: "m" })).toEqual({ model: "m" });
    expect(decodeStructuredSettings({ permissionMode: "bogus" })).toEqual({});
    expect(decodeStructuredSettings(null)).toEqual({});
  });

  it("builds the route every segment URL-encoded", () => {
    expect(structuredChatRoute()).toBe("/api/v1/structured-chat/sessions");
    expect(structuredChatRoute("a/b")).toBe("/api/v1/structured-chat/sessions/a%2Fb");
    expect(structuredChatRoute("s1", "turns")).toBe(
      "/api/v1/structured-chat/sessions/s1/turns",
    );
  });
});
