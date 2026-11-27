import { describe, expect, it } from "vitest";
import {
  STRUCTURED_CONTROL_PERMISSION_ID,
  STRUCTURED_NEW_SESSION_DIVIDER,
  structuredApprovalDecisionFor,
  structuredApprovalPrompt,
  structuredChatActivity,
  structuredChatMessages,
  structuredChatRunning,
  structuredComposerCapabilities,
  structuredFileChangePatch,
  structuredPartToReferencePart,
  structuredSettingsForControl,
} from "./structuredChatParts";
import type { StructuredPart } from "./structuredChatTypes";

function at(seq: number, extra: Record<string, unknown>): StructuredPart {
  return { seq, tsMs: 1000 + seq, ...extra } as StructuredPart;
}

describe("structured parts → reference parts", () => {
  it("maps prose, reasoning and plans without inventing text", () => {
    expect(structuredPartToReferencePart(at(1, { type: "userMessage", text: "hi" }))).toEqual({
      kind: "text",
      text: "hi",
      phase: "commentary",
    });
    expect(structuredPartToReferencePart(at(2, { type: "assistantMessage", text: "yo" }))).toEqual({
      kind: "text",
      text: "yo",
      phase: "finalAnswer",
    });
    expect(structuredPartToReferencePart(at(3, { type: "reasoning", text: "hmm" }))).toEqual({
      kind: "thinking",
      text: "hmm",
    });
    expect(structuredPartToReferencePart(at(4, { type: "plan", text: "1. do" }))).toEqual({
      kind: "notice",
      text: "1. do",
      source: "plan",
    });
  });

  it("maps a tool call's lifecycle and a file change's patch body", () => {
    const tool = structuredPartToReferencePart(
      at(5, {
        type: "toolCall",
        id: "call-1",
        name: "shell",
        status: "failed",
        output: "boom",
        durationMs: 12,
      }),
    );
    expect(tool).toMatchObject({ kind: "tool", name: "shell", output: "boom", error: true, status: "failed" });
    expect((tool as { summary: string }).summary).toContain("shell");

    const change = at(6, {
      type: "fileChange",
      id: "fc-1",
      status: "completed",
      changes: [{ path: "src/a.ts", kind: "update", diff: "@@ -1 +1 @@\n-a\n+b" }],
    });
    expect(structuredFileChangePatch(change as never)).toContain("src/a.ts");
    const view = structuredPartToReferencePart(change);
    expect(view).toMatchObject({ kind: "diff", body: expect.stringContaining("src/a.ts") });
  });

  it("leaves an open approval to the card and keeps an answered one as a notice", () => {
    // An open request is drawn once, by the prompt card at the end of the stream. Drawing the
    // same request inline as well would show it twice.
    expect(
      structuredPartToReferencePart(
        at(7, { type: "approvalRequest", requestId: "r1", kind: "command", summary: "rm -rf", answered: false }),
      ),
    ).toBeNull();
    expect(
      structuredPartToReferencePart(
        at(8, { type: "approvalRequest", requestId: "r1", kind: "command", summary: "rm -rf", answered: true }),
      ),
    ).toEqual({ kind: "notice", text: "rm -rf (answered)", source: "approval:command" });
    expect(
      structuredPartToReferencePart(
        at(9, {
          type: "usage",
          totalTokens: 10,
          inputTokens: 6,
          cachedInputTokens: 2,
          outputTokens: 2,
        }),
      ),
    ).toBeNull();
    expect(structuredPartToReferencePart(at(10, { type: "newSession" }))).toBeNull();
  });

  it("keeps an unknown kind visible as its raw payload with the kind named", () => {
    expect(
      structuredPartToReferencePart(at(11, { type: "raw", rawType: "futureThing", value: { a: 1 } })),
    ).toEqual({ kind: "notice", text: '{"a":1}', source: "unknown:futureThing" });
  });

  it("draws a rollback as a divider naming how many turns it took back", () => {
    expect(structuredPartToReferencePart(at(12, { type: "rollback", turns: 1 }))).toEqual({
      kind: "notice",
      text: "Rolled back 1 turn",
      source: "rollback",
    });
    expect(structuredPartToReferencePart(at(13, { type: "rollback", turns: 3 }))).toEqual({
      kind: "notice",
      text: "Rolled back 3 turns",
      source: "rollback",
    });
  });

  it("leaves an open extension request to its card and keeps an answered one as a notice", () => {
    const base = { type: "extensionUiRequest", requestId: "ext-1", method: "select", options: ["a", "b"] };
    // An open request is drawn by `ExtensionUiCard`, which owns the answer affordance: drawing it
    // inline as well would ask the reader the same question twice.
    expect(structuredPartToReferencePart(at(14, { ...base, answered: false }))).toBeNull();
    expect(structuredPartToReferencePart(at(15, { ...base, answered: true, title: "Pick a target" }))).toEqual({
      kind: "notice",
      text: "Pick a target (answered)",
      source: "extension:select",
    });
    // A request that never had a title falls back to the method, never to an empty notice.
    expect(structuredPartToReferencePart(at(16, { ...base, answered: true }))).toEqual({
      kind: "notice",
      text: "select (answered)",
      source: "extension:select",
    });
  });

  it("never inlines the side channel, so a heartbeat cannot become a transcript row", () => {
    const sideChannel = [
      at(17, { type: "extensionStatus", statusKey: "websearch", statusText: "Searching" }),
      at(18, { type: "extensionWidget", widgetKey: "todo", lines: ["one"] }),
      at(19, { type: "subagentProgress", taskId: "t1", name: "auditor", status: "inProgress" }),
      at(20, { type: "goalUpdate", goalId: "g1", title: "Ship it", status: "inProgress" }),
      at(21, { type: "mediaRef", toolCallId: "call-1", contentIndex: 0, mimeType: "image/png" }),
    ];
    for (const part of sideChannel) {
      expect(structuredPartToReferencePart(part)).toBeNull();
    }
    // And none of them is a message of its own either.
    expect(structuredChatMessages([at(1, { turnId: "t1", type: "userMessage", text: "go" }), ...sideChannel])).toHaveLength(1);
  });
});

