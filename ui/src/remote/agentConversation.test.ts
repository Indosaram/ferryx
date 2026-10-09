import { describe, expect, it, vi } from "vitest";
import {
  mapAgentConversation,
  type ConversationMessage,
  capRetainedMessages,
  MAX_RETAINED_CHAT_MESSAGES,
  fetchAgentConversation,
  ConversationFetchError,
  isTranscriptResponseCurrent,
  hasConversationReset,
  type TranscriptTargetIdentity,
} from "./agentConversation";
import type { MobileChatMessageProps } from "./chat/MobileChatMessage";
import type { ToolCallCardProps } from "./chat/MobileChatComponents";

describe("mapAgentConversation tool calls and thinking", () => {
  it("pairs each tool call with its result and keeps reasoning in order", () => {
    const items: ConversationMessage[] = [
      { ordinal: 0, role: "user", text: "probe the daemon" },
      {
        ordinal: 1,
        role: "assistant",
        text: "",
        thinking: "Need the session count first.",
        toolCalls: [{ id: "call_1", name: "eval", summary: "Count sessions", input: "print(sessions.length)" }],
      },
      { ordinal: 2, role: "toolResult", text: "26", toolCallId: "call_1", toolName: "eval" },
      {
        ordinal: 3,
        role: "assistant",
        text: "",
        toolCalls: [{ id: "call_2", name: "bash", input: "false" }],
      },
      { ordinal: 4, role: "toolResult", text: "exit 1", toolCallId: "call_2", toolName: "bash", isError: true },
      { ordinal: 5, role: "assistant", text: "There are 26 sessions." },
    ];

    const [user, assistant] = mapAgentConversation(items);
    expect(user.role).toBe("user");
    expect(assistant.content).toBe("There are 26 sessions.");
    expect(assistant.toolCalls).toEqual([
      { kind: "thinking", text: "Need the session count first.", workKey: "thinking-1" },
      expect.objectContaining({
        toolName: "eval",
        summary: "Count sessions",
        command: "print(sessions.length)",
        output: "26",
        status: "success",
      }),
      expect.objectContaining({ toolName: "bash", command: "false", output: "exit 1", status: "error" }),
    ]);
  });

  it("a call issued after the prose is folded into that assistant turn's work list", () => {
    const items: ConversationMessage[] = [
      { ordinal: 0, role: "user", text: "go" },
      { ordinal: 1, role: "assistant", text: "Checking.", toolCalls: [{ id: "c1", name: "read", input: "/tmp/a" }] },
    ];

    const mapped = mapAgentConversation(items, { activeTurnStartedAt: Date.now() });
    expect(mapped.map((m) => m.content)).toEqual(["go", "Checking."]);
    expect(mapped[1].toolCalls).toEqual([
      expect.objectContaining({ toolName: "read", command: "/tmp/a", status: "running" }),
    ]);
  });

  it('extracts flattened tool markers "→ read\\n→ read" and pairs them with toolResults in FIFO order with no prose bubble', () => {
    const items: ConversationMessage[] = [
      {
        ordinal: 8366,
        role: "assistant",
        text: "→ read\n→ read",
      },
      {
        ordinal: 8367,
        role: "toolResult",
        text: "first file content",
      },
      {
        ordinal: 8368,
        role: "toolResult",
        text: "second file content",
      },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped.some((m) => m.content.trim() !== "")).toBe(false);
    expect(mapped).toHaveLength(1);
    expect(mapped[0].role).toBe("assistant");
    expect(mapped[0].content).toBe("");
    expect(mapped[0].toolCalls).toEqual([
      expect.objectContaining({
        toolName: "read",
        output: "first file content",
        status: "success",
      }),
      expect.objectContaining({
        toolName: "read",
        output: "second file content",
        status: "success",
      }),
    ]);
  });

  it('handles "Prose here. \\n→ eval" followed by toolResult giving prose with no arrow plus one eval card holding the output in one folded turn', () => {
    const items: ConversationMessage[] = [
      {
        ordinal: 8374,
        role: "assistant",
        text: "Prose here. \n→ eval",
      },
      {
        ordinal: 8375,
        role: "toolResult",
        text: "eval output 42",
      },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped).toHaveLength(1);
    expect(mapped[0].role).toBe("assistant");
    expect(mapped[0].content).toBe("Prose here.");
    expect(mapped[0].content).not.toContain("→");
    expect(mapped[0].content).not.toContain("eval");
    expect(mapped[0].toolCalls).toEqual([
      expect.objectContaining({
        toolName: "eval",
        output: "eval output 42",
        status: "success",
      }),
    ]);
  });

  it('treats "← bash result" as empty output and sets output to undefined', () => {
    const items: ConversationMessage[] = [
      {
        ordinal: 1,
        role: "assistant",
        text: "→ bash",
      },
      {
        ordinal: 2,
        role: "toolResult",
        text: "← bash result",
      },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped).toHaveLength(1);
    expect(mapped[0].toolCalls).toEqual([
      expect.objectContaining({
        toolName: "bash",
        output: undefined,
        status: "success",
      }),
    ]);
  });

  it("marks an open card before a later user message as success, while keeping an open card in the last turn running", () => {
    const items: ConversationMessage[] = [
      {
        ordinal: 1,
        role: "assistant",
        text: "→ read",
      },
      {
        ordinal: 2,
        role: "user",
        text: "proceed with next step",
      },
      {
        ordinal: 3,
        role: "assistant",
        text: "→ eval",
      },
    ];

    const mapped = mapAgentConversation(items, { activeTurnStartedAt: Date.now() });
    // Turn 1's assistant fallback has read card marked success because user message moved transcript on
    expect(mapped[0].role).toBe("assistant");
    expect(mapped[0].toolCalls).toEqual([
      expect.objectContaining({
        toolName: "read",
        status: "success",
      }),
    ]);

    // User message
    expect(mapped[1].role).toBe("user");
    expect(mapped[1].content).toBe("proceed with next step");

    // Turn 2's assistant fallback has eval card staying running in the last turn
    expect(mapped[2].role).toBe("assistant");
    expect(mapped[2].toolCalls).toEqual([
      expect.objectContaining({
        toolName: "eval",
        status: "running",
      }),
    ]);
  });

  it("folds multi-step assistant turn into one turn with earlier prose as thinking work items", () => {
    const items: ConversationMessage[] = [
      { ordinal: 0, role: "user", text: "count files" },
      {
        ordinal: 1,
        role: "assistant",
        text: "Checking. \n→ read",
      },
      {
        ordinal: 2,
        role: "toolResult",
        text: "file list",
      },
      {
        ordinal: 3,
        role: "assistant",
        text: "→ eval",
      },
      {
        ordinal: 4,
        role: "toolResult",
        text: "3",
      },
      {
        ordinal: 5,
        role: "assistant",
        text: "Done, 3 files.",
      },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped).toHaveLength(2);
    const [user, assistant] = mapped;
    expect(user.role).toBe("user");
    expect(user.content).toBe("count files");

    expect(assistant.role).toBe("assistant");
    expect(assistant.content).toBe("Done, 3 files.");
    expect(assistant.durationLabel).toBeDefined();
    expect(assistant.toolCalls).toEqual([
      { kind: "thinking", text: "Checking.", source: "prose", workKey: "prose-1" },
      expect.objectContaining({
        toolName: "read",
        output: "file list",
        status: "success",
      }),
      expect.objectContaining({
        toolName: "eval",
        output: "3",
        status: "success",
      }),
    ]);
  });

  it("keeps prose arrows unchanged and creates no tool calls", () => {
    const items: ConversationMessage[] = [
      {
        ordinal: 1,
        role: "assistant",
        text: "Open Account settings → Tenants.\nThen User management → Entra tab.",
      },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped).toHaveLength(1);
    expect(mapped[0].role).toBe("assistant");
    expect(mapped[0].content).toBe(
      "Open Account settings → Tenants.\nThen User management → Entra tab.",
    );
    expect(mapped[0].toolCalls).toBeUndefined();
  });

  it('parses "Checking.\\n→ read\\n→ eval" with body "Checking." and read and eval tool rows', () => {
    const items: ConversationMessage[] = [
      {
        ordinal: 1,
        role: "assistant",
        text: "Checking.\n→ read\n→ eval",
      },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped).toHaveLength(1);
    expect(mapped[0].role).toBe("assistant");
    expect(mapped[0].content).toBe("Checking.");
    expect(mapped[0].toolCalls).toEqual([
      expect.objectContaining({ toolName: "read" }),
      expect.objectContaining({ toolName: "eval" }),
    ]);
  });

  it('parses "→ eval" alone at start followed by toolResult as eval card with output and empty body', () => {
    const items: ConversationMessage[] = [
      {
        ordinal: 1,
        role: "assistant",
        text: "→ eval",
      },
      {
        ordinal: 2,
        role: "toolResult",
        text: "42",
      },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped).toHaveLength(1);
    expect(mapped[0].id).toBe("assistant-1");
    expect(mapped[0].role).toBe("assistant");
    expect(mapped[0].content).toBe("");
    expect(mapped[0].durationLabel).toBeDefined();
    expect(mapped[0].toolCalls).toEqual([
      expect.objectContaining({
        toolName: "eval",
        output: "42",
        status: "success",
      }),
    ]);
  });

  it('parses mixed input "Settings → Tenants.\\n→ bash" with body "Settings → Tenants." and one bash row', () => {
    const items: ConversationMessage[] = [
      {
        ordinal: 1,
        role: "assistant",
        text: "Settings → Tenants.\n→ bash",
      },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped).toHaveLength(1);
    expect(mapped[0].role).toBe("assistant");
    expect(mapped[0].content).toBe("Settings → Tenants.");
    expect(mapped[0].toolCalls).toEqual([
      expect.objectContaining({ toolName: "bash" }),
    ]);
  });

  it('measures turn duration from user prompt timestamp yielding "1m" instead of "30s"', () => {
    const items: ConversationMessage[] = [
      { ordinal: 0, role: "user", text: "search logs", timestamp: 0 },
      { ordinal: 1, role: "assistant", text: "→ read", timestamp: 30 },
      { ordinal: 2, role: "toolResult", text: "logs found", timestamp: 40 },
      { ordinal: 3, role: "assistant", text: "Done", timestamp: 60 },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped).toHaveLength(2);
    expect(mapped[0].role).toBe("user");
    expect(mapped[1].role).toBe("assistant");
    expect(mapped[1].content).toBe("Done");
    expect(mapped[1].durationLabel).toBe("1m");
  });

  it("does not cache the last span's duration label", () => {
    const turnDurationsMap = new Map<string, string>();
    const itemsCall1: ConversationMessage[] = [
      { ordinal: 0, role: "user", text: "start", timestamp: 1_700_000_000 },
      { ordinal: 1, role: "assistant", text: "→ read", timestamp: 1_700_000_000 },
    ];

    const mapped1 = mapAgentConversation(itemsCall1, { turnDurationsMap });
    expect(mapped1).toHaveLength(2);
    expect(mapped1[1].durationLabel).toBe("0s");

    const itemsCall2: ConversationMessage[] = [
      { ordinal: 0, role: "user", text: "start", timestamp: 1_700_000_000 },
      { ordinal: 1, role: "assistant", text: "→ read", timestamp: 1_700_000_000 },
      { ordinal: 2, role: "toolResult", text: "done", timestamp: 1_700_000_120 },
    ];

    const mapped2 = mapAgentConversation(itemsCall2, {
      turnDurationsMap,
      previousMessages: mapped1,
    });
    expect(mapped2).toHaveLength(2);
    expect(mapped2[1].durationLabel).toBe("2m");
    expect(turnDurationsMap.size).toBe(0);
  });

  it("maps assistant followed by system record to ids in ordinal order", () => {
    const items: ConversationMessage[] = [
      { ordinal: 1, role: "assistant", text: "Done." },
      { ordinal: 2, role: "system", text: "Model switched" },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped.map((m) => m.id)).toEqual(["assistant-1", "system-2"]);
  });

  it("retains stable assistant turn id from first span record across progressive prose additions", () => {
    const baseItems: ConversationMessage[] = [
      { ordinal: 0, role: "user", text: "query" },
      { ordinal: 1, role: "assistant", text: "→ read" },
      { ordinal: 2, role: "toolResult", text: "x" },
    ];

    const mapped1 = mapAgentConversation(baseItems);
    const mapped2 = mapAgentConversation([
      ...baseItems,
      { ordinal: 3, role: "assistant", text: "Final." },
    ]);

    const assistant1 = mapped1.find((m) => m.role === "assistant");
    const assistant2 = mapped2.find((m) => m.role === "assistant");

    expect(assistant1?.id).toBe("assistant-1");
    expect(assistant2?.id).toBe("assistant-1");
  });

  it("falls back to oldest still-open card when toolResult has no toolCallId", () => {
    const items: ConversationMessage[] = [
      {
        ordinal: 0,
        role: "assistant",
        text: "",
        toolCalls: [{ id: "c1", name: "eval", input: "x" }],
      },
      {
        ordinal: 1,
        role: "toolResult",
        text: "26",
        toolName: "eval",
      },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped).toHaveLength(1);
    expect(mapped[0].role).toBe("assistant");
    expect(mapped[0].toolCalls).toHaveLength(1);
    expect(mapped[0].toolCalls![0]).toMatchObject({
      toolName: "eval",
      command: "x",
      status: "success",
      output: "26",
    });
  });

  it("a result with an unknown toolCallId prefers the oldest id-keyed card; an id-less result takes the id-less card", () => {
    const items: ConversationMessage[] = [
      {
        ordinal: 0,
        role: "assistant",
        text: "→ bash",
      },
      {
        ordinal: 1,
        role: "assistant",
        text: "",
        toolCalls: [{ id: "c1", name: "eval", input: "x" }],
      },
      {
        ordinal: 2,
        role: "toolResult",
        text: "bash result",
        toolCallId: "nonexistent_id",
      },
      {
        ordinal: 3,
        role: "toolResult",
        text: "eval result",
      },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped).toHaveLength(1);
    expect(mapped[0].role).toBe("assistant");
    expect(mapped[0].toolCalls).toHaveLength(2);
    expect(mapped[0].toolCalls![0]).toMatchObject({
      toolName: "bash",
      output: "eval result",
      status: "success",
    });
    expect(mapped[0].toolCalls![1]).toMatchObject({
      toolName: "eval",
      output: "bash result",
      status: "success",
    });
  });

  it("normalizes CRLF so '→ read\\r\\n→ read' gives empty content and two read cards", () => {
    const items: ConversationMessage[] = [
      {
        ordinal: 0,
        role: "assistant",
        text: "→ read\r\n→ read",
      },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped).toHaveLength(1);
    expect(mapped[0].role).toBe("assistant");
    expect(mapped[0].content).toBe("");
    expect(mapped[0].toolCalls).toHaveLength(2);
    expect(mapped[0].toolCalls![0]).toMatchObject({ toolName: "read" });
    expect(mapped[0].toolCalls![1]).toMatchObject({ toolName: "read" });
  });

  it("preserves fenced code blocks without stripping or extracting markers", () => {
    const content = "The wire looks like:\n```\n→ read\n```\nThat is the format.";
    const items: ConversationMessage[] = [
      {
        ordinal: 0,
        role: "assistant",
        text: content,
      },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped).toHaveLength(1);
    expect(mapped[0].role).toBe("assistant");
    expect(mapped[0].content).toBe(content);
    expect(mapped[0].toolCalls).toBeUndefined();
  });

  it("updates live last turn duration from prompt timestamp when activeTurnStartedAt is set", () => {
    vi.useFakeTimers();
    try {
      const now = new Date("2026-09-27T12:00:00.000Z");
      vi.setSystemTime(now);

      const promptTimestamp = now.getTime() - 10_000;
      const items: ConversationMessage[] = [
        { ordinal: 0, role: "user", text: "query", timestamp: promptTimestamp },
        { ordinal: 1, role: "assistant", text: "→ read", timestamp: promptTimestamp + 1_000 },
      ];

      const mapped1 = mapAgentConversation(items, {
        activeTurnStartedAt: now.getTime(),
      });
      const assistant1 = mapped1.find((m) => m.role === "assistant");
      expect(assistant1?.durationLabel).toBe("10s");

      vi.setSystemTime(new Date(now.getTime() + 50_000));

      const mapped2 = mapAgentConversation(items, {
        activeTurnStartedAt: now.getTime(),
      });
      const assistant2 = mapped2.find((m) => m.role === "assistant");
      expect(assistant2?.durationLabel).toBe("1m");
    } finally {
      vi.useRealTimers();
    }
  });

  it("marks card status as error when toolResult has status error without isError", () => {
    const items: ConversationMessage[] = [
      {
        ordinal: 0,
        role: "assistant",
        text: "",
        toolCalls: [{ id: "c1", name: "bash" }],
      },
      {
        ordinal: 1,
        role: "toolResult",
        toolCallId: "c1",
        text: "boom",
        status: "error",
      },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped).toHaveLength(1);
    expect(mapped[0].toolCalls).toHaveLength(1);
    expect(mapped[0].toolCalls![0]).toMatchObject({
      toolName: "bash",
      output: "boom",
      status: "error",
    });
  });

  it("gives paired card durationMs and sets turn durationLabel to 1m 30s for 90000ms duration with no timestamps", () => {
    const items: ConversationMessage[] = [
      {
        ordinal: 0,
        role: "assistant",
        text: "",
        toolCalls: [{ id: "c1", name: "bash" }],
      },
      {
        ordinal: 1,
        role: "toolResult",
        toolCallId: "c1",
        text: "boom",
        status: "error",
        durationMs: 90000,
      },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped).toHaveLength(1);
    expect(mapped[0].durationLabel).toBe("1m 30s");
    expect(mapped[0].toolCalls).toHaveLength(1);
    expect(mapped[0].toolCalls![0]).toMatchObject({
      toolName: "bash",
      durationMs: 90000,
    });
  });

  it("prefers the timestamp span over summed card durationMs for an idle completed turn", () => {
    const items: ConversationMessage[] = [
      { ordinal: 0, role: "user", text: "inspect", timestamp: "2026-09-27T12:00:00.000Z" },
      {
        ordinal: 1,
        role: "assistant",
        text: "",
        timestamp: "2026-09-27T12:00:05.000Z",
        toolCalls: [{ id: "c1", name: "bash" }],
      },
      {
        ordinal: 2,
        role: "toolResult",
        toolCallId: "c1",
        text: "ok",
        timestamp: "2026-09-27T12:00:10.000Z",
        durationMs: 5000,
      },
      { ordinal: 3, role: "assistant", text: "Done.", timestamp: "2026-09-27T12:02:00.000Z" },
    ];

    const mapped = mapAgentConversation(items);
    const assistant = mapped.find((m) => m.role === "assistant");
    expect(assistant?.content).toBe("Done.");
    expect(assistant?.durationLabel).toBe("2m");
  });

  it("strips legacy markers and builds cards when toolCalls is an empty array", () => {
    const items: ConversationMessage[] = [
      { ordinal: 0, role: "assistant", text: "Checking.\n\u2192 read", toolCalls: [] },
      { ordinal: 1, role: "toolResult", text: "file body" },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped.every((m) => !m.content.includes("\u2192"))).toBe(true);
    const assistant = mapped.find((m) => m.role === "assistant");
    expect(assistant?.content).toBe("Checking.");
    expect(assistant?.toolCalls).toEqual([
      expect.objectContaining({ toolName: "read", output: "file body", status: "success" }),
    ]);
  });

  it("does not leak activeTurnStartedAt duration into historical assistant turn before first user message", () => {
    const now = Date.now();
    const activeTurnStartedAt = now - 600000;
    const items: ConversationMessage[] = [
      {
        ordinal: 0,
        role: "assistant",
        text: "→ read",
      },
      {
        ordinal: 1,
        role: "toolResult",
        text: "historical content",
      },
      {
        ordinal: 2,
        role: "user",
        text: "next step",
        timestamp: now - 30000,
      },
      {
        ordinal: 3,
        role: "assistant",
        text: "Done.",
        timestamp: now,
      },
    ];

    const mapped = mapAgentConversation(items, { activeTurnStartedAt });
    const assistantTurns = mapped.filter((m) => m.role === "assistant");
    expect(assistantTurns).toHaveLength(2);
    const firstAssistant = assistantTurns[0];

    expect(firstAssistant.durationLabel).toBe("0s");
  });

  it("matches toolResult to structured toolCall by id when span also contains id-less marker card", () => {
    const items: ConversationMessage[] = [
      {
        ordinal: 0,
        role: "assistant",
        text: "→ read",
      },
      {
        ordinal: 1,
        role: "assistant",
        text: "",
        toolCalls: [{ id: "k1", name: "eval" }],
      },
      {
        ordinal: 2,
        role: "toolResult",
        toolCallId: "k1",
        text: "eval out",
      },
    ];

    const mapped = mapAgentConversation(items);
    expect(mapped).toHaveLength(1);
    const toolCalls = mapped[0].toolCalls;
    expect(toolCalls).toBeDefined();

    const readCard: ToolCallCardProps | undefined = toolCalls?.find(
      (c): c is ToolCallCardProps => "toolName" in c && c.toolName === "read"
    );
    const evalCard: ToolCallCardProps | undefined = toolCalls?.find(
      (c): c is ToolCallCardProps => "toolName" in c && c.toolName === "eval"
    );

    expect(evalCard).toBeDefined();
    expect(evalCard?.output).toBe("eval out");
    expect(readCard).toBeDefined();
    expect(readCard?.output).toBeUndefined();
  });

  it("the last turn is idle and its label was already settled", () => {
    const items: ConversationMessage[] = [
      { ordinal: 0, role: "user", text: "check status", timestamp: 1_700_000_000 },
      { ordinal: 1, role: "assistant", text: "→ read" },
      { ordinal: 2, role: "toolResult", text: "file content" },
    ];

    const turnDurationsMap = new Map<string, string>();
    const firstMapped = mapAgentConversation(items);
    const lastAssistantId = firstMapped.filter((m) => m.role === "assistant").at(-1)!.id;
    turnDurationsMap.set(lastAssistantId, "1m 12s");

    const secondMapped = mapAgentConversation(items, { turnDurationsMap });
    const lastAssistant = secondMapped.filter((m) => m.role === "assistant").at(-1);
    expect(lastAssistant?.durationLabel).toBe("1m 12s");
  });

  it("gives unanswered marker in idle last turn status success and running when active", () => {
    const items: ConversationMessage[] = [
      { ordinal: 0, role: "user", text: "run command" },
      { ordinal: 1, role: "assistant", text: "→ bash" },
    ];

    const idleMapped = mapAgentConversation(items);
    const idleAssistant = idleMapped.find((m) => m.role === "assistant");
    const idleCard = idleAssistant?.toolCalls?.[0];
    expect(idleCard).toBeDefined();
    expect(idleCard).toMatchObject({
      toolName: "bash",
      status: "success",
    });

    const runningMapped = mapAgentConversation(items, {
      activeTurnStartedAt: Date.now(),
    });
    const runningAssistant = runningMapped.find((m) => m.role === "assistant");
    const runningCard = runningAssistant?.toolCalls?.[0];
    expect(runningCard).toBeDefined();
    expect(runningCard).toMatchObject({
      toolName: "bash",
      status: "running",
    });
  });

  it("preserves workKeys across progressive prose additions and ensures all workKeys within a turn are unique", () => {
    const firstItems: ConversationMessage[] = [
      { ordinal: 1, role: "assistant", text: "Checking.\n→ read" },
      { ordinal: 2, role: "toolResult", text: "file content" },
    ];
    const firstMapped = mapAgentConversation(firstItems);
    const firstAssistant = firstMapped.find((m) => m.role === "assistant");
    const firstWorkKeys = (firstAssistant?.toolCalls ?? []).map((w) => w.workKey);

    const secondItems: ConversationMessage[] = [
      ...firstItems,
      { ordinal: 3, role: "assistant", text: "Done." },
    ];
    const secondMapped = mapAgentConversation(secondItems);
    const secondAssistant = secondMapped.find((m) => m.role === "assistant");
    const secondWorkKeys = (secondAssistant?.toolCalls ?? []).map((w) => w.workKey);

    expect(firstWorkKeys.length).toBeGreaterThan(0);
    for (const key of firstWorkKeys) {
      expect(secondWorkKeys).toContain(key);
    }
    expect(new Set(firstWorkKeys).size).toBe(firstWorkKeys.length);
    expect(new Set(secondWorkKeys).size).toBe(secondWorkKeys.length);
  });

  it("caps retained messages to the newest MAX_RETAINED_CHAT_MESSAGES when over the limit and preserves order", () => {
    const totalCount = MAX_RETAINED_CHAT_MESSAGES + 25;
    const overMessages: MobileChatMessageProps[] = Array.from({ length: totalCount }, (_, i) => ({
      id: `msg-${i}`,
      role: i % 2 === 0 ? "user" : "assistant",
      content: `Message ${i}`,
    }));

    const overResult = capRetainedMessages(overMessages);
    expect(overResult.truncated).toBe(true);
    expect(overResult.messages).toHaveLength(MAX_RETAINED_CHAT_MESSAGES);
    expect(overResult.messages[0].id).toBe("msg-25");
    expect(overResult.messages[MAX_RETAINED_CHAT_MESSAGES - 1].id).toBe(`msg-${totalCount - 1}`);
    expect(overResult.messages).toEqual(overMessages.slice(25));

    const exactMessages: MobileChatMessageProps[] = Array.from(
      { length: MAX_RETAINED_CHAT_MESSAGES },
      (_, i) => ({
        id: `exact-${i}`,
        role: i % 2 === 0 ? "user" : "assistant",
        content: `Exact message ${i}`,
      }),
    );
    const exactResult = capRetainedMessages(exactMessages);
    expect(exactResult.truncated).toBe(false);
    expect(exactResult.messages).toHaveLength(MAX_RETAINED_CHAT_MESSAGES);
    expect(exactResult.messages).toEqual(exactMessages);
  });
});

