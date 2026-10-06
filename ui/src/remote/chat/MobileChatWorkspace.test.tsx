/**
 * Workspace presentation regressions (plan task 12).
 *
 * AUTHORED, NOT EXECUTED: the run is deferred to the post-merge gate.
 *
 * The workspace no longer mounts a terminal: chat is the default at every width and the
 * terminal is an explicit mode the owner switches to, so the drawer scenarios this suite used
 * to drive are replaced by the contract that exists now (an explicit action, a page-level
 * disclosure, older-page control and rich reference parts reaching the message renderer).
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { MobileChatWorkspace } from "./MobileChatWorkspace";
import type { MobileChatMessageProps } from "./MobileChatMessage";

window.HTMLElement.prototype.scrollIntoView = vi.fn();

describe("MobileChatWorkspace", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("1. renders a quiet empty state with context line and no starter prompts", () => {
    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
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
      <MobileChatWorkspace messages={[]} onSendMessage={vi.fn()} workspaceLabel="ferryx-ui" worktreeLabel="main" />
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
        toolCalls: [{ toolName: "bash", command: "git status", status: "success" }],
      },
    ];
    render(<MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} />);

    const toggle = screen.getByTestId("worked-for-toggle");
    expect(toggle).toHaveTextContent("Worked for 2m");
    expect(screen.getByTestId("assistant-message-body")).toHaveTextContent("Summary of the work done.");
    expect(screen.queryByText("git status")).not.toBeInTheDocument();

    fireEvent.click(toggle);
    expect(screen.getByText("git status")).toBeInTheDocument();
  });

  it("4. renders message list history", () => {
    const messages: MobileChatMessageProps[] = [
      { id: "msg-1", role: "user", content: "What is the git status?", timestamp: Date.now() },
      { id: "msg-2", role: "assistant", content: "All clean and up to date.", timestamp: Date.now() + 1000 },
    ];
    render(<MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} />);

    expect(screen.queryByTestId("chat-empty-state")).not.toBeInTheDocument();
    expect(screen.getByText("What is the git status?")).toBeInTheDocument();
    expect(screen.getByText("All clean and up to date.")).toBeInTheDocument();
  });

  it("5. sends the draft through onSendMessage with the text alone", () => {
    const handleSend = vi.fn();
    render(<MobileChatWorkspace messages={[]} onSendMessage={handleSend} />);

    const textarea = screen.getByTestId("chat-composer-textarea") as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: "   " } });
    fireEvent.click(screen.getByTestId("send-button"));
    expect(handleSend).not.toHaveBeenCalled();

    fireEvent.change(textarea, { target: { value: "Please review my PR" } });
    fireEvent.click(screen.getByTestId("send-button"));
    expect(handleSend).toHaveBeenCalledTimes(1);
    expect(handleSend).toHaveBeenCalledWith("Please review my PR");
    expect(textarea.value).toBe("");
  });

  it("6. renders composer when quickActions are absent", () => {
    render(<MobileChatWorkspace messages={[]} onSendMessage={vi.fn()} />);
    expect(screen.getByTestId("chat-composer-textarea")).toBeInTheDocument();
    expect(screen.queryByTestId("mobile-chat-quick-actions")).not.toBeInTheDocument();
  });

  it("7. mounts no terminal of its own: the header offers an explicit action instead", () => {
    const onOpenTerminal = vi.fn();
    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
        onOpenTerminal={onOpenTerminal}
      />
    );

    // there is no nested drawer, so no second owner of this session and no terminal DOM here
    expect(screen.queryByTestId("terminal-drawer")).not.toBeInTheDocument();
    expect(screen.queryByTestId("terminal-toggle-button")).not.toBeInTheDocument();
    const open = screen.getByTestId("open-terminal-button");
    fireEvent.click(open);
    expect(onOpenTerminal).toHaveBeenCalledTimes(1);
  });

  it("8. scroll pill updates bottom state and triggers scroll", () => {
    const messages: MobileChatMessageProps[] = Array.from({ length: 20 }, (_, idx) => ({
      id: `msg-${idx}`,
      role: idx % 2 === 0 ? "user" : "assistant",
      content: `Message content ${idx}`,
      timestamp: Date.now() + idx * 1000,
    }));
    render(<MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} />);

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

  it("9. onStopExecution triggers stop when the pane is running", () => {
    const handleStop = vi.fn();
    render(<MobileChatWorkspace messages={[]} onSendMessage={vi.fn()} onStopExecution={handleStop} isRunning={true} />);

    const stopButton = screen.getByTestId("stop-button");
    fireEvent.click(stopButton);
    expect(handleStop).toHaveBeenCalledTimes(1);
  });

  it("9b. surfaces history warnings so a truncated conversation is not silent", () => {
    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
        warnings={["This session's transcript is not available to this device."]}
      />
    );
    expect(screen.getByTestId("chat-history-warning")).toHaveTextContent("not available");
  });

  it("10. header degrades safely when optional header props are absent", () => {
    render(<MobileChatWorkspace messages={[]} onSendMessage={vi.fn()} />);

    expect(screen.queryByTestId("thread-header-back")).toBeNull();
    expect(screen.queryByTestId("open-terminal-button")).toBeNull();
    expect(screen.getByText("Agent Workspace")).toBeInTheDocument();
    expect(screen.getByTestId("mobile-chat-header")).toBeInTheDocument();
    expect(screen.getByTestId("chat-composer-textarea")).toBeInTheDocument();
    expect(screen.getByTestId("chat-empty-state")).toBeInTheDocument();
  });

  it("11. discloses a non-native page once, above the transcript", () => {
    render(
      <MobileChatWorkspace
        messages={[{ id: "t1", role: "assistant", content: "same-pane output", timestamp: Date.now() }]}
        onSendMessage={vi.fn()}
        pageDisclosure="Conversation unavailable — show terminal output"
      />
    );
    const disclosures = screen.getAllByTestId("reference-disclosure");
    expect(disclosures).toHaveLength(1);
    expect(disclosures[0]).toHaveTextContent(/show terminal output/i);
  });

  it("12. composes abandoned branches once for the page, never once per turn", () => {
    const messages: MobileChatMessageProps[] = [
      {
        id: "t1",
        role: "assistant",
        content: "kept one",
        timestamp: Date.now(),
        referenceAbandoned: { count: 1, branches: 1 },
      },
      {
        id: "t2",
        role: "assistant",
        content: "kept two",
        timestamp: Date.now(),
        referenceAbandoned: { count: 2, branches: 2 },
      },
    ];
    render(<MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} />);

    const abandoned = screen.getAllByTestId("reference-abandoned");
    expect(abandoned).toHaveLength(1);
    expect(abandoned[0]).toHaveTextContent(/3 earlier turns on 3 branches/);
  });

  it("13. offers the older page only when there is one, and its endcap once loaded", () => {
    const { rerender } = render(<MobileChatWorkspace messages={[]} onSendMessage={vi.fn()} hasOlderPage={false} />);
    expect(screen.queryByTestId("reference-older-button")).not.toBeInTheDocument();

    const onLoadOlder = vi.fn();
    rerender(
      <MobileChatWorkspace messages={[]} onSendMessage={vi.fn()} hasOlderPage={true} onLoadOlder={onLoadOlder} />
    );
    fireEvent.click(screen.getByTestId("reference-older-button"));
    expect(onLoadOlder).toHaveBeenCalledTimes(1);

    rerender(
      <MobileChatWorkspace messages={[]} onSendMessage={vi.fn()} hasOlderPage={false} loadedOlder={true} />
    );
    expect(screen.getByTestId("reference-older-endcap")).toBeInTheDocument();
  });

  it("14. passes rich reference parts through to the message renderer", () => {
    const messages: MobileChatMessageProps[] = [
      {
        id: "t1",
        role: "assistant",
        content: "",
        timestamp: Date.now(),
        referenceParts: [{ kind: "text", text: "rich turn body" }],
      },
    ];
    render(<MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} />);

    expect(screen.getByTestId("assistant-reference-body")).toHaveTextContent("rich turn body");
  });

  it("15. draws the prompt card above the message box", () => {
    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
        promptCard={<div data-testid="stub-prompt-card">waiting</div>}
      />
    );
    expect(screen.getByTestId("chat-composer-prompt")).toHaveTextContent("waiting");
  });

  it("16. keeps the reader's place when an older page lands above them", () => {
    const firstPage: MobileChatMessageProps[] = Array.from({ length: 4 }, (_, idx) => ({
      id: `new-${idx}`,
      role: idx % 2 === 0 ? "user" : "assistant",
      content: `Newest ${idx}`,
      timestamp: Date.now() + idx,
    }));
    const { rerender } = render(<MobileChatWorkspace messages={firstPage} onSendMessage={vi.fn()} />);

    const stream = screen.getByTestId("chat-message-stream");
    Object.defineProperty(stream, "clientHeight", { value: 300, configurable: true });
    Object.defineProperty(stream, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(stream, "scrollTop", { value: 100, configurable: true, writable: true });

    // the reader scrolls away from the bottom, which is what records their place
    fireEvent.scroll(stream);
    expect(stream.scrollTop).toBe(100);

    // an older page is prepended: the content above grows by 200px
    const older: MobileChatMessageProps[] = Array.from({ length: 2 }, (_, idx) => ({
      id: `old-${idx}`,
      role: idx % 2 === 0 ? "user" : "assistant",
      content: `Older ${idx}`,
      timestamp: Date.now() - 1000 + idx,
    }));
    Object.defineProperty(stream, "scrollHeight", { value: 1200, configurable: true });
    rerender(<MobileChatWorkspace messages={[...older, ...firstPage]} onSendMessage={vi.fn()} />);

    // the same content stays under the reader's eye instead of jumping down by the inserted height
    expect(stream.scrollTop).toBe(300);
  });

  it("17. follows the bottom when the reader was already at the bottom", () => {
    const firstPage: MobileChatMessageProps[] = [{ id: "new-0", role: "assistant", content: "First", timestamp: Date.now() }];
    const { rerender } = render(<MobileChatWorkspace messages={firstPage} onSendMessage={vi.fn()} />);

    const stream = screen.getByTestId("chat-message-stream");
    Object.defineProperty(stream, "clientHeight", { value: 300, configurable: true });
    Object.defineProperty(stream, "scrollHeight", { value: 300, configurable: true });
    Object.defineProperty(stream, "scrollTop", { value: 0, configurable: true, writable: true });
    fireEvent.scroll(stream);

    Object.defineProperty(stream, "scrollHeight", { value: 600, configurable: true });
    rerender(
      <MobileChatWorkspace
        messages={[...firstPage, { id: "new-1", role: "assistant", content: "Second", timestamp: Date.now() }]}
        onSendMessage={vi.fn()}
      />
    );

    // at the bottom the view follows the newest turn rather than being pinned to a stale offset
    expect(window.HTMLElement.prototype.scrollIntoView).toHaveBeenCalled();
  });
});