describe("structured stream as workspace messages", () => {
  it("groups a user run into one message and the reply run into one assistant message", () => {
    const messages = structuredChatMessages([
      at(1, { turnId: "t1", type: "userMessage", text: "hello" }),
      at(2, { turnId: "t1", type: "reasoning", text: "think" }),
      at(3, { turnId: "t1", type: "assistantMessage", text: "world" }),
      at(4, { turnId: "t1", type: "usage", totalTokens: 3, inputTokens: 2, cachedInputTokens: 0, outputTokens: 1 }),
    ]);
    expect(messages.map((message) => message.role)).toEqual(["user", "assistant"]);
    expect(messages[0].content).toBe("hello");
    expect(messages[1].content).toBe("world");
    expect(messages[1].referenceParts?.map((part) => part.kind)).toEqual(["thinking", "text"]);
  });

  it("draws /clear as a system divider between two conversations", () => {
    const messages = structuredChatMessages([
      at(1, { turnId: "t1", type: "userMessage", text: "first" }),
      at(2, { type: "newSession" }),
      at(3, { turnId: "t2", type: "userMessage", text: "second" }),
    ]);
    expect(messages.map((message) => [message.role, message.content])).toEqual([
      ["user", "first"],
      ["system", STRUCTURED_NEW_SESSION_DIVIDER],
      ["user", "second"],
    ]);
  });

  it("keeps a steer inside the turn it landed in, in written order", () => {
    const messages = structuredChatMessages([
      at(1, { turnId: "t1", type: "userMessage", text: "go" }),
      at(2, { turnId: "t1", type: "toolCall", id: "c1", name: "shell", status: "inProgress" }),
      at(3, { turnId: "t1", type: "userMessage", text: "also this" }),
      at(4, { turnId: "t1", type: "assistantMessage", text: "done" }),
    ]);
    expect(messages.map((message) => [message.role, message.content])).toEqual([
      ["user", "go"],
      ["assistant", ""],
      ["user", "also this"],
      ["assistant", "done"],
    ]);
  });

  it("falls back to the last notice or tool summary when an assistant run has no prose", () => {
    const messages = structuredChatMessages([
      at(1, { turnId: "t1", type: "userMessage", text: "go" }),
      at(2, { turnId: "t1", type: "error", message: "provider died" }),
    ]);
    expect(messages[1].content).toBe("provider died");
  });

  it("draws no empty assistant bubble for a run that has nothing to show", () => {
    const messages = structuredChatMessages([
      at(1, { turnId: "t1", type: "userMessage", text: "go" }),
      at(2, { turnId: "t1", type: "approvalRequest", requestId: "r", kind: "command", summary: "s", answered: false }),
    ]);
    expect(messages.map((message) => [message.role, message.content])).toEqual([["user", "go"]]);
    // A tool call is drawable without prose, so its run survives.
    const withTool = structuredChatMessages([
      at(1, { turnId: "t1", type: "userMessage", text: "go" }),
      at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "completed" }),
    ]);
    expect(withTool.map((message) => message.role)).toEqual(["user", "assistant"]);
    expect(withTool[1].content).toBe("");
    expect(withTool[1].referenceParts?.map((part) => part.kind)).toEqual(["tool"]);
  });
});

