import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MobileChatMessage } from "../MobileChatMessage";
import { ThreadFindProvider, useThreadFind, useThreadFindHotkey } from "./threadFind";
import type { ReferencePart } from "../referenceTypes";

afterEach(cleanup);

/** The list pane mounts the hotkey beside the provider; the test mirrors that wiring. */
function HotkeyMount() {
  useThreadFindHotkey(useThreadFind());
  return null;
}

const tool = (overrides: Partial<Extract<ReferencePart, { kind: "tool" }>> = {}): ReferencePart => ({
  kind: "tool",
  name: "bash",
  summary: "",
  input: "ls",
  output: "files",
  status: "completed",
  id: "t",
  ...overrides,
});

describe("tool calls visibility on a turn", () => {
  it("hide-successful keeps a running call and drops the completed one", () => {
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content="done"
        referenceParts={[tool({ status: "running", id: "r" }), tool()]}
        toolCallVisibility="hide-successful"
      />,
    );
    expect(screen.getByTestId("reference-tool-part")).toHaveTextContent("ls");
    expect(screen.getAllByTestId("reference-tool-part")).toHaveLength(1);
  });

  it("hide-all drops every call, keeping prose", () => {
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content="The answer."
        referenceParts={[tool(), { kind: "text", text: "The answer.", id: "x" }]}
        toolCallVisibility="hide-all"
      />,
    );
    expect(screen.queryByTestId("reference-tool-part")).not.toBeInTheDocument();
    expect(screen.getByTestId("assistant-reference-body")).toHaveTextContent("The answer.");
  });

  it("zen keeps the user message and the final reply, and always shows a failed call", () => {
    const parts = [tool(), { kind: "text", text: "final reply", id: "f" } as ReferencePart];
    const { rerender } = render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        referenceParts={parts}
        toolCallVisibility="zen"
        isTurnFinalReply={true}
      />,
    );
    expect(screen.getByTestId("assistant-reference-body")).toHaveTextContent("final reply");
    expect(screen.queryByTestId("reference-tool-part")).not.toBeInTheDocument();

    rerender(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        referenceParts={[tool({ status: "failed", error: true, id: "bad" })]}
        toolCallVisibility="zen"
        isTurnFinalReply={false}
      />,
    );
    // An in-between reply hides its prose and its successful work, but errors always show.
    expect(screen.getByTestId("reference-tool-part")).toHaveTextContent("bash");
    expect(screen.queryByTestId("assistant-reference-body")).not.toBeInTheDocument();
  });

  it("zen keeps the reply the transcript phases as the final answer without a caller flag", () => {
    render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        referenceParts={[tool(), { kind: "text", text: "closing reply", phase: "finalAnswer", id: "f" }]}
        toolCallVisibility="zen"
      />,
    );
    // activity-log.md: zen shows "the agent's last reply, which appears when the turn ends".
    expect(screen.getByTestId("assistant-reference-body")).toHaveTextContent("closing reply");
  });

  it("zen hides commentary prose and keeps a reply the transcript marks as the answer", () => {
    const { rerender } = render(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        referenceParts={[{ kind: "text", text: "steered answer", phase: "finalAnswer", id: "a" }]}
        toolCallVisibility="zen"
      />,
    );
    expect(screen.getByTestId("assistant-reference-body")).toHaveTextContent("steered answer");

    rerender(
      <MobileChatMessage
        id="m"
        role="assistant"
        content=""
        referenceParts={[{ kind: "text", text: "thinking out loud", phase: "commentary", id: "c" }]}
        toolCallVisibility="zen"
      />,
    );
    expect(screen.queryByTestId("assistant-reference-body")).not.toBeInTheDocument();
  });

  it("a user message is never hidden in zen", () => {
    render(
      <MobileChatMessage
        id="m"
        role="user"
        content="please fix the parser"
        referenceParts={[{ kind: "text", text: "please fix the parser", id: "u" }]}
        toolCallVisibility="zen"
      />,
    );
    expect(screen.getByTestId("user-message-bubble")).toHaveTextContent("please fix the parser");
  });
});

describe("find shows zen-hidden messages", () => {
  it("renders an in-between reply while find is open", async () => {
    render(
      <ThreadFindProvider>
        <HotkeyMount />
        <MobileChatMessage
          id="m"
          role="assistant"
          content=""
          referenceParts={[
            tool(),
            { kind: "text", text: "in-between note", id: "i" } as ReferencePart,
          ]}
          toolCallVisibility="zen"
          isTurnFinalReply={false}
        />
      </ThreadFindProvider>,
    );
    // The bar is closed, so the in-between reply is zen-hidden.
    expect(screen.queryByTestId("assistant-reference-body")).not.toBeInTheDocument();
    fireEvent.keyDown(window, { key: "f", ctrlKey: true });
    const input = screen.getByTestId("thread-find-input");
    await act(async () => {
      fireEvent.change(input, { target: { value: "in-between" } });
    });
    // Find overrides zen: the hidden reply shows again, and its prose is counted.
    expect(screen.getByTestId("assistant-reference-body")).toHaveTextContent("in-between note");
    expect(screen.getByTestId("thread-find-count")).toHaveTextContent("1/1");
  });
});
