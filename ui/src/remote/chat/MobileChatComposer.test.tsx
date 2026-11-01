/**
 * Composer regressions (plan task 12).
 *
 *
 * The composer no longer keeps browser-side attachment blobs and no longer blocks sending while
 * one is attached: a file is staged on the OWNING host and its mention is plain text in the
 * draft. The scenarios that asserted the blocked-attachment path are replaced by the contract
 * that exists now, and the send/IME/stop scenarios are kept.
 */
import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, fireEvent } from "@testing-library/react";
import { MobileChatComposer } from "./MobileChatComposer";
import type { ReferenceFileReceipt } from "./referenceTypes";

function stagedFile(overrides: Partial<ReferenceFileReceipt> = {}): ReferenceFileReceipt {
  return {
    receipt: { hostId: "local", attachmentId: "att-1", sha256: "abc", sizeBytes: 2048, mediaType: "text/plain" },
    displayName: "notes.txt",
    mentionText: "@notes.txt ",
    ...overrides,
  };
}

function setCoarsePointer(coarse: boolean) {
  window.matchMedia = ((media: string) =>
    Object.assign(new EventTarget(), {
      media,
      matches: coarse && media === "(pointer: coarse)",
      onchange: null,
      addListener() {},
      removeListener() {},
    })) as unknown as typeof window.matchMedia;
}

describe("MobileChatComposer touch safety", () => {
  const originalMatchMedia = window.matchMedia;
  beforeEach(cleanup);
  afterEach(() => {
    cleanup();
    window.matchMedia = originalMatchMedia;
  });

  it("puts Stop at the opposite edge from Send while a prompt is open", () => {
    render(
      <MobileChatComposer onSend={vi.fn()} onStop={vi.fn()} isRunning promptOpen value="1" />,
    );
    const stop = screen.getByTestId("stop-button");
    const send = screen.getByTestId("send-button");
    expect(stop).toHaveClass("h-11");
    expect(stop).toHaveTextContent("Stop");
    // different flex groups at either end of a justify-between row
    expect(stop.parentElement).not.toBe(send.parentElement);
    expect(send.parentElement).toHaveClass("justify-between", "gap-3");
  });

  it("needs a second tap to stop on a touch screen", () => {
    setCoarsePointer(true);
    const onStop = vi.fn();
    render(<MobileChatComposer onSend={vi.fn()} onStop={onStop} isRunning />);
    fireEvent.click(screen.getByTestId("stop-button"));
    expect(onStop).not.toHaveBeenCalled();
    expect(screen.getByTestId("stop-button")).toHaveAccessibleName("Tap again to stop the running turn");
    fireEvent.click(screen.getByTestId("stop-button"));
    expect(onStop).toHaveBeenCalledTimes(1);
  });

  it("keeps Enter as a new line on a touch screen; the Send button sends", () => {
    setCoarsePointer(true);
    const onSend = vi.fn();
    render(<MobileChatComposer onSend={onSend} />);
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "line one" } });
    fireEvent.keyDown(textarea, { key: "Enter", keyCode: 13 });
    expect(onSend).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("send-button"));
    expect(onSend).toHaveBeenCalledWith("line one");
  });

  it("names held rows with a short excerpt, not the whole message", () => {
    const long = "a".repeat(200);
    render(<MobileChatComposer onSend={vi.fn()} heldMessages={[{ id: "h", text: long }]} />);
    const name = screen.getByTestId("held-message-send-h").getAttribute("aria-label") ?? "";
    expect(name.length).toBeLessThan(70);
    expect(screen.getByTestId("held-message-remove-h")).toHaveClass("size-11");
  });

  it("draws the warning's action as a 44px button and runs it", () => {
    const onAction = vi.fn();
    const { rerender } = render(<MobileChatComposer onSend={vi.fn()} warning="Lost the connection." />);
    expect(screen.queryByTestId("chat-composer-warning-action")).not.toBeInTheDocument();

    rerender(
      <MobileChatComposer
        onSend={vi.fn()}
        warning="Lost the connection."
        warningAction={{ label: "Reconnect", onAction }}
      />,
    );
    const action = screen.getByRole("button", { name: "Reconnect" });
    expect(action).toHaveClass("min-h-11");
    expect(screen.getByTestId("chat-composer-warning")).toContainElement(action);
    fireEvent.click(action);
    expect(onAction).toHaveBeenCalledTimes(1);
  });

  it("gives held-row send, edit and remove 44px hit areas", () => {
    render(<MobileChatComposer onSend={vi.fn()} heldMessages={[{ id: "h", text: "later" }]} />);
    for (const id of ["held-message-send-h", "held-message-edit-h", "held-message-remove-h"]) {
      expect(screen.getByTestId(id)).toHaveClass("size-11");
    }
  });

  it("caps the banners and held rows in their own scroller and never draws a prompt card in the footer", () => {
    render(
      <MobileChatComposer onSend={vi.fn()} promptOpen warning="w" heldMessages={[{ id: "h", text: "later" }]} />,
    );
    expect(screen.getByTestId("chat-composer-extras")).toHaveClass("overflow-y-auto");
    expect(screen.getByTestId("chat-composer-extras")).toContainElement(screen.getByTestId("chat-composer-held-toggle"));
    expect(screen.queryByTestId("chat-composer-prompt")).not.toBeInTheDocument();
  });

  it("folds staged files and held rows behind count chips that expand on tap", () => {
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        attachments={[stagedFile(), stagedFile({ receipt: { ...stagedFile().receipt, attachmentId: "att-2" } })]}
        heldMessages={[{ id: "h", text: "later" }]}
      />,
    );
    const files = screen.getByTestId("chat-composer-files-toggle");
    const held = screen.getByTestId("chat-composer-held-toggle");
    expect(files).toHaveTextContent("2 files");
    expect(held).toHaveTextContent("1 held");
    expect(files).toHaveClass("min-h-11");
    expect(held).toHaveClass("min-h-11");
    expect(files).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByTestId("chat-composer-attachments")).not.toBeVisible();
    expect(screen.getByTestId("chat-composer-held")).not.toBeVisible();

    fireEvent.click(files);
    expect(files).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("chat-composer-attachments")).toBeVisible();
    expect(screen.getByTestId("chat-composer-held")).not.toBeVisible();

    fireEvent.click(held);
    expect(held).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("chat-composer-held")).toBeVisible();
    expect(screen.getByRole("button", { name: "Send held message later" })).toBeInTheDocument();
  });
});