describe("structured run state", () => {
  it("treats a turn as open until a terminal part lands", () => {
    const open = [
      at(1, { turnId: "t1", type: "userMessage", text: "go" }),
      at(2, { turnId: "t1", type: "assistantMessage", text: "partial" }),
    ];
    expect(structuredChatRunning(open)).toBe(false);
    expect(structuredChatRunning([...open, at(3, { turnId: "t1", type: "toolCall", id: "c", name: "x", status: "inProgress" })])).toBe(true);
    expect(structuredChatActivity([...open, at(3, { turnId: "t1", type: "toolCall", id: "c", name: "x", status: "inProgress" })])).toBe("running_tool");
    expect(
      structuredChatActivity([
        ...open,
        at(3, { turnId: "t1", type: "approvalRequest", requestId: "r", kind: "command", summary: "s", answered: false }),
      ]),
    ).toBe("waiting_for_input");
    expect(structuredChatActivity(open)).toBe("idle");
  });

  it("ends a turn when its usage total lands and never for an empty stream", () => {
    expect(
      structuredChatRunning([
        at(1, { turnId: "t1", type: "userMessage", text: "go" }),
        at(2, { turnId: "t1", type: "usage", totalTokens: 1, inputTokens: 1, cachedInputTokens: 0, outputTokens: 0 }),
      ]),
    ).toBe(false);
    expect(structuredChatRunning([])).toBe(false);
    expect(structuredChatActivity([])).toBe("idle");
  });

  it("ignores parts it cannot read when resolving the tail, so an unknown kind never pins a turn", () => {
    const stream = [
      at(1, { turnId: "t1", type: "userMessage", text: "go" }),
      at(2, { turnId: "t1", type: "assistantMessage", text: "done" }),
      at(3, { turnId: "t1", type: "raw", rawType: "somethingNew", value: {} }),
    ];
    expect(structuredChatRunning(stream)).toBe(false);
    expect(structuredChatActivity(stream)).toBe("idle");
  });

  it("reads the tail across a session divider, because /clear ends the turn it interrupts", () => {
    const stream = [
      at(1, { turnId: "t1", type: "userMessage", text: "go" }),
      at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" }),
      at(3, { type: "newSession" }),
    ];
    expect(structuredChatRunning(stream)).toBe(false);
    expect(structuredChatActivity(stream)).toBe("idle");
  });

  it("keeps a turn open while an extension request waits for its answer", () => {
    const asked = [
      at(1, { turnId: "t1", type: "userMessage", text: "go" }),
      at(2, {
        turnId: "t1",
        type: "extensionUiRequest",
        requestId: "ext-1",
        method: "confirm",
        options: [],
        answered: false,
      }),
    ];
    expect(structuredChatRunning(asked)).toBe(true);
    expect(structuredChatActivity(asked)).toBe("waiting_for_input");
    // An answered request is no longer a stop on the reader. The turn is not held open by the
    // request itself any more - the same rule an answered approval follows - so the stream says
    // what happens next, not this card.
    const answered = [...asked.slice(0, 1), { ...asked[1], answered: true }];
    expect(structuredChatRunning(answered)).toBe(false);
    expect(structuredChatActivity(answered)).toBe("idle");
  });

  it("does not reopen a settled turn when a status heartbeat lands after the answer", () => {
    const settled = [
      at(1, { turnId: "t1", type: "userMessage", text: "go" }),
      at(2, { turnId: "t1", type: "assistantMessage", text: "done" }),
      at(3, { turnId: "t1", type: "extensionStatus", statusKey: "websearch", statusText: "Searching" }),
      at(4, { turnId: "t1", type: "subagentProgress", taskId: "t1", name: "auditor", status: "completed" }),
      at(5, { turnId: "t1", type: "goalUpdate", goalId: "g1", title: "Ship it", status: "completed" }),
      at(6, { turnId: "t1", type: "extensionWidget", widgetKey: "todo", lines: ["done"] }),
      at(7, { turnId: "t1", type: "mediaRef", toolCallId: "c", contentIndex: 0, mimeType: "image/png" }),
    ];
    expect(structuredChatRunning(settled)).toBe(false);
    expect(structuredChatActivity(settled)).toBe("idle");
  });

  it("still reports a tool as running behind a status heartbeat, because the heartbeat says nothing about the turn", () => {
    const stream = [
      at(1, { turnId: "t1", type: "userMessage", text: "go" }),
      at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" }),
      at(3, { turnId: "t1", type: "extensionStatus", statusKey: "websearch", statusText: "Searching" }),
    ];
    expect(structuredChatRunning(stream)).toBe(true);
    expect(structuredChatActivity(stream)).toBe("running_tool");
  });

  it("ends the turn at a rollback divider, because what it interrupted was taken back", () => {
    const stream = [
      at(1, { turnId: "t1", type: "userMessage", text: "go" }),
      at(2, { turnId: "t1", type: "toolCall", id: "c", name: "shell", status: "inProgress" }),
      at(3, { type: "rollback", turns: 1 }),
    ];
    expect(structuredChatRunning(stream)).toBe(false);
    expect(structuredChatActivity(stream)).toBe("idle");
  });
});

