/**
 * G3 chat-parity regressions for the composer (t3code docs/user/composer.md).
 *
 * Each scenario pins one documented behaviour: the 120,000-character limit, large-paste
 * attachment, image paste and drop, queued message edit/remove/send-now/interrupt with the
 * follow-up shortcuts, prompt recall, the prompt stash, inline chips, citation insertion with
 * its comment, the permission control and the plain-textarea fallback.
 */
import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, fireEvent, act } from "@testing-library/react";
import { createRef, forwardRef, useState } from "react";
import { MobileChatComposer, type MobileChatComposerHandle } from "./MobileChatComposer";
import type { ChipSources } from "./composer/chipTypes";

const queuedRow = (id: string, text: string) => ({
  id,
  text,
  ownerId: "o",
  epoch: "1",
  phase: "queued" as const,
  requestId: null,
});

/** The composer with its own draft state, recording every change. */
const DraftingComposer = forwardRef<MobileChatComposerHandle, Omit<Parameters<typeof MobileChatComposer>[0], "value" | "onValueChange" | "onSend"> & { onValueChange?: (text: string) => void; onSend?: Parameters<typeof MobileChatComposer>[0]["onSend"] }>(
  function DraftingComposer({ onValueChange, onSend, ...rest }, ref) {
    const [text, setText] = useState("");
    return (
      <MobileChatComposer
        ref={ref}
        {...rest}
        onSend={onSend ?? vi.fn()}
        value={text}
        onValueChange={(next) => {
          setText(next);
          onValueChange?.(next);
        }}
      />
    );
  },
);

function pasteEvent(init: { text?: string; files?: File[]; shiftKey?: boolean; ctrlKey?: boolean }) {
  return {
    clipboardData: {
      files: init.files ?? [],
      getData: (type: string) => (type === "text/plain" ? (init.text ?? "") : ""),
    },
    shiftKey: init.shiftKey ?? false,
    ctrlKey: init.ctrlKey ?? false,
    metaKey: false,
    preventDefault: vi.fn(),
  };
}

