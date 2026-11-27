/**
 * Falsification probes for G3 at the composer surface, against t3code docs/user/composer.md.
 *
 * Each case pins a documented promise: "On a hardware keyboard, use Cmd+Shift+V ... to keep a
 * large paste editable" (that paste only), "When the composer has text, the arrow keys move the
 * caret as usual", "Enter sends. In a list or quote, Shift+Enter continues it", and the queued
 * edit's ending rule.
 */
import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, act } from "@testing-library/react";
import { forwardRef, useState } from "react";
import { chipMarkdown, citationMarkdown, parseCitation } from "./composer/chips";
import { parseInline } from "./composer/markdown";
import { MobileChatComposer, type MobileChatComposerHandle } from "./MobileChatComposer";

const DraftingComposer = forwardRef<
  MobileChatComposerHandle,
  Omit<Parameters<typeof MobileChatComposer>[0], "value" | "onValueChange"> & {
    onValueChange?: (text: string) => void;
  }
>(function DraftingComposer({ onValueChange, ...rest }, ref) {
  const [text, setText] = useState("");
  return (
    <MobileChatComposer
      ref={ref}
      {...rest}
      value={text}
      onValueChange={(next) => {
        setText(next);
        onValueChange?.(next);
      }}
    />
  );
});

const imageReceipt = {
  receipt: {
    hostId: "local",
    attachmentId: "att-1",
    sha256: "abc",
    sizeBytes: 10,
    mediaType: "image/png" as const,
  },
  displayName: "shot.png",
  mentionText: "@shot.png ",
};

const queuedRow = (id: string, text: string) => ({
  id,
  text,
  ownerId: "o",
  epoch: "1",
  phase: "queued" as const,
  requestId: null,
});

function pasteEvent(init: { text?: string; files?: File[] }) {
  return {
    clipboardData: {
      files: init.files ?? [],
      getData: (type: string) => (type === "text/plain" ? (init.text ?? "") : ""),
    },
    preventDefault: vi.fn(),
  };
}

describe("P1 the keep-editable paste chord belongs to its own paste", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  // composer.md: "Pasting 32 KiB or more of text adds that fragment as a text-file attachment...
  // use Cmd+Shift+V ... to keep a large paste editable in the composer instead." The shortcut is
  // for that paste; a later plain paste must still become an attachment.
  it("attaches a plain 32 KiB text paste after a Ctrl+Shift+V image paste", () => {
    const onAttachFiles = vi.fn();
    render(<MobileChatComposer onSend={vi.fn()} onAttachFiles={onAttachFiles} />);
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.keyDown(textarea, { key: "v", keyCode: 86, ctrlKey: true, shiftKey: true });
    fireEvent.paste(textarea, pasteEvent({ files: [new File(["b"], "shot.png", { type: "image/png" })] }));
    expect(onAttachFiles).toHaveBeenCalledTimes(1);
    fireEvent.paste(textarea, pasteEvent({ text: "y".repeat(32 * 1024) }));
    expect(onAttachFiles).toHaveBeenCalledTimes(2);
    expect(onAttachFiles.mock.calls[1][0][0].name).toBe("pasted text.txt");
  });

  it("attaches a plain 32 KiB text paste after a Ctrl+Shift+V paste of nothing", () => {
    const onAttachFiles = vi.fn();
    render(<MobileChatComposer onSend={vi.fn()} onAttachFiles={onAttachFiles} />);
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.keyDown(textarea, { key: "v", keyCode: 86, ctrlKey: true, shiftKey: true });
    fireEvent.paste(textarea, pasteEvent({ text: "" }));
    fireEvent.paste(textarea, pasteEvent({ text: "z".repeat(32 * 1024) }));
    expect(onAttachFiles).toHaveBeenCalledTimes(1);
  });
});

describe("P2 recall obeys the documented empty rule", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  it("does not recall while the composer holds an attachment", () => {
    const onValueChange = vi.fn();
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        value=""
        onValueChange={onValueChange}
        attachments={[imageReceipt]}
        sentPrompts={["earlier prompt"]}
      />,
    );
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "ArrowUp", keyCode: 38 });
    expect(onValueChange).not.toHaveBeenCalled();
  });

  // composer.md: "When the composer has text, the arrow keys move the caret as usual." t3code
  // gates recall on `currentPrompt.length > 0` (composerPromptHistory.ts stepComposerPromptHistory),
  // so a whitespace-only draft is text and the caret moves.
  it("moves the caret instead of replacing a whitespace-only draft", () => {
    const onValueChange = vi.fn();
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        value="   "
        onValueChange={onValueChange}
        sentPrompts={["earlier prompt"]}
      />,
    );
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "ArrowUp", keyCode: 38 });
    expect(onValueChange).not.toHaveBeenCalled();
  });
});