describe("structured approvals as the prompt card", () => {
  const approval = {
    requestId: "req-1",
    kind: "command",
    summary: "Run rm -rf build",
    reason: "cleanup",
    seq: 4,
    tsMs: 1000,
  };

  it("maps the three decisions onto the card's options in grant order", () => {
    const prompt = structuredApprovalPrompt(approval);
    expect(prompt.id).toBe("req-1");
    expect(prompt.kind).toBe("approval");
    expect(prompt.question).toBe("Run rm -rf build");
    expect(prompt.body).toBe("cleanup");
    expect(prompt.options).toHaveLength(3);
    expect(prompt.multiSelect).toBe(false);
    expect(structuredApprovalDecisionFor(0)).toBe("accept");
    expect(structuredApprovalDecisionFor(1)).toBe("acceptForSession");
    expect(structuredApprovalDecisionFor(2)).toBe("decline");
    expect(structuredApprovalDecisionFor(3)).toBeNull();
  });
});

describe("structured capabilities as composer controls", () => {
  const capabilities = {
    steer: true,
    rollback: true,
    permissionModes: ["supervised", "full-access"] as const,
    models: [{ id: "gpt-5", label: "GPT-5", efforts: ["low", "high"] }],
    slashCommands: ["/compact"],
  };

  it("lights up models, efforts, permissions and slash commands", () => {
    const composer = structuredComposerCapabilities(
      { ...capabilities, permissionModes: [...capabilities.permissionModes] },
      { model: "gpt-5", permissionMode: "supervised" },
    );
    expect(composer).not.toBeNull();
    expect(composer?.models).toEqual({
      current: "gpt-5",
      choices: [{ id: "gpt-5", label: "GPT-5", efforts: ["low", "high"] }],
      apply: "command",
    });
    expect(composer?.options[0]).toMatchObject({
      id: STRUCTURED_CONTROL_PERMISSION_ID,
      values: ["supervised", "full-access"],
      current: "supervised",
      apply: "command",
    });
    expect(composer?.slashCommands).toEqual([{ name: "/compact", description: null }]);
    expect(composer?.followUp).toEqual({ queue: true, steer: true });
  });

  it("hides the model picker when the agent lists no models and keeps a null capability null", () => {
    expect(
      structuredComposerCapabilities({ ...capabilities, permissionModes: [], models: [] }, {}),
    ).toMatchObject({ models: null, followUp: { queue: true, steer: true } });
    expect(structuredComposerCapabilities(null, {})).toBeNull();
  });

  it("draws no permission picker for an agent that advertises no permission mode", () => {
    const composer = structuredComposerCapabilities({ ...capabilities, permissionModes: [] }, {});
    expect(composer?.options).toEqual([]);
  });

  it("maps a control change to the settings it asks for", () => {
    expect(structuredSettingsForControl({ controlId: "model", valueId: "gpt-5" })).toEqual({ model: "gpt-5" });
    expect(structuredSettingsForControl({ controlId: "effort", valueId: "high" })).toEqual({ effort: "high" });
    expect(
      structuredSettingsForControl({ controlId: STRUCTURED_CONTROL_PERMISSION_ID, valueId: "full-access" }),
    ).toEqual({ permissionMode: "full-access" });
    expect(
      structuredSettingsForControl({ controlId: STRUCTURED_CONTROL_PERMISSION_ID, valueId: "bogus" }),
    ).toBeNull();
    expect(structuredSettingsForControl({ controlId: "unknown", valueId: "x" })).toBeNull();
  });
});
