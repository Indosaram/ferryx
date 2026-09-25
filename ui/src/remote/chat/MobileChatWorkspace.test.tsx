import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { MobileChatWorkspace } from "./MobileChatWorkspace";
import type { MobileChatMessageProps } from "./MobileChatMessage";

vi.mock("../RemoteTerminal", () => ({
  RemoteTerminal: () => <div data-testid="mock-remote-terminal">Terminal Mock</div>,
}));

window.HTMLElement.prototype.scrollIntoView = vi.fn();

describe("MobileChatWorkspace", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("1. renders a quiet empty state with context line and no starter prompts", () => {
    const handleSend = vi.fn();

    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={handleSend}
        workspaceLabel="ferryx-ui"
        worktreeLabel="main"
      />
    );

    expect(screen.getByTestId("chat-empty-state")).toBeInTheDocument();
    expect(screen.getByTestId("chat-empty-context")).toHaveTextContent("ferryx-ui · main");
    expect(screen.getByText("Prompts run against the focused terminal.")).toBeInTheDocument();
    expect(screen.queryByText("How can I help you today?")).not.toBeInTheDocument();
    expect(screen.queryByTestId(/starter-prompt-/)).not.toBeInTheDocument();
  });

  it("2. header shows monospace workspace · worktree subtitle when provided", () => {
    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
        workspaceLabel="ferryx-ui"
        worktreeLabel="main"
      />
    );

    expect(screen.getByTestId("chat-header-subtitle")).toHaveTextContent("ferryx-ui · main");
  });

  it("3. assistant turn with durationLabel collapses behind a Worked for row", () => {
    const messages: MobileChatMessageProps[] = [
      {
        id: "msg-1",
        role: "assistant",
        content: "Summary of the work done.",
        timestamp: Date.now(),
        durationLabel: "2m",
      },
    ];

    render(
      <MobileChatWorkspace
        messages={messages}
        onSendMessage={vi.fn()}
      />
    );

    const toggle = screen.getByTestId("worked-for-toggle");
    expect(toggle).toHaveTextContent("Worked for 2m");
    expect(screen.queryByText("Summary of the work done.")).not.toBeInTheDocument();

    fireEvent.click(toggle);
    expect(screen.getByText("Summary of the work done.")).toBeInTheDocument();
  });

  it("4. renders message list history", () => {
    cleanup();
    const messages: MobileChatMessageProps[] = [
      {
        id: "msg-1",
        role: "user",
        content: "What is the git status?",
        timestamp: Date.now(),
      },
      {
        id: "msg-2",
        role: "assistant",
        content: "All clean and up to date.",
        timestamp: Date.now() + 1000,
      },
    ];

    render(
      <MobileChatWorkspace
        messages={messages}
        onSendMessage={vi.fn()}
      />
    );

    expect(screen.queryByTestId("chat-empty-state")).not.toBeInTheDocument();
    expect(screen.getByText("What is the git status?")).toBeInTheDocument();
    expect(screen.getByText("All clean and up to date.")).toBeInTheDocument();
  });

  it("5. sending messages via onSendMessage", () => {
    const handleSend = vi.fn();
    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={handleSend}
      />
    );

    const textarea = screen.getByTestId("chat-composer-textarea") as HTMLTextAreaElement;
    const sendButton = screen.getByTestId("send-button");

    fireEvent.change(textarea, { target: { value: "   " } });
    fireEvent.click(sendButton);
    expect(handleSend).not.toHaveBeenCalled();

    fireEvent.change(textarea, { target: { value: "Please review my PR" } });
    expect(textarea.value).toBe("Please review my PR");

    fireEvent.click(sendButton);
    expect(handleSend).toHaveBeenCalledTimes(1);
    expect(handleSend).toHaveBeenCalledWith("Please review my PR", []);
    expect(textarea.value).toBe("");
  });

  it("6. quick action chip clicks trigger action or populate input", () => {
    const handleSend = vi.fn();
    const handleSelectQuickAction = vi.fn();

    const quickActions = [
      {
        id: "git-diff",
        label: "Git Diff",
        prompt: "Show the current git diff",
      },
      {
        id: "run-tests",
        label: "Run Tests",
        prompt: "Run all test suites",
      },
    ];

    const { rerender } = render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={handleSend}
        quickActions={quickActions}
        onSelectQuickAction={handleSelectQuickAction}
      />
    );

    expect(screen.getByText("Git Diff")).toBeInTheDocument();
    expect(screen.getByText("Run Tests")).toBeInTheDocument();

    fireEvent.click(screen.getByText("Git Diff"));
    expect(handleSelectQuickAction).toHaveBeenCalledWith(quickActions[0]);

    rerender(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={handleSend}
        quickActions={quickActions}
      />
    );

    const textarea = screen.getByTestId("chat-composer-textarea") as HTMLTextAreaElement;
    fireEvent.click(screen.getByText("Run Tests"));
    expect(textarea.value).toBe("Run all test suites");
  });

  it("7. terminal is only mounted when drawer is opened and unmounted when closed", () => {
    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
        sessionId="sess-123"
        token="tok-456"
      />
    );

    const toggleButton = screen.getByTestId("terminal-toggle-button");
    expect(screen.queryByTestId("mock-remote-terminal")).not.toBeInTheDocument();

    fireEvent.click(toggleButton);
    expect(screen.getByTestId("mock-remote-terminal")).toBeInTheDocument();

    const closeButton = screen.getByTestId("terminal-close-button");
    fireEvent.click(closeButton);
    expect(screen.queryByTestId("mock-remote-terminal")).not.toBeInTheDocument();
  });

  it("8. scroll pill updates bottom state and triggers scroll", () => {
    const messages: MobileChatMessageProps[] = Array.from({ length: 20 }, (_, idx) => ({
      id: `msg-${idx}`,
      role: idx % 2 === 0 ? "user" : "assistant",
      content: `Message content ${idx}`,
      timestamp: Date.now() + idx * 1000,
    }));

    render(
      <MobileChatWorkspace
        messages={messages}
        onSendMessage={vi.fn()}
      />
    );

    const stream = screen.getByTestId("chat-message-stream");
    Object.defineProperty(stream, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(stream, "clientHeight", { value: 300, configurable: true });
    Object.defineProperty(stream, "scrollTop", { value: 100, configurable: true, writable: true });

    fireEvent.scroll(stream);

    const scrollPill = screen.getByTestId("scroll-to-latest-pill");
    expect(scrollPill).toBeInTheDocument();

    fireEvent.click(scrollPill);
    expect(window.HTMLElement.prototype.scrollIntoView).toHaveBeenCalled();
  });

  it("9. onStopExecution triggers stop when execution is running", () => {
    const handleStop = vi.fn();
    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
        onStopExecution={handleStop}
        isRunning={true}
      />
    );

    const stopButton = screen.getByTestId("stop-button");
    expect(stopButton).toBeInTheDocument();
    fireEvent.click(stopButton);
    expect(handleStop).toHaveBeenCalledTimes(1);
  });

  it("10. supports terminal WebSocket transport props integration", () => {
    const mockCreateWebSocket = vi.fn().mockImplementation((path: string) => ({
      send: vi.fn(),
      close: vi.fn(),
      readyState: 1,
      onopen: null,
      onmessage: null,
      onerror: null,
      onclose: null,
    }));

    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
        sessionId="sess-ws-1"
        token="tok-ws-1"
        transportUrl="http://localhost:3000"
        isAccountSession={true}
        createWebSocket={mockCreateWebSocket}
      />
    );

    const toggleButton = screen.getByTestId("terminal-toggle-button");
    fireEvent.click(toggleButton);
    expect(screen.getByTestId("mock-remote-terminal")).toBeInTheDocument();
  });
});
