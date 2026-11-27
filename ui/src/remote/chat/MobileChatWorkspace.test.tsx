/**
 * Workspace presentation regressions (plan task 12).
 *
 *
 * The workspace no longer mounts a terminal: chat is the default at every width and the
 * terminal is an explicit mode the owner switches to, so the drawer scenarios this suite used
 * to drive are replaced by the contract that exists now (an explicit action, a page-level
 * disclosure, older-page control and rich reference parts reaching the message renderer).
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { MobileChatWorkspace, chatStarterPrompts } from "./MobileChatWorkspace";
import type { MobileChatMessageProps } from "./MobileChatMessage";

window.HTMLElement.prototype.scrollIntoView = vi.fn();

describe("MobileChatWorkspace", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("switching the owner replaces both optimistic messages and the controlled draft", () => {
    const { rerender } = render(<MobileChatWorkspace
      messages={[{ id: "pending-a", role: "user", content: "OPTIMISTIC_A_MARKER", timestamp: 1 }]}
      draft="A_DRAFT_MARKER" onSendMessage={vi.fn()}
    />);
    expect(screen.getByText("OPTIMISTIC_A_MARKER")).toBeInTheDocument();
    rerender(<MobileChatWorkspace messages={[]} draft="B_DRAFT_MARKER" onSendMessage={vi.fn()} />);
    expect(screen.queryByText("OPTIMISTIC_A_MARKER")).not.toBeInTheDocument();
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("B_DRAFT_MARKER");
  });

  it("forwards advertised queue and steer actions and a cancel before delivery", () => {
    const onQueueMessage = vi.fn(() => true);
    const onSteerMessage = vi.fn();
    const onCancelQueued = vi.fn();
    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
        isRunning
        draft="follow up"
        composerCapabilities={{ models: null, options: [], slashCommands: [], followUp: { queue: true, steer: false } }}
        queuedMessages={[{ id: "q1", text: "waiting", ownerId: "o", epoch: "1", phase: "queued", requestId: null }]}
        onQueueMessage={onQueueMessage}
        onSteerMessage={onSteerMessage}
        onCancelQueued={onCancelQueued}
      />,
    );
    expect(screen.queryByTestId("steer-button")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("queue-follow-up-button"));
    expect(onQueueMessage).toHaveBeenCalledWith("follow up");
    fireEvent.click(screen.getByTestId("queued-follow-up-cancel-q1"));
    expect(onCancelQueued).toHaveBeenCalledWith("q1");
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
    expect(screen.getByText("Messages go to the agent in this session's terminal.")).toBeInTheDocument();
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

  it("announces a finished assistant turn through a polite live region", () => {
    const messages: MobileChatMessageProps[] = [
      { id: "u", role: "user", content: "go", timestamp: 1 },
      { id: "a", role: "assistant", content: "finished", timestamp: 2 },
    ];
    const { rerender } = render(<MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} isRunning />);
    const announcer = screen.getByTestId("chat-turn-announcer");
    expect(announcer).toHaveAttribute("aria-live", "polite");
    expect(announcer).toBeEmptyDOMElement();
    rerender(<MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} isRunning={false} />);
    expect(screen.getByTestId("chat-turn-announcer")).toHaveTextContent("Assistant finished responding.");
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

    expect(scrollPill).toHaveClass("min-h-11");
    fireEvent.click(scrollPill);
    expect(window.HTMLElement.prototype.scrollIntoView).toHaveBeenLastCalledWith({ behavior: "smooth", block: "end" });
  });

  it("follows a growing stream without animating it", () => {
    vi.mocked(window.HTMLElement.prototype.scrollIntoView).mockClear();
    const first: MobileChatMessageProps[] = [{ id: "a", role: "assistant", content: "one", timestamp: 1 }];
    const { rerender } = render(<MobileChatWorkspace messages={first} onSendMessage={vi.fn()} />);
    rerender(
      <MobileChatWorkspace
        messages={[...first, { id: "b", role: "assistant", content: "two", timestamp: 2 }]}
        onSendMessage={vi.fn()}
      />,
    );
    expect(window.HTMLElement.prototype.scrollIntoView).toHaveBeenLastCalledWith({ behavior: "auto", block: "end" });
  });

  it("offers the pill exactly where it stops following the stream", () => {
    render(
      <MobileChatWorkspace
        messages={[{ id: "a", role: "assistant", content: "one", timestamp: 1 }]}
        onSendMessage={vi.fn()}
      />,
    );
    const stream = screen.getByTestId("chat-message-stream");
    Object.defineProperty(stream, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(stream, "clientHeight", { value: 300, configurable: true });
    // 90px from the bottom: previously neither following nor offering the pill
    Object.defineProperty(stream, "scrollTop", { value: 610, configurable: true, writable: true });
    fireEvent.scroll(stream);
    expect(screen.getByTestId("scroll-to-latest-pill")).toBeInTheDocument();
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

  it("forwards the composer warning's action to the message box", () => {
    const onAction = vi.fn();
    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
        composerWarning="The stop request was not delivered."
        composerWarningAction={{ label: "Try again", onAction }}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(onAction).toHaveBeenCalledTimes(1);
  });

  it("marks the owner's optimistic echo of a send as sending until the transcript carries it", () => {
    render(
      <MobileChatWorkspace
        messages={[{ id: "pending-r1", role: "user", content: "please review", timestamp: 1 }]}
        onSendMessage={vi.fn()}
      />
    );
    expect(screen.getByTestId("user-message-sending")).toHaveTextContent("Sending");
  });

  it("10. header degrades safely when optional header props are absent", () => {
    render(<MobileChatWorkspace messages={[]} onSendMessage={vi.fn()} />);

    expect(screen.queryByTestId("thread-header-back")).toBeNull();
    expect(screen.queryByTestId("open-terminal-button")).toBeNull();
    expect(screen.getByText("Chat")).toBeInTheDocument();
    // no invented "connected" dot when the connection state is unknown
    expect(screen.queryByText("ferryx remote")).not.toBeInTheDocument();
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

  it("ui-workspace-wiring-8: the older-page control sits above the first message", () => {
    render(
      <MobileChatWorkspace
        messages={[{ id: "m1", role: "user", content: "first visible turn", timestamp: 1 }]}
        onSendMessage={vi.fn()}
        hasOlderPage={true}
      />
    );
    const button = screen.getByTestId("reference-older-button");
    const first = screen.getByText("first visible turn");
    expect(button.compareDocumentPosition(first) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
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

  it("15. draws the prompt card in the message stream, not the composer footer", () => {
    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
        promptCard={<div data-testid="stub-prompt-card">waiting</div>}
      />
    );
    expect(screen.getByTestId("chat-stream-prompt")).toHaveTextContent("waiting");
    expect(screen.getByTestId("chat-message-stream")).toContainElement(screen.getByTestId("stub-prompt-card"));
    expect(screen.getByTestId("mobile-chat-composer")).not.toContainElement(screen.getByTestId("stub-prompt-card"));
    // the box still answers the prompt: Send stays beside Stop
    expect(screen.getByTestId("send-button")).toBeInTheDocument();
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

  it("ui-workspace-wiring-r14-1: the older-page anchor jump is instant, not smooth-scrolled", () => {
    const firstPage: MobileChatMessageProps[] = Array.from({ length: 4 }, (_, idx) => ({
      id: `new-${idx}`,
      role: idx % 2 === 0 ? "user" : "assistant",
      content: `Newest ${idx}`,
      timestamp: 1_000 + idx,
    }));
    const { rerender } = render(<MobileChatWorkspace messages={firstPage} onSendMessage={vi.fn()} />);

    const stream = screen.getByTestId("chat-message-stream");
    // record the inline scroll-behavior each write runs under
    expect(stream).not.toHaveClass("scroll-smooth");
    let top = 100;
    const writes: { top: number; behavior: string }[] = [];
    Object.defineProperty(stream, "clientHeight", { value: 300, configurable: true });
    Object.defineProperty(stream, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(stream, "scrollTop", {
      configurable: true,
      get: () => top,
      set: (value: number) => {
        writes.push({ top: value, behavior: stream.style.scrollBehavior });
        top = value;
      },
    });
    fireEvent.scroll(stream);

    const older: MobileChatMessageProps[] = Array.from({ length: 2 }, (_, idx) => ({
      id: `old-${idx}`,
      role: idx % 2 === 0 ? "user" : "assistant",
      content: `Older ${idx}`,
      timestamp: 500 + idx,
    }));
    Object.defineProperty(stream, "scrollHeight", { value: 1200, configurable: true });
    rerender(<MobileChatWorkspace messages={[...older, ...firstPage]} onSendMessage={vi.fn()} />);

    // the anchor write never animates the reader's place
    expect(writes).toEqual([{ top: 300, behavior: "auto" }]);
    // and the inline override is cleared afterwards
    expect(stream.style.scrollBehavior).toBe("");
  });

  it("16b. leaves the reader's place alone when a turn is appended below them", () => {
    const firstPage: MobileChatMessageProps[] = Array.from({ length: 4 }, (_, idx) => ({
      id: `new-${idx}`,
      role: idx % 2 === 0 ? "user" : "assistant",
      content: `Newest ${idx}`,
      timestamp: Date.now() + idx,
    }));
    const { rerender } = render(<MobileChatWorkspace messages={firstPage} onSendMessage={vi.fn()} />);

    const stream = screen.getByTestId("chat-message-stream");
    Object.defineProperty(stream, "clientHeight", { value: 300, configurable: true });
    Object.defineProperty(stream, "scrollHeight", { value: 3000, configurable: true });
    Object.defineProperty(stream, "scrollTop", { value: 2000, configurable: true, writable: true });
    fireEvent.scroll(stream);

    // a new turn lands at the bottom: content below the reader grows by 200px
    Object.defineProperty(stream, "scrollHeight", { value: 3200, configurable: true });
    rerender(
      <MobileChatWorkspace
        messages={[...firstPage, { id: "new-4", role: "assistant", content: "Appended", timestamp: Date.now() + 10 }]}
        onSendMessage={vi.fn()}
      />
    );

    expect(stream.scrollTop).toBe(2000);
  });

  it("16c. keeps the reader's place when an older page loads while they sit at the very top", () => {
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
    // the Load-earlier control sits at the top, so a reader who taps it is at scrollTop 0
    Object.defineProperty(stream, "scrollTop", { value: 0, configurable: true, writable: true });
    fireEvent.scroll(stream);

    const older: MobileChatMessageProps[] = Array.from({ length: 2 }, (_, idx) => ({
      id: `old-${idx}`,
      role: idx % 2 === 0 ? "user" : "assistant",
      content: `Older ${idx}`,
      timestamp: Date.now() - 1000 + idx,
    }));
    Object.defineProperty(stream, "scrollHeight", { value: 1200, configurable: true });
    rerender(<MobileChatWorkspace messages={[...older, ...firstPage]} onSendMessage={vi.fn()} />);

    expect(stream.scrollTop).toBe(200);
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

  it("18. renders header by default and hides header when showHeader is false", () => {
    const { rerender } = render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
        showHeader={true}
      />
    );
    expect(screen.getByTestId("mobile-chat-header")).toBeInTheDocument();

    rerender(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
        showHeader={false}
      />
    );
    expect(screen.queryByTestId("mobile-chat-header")).not.toBeInTheDocument();
  });

  it("(a) empty state renders starter prompts that name the agent pane's worktree", () => {
    render(<MobileChatWorkspace messages={[]} onSendMessage={vi.fn()} agentType="claude" worktreeLabel="feat-x" />);

    const starters = screen.getAllByTestId("chat-empty-starter");
    expect(starters.map((starter) => starter.textContent)).toEqual(chatStarterPrompts("claude", "feat-x"));
    for (const starter of starters) expect(starter.textContent).toContain("feat-x");
  });

  it("(a2) a pane with no detected agent gets no starter prompts", () => {
    render(<MobileChatWorkspace messages={[]} onSendMessage={vi.fn()} worktreeLabel="feat-x" />);
    expect(screen.getByTestId("chat-empty-state")).toBeInTheDocument();
    expect(screen.queryByTestId("chat-empty-starter")).not.toBeInTheDocument();
    expect(chatStarterPrompts(null, "feat-x")).toEqual([]);
  });

  it("(b) tapping a chip fills the composer with that text, focuses it, and does not call onSendMessage", () => {
    const handleSend = vi.fn();
    render(<MobileChatWorkspace messages={[]} onSendMessage={handleSend} agentType="codex" />);

    const textarea = screen.getByTestId("chat-composer-textarea");
    expect(textarea).toHaveValue("");

    const [first] = chatStarterPrompts("codex", undefined);
    const chip = screen.getByRole("button", { name: first });
    fireEvent.click(chip);

    expect(textarea).toHaveValue(first);
    expect(textarea).toHaveFocus();
    expect(handleSend).not.toHaveBeenCalled();
  });

  it("(c) chips disappear once a message exists", () => {
    const { rerender } = render(<MobileChatWorkspace messages={[]} onSendMessage={vi.fn()} agentType="claude" />);
    expect(screen.getAllByTestId("chat-empty-starter").length).toBeGreaterThan(0);

    const messages: MobileChatMessageProps[] = [
      { id: "msg-1", role: "user", content: "Hello", timestamp: Date.now() },
    ];
    rerender(<MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} />);
    expect(screen.queryByTestId("chat-empty-state")).not.toBeInTheDocument();
    expect(screen.queryByTestId("chat-empty-starter")).not.toBeInTheDocument();
  });

  it("hides composer controls without capabilities and shows typed continuation reasons", () => {
    const onContinue = vi.fn();
    render(
      <MobileChatWorkspace
        messages={[]}
        onSendMessage={vi.fn()}
        continuationOffers={[
          { mode: "fork", available: true, reason: null },
          { mode: "resume", available: false, reason: "liveClaim" },
        ]}
        onContinueInNewPane={onContinue}
      />,
    );
    expect(screen.queryByTestId("composer-model-control")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("continue-in-new-pane-fork"));
    expect(onContinue).toHaveBeenCalledWith("fork");
    expect(screen.getByTestId("continue-unavailable-resume")).toHaveAttribute("data-reason", "liveClaim");
    expect(screen.queryByTestId("continue-in-new-pane-resume")).not.toBeInTheDocument();
  });

  it("draft hero names the project like t3code's DraftHeroProjectHeading", () => {
    render(
      <MobileChatWorkspace messages={[]} onSendMessage={vi.fn()} workspaceLabel="maho-workspace" worktreeLabel="feature/mobile-chat" />,
    );
    const hero = screen.getByTestId("chat-draft-hero");
    expect(hero).toHaveTextContent("What should we build in feature/mobile-chat?");
    // the context line survives inside the hero
    expect(screen.getByTestId("chat-empty-context")).toBeInTheDocument();
  });

  it("draft hero falls back to the workspace label and then to the bare question", () => {
    const { rerender } = render(
      <MobileChatWorkspace messages={[]} onSendMessage={vi.fn()} workspaceLabel="maho-workspace" />,
    );
    expect(screen.getByTestId("chat-draft-hero")).toHaveTextContent("What should we build in maho-workspace?");
    rerender(<MobileChatWorkspace messages={[]} onSendMessage={vi.fn()} />);
    expect(screen.getByTestId("chat-draft-hero")).toHaveTextContent("What should we build?");
  });

  it("draws the new-messages watermark at the first turn that arrived while the reader was away", () => {
    const first: MobileChatMessageProps[] = [
      { id: "u1", role: "user", content: "seen", timestamp: 1 },
      { id: "a1", role: "assistant", content: "seen answer", timestamp: 2 },
    ];
    const { rerender } = render(<MobileChatWorkspace messages={first} onSendMessage={vi.fn()} />);
    const stream = screen.getByTestId("chat-message-stream");
    Object.defineProperty(stream, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(stream, "clientHeight", { value: 300, configurable: true });
    Object.defineProperty(stream, "scrollTop", { value: 610, configurable: true, writable: true });
    // the reader scrolls away from the bottom, so what is on screen counts as visited
    fireEvent.scroll(stream);
    expect(screen.queryByTestId("thread-visit-watermark")).not.toBeInTheDocument();

    rerender(
      <MobileChatWorkspace
        messages={[...first, { id: "u2", role: "user", content: "arrived later", timestamp: 3 } as MobileChatMessageProps]}
        onSendMessage={vi.fn()}
      />,
    );
    expect(screen.getByTestId("thread-visit-watermark")).toBeInTheDocument();
  });

  it("tool call visibility hides the tool rows the pane preference names", () => {
    const messages: MobileChatMessageProps[] = [
      { id: "u1", role: "user", content: "do it", timestamp: 1 },
      {
        id: "a1",
        role: "assistant",
        content: "done",
        timestamp: 2,
        toolCalls: [
          { toolName: "grep", summary: "searched", output: "2 files", status: "success", workKey: "w1" },
          { toolName: "bash", summary: "running", status: "running", workKey: "w2" },
          { toolName: "edit", summary: "failed", status: "error", workKey: "w3" },
        ],
      },
    ];
    const base = { messages, onSendMessage: vi.fn() };
    const rows = () => screen.getAllByTestId("work-row").length;
    const { rerender } = render(<MobileChatWorkspace {...base} />);
    expect(rows()).toBe(3);

    rerender(<MobileChatWorkspace {...base} toolCallVisibility="hide-successful" />);
    expect(rows()).toBe(2);

    rerender(<MobileChatWorkspace {...base} toolCallVisibility="hide-all" />);
    // Approvals, questions, and errors always show: the failed call survives every mode.
    expect(screen.getAllByTestId("work-row")).toHaveLength(1);
    expect(screen.getByText("failed")).toBeInTheDocument();

    rerender(<MobileChatWorkspace {...base} toolCallVisibility="zen" />);
    expect(screen.getAllByTestId("work-row")).toHaveLength(1);
    // prose stays in every mode
    expect(screen.getByText("done")).toBeInTheDocument();
  });

  it("a bare PageDown pages the stream and an IME-buffered one does nothing", () => {    // Reduced motion makes the reference page move a single synchronous jump, so the target
    // is observable without frames; the IME refusal below must not scroll at all.
    const matchMedia = vi.spyOn(window, "matchMedia").mockImplementation(((query: string) => ({
      matches: query.includes("prefers-reduced-motion"),
      media: query,
    })) as unknown as typeof window.matchMedia);
    render(
      <MobileChatWorkspace
        messages={[
          { id: "u1", role: "user", content: "one", timestamp: 1 },
          { id: "a1", role: "assistant", content: "two", timestamp: 2 },
        ]}
        onSendMessage={vi.fn()}
      />,
    );
    const stream = screen.getByTestId("chat-message-stream");
    Object.defineProperty(stream, "scrollHeight", { value: 2000, configurable: true });
    Object.defineProperty(stream, "clientHeight", { value: 400, configurable: true });
    Object.defineProperty(stream, "scrollTop", { value: 100, configurable: true, writable: true });
    const el = stream as unknown as { scrollTo: (options: { top: number }) => void };
    el.scrollTo = vi.fn();
    fireEvent.keyDown(stream, { key: "PageDown" });
    expect(el.scrollTo).toHaveBeenCalledWith({ top: 100 + (400 - 36) });

    el.scrollTo = vi.fn();
    fireEvent.keyDown(stream, { key: "PageDown", isComposing: true });
    expect(el.scrollTo).not.toHaveBeenCalled();
    matchMedia.mockRestore();
  });

  it("timeline minimap renders on desktop widths and navigates by user turn", () => {
    const matchMedia = vi.spyOn(window, "matchMedia").mockImplementation(((query: string) => ({
      matches: query.includes("pointer: fine"),
      media: query,
    })) as unknown as typeof window.matchMedia);
    const messages: MobileChatMessageProps[] = [
      { id: "u1", role: "user", content: "first turn", timestamp: 1 },
      { id: "a1", role: "assistant", content: "first answer", timestamp: 2 },
      { id: "u2", role: "user", content: "second turn", timestamp: 3 },
      { id: "a2", role: "assistant", content: "second answer", timestamp: 4 },
    ];
    render(<MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} />);
    const stream = screen.getByTestId("chat-message-stream");
    Object.defineProperty(stream, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(stream, "clientHeight", { value: 300, configurable: true });
    Object.defineProperty(stream, "scrollTop", { value: 0, configurable: true, writable: true });
    fireEvent.scroll(stream);
    expect(screen.getByTestId("timeline-minimap")).toBeInTheDocument();
    expect(screen.getAllByTestId("timeline-minimap-item")).toHaveLength(2);
    expect(screen.getAllByTestId("timeline-minimap-item")[0]).toHaveAttribute("aria-label", "first turn");
    matchMedia.mockRestore();
  });

  it("timeline minimap is absent under two turns even on desktop widths", () => {
    const matchMedia = vi.spyOn(window, "matchMedia").mockImplementation(((query: string) => ({
      matches: query.includes("pointer: fine"),
      media: query,
    })) as unknown as typeof window.matchMedia);
    render(
      <MobileChatWorkspace
        messages={[{ id: "u1", role: "user", content: "only", timestamp: 1 }]}
        onSendMessage={vi.fn()}
      />,
    );
    expect(screen.queryByTestId("timeline-minimap")).not.toBeInTheDocument();
    matchMedia.mockRestore();
  });

  it("mounts find in thread: Cmd/Ctrl+F opens the bar and a query counts the matches", () => {
    render(
      <MobileChatWorkspace
        messages={[
          { id: "u1", role: "user", content: "find the needle", timestamp: 1 },
          { id: "a1", role: "assistant", content: "a needle and another needle", timestamp: 2 },
        ]}
        onSendMessage={vi.fn()}
      />,
    );
    // Nothing mounts the provider on its own, so the bar being reachable from the keyboard is
    // the proof this pane wires it rather than only importing it.
    expect(screen.queryByTestId("thread-find-bar")).not.toBeInTheDocument();
    fireEvent.keyDown(window, { key: "f", ctrlKey: true });
    const input = screen.getByTestId("thread-find-input");
    fireEvent.change(input, { target: { value: "needle" } });
    expect(screen.getByTestId("thread-find-count")).toHaveTextContent("1/3");
  });

  it("zen keeps the reply that closes a turn, and find shows the turns it hides", () => {
    const messages: MobileChatMessageProps[] = [
      { id: "u1", role: "user", content: "do it", timestamp: 1 },
      { id: "a1", role: "assistant", content: "INTERIM_MARKER", timestamp: 2 },
      { id: "a2", role: "assistant", content: "FINAL_MARKER", timestamp: 3 },
    ];
    const { rerender } = render(
      <MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} toolCallVisibility="zen" />,
    );
    // Zen hides the in-between reply but never the answer that ends the turn.
    expect(screen.queryByText("INTERIM_MARKER")).not.toBeInTheDocument();
    expect(screen.getByText("FINAL_MARKER")).toBeInTheDocument();

    // While find is open zen steps aside, so a hidden turn is searchable again.
    fireEvent.keyDown(window, { key: "f", ctrlKey: true });
    fireEvent.change(screen.getByTestId("thread-find-input"), { target: { value: "INTERIM_MARKER" } });
    rerender(
      <MobileChatWorkspace messages={messages} onSendMessage={vi.fn()} toolCallVisibility="zen" />,
    );
    expect(screen.getByText("INTERIM_MARKER")).toBeInTheDocument();
  });
});