describe("P3 follow-up shortcuts leave the draft alone", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  it("steers the oldest queued message and keeps the current draft", () => {
    const onSendQueuedNow = vi.fn();
    const onValueChange = vi.fn();
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        isRunning
        value="my draft"
        onValueChange={onValueChange}
        queuedMessages={[queuedRow("q1", "oldest"), queuedRow("q2", "newer")]}
        onSendQueuedNow={onSendQueuedNow}
      />,
    );
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), {
      key: "Enter",
      keyCode: 13,
      metaKey: true,
      shiftKey: true,
    });
    expect(onSendQueuedNow).toHaveBeenCalledWith("q1");
    expect(onValueChange).not.toHaveBeenCalled();
  });
});

describe("P4 Korean IME composition", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  it("does not send while composing and sends the composed text afterwards", () => {
    const onSend = vi.fn(() => true);
    render(<DraftingComposer onSend={onSend} />);
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.compositionStart(textarea);
    fireEvent.change(textarea, { target: { value: "안녕하세요", selectionStart: 5 } });
    fireEvent.keyDown(textarea, { key: "Enter", keyCode: 13 });
    expect(onSend).not.toHaveBeenCalled();
    fireEvent.compositionEnd(textarea);
    fireEvent.keyDown(textarea, { key: "Enter", keyCode: 13 });
    expect(onSend).toHaveBeenCalledWith("안녕하세요");
  });
});

describe("P5 a markdown list in the draft stays exactly the text that is sent", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  // composer.md:25 "What you typed is what the agent receives, markers and numbering included".
  // The composer is a plain textarea, so the markers are literal draft text: no document is
  // parsed and none is re-serialized behind the user's back.
  it("sends the list markers the user typed, unchanged", () => {
    const onSend = vi.fn(() => true);
    const draft = "- item **bold**";
    render(<MobileChatComposer onSend={onSend} value={draft} onValueChange={() => {}} />);
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue(draft);
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "Enter", keyCode: 13 });
    expect(onSend).toHaveBeenCalledWith(draft);
  });

  it("keeps a task-list line literal in the draft", () => {
    const draft = "- [ ] one\n- [ ] two";
    render(<MobileChatComposer onSend={vi.fn()} value={draft} onValueChange={() => {}} />);
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue(draft);
  });
});

describe("P6 Enter sends from a list or a quote", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  // Control: the same harness must send from a plain paragraph, so a list failure below is the
  // list rule and not a probe that never reaches the handler.
  it("sends on Enter with the caret in a plain paragraph", () => {
    const onSend = vi.fn(() => true);
    render(<MobileChatComposer onSend={onSend} value="hello" onValueChange={() => {}} />);
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "Enter", keyCode: 13 });
    expect(onSend).toHaveBeenCalledWith("hello");
  });

  // composer.md: "Enter sends. In a list or quote, Shift+Enter continues it".
  it("sends on Enter with the caret in a list item", () => {
    const onSend = vi.fn(() => true);
    render(<MobileChatComposer onSend={onSend} value="- item one" onValueChange={() => {}} />);
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "Enter", keyCode: 13 });
    expect(onSend).toHaveBeenCalledWith("- item one");
  });

  it("sends on Enter with the caret in a block quote", () => {
    const onSend = vi.fn(() => true);
    render(<MobileChatComposer onSend={onSend} value="> quoted" onValueChange={() => {}} />);
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "Enter", keyCode: 13 });
    expect(onSend).toHaveBeenCalledWith("> quoted");
  });

  it("continues the line instead of sending on Shift+Enter", () => {
    const onSend = vi.fn(() => true);
    render(<MobileChatComposer onSend={onSend} value="- item one" onValueChange={() => {}} />);
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "Enter", keyCode: 13, shiftKey: true });
    expect(onSend).not.toHaveBeenCalled();
  });
});

