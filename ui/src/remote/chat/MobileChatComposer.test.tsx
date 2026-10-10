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

  it("renders the T3 composer row with attach, mic, and circular send controls", () => {
    const onSend = vi.fn();
    render(<MobileChatComposer onSend={onSend} />);

    expect(screen.getByTestId("chat-composer-textarea")).toHaveAttribute("placeholder", "Ask the agent…");
    expect(screen.getByTestId("attach-file-button")).toBeInTheDocument();
    expect(screen.getByTestId("mic-button")).toBeDisabled();
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

  it("draws the prompt card above the input row and reports attach progress", () => {
    render(
      <MobileChatComposer
        onSend={vi.fn()}
        promptCard={<div data-testid="stub-card">approve?</div>}
        attaching={true}
        attachError="the host refused the file"
        warning="The stop request was not delivered."
      />
    );

    expect(screen.getByTestId("chat-composer-prompt")).toHaveTextContent("approve?");
    expect(screen.getByTestId("attach-file-button")).toBeDisabled();
    expect(screen.getByTestId("chat-composer-attach-error")).toHaveTextContent("refused");
    expect(screen.getByTestId("chat-composer-warning")).toHaveTextContent("stop request");
  });
});
