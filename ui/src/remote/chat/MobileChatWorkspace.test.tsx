import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { MobileChatWorkspace } from "./MobileChatWorkspace";
import type { MobileChatMessageProps } from "./MobileChatMessage";

const mockRemoteTerminal = vi.fn();

vi.mock("../RemoteTerminal", () => ({
  RemoteTerminal: (props: unknown) => {
    mockRemoteTerminal(props);
    return <div data-testid="mock-remote-terminal">Terminal Mock</div>;
  },
}));

const originalScrollIntoView = window.HTMLElement.prototype.scrollIntoView;

describe("MobileChatWorkspace", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.HTMLElement.prototype.scrollIntoView = vi.fn();
  });

  afterEach(() => {
    cleanup();
    window.HTMLElement.prototype.scrollIntoView = originalScrollIntoView;
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

  it("9. surfaces history warnings so a truncated conversation is not silent", () => {
    const { unmount } = render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
        workspaceLabel="ferryx-ui"
        warnings={["older history is not available for paired-host sessions; showing the most recent messages"]}
      />
    );

    const banner = screen.getByTestId("chat-history-warning");
    expect(banner).toHaveTextContent("older history is not available for paired-host sessions");

    unmount();
    render(<MobileChatWorkspace messages={[]} onSendMessage={vi.fn()} workspaceLabel="ferryx-ui" />);
    expect(screen.queryByTestId("chat-history-warning")).toBeNull();
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
        toolCalls: [
          {
            toolName: "bash",
            command: "git status",
            status: "success",
          },
        ],
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
    expect(screen.getByTestId("assistant-message-body")).toHaveTextContent("Summary of the work done.");
    expect(screen.queryByText("git status")).not.toBeInTheDocument();

    fireEvent.click(toggle);
    expect(screen.getByText("git status")).toBeInTheDocument();
    expect(screen.getByTestId("assistant-message-body")).toHaveTextContent("Summary of the work done.");
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

  it("6. renders composer when quickActions are absent", () => {
    const handleSend = vi.fn();

    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={handleSend}
      />
    );

    expect(screen.getByTestId("chat-composer-textarea")).toBeInTheDocument();
    expect(screen.queryByTestId("mobile-chat-quick-actions")).not.toBeInTheDocument();
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

  it("11. header degrades safely when optional header props are absent", () => {
    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
      />
    );

    expect(screen.queryByTestId("thread-header-back")).toBeNull();
    expect(screen.queryByTestId("thread-header-action-terminal")).toBeNull();
    expect(screen.getByText("Agent Workspace")).toBeInTheDocument();
    expect(screen.getByTestId("mobile-chat-header")).toBeInTheDocument();
    expect(screen.getByTestId("chat-composer-textarea")).toBeInTheDocument();
    expect(screen.getByTestId("chat-empty-state")).toBeInTheDocument();
  });

  it("12. sizes the drawer to this device instead of the desktop grid", () => {
    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
        sessionId="sess-follow-1"
        token="tok-follow-1"
        transportUrl="http://localhost:3000"
      />
    );

    const toggleButton = screen.getByTestId("terminal-toggle-button");
    fireEvent.click(toggleButton);
    expect(screen.getByTestId("mock-remote-terminal")).toBeInTheDocument();
    expect(mockRemoteTerminal).toHaveBeenCalledWith(
      expect.objectContaining({
        sessionId: "sess-follow-1",
        token: "tok-follow-1",
      })
    );
    // Opening the drawer makes this device the size owner.
    expect(mockRemoteTerminal.mock.calls.at(-1)?.[0]).not.toMatchObject({ followHostSize: true });
  });

  it("13. closed drawer carries inert attribute and aria-hidden true", () => {
    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
        sessionId="sess-closed-1"
        token="tok-closed-1"
        transportUrl="http://localhost:3000"
      />
    );

    const drawer = screen.getByTestId("terminal-drawer");
    expect(drawer).toHaveAttribute("aria-hidden", "true");
    expect(drawer).toHaveAttribute("inert");
  });

  it("shows server-issued result files with the agreed open controls", () => {
    const onOpen = vi.fn();
    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
        resultFiles={[{ fileId: "result-123", displayName: "step_1.txt" }]}
        onOpenResultFile={onOpen}
      />
    );
    expect(screen.getByTestId("chat-result-files")).toHaveTextContent("step_1.txt");
    fireEvent.click(screen.getByTestId("result-file-open-result-123"));
    expect(onOpen).toHaveBeenCalledWith("result-123");
  });
});