describe("P8 Tab is a plain-text tab, not a list nest", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  // The composer no longer owns a rich document, so Tab must never rewrite the draft: it either
  // moves focus or is left to the browser. Nothing in the draft changes either way.
  it("leaves the task list draft untouched on Tab", () => {
    const onValueChange = vi.fn();
    const draft = "- [ ] one\n- [ ] two";
    render(<MobileChatComposer onSend={vi.fn()} value={draft} onValueChange={onValueChange} />);
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "Tab", keyCode: 9 });
    expect(onValueChange).not.toHaveBeenCalled();
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue(draft);
  });

  it("leaves the bullet list draft untouched on Tab", () => {
    const onValueChange = vi.fn();
    const draft = "- one\n- two";
    render(<MobileChatComposer onSend={vi.fn()} value={draft} onValueChange={onValueChange} />);
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "Tab", keyCode: 9 });
    expect(onValueChange).not.toHaveBeenCalled();
  });
});

describe("P10 the documented chords reach their controls", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  it("cycles the permission mode on mod+shift+a", () => {
    const onChange = vi.fn();
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        permissionControl={{ modes: ["supervised", "auto-accept-edits", "auto"], current: "supervised", onChange }}
      />,
    );
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), {
      key: "a",
      keyCode: 65,
      ctrlKey: true,
      shiftKey: true,
    });
    expect(onChange).toHaveBeenCalledWith("auto-accept-edits");
  });

  it("stashes the draft under a ferryx.* key on mod+s", () => {
    localStorage.clear();
    render(<MobileChatComposer onSend={vi.fn()} value="keep me" onValueChange={() => {}} />);
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "s", keyCode: 83, ctrlKey: true });
    const keys = Object.keys(localStorage);
    expect(keys.some((key) => key.startsWith("ferryx."))).toBe(true);
  });

  it("does not send an over-limit draft", () => {
    const onSend = vi.fn(() => true);
    render(<MobileChatComposer onSend={onSend} value={"x".repeat(120_001)} onValueChange={() => {}} />);
    expect(screen.getByTestId("chat-composer-length-error")).toBeInTheDocument();
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "Enter", keyCode: 13 });
    expect(onSend).not.toHaveBeenCalled();
  });

  it("sends a draft of exactly the limit", () => {
    const onSend = vi.fn(() => true);
    render(<MobileChatComposer onSend={onSend} value={"x".repeat(120_000)} onValueChange={() => {}} />);
    expect(screen.queryByTestId("chat-composer-length-error")).toBeNull();
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "Enter", keyCode: 13 });
    expect(onSend).toHaveBeenCalledTimes(1);
  });
});

describe("P11 mod+enter takes the opposite follow-up action", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  const caps = (queue: boolean, steer: boolean) => ({
    models: null,
    options: [],
    slashCommands: [],
    followUp: { queue, steer },
  });

  it("queues when the button steers", () => {
    const onQueue = vi.fn();
    const onSteer = vi.fn();
    render(
      <MobileChatComposer
        onSend={vi.fn(() => true)}
        isRunning
        followUpDefault="steer"
        composerCapabilities={caps(true, true) as never}
        value="draft"
        onValueChange={() => {}}
        onQueue={onQueue}
        onSteer={onSteer}
      />,
    );
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), {
      key: "Enter",
      keyCode: 13,
      ctrlKey: true,
    });
    expect(onQueue).toHaveBeenCalledWith("draft");
    expect(onSteer).not.toHaveBeenCalled();
  });

  it("steers when the button queues", () => {
    const onQueue = vi.fn();
    const onSteer = vi.fn();
    render(
      <MobileChatComposer
        onSend={vi.fn(() => true)}
        isRunning
        followUpDefault="queue"
        composerCapabilities={caps(true, true) as never}
        value="draft"
        onValueChange={() => {}}
        onQueue={onQueue}
        onSteer={onSteer}
      />,
    );
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), {
      key: "Enter",
      keyCode: 13,
      ctrlKey: true,
    });
    expect(onSteer).toHaveBeenCalledWith("draft");
    expect(onQueue).not.toHaveBeenCalled();
  });
});

describe("P12 a chip whose label holds brackets still round-trips", () => {
  // composer.md: copying a message gives "a link in place of each chip", and a chip in the draft
  // is markdown the agent reads. A pull request titled "[WIP] fix" must survive that link.
  it("serializes a bracketed chip label into a link that parses back", () => {
    const markdown = chipMarkdown({ kind: "pr", id: "42", label: "[WIP] fix" });
    const tokens = parseInline(markdown);
    expect(tokens.some((token) => token.type === "link")).toBe(true);
  });

  it("keeps a plain chip label as a link", () => {
    const markdown = chipMarkdown({ kind: "pr", id: "42", label: "plain title" });
    const tokens = parseInline(markdown);
    expect(tokens.some((token) => token.type === "link")).toBe(true);
  });
});

