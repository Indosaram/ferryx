/**
 * Zen and failed legacy tool calls.
 *
 * The renderer's contract (t3code docs/user/activity-log.md, and G4's own `zenErrorParts`) is that
 * approvals, questions and errors always show in every mode. The workspace pre-applies the mode
 * to the legacy `toolCalls` path (`applyToolCallVisibility`), so that function has to keep a failed
 * call instead of dropping the whole array: the renderer's error-preserving branch can only act on
 * rows it still receives.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { MobileChatWorkspace, applyToolCallVisibility } from "./MobileChatWorkspace";
import type { MobileChatMessageProps } from "./MobileChatMessage";

window.HTMLElement.prototype.scrollIntoView = vi.fn();

afterEach(() => {
  cleanup();
});

describe("zen and failed legacy tool calls", () => {
  it("keeps a failed legacy tool call, because errors always show in every mode", () => {
    const failed = { kind: "tool" as const, status: "error" as const, toolName: "bash", summary: "FAILED_TOOL_MARKER" };
    expect(applyToolCallVisibility("zen", [failed])).toEqual([failed]);
    // `hide-all` carries the same promise: "Approvals, questions, and errors always show."
    expect(applyToolCallVisibility("hide-all", [failed])).toEqual([failed]);
  });

  it("renders the failed legacy tool card on a zen final reply", () => {
    const messages: MobileChatMessageProps[] = [
      { id: "u1", role: "user", content: "run it", timestamp: 1 },
      {
        id: "a1",
        role: "assistant",
        content: "done",
        timestamp: 2,
        toolCalls: [{ kind: "tool", status: "error", toolName: "bash", summary: "FAILED_TOOL_MARKER" }],
      },
    ];
    render(
      <MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} toolCallVisibility="zen" />,
    );
    expect(screen.getByText("FAILED_TOOL_MARKER")).toBeInTheDocument();
  });

  it("keeps a turn's prose answer when a tool-only message trails it in zen", () => {
    // `finalReplyIds` marks the last assistant message of a turn that has prose. A trailing
    // tool-only message must not take the mark: if it did, the real answer would be treated as
    // in-between and hidden, and the surviving message would have no prose to show.
    const messages: MobileChatMessageProps[] = [
      { id: "u1", role: "user", content: "explain the failure", timestamp: 1 },
      { id: "a1", role: "assistant", content: "THE_ACTUAL_ANSWER", timestamp: 2 },
      {
        id: "a2",
        role: "assistant",
        content: "",
        timestamp: 3,
        toolCalls: [{ kind: "tool", status: "success", toolName: "bash", summary: "trailing work" }],
      },
      { id: "u2", role: "user", content: "thanks", timestamp: 4 },
    ];
    render(
      <MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} toolCallVisibility="zen" />,
    );
    expect(screen.getByText("THE_ACTUAL_ANSWER")).toBeInTheDocument();
  });
});
