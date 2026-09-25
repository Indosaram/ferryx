import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import {
  ActivityIndicator,
  AttachmentList,
  ToolCallCard,
  ApprovalActionCard,
} from "./MobileChatComponents";
import { MobileChatMessage } from "./MobileChatMessage";

describe("MobileChatComponents & MobileChatMessage", () => {
  afterEach(() => {
    cleanup();
  });

  it("renders ActivityIndicator for different states", () => {
    const { container: thinking } = render(<ActivityIndicator state="thinking" />);
    expect(thinking.textContent).toContain("Thinking...");

    const { container: runningTool } = render(<ActivityIndicator state="running_tool" label="Executing" />);
    expect(runningTool.textContent).toContain("Executing");

    const { container: waiting } = render(<ActivityIndicator state="waiting_for_input" />);
    expect(waiting.textContent).toContain("Waiting for input...");
  });

  it("renders AttachmentList images and files", () => {
    const attachments = [
      { id: "1", name: "diagram.png", type: "image" as const, url: "https://example.com/img.png" },
      { id: "2", name: "log.txt", type: "file" as const, size: "12 KB" },
    ];
    const { container } = render(<AttachmentList attachments={attachments} />);
    expect(container.textContent).toContain("diagram.png");
    expect(container.textContent).toContain("log.txt");
    expect(container.textContent).toContain("12 KB");
  });

  it("renders ToolCallCard with command and status", () => {
    const { container } = render(
      <ToolCallCard
        toolName="bash"
        command="cargo test"
        output="test passed"
        status="success"
        durationMs={450}
        initiallyExpanded={true}
      />
    );
    expect(container.textContent).toContain("bash");
    expect(container.textContent).toContain("cargo test");
    expect(container.textContent).toContain("test passed");
    expect(container.textContent).toContain("Completed");
    expect(container.textContent).toContain("450ms");
  });

  it("renders ApprovalActionCard", () => {
    const { container } = render(
      <ApprovalActionCard
        title="Allow Command"
        description="Proceed with running rm -rf?"
        onAccept={() => {}}
        onDecline={() => {}}
      />
    );
    expect(container.textContent).toContain("Allow Command");
    expect(container.textContent).toContain("Proceed with running rm -rf?");
    expect(container.textContent).toContain("Approve");
    expect(container.textContent).toContain("Decline");
  });

  it("renders user MobileChatMessage as a right-aligned accent bubble", () => {
    const { container } = render(
      <MobileChatMessage
        id="msg-1"
        role="user"
        content="Please run the tests"
        timestamp={1700000000000}
      />
    );
    expect(container.textContent).toContain("Please run the tests");
    const bubble = container.querySelector("[data-testid='user-message-bubble']");
    expect(bubble).not.toBeNull();
    expect(bubble?.className).toContain("bg-primary");
    expect(bubble?.parentElement?.className).toContain("ml-auto");
  });

  it("renders assistant MobileChatMessage as plain prose without avatar or bubble", () => {
    const { container } = render(
      <MobileChatMessage
        id="msg-2"
        role="assistant"
        content="Here is the result: `done`"
        activityState="running_tool"
        toolCalls={[
          {
            toolName: "test_runner",
            status: "running",
            command: "bun test",
          },
        ]}
      />
    );
    expect(container.textContent).toContain("Here is the result");
    expect(container.textContent).toContain("test_runner");
    const body = container.querySelector("[data-testid='assistant-message-body']");
    expect(body).not.toBeNull();
    expect(body?.className).not.toContain("rounded-2xl");
    expect(body?.className).not.toContain("border");
    expect(container.querySelector("img")).toBeNull();
  });

  it("renders collapsible Worked for row for assistant turns with durationLabel", () => {
    render(
      <MobileChatMessage
        id="msg-6"
        role="assistant"
        content="Summarized the diff."
        durationLabel="2m"
      />
    );

    const toggle = screen.getByTestId("worked-for-toggle");
    expect(toggle).toHaveTextContent("Worked for 2m");
    expect(screen.queryByText("Summarized the diff.")).not.toBeInTheDocument();

    fireEvent.click(toggle);
    expect(screen.getByText("Summarized the diff.")).toBeInTheDocument();
  });

  it("renders a copy button under each turn", () => {
    render(
      <MobileChatMessage
        id="msg-7"
        role="assistant"
        content="Copy me"
        timestamp={1700000000000}
      />
    );
    expect(screen.getByTestId("message-copy-button")).toBeInTheDocument();
  });

  it("copies message content via the copy button", () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    render(
      <MobileChatMessage
        id="msg-8"
        role="assistant"
        content="Copy me"
      />
    );
    fireEvent.click(screen.getByTestId("message-copy-button"));
    expect(writeText).toHaveBeenCalledWith("Copy me");
  });

  it("renders fenced code block with CodeBlock component even if single line", () => {
    const { container } = render(
      <MobileChatMessage
        id="msg-3"
        role="assistant"
        content={"```ts\nconst x = 1;\n```"}
      />
    );
    expect(container.textContent).toContain("ts");
    expect(container.textContent).toContain("const x = 1;");
    expect(container.textContent).toContain("Copy");
  });

  it("renders pure inline code with InlineCode component", () => {
    const { container } = render(
      <MobileChatMessage
        id="msg-4"
        role="assistant"
        content="Run `cargo check` to verify."
      />
    );
    expect(container.textContent).toContain("cargo check");
    const codeEl = container.querySelector("code");
    expect(codeEl?.className).toContain("text-sky-300");
  });

  it("renders fenced code inside markdown without validateDOMNesting warning", () => {
    const consoleError = vi.spyOn(console, "error");
    render(
      <MobileChatMessage
        id="msg-5"
        role="assistant"
        content={"Paragraph before\n```js\nconsole.log('hi');\n```\nParagraph after"}
      />
    );
    const nestingWarning = consoleError.mock.calls.find((call: any[]) =>
      call.some((arg: any) => typeof arg === "string" && arg.includes("validateDOMNesting"))
    );
    expect(nestingWarning).toBeUndefined();
    consoleError.mockRestore();
  });
});