describe("P13 image paste adds an attachment at the caret", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  it("attaches a pasted image", () => {
    const onAttachFiles = vi.fn();
    render(<MobileChatComposer onSend={vi.fn()} onAttachFiles={onAttachFiles} />);
    fireEvent.paste(
      screen.getByTestId("chat-composer-textarea"),
      pasteEvent({ files: [new File(["i"], "shot.png", { type: "image/png" })] }),
    );
    expect(onAttachFiles).toHaveBeenCalledTimes(1);
  });

  it("does not attach a pasted non-image file", () => {
    const onAttachFiles = vi.fn();
    render(<MobileChatComposer onSend={vi.fn()} onAttachFiles={onAttachFiles} />);
    fireEvent.paste(
      screen.getByTestId("chat-composer-textarea"),
      pasteEvent({ files: [new File(["t"], "notes.txt", { type: "text/plain" })] }),
    );
    expect(onAttachFiles).not.toHaveBeenCalled();
  });
});

describe("P14 the composer survives a quote and comment round trip", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  it("writes the comment beside the quote and reads it back", () => {
    const markdown = citationMarkdown({ messageId: "m1", text: "quoted words" }, "my comment");
    expect(parseCitation(`ferryx://cite/m1`, "my comment")).toEqual({ messageId: "m1", comment: "my comment" });
    expect(markdown).toContain("quoted words");
    expect(markdown).toContain("my comment");
  });

  it("renders the citation row for a single-line quote", () => {
    const ref = { current: null as MobileChatComposerHandle | null };
    render(<DraftingComposer ref={ref} onSend={vi.fn()} />);
    act(() => {
      ref.current!.insertCitation({ messageId: "m1", text: "one line only" });
    });
    expect(screen.getByTestId("composer-citations")).toBeInTheDocument();
  });
});

describe("P15 mod+shift+d toggles dictation", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  it("starts a dictation session on mod+shift+d", () => {
    const start = vi.fn();
    class FakeRecognition {
      lang = "";
      continuous = false;
      interimResults = false;
      start = start;
      stop = vi.fn();
      abort = vi.fn();
      addEventListener = vi.fn();
      removeEventListener = vi.fn();
    }
    const scope = window as unknown as { SpeechRecognition?: unknown; webkitSpeechRecognition?: unknown };
    scope.SpeechRecognition = FakeRecognition;
    scope.webkitSpeechRecognition = FakeRecognition;
    render(<MobileChatComposer onSend={vi.fn()} />);
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), {
      key: "d",
      keyCode: 68,
      ctrlKey: true,
      shiftKey: true,
    });
    expect(start).toHaveBeenCalled();
    delete scope.SpeechRecognition;
    delete scope.webkitSpeechRecognition;
  });
});

describe("P9 a citation chip survives a quote that spans lines", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  // composer.md: a citation is a chip whose comment is editable and whose quote is sent.
  it("renders a citation row for a two-line quote", () => {
    const ref = { current: null as MobileChatComposerHandle | null };
    render(<DraftingComposer ref={ref} onSend={vi.fn()} />);
    act(() => {
      ref.current!.insertCitation({ messageId: "m1", text: "line one\nline two" });
    });
    expect(screen.getByTestId("composer-citations")).toBeInTheDocument();
  });
});

describe("P7 a queued edit that ends while open", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  function EditHost({ rows }: { rows: ReturnType<typeof queuedRow>[] }) {
    const [text, setText] = useState("");
    return (
      <MobileChatComposer
        value={text}
        onValueChange={setText}
        onSend={vi.fn()}
        queuedMessages={rows}
        onEditQueued={vi.fn()}
      />
    );
  }

  it("keeps the changed content when the queued message goes away and the draft was empty", () => {
    const { rerender } = render(<EditHost rows={[queuedRow("q1", "queued text")]} />);
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.click(screen.getByTestId("queued-follow-up-edit-q1"));
    fireEvent.change(textarea, { target: { value: "queued text edited" } });
    rerender(<EditHost rows={[]} />);
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("queued text edited");
  });

  it("discards the changed content when a draft was already there", () => {
    const { rerender } = render(<EditHost rows={[queuedRow("q1", "queued text")]} />);
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "my own draft" } });
    fireEvent.click(screen.getByTestId("queued-follow-up-edit-q1"));
    fireEvent.change(textarea, { target: { value: "queued text edited" } });
    rerender(<EditHost rows={[]} />);
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("my own draft");
  });
});