describe("MobileChatComposer", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  it("host-staged attachments allocate no blob URLs on selection, send, removal or unmount", () => {
    const create = vi.fn(() => "blob:attachment");
    const revoke = vi.fn();
    const originalCreate = URL.createObjectURL;
    const originalRevoke = URL.revokeObjectURL;
    URL.createObjectURL = create;
    URL.revokeObjectURL = revoke;
    try {
      const attach = vi.fn();
      const remove = vi.fn();
      const send = vi.fn();
      const { unmount } = render(<MobileChatComposer
        onSend={send} value="@notes.txt" attachments={[stagedFile()]}
        onAttachFiles={attach} onRemoveAttachment={remove}
      />);
      const image = new File(["image"], "image.png", { type: "image/png" });
      fireEvent.change(screen.getByTestId("file-upload-input"), { target: { files: [image] } });
      expect(attach).toHaveBeenCalledWith([image], expect.any(Number));
      fireEvent.click(screen.getByTestId("send-button"));
      expect(send).toHaveBeenCalledWith("@notes.txt");
      fireEvent.click(screen.getByTestId("remove-attachment-att-1"));
      expect(remove).toHaveBeenCalledWith("att-1");
      unmount();
      // No browser-owned resource exists to revoke: host receipts replace blob previews.
      expect(create).not.toHaveBeenCalled();
      expect(revoke).not.toHaveBeenCalled();
    } finally {
      URL.createObjectURL = originalCreate;
      URL.revokeObjectURL = originalRevoke;
    }
  });

  it("renders the composer row with attach and a 44px send control, and no dead mic button", () => {
    const onSend = vi.fn();
    render(<MobileChatComposer onSend={onSend} />);

    expect(screen.getByTestId("chat-composer-textarea")).toHaveAttribute("placeholder", "Ask the agent…");
    expect(screen.getByTestId("attach-file-button")).toBeInTheDocument();
    expect(screen.queryByTestId("mic-button")).not.toBeInTheDocument();
    expect(screen.getByTestId("send-button")).toHaveClass("size-11");
    expect(screen.getByTestId("attach-file-button")).toHaveClass("size-11");
    expect(screen.getByTestId("chat-composer-textarea")).toHaveAccessibleName("Ask the agent");
    expect(screen.getByTestId("file-upload-input")).toBeInTheDocument();
    expect(screen.queryByTestId("mobile-chat-quick-actions")).not.toBeInTheDocument();
    expect(screen.queryByTestId("terminal-accessory-bar")).not.toBeInTheDocument();
  });

  it("handles Korean IME composition safely and sends the text alone", () => {
    const onSend = vi.fn();
    render(<MobileChatComposer onSend={onSend} />);

    const textarea = screen.getByTestId("chat-composer-textarea");
    const sendButton = screen.getByTestId("send-button");
    expect(sendButton).toBeDisabled();

    fireEvent.change(textarea, { target: { value: "안녕하세요" } });
    expect(sendButton).not.toBeDisabled();

    fireEvent.compositionStart(textarea);
    fireEvent.keyDown(textarea, { key: "Enter" });
    expect(onSend).not.toHaveBeenCalled();

    fireEvent.compositionEnd(textarea);
    fireEvent.keyDown(textarea, { key: "Enter" });
    expect(onSend).toHaveBeenCalledWith("안녕하세요");
  });

  it("does not send on Enter while the IME is confirming (isComposing or keyCode 229)", () => {
    const onSend = vi.fn();
    render(<MobileChatComposer onSend={onSend} />);

    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "안녕하세요" } });

    fireEvent.keyDown(textarea, { key: "Enter", isComposing: true });
    expect(onSend).not.toHaveBeenCalled();
    fireEvent.keyDown(textarea, { key: "Enter", keyCode: 229 });
    expect(onSend).not.toHaveBeenCalled();

    fireEvent.keyDown(textarea, { key: "Enter", keyCode: 13 });
    expect(onSend).toHaveBeenCalledTimes(1);
  });

  it("toggles between send and stop buttons based on isRunning prop", () => {
    const onSend = vi.fn();
    const onStop = vi.fn();
    const { rerender } = render(<MobileChatComposer onSend={onSend} onStop={onStop} isRunning={false} />);

    expect(screen.getByTestId("send-button")).toBeInTheDocument();
    expect(screen.queryByTestId("stop-button")).not.toBeInTheDocument();

    rerender(<MobileChatComposer onSend={onSend} onStop={onStop} isRunning={true} />);
    expect(screen.queryByTestId("send-button")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("stop-button"));
    expect(onStop).toHaveBeenCalledTimes(1);
  });

  it("does not call onSend with an empty or whitespace-only draft", () => {
    const onSend = vi.fn();
    render(<MobileChatComposer onSend={onSend} />);

    const textarea = screen.getByTestId("chat-composer-textarea");
    expect(screen.getByTestId("send-button")).toBeDisabled();
    fireEvent.click(screen.getByTestId("send-button"));
    expect(onSend).not.toHaveBeenCalled();

    fireEvent.change(textarea, { target: { value: "   \n  " } });
    expect(screen.getByTestId("send-button")).toBeDisabled();
    fireEvent.keyDown(textarea, { key: "Enter" });
    expect(onSend).not.toHaveBeenCalled();
  });

  it("does not send while a file is still staging, so the file is not left behind", () => {
    const onSend = vi.fn();
    const { rerender } = render(<MobileChatComposer onSend={onSend} value="see this" attaching={true} />);
    expect(screen.getByTestId("send-button")).toBeDisabled();
    fireEvent.click(screen.getByTestId("send-button"));
    fireEvent.keyDown(screen.getByTestId("chat-composer-textarea"), { key: "Enter" });
    expect(onSend).not.toHaveBeenCalled();

    rerender(<MobileChatComposer onSend={onSend} value="see this @shot.png " attaching={false} />);
    fireEvent.click(screen.getByTestId("send-button"));
    expect(onSend).toHaveBeenCalledWith("see this @shot.png");
  });

  it("keeps the draft when the owner refuses the send", () => {
    const onSend = vi.fn(() => false);
    render(<MobileChatComposer onSend={onSend} />);

    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "held text" } });
    fireEvent.click(screen.getByTestId("send-button"));

    expect(onSend).toHaveBeenCalledWith("held text");
    // the owner kept it (a prompt took it as a held row): the box must not clear
    expect((textarea as HTMLTextAreaElement).value).toBe("held text");
  });

  it("reports the caret it wants a staged file inserted at", () => {
    const onAttachFiles = vi.fn();
    render(<MobileChatComposer onSend={vi.fn()} onAttachFiles={onAttachFiles} />);

    const textarea = screen.getByTestId("chat-composer-textarea") as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: "look at this" } });
    Object.defineProperty(textarea, "selectionStart", { value: 5, configurable: true });

    const file = new File(["abc"], "notes.txt", { type: "text/plain" });
    fireEvent.change(screen.getByTestId("file-upload-input"), { target: { files: [file] } });

    expect(onAttachFiles).toHaveBeenCalledTimes(1);
    expect(onAttachFiles.mock.calls[0][0]).toEqual([file]);
    expect(onAttachFiles.mock.calls[0][1]).toBe(5);
  });

  it("shows a staged file with its mention and removes it explicitly", () => {
    const onRemoveAttachment = vi.fn();
    render(
      <MobileChatComposer onSend={vi.fn()} attachments={[stagedFile()]} onRemoveAttachment={onRemoveAttachment} />
    );

    const chip = screen.getByTestId("attachment-preview-att-1");
    expect(chip).toHaveTextContent("notes.txt");
    expect(chip).toHaveTextContent("@notes.txt");
    fireEvent.click(screen.getByTestId("remove-attachment-att-1"));
    expect(onRemoveAttachment).toHaveBeenCalledWith("att-1");
    // nothing about the old blocked-attachment path is left
    expect(screen.queryByTestId("chat-composer-attachments-blocked")).not.toBeInTheDocument();
  });

  it("sends while a staged file is attached: the mention is plain text, not an upload", () => {
    const onSend = vi.fn();
    render(<MobileChatComposer onSend={onSend} attachments={[stagedFile()]} value="@notes.txt " />);

    expect(screen.getByTestId("send-button")).not.toBeDisabled();
    fireEvent.click(screen.getByTestId("send-button"));
    expect(onSend).toHaveBeenCalledWith("@notes.txt");
  });

  it("shows an unsaved draft notice without discarding the draft", () => {
    render(<MobileChatComposer onSend={vi.fn()} value="kept locally" draftUnsaved={true} />);

    expect(screen.getByTestId("chat-composer-draft-unsaved")).toHaveTextContent(/could not be saved/i);
    expect(screen.getByTestId("chat-composer-textarea")).toHaveValue("kept locally");
  });

  it("renders held rows and only sends one when its own button is pressed", () => {
    const onSendHeld = vi.fn();
    const onEditHeld = vi.fn();
    const onRemoveHeld = vi.fn();
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        heldMessages={[{ id: "held-1", text: "waiting to send" }]}
        onSendHeld={onSendHeld}
        onEditHeld={onEditHeld}
        onRemoveHeld={onRemoveHeld}
      />
    );

    const row = screen.getByTestId("held-message-held-1");
    expect(row).toHaveTextContent("waiting to send");
    expect(onSendHeld).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("held-message-send-held-1"));
    expect(onSendHeld).toHaveBeenCalledWith("held-1");

    fireEvent.click(screen.getByTestId("held-message-edit-held-1"));
    fireEvent.change(screen.getByTestId("held-message-input-held-1"), { target: { value: "edited" } });
    fireEvent.keyDown(screen.getByTestId("held-message-input-held-1"), { key: "Enter" });
    expect(onEditHeld).toHaveBeenCalledWith("held-1", "edited");

    fireEvent.click(screen.getByTestId("held-message-remove-held-1"));
    expect(onRemoveHeld).toHaveBeenCalledWith("held-1");
  });

  it("does not commit a held edit on the Enter that confirms an IME candidate (Safari keyCode 229)", () => {
    const onEditHeld = vi.fn();
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        heldMessages={[{ id: "held-1", text: "waiting to send" }]}
        onEditHeld={onEditHeld}
      />
    );
    fireEvent.click(screen.getByTestId("held-message-edit-held-1"));
    const input = screen.getByTestId("held-message-input-held-1");
    fireEvent.change(input, { target: { value: "안녕" } });
    fireEvent.keyDown(input, { key: "Enter", keyCode: 229 });
    expect(onEditHeld).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter", keyCode: 13 });
    expect(onEditHeld).toHaveBeenCalledWith("held-1", "안녕");
  });

  it("keeps Send while a prompt is open and reports attach progress", () => {
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        promptOpen
        isRunning
        attaching={true}
        attachError="the host refused the file"
        warning="The stop request was not delivered."
      />
    );

    expect(screen.getByTestId("send-button")).toBeInTheDocument();
    expect(screen.getByTestId("attach-file-button")).toBeDisabled();
    expect(screen.getByTestId("chat-composer-attach-error")).toHaveTextContent("refused");
    expect(screen.getByTestId("chat-composer-warning")).toHaveTextContent("stop request");
  });

  it("provides title tooltips matching accessible names on icon buttons (+, send, stop)", () => {
    const { rerender } = render(
      <MobileChatComposer onSend={vi.fn()} onStop={vi.fn()} isRunning={false} value="hello" />
    );

    expect(screen.getByTestId("attach-file-button")).toHaveAttribute("title", "Attach file");
    expect(screen.getByTestId("send-button")).toHaveAttribute("title", "Send message");

    rerender(
      <MobileChatComposer onSend={vi.fn()} onStop={vi.fn()} isRunning={true} value="hello" />
    );
    expect(screen.getByTestId("stop-button")).toHaveAttribute("title", "Stop the running turn");
  });
});