describe("MobileChatComposer parity (G3)", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  it("keeps an over-limit draft and disables sending with a visible count message", () => {
    const onSend = vi.fn();
    render(<MobileChatComposer onSend={onSend} value={"x".repeat(120001)} />);
    const error = screen.getByTestId("chat-composer-length-error");
    expect(error).toHaveTextContent("120,001");
    expect(error).toHaveTextContent("120,000");
    expect(screen.getByTestId("send-button")).toBeDisabled();
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("x".repeat(120001));
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "Enter", keyCode: 13 });
    expect(onSend).not.toHaveBeenCalled();
  });

  it("lets a draft of exactly the limit send", () => {
    const onSend = vi.fn(() => true);
    render(<MobileChatComposer onSend={onSend} value={"x".repeat(120000)} />);
    expect(screen.queryByTestId("chat-composer-length-error")).not.toBeInTheDocument();
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "Enter", keyCode: 13 });
    expect(onSend).toHaveBeenCalledWith("x".repeat(120000));
  });

  it("turns a paste of 32 KiB or more into a text-file attachment at the caret", () => {
    const onAttachFiles = vi.fn();
    render(
      <MobileChatComposer onSend={vi.fn()} onAttachFiles={onAttachFiles} value="before" />,
    );
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "before", selectionStart: 6 } });
    const big = "y".repeat(32 * 1024);
    fireEvent.paste(textarea, pasteEvent({ text: big }));
    expect(onAttachFiles).toHaveBeenCalledTimes(1);
    const [files, caret] = onAttachFiles.mock.calls[0];
    expect(files).toHaveLength(1);
    expect(files[0].name).toBe("pasted text.txt");
    expect(files[0].size).toBe(big.length);
    expect(caret).toBe(6);
    // The draft stays: the paste never entered the text.
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("before");
  });

  it("keeps a large paste editable with Cmd/Ctrl+Shift+V", () => {
    const onAttachFiles = vi.fn();
    render(<MobileChatComposer onSend={vi.fn()} onAttachFiles={onAttachFiles} />);
    const textarea = screen.getByTestId("chat-composer-textarea");
    // A paste event carries no modifier keys, so the browser reports the chord on the keydown
    // that precedes it; the composer remembers that. Both are fired here, in that order.
    fireEvent.keyDown(textarea, { key: "v", keyCode: 86, ctrlKey: true, shiftKey: true });
    fireEvent.paste(textarea, pasteEvent({ text: "y".repeat(32 * 1024), shiftKey: true, ctrlKey: true }));
    expect(onAttachFiles).not.toHaveBeenCalled();
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("y".repeat(32 * 1024));
  });

  it("keeps a large paste editable when only the keydown reports the chord", () => {
    const onAttachFiles = vi.fn();
    render(<MobileChatComposer onSend={vi.fn()} onAttachFiles={onAttachFiles} />);
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.keyDown(textarea, { key: "v", keyCode: 86, metaKey: true, shiftKey: true });
    fireEvent.paste(textarea, pasteEvent({ text: "z".repeat(32 * 1024) }));
    expect(onAttachFiles).not.toHaveBeenCalled();
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("z".repeat(32 * 1024));
  });

  it("attaches a small paste only when it would exceed the limit", () => {
    const onAttachFiles = vi.fn();
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        onAttachFiles={onAttachFiles}
        value={"x".repeat(119999)}
      />,
    );
    fireEvent.paste(
      screen.getByTestId("chat-composer-textarea"),
      pasteEvent({ text: "ab" }),
    );
    expect(onAttachFiles).toHaveBeenCalledTimes(1);
    expect(onAttachFiles.mock.calls[0][0][0].name).toBe("pasted text.txt");
  });

  it("sends image pastes and image drops through onAttachFiles", () => {
    const onAttachFiles = vi.fn();
    render(<MobileChatComposer onSend={vi.fn()} onAttachFiles={onAttachFiles} />);
    const textarea = screen.getByTestId("chat-composer-textarea");
    const image = new File(["bytes"], "shot.png", { type: "image/png" });
    const plain = new File(["bytes"], "notes.txt", { type: "text/plain" });
    fireEvent.paste(textarea, pasteEvent({ files: [image, plain] }));
    expect(onAttachFiles).toHaveBeenCalledTimes(1);
    expect(onAttachFiles.mock.calls[0][0]).toEqual([image]);
    fireEvent.drop(textarea, {
      dataTransfer: { files: [new File(["b"], "pic.jpg", { type: "image/jpeg" })] },
    });
    expect(onAttachFiles).toHaveBeenCalledTimes(2);
    expect(onAttachFiles.mock.calls[1][0][0].type).toBe("image/jpeg");
  });

  it("edits a queued message in the composer and saves it in place", () => {
    const onEditQueued = vi.fn();
    render(
      <DraftingComposer queuedMessages={[queuedRow("q1", "queued text")]} onEditQueued={onEditQueued} />,
    );
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "current draft", selectionStart: 13 } });
    fireEvent.click(screen.getByTestId("queued-follow-up-edit-q1"));
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("queued text");
    fireEvent.change(textarea, { target: { value: "queued text edited" } });
    fireEvent.click(screen.getByTestId("queued-follow-up-save-q1"));
    expect(onEditQueued).toHaveBeenCalledWith("q1", "queued text edited");
    // The previous draft is restored after the edit ends.
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("current draft");
  });

  it("restores the previous draft when a queued edit is cancelled", () => {
    const onEditQueued = vi.fn();
    render(
      <DraftingComposer queuedMessages={[queuedRow("q1", "queued text")]} onEditQueued={onEditQueued} />,
    );
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "my draft" } });
    fireEvent.click(screen.getByTestId("queued-follow-up-edit-q1"));
    fireEvent.click(screen.getByTestId("queued-follow-up-cancel-edit-q1"));
    expect(onEditQueued).not.toHaveBeenCalled();
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("my draft");
  });

  it("sends a queued message now and removes it", () => {
    const onSendQueuedNow = vi.fn();
    const onRemoveQueued = vi.fn();
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        queuedMessages={[queuedRow("q1", "waiting"), queuedRow("q2", "second")]}
        onSendQueuedNow={onSendQueuedNow}
        onRemoveQueued={onRemoveQueued}
      />,
    );
    fireEvent.click(screen.getByTestId("queued-follow-up-send-now-q1"));
    expect(onSendQueuedNow).toHaveBeenCalledWith("q1");
    fireEvent.click(screen.getByTestId("queued-follow-up-remove-q2"));
    expect(onRemoveQueued).toHaveBeenCalledWith("q2");
  });

  it("shows Interrupt while running with an empty draft, and steers the oldest queued with mod+shift+Enter", () => {
    const onInterrupt = vi.fn();
    const onSendQueuedNow = vi.fn();
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        isRunning
        onInterrupt={onInterrupt}
        queuedMessages={[queuedRow("q1", "first"), queuedRow("q2", "later")]}
        onSendQueuedNow={onSendQueuedNow}
      />,
    );
    fireEvent.click(screen.getByTestId("interrupt-button"));
    expect(onInterrupt).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), {
      key: "Enter",
      keyCode: 13,
      metaKey: true,
      shiftKey: true,
    });
    expect(onSendQueuedNow).toHaveBeenCalledWith("q1");
  });

  it("does the opposite follow-up action on mod+Enter and follows the default on the button", () => {
    const onSend = vi.fn();
    const onQueue = vi.fn(() => true);
    const onSteer = vi.fn(() => true);
    const caps = { models: null, options: [], slashCommands: [], followUp: { queue: true, steer: true } };
    render(
      <MobileChatComposer
        onSend={onSend}
        isRunning
        value="mid-turn"
        composerCapabilities={caps}
        onQueue={onQueue}
        onSteer={onSteer}
        followUpDefault="queue"
      />,
    );
    fireEvent.click(screen.getByTestId("queue-follow-up-button"));
    expect(onQueue).toHaveBeenCalledWith("mid-turn");
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), {
      key: "Enter",
      keyCode: 13,
      metaKey: true,
    });
    expect(onSteer).toHaveBeenCalledWith("mid-turn");
    expect(onSend).not.toHaveBeenCalled();
  });

  it("buttons follow a steer default when that is the follow-up default", () => {
    const onQueue = vi.fn(() => true);
    const onSteer = vi.fn(() => true);
    const caps = { models: null, options: [], slashCommands: [], followUp: { queue: true, steer: true } };
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        isRunning
        value="mid-turn"
        composerCapabilities={caps}
        onQueue={onQueue}
        onSteer={onSteer}
        followUpDefault="steer"
      />,
    );
    fireEvent.click(screen.getByTestId("steer-button"));
    expect(onSteer).toHaveBeenCalledWith("mid-turn");
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), {
      key: "Enter",
      keyCode: 13,
      metaKey: true,
    });
    expect(onQueue).toHaveBeenCalledWith("mid-turn");
  });

  it("edits the newest queued message with Alt+Up at the start of the composer", () => {
    const onEditQueued = vi.fn();
    render(
      <DraftingComposer
        queuedMessages={[queuedRow("q1", "older"), queuedRow("q2", "newest")]}
        onEditQueued={onEditQueued}
      />,
    );
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "", selectionStart: 0 } });
    fireEvent.keyDown(textarea, { key: "ArrowUp", keyCode: 38, altKey: true });
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("newest");
    expect(screen.getByTestId("queued-follow-up-save-q2")).toBeInTheDocument();
  });

  it("recalls sent prompts with ArrowUp and clears past the newest with ArrowDown", () => {
    const onValueChange = vi.fn();
    render(
      <DraftingComposer onValueChange={onValueChange} sentPrompts={["first prompt", "second prompt"]} />,
    );
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.keyDown(textarea, { key: "ArrowUp", keyCode: 38 });
    expect(onValueChange).toHaveBeenLastCalledWith("second prompt");
    fireEvent.keyDown(textarea, { key: "ArrowUp", keyCode: 38 });
    expect(onValueChange).toHaveBeenLastCalledWith("first prompt");
    fireEvent.keyDown(textarea, { key: "ArrowDown", keyCode: 40 });
    expect(onValueChange).toHaveBeenLastCalledWith("second prompt");
    fireEvent.keyDown(textarea, { key: "ArrowDown", keyCode: 40 });
    expect(onValueChange).toHaveBeenLastCalledWith("");
  });

  it("does not recall when the composer holds a draft", () => {
    const onValueChange = vi.fn();
    render(
      <DraftingComposer onValueChange={onValueChange} sentPrompts={["sent one"]} />,
    );
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "a draft" } });
    onValueChange.mockClear();
    fireEvent.keyDown(textarea, { key: "ArrowUp", keyCode: 38 });
    expect(onValueChange).not.toHaveBeenCalled();
  });

  it("stashes the draft with mod+S and restores it on the empty-composer shortcut", () => {
    const onValueChange = vi.fn();
    render(<DraftingComposer onValueChange={onValueChange} />);
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "stash me" } });
    fireEvent.keyDown(textarea, { key: "s", keyCode: 83, metaKey: true });
    expect(onValueChange).toHaveBeenLastCalledWith("");
    fireEvent.keyDown(textarea, { key: "s", keyCode: 83, metaKey: true });
    expect(onValueChange).toHaveBeenLastCalledWith("stash me");
  });

  it("opens a stash menu with several stashes and restores or removes a row", () => {
    const onValueChange = vi.fn();
    window.localStorage.setItem(
      "ferryx.chat.composerStash:v1",
      JSON.stringify([
        { id: "s2", text: "second", attachmentNames: [], savedAt: 2 },
        { id: "s1", text: "first", attachmentNames: ["a.txt"], savedAt: 1 },
      ]),
    );
    try {
      render(<DraftingComposer onValueChange={onValueChange} />);
      const textarea = screen.getByTestId("chat-composer-textarea");
      fireEvent.keyDown(textarea, { key: "s", keyCode: 83, metaKey: true });
      expect(screen.getByTestId("composer-stash-menu")).toBeInTheDocument();
      fireEvent.click(screen.getByTestId("composer-stash-restore-s1"));
      expect(onValueChange).toHaveBeenLastCalledWith("first");
      // The restored prompt fills the composer, so reopen the menu from an empty composer.
      fireEvent.change(textarea, { target: { value: "" } });
      fireEvent.keyDown(textarea, { key: "s", keyCode: 83, metaKey: true });
      fireEvent.click(screen.getByTestId("composer-stash-remove-s1"));
      expect(screen.queryByTestId("composer-stash-restore-s1")).not.toBeInTheDocument();
    } finally {
      window.localStorage.removeItem("ferryx.chat.composerStash:v1");
    }
  });

  it("opens the model chip menu on @ and inserts the chip as readable markdown", async () => {
    const chipSources: ChipSources = {
      models: [
        { id: "opus", kind: "model", label: "Opus", detail: "claude-opus" },
        { id: "haiku", kind: "model", label: "Haiku" },
      ],
    };
    const onValueChange = vi.fn();
    render(<DraftingComposer onValueChange={onValueChange} chipSources={chipSources} />);
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "delegate to @op", selectionStart: 15 } });
    await screen.findByTestId("composer-chip-menu");
    fireEvent.click(screen.getByTestId("composer-chip-option-@-0"));
    expect(onValueChange).toHaveBeenLastCalledWith("delegate to [Opus](ferryx://chip/model/opus) ");
  });

  it("queries thread and pull-request chips from their sources", async () => {
    const pullRequests = vi.fn(async (query: string) => [
      { id: "12", kind: "pr" as const, label: "Fix login " + query },
    ]);
    const threads = vi.fn(async () => [{ id: "t1", kind: "thread" as const, label: "Thread" }]);
    const onValueChange = vi.fn();
    render(
      <DraftingComposer onValueChange={onValueChange} chipSources={{ threads, pullRequests }} />,
    );
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "see #12", selectionStart: 7 } });
    fireEvent.click(await screen.findByTestId("composer-chip-option-#-0"));
    expect(pullRequests).toHaveBeenCalled();
    expect(onValueChange).toHaveBeenLastCalledWith("see [Fix login 12](ferryx://chip/pr/12) ");
  });

  it("lists skills on $", async () => {
    const chipSources: ChipSources = { skills: [{ id: "review", kind: "skill", label: "review" }] };
    render(<DraftingComposer chipSources={chipSources} />);
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "$rev", selectionStart: 4 } });
    expect(await screen.findByTestId("composer-chip-option-$-0")).toBeInTheDocument();
  });

  it("inserts a citation at the caret through the handle and edits its comment with the pencil", () => {
    const onValueChange = vi.fn();
    const ref = createRef<MobileChatComposerHandle>();
    render(<DraftingComposer ref={ref} onValueChange={onValueChange} />);
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "look: ", selectionStart: 6 } });
    act(() => ref.current?.insertCitation({ messageId: "m1", text: "quoted words" }));
    expect(onValueChange).toHaveBeenLastCalledWith("look: [quoted words](ferryx://cite/m1)");
    expect(screen.getByTestId("composer-citation-0")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("citation-comment-edit-0"));
    fireEvent.change(screen.getByTestId("citation-comment-input-0"), {
      target: { value: "why this matters" },
    });
    fireEvent.click(screen.getByTestId("citation-comment-save-0"));
    expect(onValueChange).toHaveBeenLastCalledWith(
      'look: [quoted words](ferryx://cite/m1 "why this matters")',
    );
  });

  it("renders the permission control and cycles it with mod+shift+A", () => {
    const onChange = vi.fn();
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        value="x"
        permissionControl={{
          modes: ["supervised", "auto-accept-edits", "full-access"],
          current: "supervised",
          onChange,
        }}
      />,
    );
    expect(screen.getByTestId("composer-permission-control")).toHaveValue("supervised");
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), {
      key: "A",
      keyCode: 65,
      metaKey: true,
      shiftKey: true,
    });
    expect(onChange).toHaveBeenCalledWith("auto-accept-edits");
  });

  it("is one plain textarea and never mounts a rich editor", () => {
    // The user's decision: no WYSIWYG surface in the app. The draft is markdown text, exactly
    // as typed, and the textarea is the only composer input.
    render(<MobileChatComposer onSend={vi.fn()} value="plain **md**" />);
    expect(screen.queryByTestId("chat-composer-rich-editor-wrap")).not.toBeInTheDocument();
    expect(screen.queryByTestId("chat-composer-rich-editor")).not.toBeInTheDocument();
    expect(document.querySelector("[contenteditable='true']")).toBeNull();
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("plain **md**");
  });

  it("renders the execution mode, agent and thinking selectors and reports a change each", () => {
    const onExecutionMode = vi.fn();
    const onAgent = vi.fn();
    const onThinking = vi.fn();
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        executionModeControl={{ modes: ["plan", "act", "auto", "accept-edits"], current: "act", onChange: onExecutionMode }}
        agentControl={{ agents: ["claude", "codex", "gemini"], current: "codex", onChange: onAgent }}
        thinkingControl={{ levels: ["off", "minimal", "low", "medium", "high", "xhigh", "max"], current: "medium", onChange: onThinking }}
      />,
    );
    const mode = screen.getByTestId("composer-execution-mode-control");
    const agent = screen.getByTestId("composer-agent-control");
    const thinking = screen.getByTestId("composer-thinking-control");
    expect(mode).toHaveValue("act");
    expect(agent).toHaveValue("codex");
    expect(thinking).toHaveValue("medium");
    expect(screen.getByRole("option", { name: "accept-edits" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "max" })).toBeInTheDocument();
    fireEvent.change(mode, { target: { value: "plan" } });
    fireEvent.change(agent, { target: { value: "gemini" } });
    fireEvent.change(thinking, { target: { value: "xhigh" } });
    expect(onExecutionMode).toHaveBeenCalledWith("plan");
    expect(onAgent).toHaveBeenCalledWith("gemini");
    expect(onThinking).toHaveBeenCalledWith("xhigh");
  });

  it("toggles fast mode and reports the flipped value", () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <MobileChatComposer onSend={vi.fn()} fastModeControl={{ enabled: false, onChange }} />,
    );
    const toggle = screen.getByTestId("composer-fast-mode-toggle");
    expect(toggle).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(toggle);
    expect(onChange).toHaveBeenCalledWith(true);
    rerender(<MobileChatComposer onSend={vi.fn()} fastModeControl={{ enabled: true, onChange }} />);
    expect(screen.getByTestId("composer-fast-mode-toggle")).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(screen.getByTestId("composer-fast-mode-toggle"));
    expect(onChange).toHaveBeenLastCalledWith(false);
  });

  it("lists additional directories as pills and adds and removes them", () => {
    const onAdd = vi.fn();
    const onRemove = vi.fn();
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        additionalDirsControl={{ dirs: ["/repo/one", "/repo/two"], onAdd, onRemove }}
      />,
    );
    const group = screen.getByTestId("composer-additional-dirs");
    expect(group).toHaveTextContent("/repo/one");
    expect(group).toHaveTextContent("/repo/two");
    fireEvent.click(screen.getByTestId("composer-additional-dir-remove-1"));
    expect(onRemove).toHaveBeenCalledWith("/repo/two");
    const input = screen.getByTestId("composer-additional-dir-input");
    fireEvent.change(input, { target: { value: "/repo/three" } });
    fireEvent.keyDown(input, { key: "Enter", keyCode: 13 });
    expect(onAdd).toHaveBeenCalledWith("/repo/three");
    expect(input).toHaveValue("");
    fireEvent.change(input, { target: { value: "/repo/four" } });
    fireEvent.click(screen.getByTestId("composer-additional-dir-add"));
    expect(onAdd).toHaveBeenLastCalledWith("/repo/four");
  });

  it("never adds an empty additional directory", () => {
    const onAdd = vi.fn();
    render(<MobileChatComposer onSend={vi.fn()} additionalDirsControl={{ dirs: [], onAdd, onRemove: vi.fn() }} />);
    const input = screen.getByTestId("composer-additional-dir-input");
    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.keyDown(input, { key: "Enter", keyCode: 13 });
    fireEvent.click(screen.getByTestId("composer-additional-dir-add"));
    expect(onAdd).not.toHaveBeenCalled();
  });

  it("changes the steering and follow-up delivery modes independently", () => {
    const onChangeSteering = vi.fn();
    const onChangeFollowUp = vi.fn();
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        queueModeControl={{
          steeringMode: "all",
          followUpMode: "one-at-a-time",
          onChangeSteering,
          onChangeFollowUp,
        }}
      />,
    );
    const group = screen.getByTestId("composer-queue-mode-control");
    expect(group).toBeInTheDocument();
    const steering = screen.getByTestId("composer-steering-mode-control");
    const followUp = screen.getByTestId("composer-followup-mode-control");
    expect(steering).toHaveValue("all");
    expect(followUp).toHaveValue("one-at-a-time");
    fireEvent.change(steering, { target: { value: "one-at-a-time" } });
    expect(onChangeSteering).toHaveBeenCalledWith("one-at-a-time");
    expect(onChangeFollowUp).not.toHaveBeenCalled();
    fireEvent.change(followUp, { target: { value: "all" } });
    expect(onChangeFollowUp).toHaveBeenCalledWith("all");
    expect(onChangeSteering).toHaveBeenCalledTimes(1);
  });

  it("hides every harness control when none is supplied", () => {
    render(<MobileChatComposer onSend={vi.fn()} value="x" />);
    expect(screen.queryByTestId("composer-harness-controls")).not.toBeInTheDocument();
    expect(screen.queryByTestId("composer-execution-mode-control")).not.toBeInTheDocument();
    expect(screen.queryByTestId("composer-agent-control")).not.toBeInTheDocument();
    expect(screen.queryByTestId("composer-thinking-control")).not.toBeInTheDocument();
    expect(screen.queryByTestId("composer-fast-mode-toggle")).not.toBeInTheDocument();
    expect(screen.queryByTestId("composer-additional-dirs")).not.toBeInTheDocument();
    expect(screen.queryByTestId("composer-queue-mode-control")).not.toBeInTheDocument();
    expect(screen.queryByTestId("composer-permission-control")).not.toBeInTheDocument();
  });

  it("shows a staged image as a 64px tile with its thumbnail and a remove control", () => {
    const onRemoveAttachment = vi.fn();
    const receipt = {
      receipt: { hostId: "local", attachmentId: "att-img", sha256: "abc", sizeBytes: 4096, mediaType: "image/png" as const },
      displayName: "shot.png",
      mentionText: "@shot.png ",
    };
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        attachments={[receipt]}
        onRemoveAttachment={onRemoveAttachment}
        attachmentPreviewUrl={() => "/preview/att-img"}
      />,
    );
    fireEvent.click(screen.getByTestId("chat-composer-files-toggle"));
    const thumb = screen.getByTestId("attachment-thumbnail-att-img");
    expect(thumb).toHaveAttribute("src", "/preview/att-img");
    expect(thumb).toHaveAttribute("alt", "shot.png");
    fireEvent.click(screen.getByTestId("remove-attachment-att-img"));
    expect(onRemoveAttachment).toHaveBeenCalledWith("att-img");
  });

  it("keeps the file name on an image tile whose bytes cannot be shown", () => {
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        attachments={[{
          receipt: { hostId: "local", attachmentId: "att-2", sha256: "abc", sizeBytes: 10, mediaType: "image/jpeg" },
          displayName: "pic.jpg",
          mentionText: "@pic.jpg ",
        }]}
      />,
    );
    fireEvent.click(screen.getByTestId("chat-composer-files-toggle"));
    expect(screen.queryByTestId("attachment-thumbnail-att-2")).not.toBeInTheDocument();
    expect(screen.getByTestId("attachment-preview-att-2")).toHaveTextContent("pic.jpg");
  });

  it("toggles dictation with mod+shift+D", () => {
    class FakeSpeech {
      onresult: ((event: unknown) => void) | null = null;
      onerror: ((event: unknown) => void) | null = null;
      onend: (() => void) | null = null;
      start() {}
      stop() {}
      abort() {}
    }
    (window as { SpeechRecognition?: unknown }).SpeechRecognition = FakeSpeech;
    try {
      render(<MobileChatComposer onSend={vi.fn()} value="keep me" dictationFence="pane-a" />);
      const mic = screen.getByTestId("dictation-mic-button");
      expect(mic).toHaveAttribute("aria-pressed", "false");
      fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), {
        key: "d",
        keyCode: 68,
        metaKey: true,
        shiftKey: true,
      });
      expect(mic).toHaveAttribute("aria-pressed", "true");
      fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), {
        key: "D",
        keyCode: 68,
        ctrlKey: true,
        shiftKey: true,
      });
      expect(mic).toHaveAttribute("aria-pressed", "false");
    } finally {
      delete (window as { SpeechRecognition?: unknown }).SpeechRecognition;
    }
  });
});