describe("agentConversation transcript identity and generation fencing", () => {
  it("parses valid optional conversationGeneration string from server response", async () => {
    const rawBody = {
      sessionId: "session-1",
      conversationGeneration: "conv-token-42",
      items: [
        { ordinal: 0, role: "user", text: "hello" },
        { ordinal: 1, role: "assistant", text: "world" },
      ],
      nextCursor: null,
      partial: false,
      warnings: [],
    };

    const mockFetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => rawBody,
    });
    vi.stubGlobal("fetch", mockFetch);

    try {
      const page = await fetchAgentConversation({
        baseUrl: "http://127.0.0.1:8899",
        sessionId: "session-1",
        token: "test-token",
      });

      expect(page.sessionId).toBe("session-1");
      expect(page.conversationGeneration).toBe("conv-token-42");
      expect(page.items).toHaveLength(2);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("parses valid null conversationGeneration from server response", async () => {
    const rawBody = {
      sessionId: "session-1",
      conversationGeneration: null,
      items: [],
      nextCursor: null,
      partial: false,
      warnings: [],
    };

    const mockFetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => rawBody,
    });
    vi.stubGlobal("fetch", mockFetch);

    try {
      const page = await fetchAgentConversation({
        baseUrl: "http://127.0.0.1:8899",
        sessionId: "session-1",
        token: "test-token",
      });

      expect(page.conversationGeneration).toBeNull();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("preserves wire compatibility when conversationGeneration is absent from server response", async () => {
    const rawBody = {
      sessionId: "session-1",
      items: [],
      nextCursor: null,
      partial: false,
      warnings: [],
    };

    const mockFetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => rawBody,
    });
    vi.stubGlobal("fetch", mockFetch);

    try {
      const page = await fetchAgentConversation({
        baseUrl: "http://127.0.0.1:8899",
        sessionId: "session-1",
        token: "test-token",
      });

      expect(page.conversationGeneration).toBeUndefined();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("throws MALFORMED_RESPONSE when conversationGeneration is not a string or null", async () => {
    for (const invalidGen of [12345, true, { id: "token" }, ["array"]]) {
      const rawBody = {
        sessionId: "session-1",
        conversationGeneration: invalidGen,
        items: [],
        nextCursor: null,
        partial: false,
        warnings: [],
      };

      const mockFetch = vi.fn().mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => rawBody,
      });
      vi.stubGlobal("fetch", mockFetch);

      try {
        await expect(
          fetchAgentConversation({
            baseUrl: "http://127.0.0.1:8899",
            sessionId: "session-1",
            token: "test-token",
          }),
        ).rejects.toThrowError(ConversationFetchError);
      } finally {
        vi.unstubAllGlobals();
      }
    }
  });

  it("isTranscriptResponseCurrent enforces strict target epoch match including known-to-null transitions", () => {
    const baseExpected: TranscriptTargetIdentity = {
      sessionId: "sess-main",
      targetEpoch: "epoch-10",
      generation: 3,
    };

    // Strict match
    expect(
      isTranscriptResponseCurrent(baseExpected, {
        sessionId: "sess-main",
        targetEpoch: "epoch-10",
        generation: 3,
      }),
    ).toBe(true);

    // Mismatched epoch string
    expect(
      isTranscriptResponseCurrent(baseExpected, {
        sessionId: "sess-main",
        targetEpoch: "epoch-11",
        generation: 3,
      }),
    ).toBe(false);

    // Known to null: must reject
    expect(
      isTranscriptResponseCurrent(baseExpected, {
        sessionId: "sess-main",
        targetEpoch: null,
        generation: 3,
      }),
    ).toBe(false);

    // Null to known: must reject
    const nullEpochExpected: TranscriptTargetIdentity = {
      sessionId: "sess-main",
      targetEpoch: null,
      generation: 3,
    };
    expect(
      isTranscriptResponseCurrent(nullEpochExpected, {
        sessionId: "sess-main",
        targetEpoch: "epoch-10",
        generation: 3,
      }),
    ).toBe(false);

    // Both null: accepted
    expect(
      isTranscriptResponseCurrent(nullEpochExpected, {
        sessionId: "sess-main",
        targetEpoch: null,
        generation: 3,
      }),
    ).toBe(true);
  });

  it("isTranscriptResponseCurrent enforces sequence generation and session identity fencing", () => {
    const expected: TranscriptTargetIdentity = {
      sessionId: "sess-a",
      targetEpoch: "epoch-1",
      generation: 5,
    };

    // Stale generation from previous poll
    expect(
      isTranscriptResponseCurrent(expected, {
        sessionId: "sess-a",
        targetEpoch: "epoch-1",
        generation: 6,
      }),
    ).toBe(false);

    // Mismatched current session
    expect(
      isTranscriptResponseCurrent(expected, {
        sessionId: "sess-b",
        targetEpoch: "epoch-1",
        generation: 5,
      }),
    ).toBe(false);

    // Mismatched payload session
    expect(
      isTranscriptResponseCurrent(
        expected,
        {
          sessionId: "sess-a",
          targetEpoch: "epoch-1",
          generation: 5,
        },
        { sessionId: "sess-c" },
      ),
    ).toBe(false);

    // Matching payload session
    expect(
      isTranscriptResponseCurrent(
        expected,
        {
          sessionId: "sess-a",
          targetEpoch: "epoch-1",
          generation: 5,
        },
        { sessionId: "sess-a" },
      ),
    ).toBe(true);
  });

  it("hasConversationReset detects authoritative generation change without false resets on initial load or omitted token", () => {
    // Rotated conversation (e.g. /new or transcript truncation)
    expect(hasConversationReset("gen-token-1", "gen-token-2")).toBe(true);

    // Downgrade / cleared conversation (string -> null)
    expect(hasConversationReset("gen-token-1", null)).toBe(true);

    // Upgrade / started conversation (null -> string)
    expect(hasConversationReset(null, "gen-token-2")).toBe(true);

    // Explicitly same null state
    expect(hasConversationReset(null, null)).toBe(false);

    // Same conversation string
    expect(hasConversationReset("gen-token-1", "gen-token-1")).toBe(false);

    // Initial load: previous generation was undefined (omitted)
    expect(hasConversationReset(undefined, "gen-token-1")).toBe(false);
    expect(hasConversationReset(undefined, null)).toBe(false);

    // Server omits token (undefined)
    expect(hasConversationReset("gen-token-1", undefined)).toBe(false);
    expect(hasConversationReset(null, undefined)).toBe(false);
    expect(hasConversationReset(undefined, undefined)).toBe(false);
  });
});
