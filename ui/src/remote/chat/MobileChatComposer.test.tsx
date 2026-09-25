import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, fireEvent } from "@testing-library/react";
import {
  MobileChatQuickActions,
  DEFAULT_QUICK_ACTIONS,
} from "./MobileChatQuickActions";
import { MobileChatComposer } from "./MobileChatComposer";

describe("MobileChatQuickActions", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  it("renders default quick action chips", () => {
    const onSelectAction = vi.fn();
    render(<MobileChatQuickActions onSelectAction={onSelectAction} />);

    expect(screen.getByTestId("mobile-chat-quick-actions")).toBeInTheDocument();
    expect(screen.getByText("Git status")).toBeInTheDocument();
    expect(screen.getByText("Run tests")).toBeInTheDocument();
    expect(screen.getByText("Explain")).toBeInTheDocument();
    expect(screen.getByText("Review diff")).toBeInTheDocument();
    expect(screen.getByText("Stop")).toBeInTheDocument();

    fireEvent.click(screen.getByText("Git status"));
    expect(onSelectAction).toHaveBeenCalledWith(DEFAULT_QUICK_ACTIONS[0]);
  });

  it("handles isRunning state on Stop action chip", () => {
    const onSelectAction = vi.fn();
    render(
      <MobileChatQuickActions
        onSelectAction={onSelectAction}
        isRunning={true}
      />
    );
    const stopButton = screen.getByTestId("quick-action-stop");
    expect(stopButton.className).toContain("text-red-400");
  });
});

describe("MobileChatComposer", () => {
  beforeEach(cleanup);
  afterEach(cleanup);

  it("renders the T3 composer row with attach, mic, and circular send controls", () => {
    const onSend = vi.fn();
    render(<MobileChatComposer onSend={onSend} />);

    const textarea = screen.getByTestId("chat-composer-textarea");
    expect(textarea).toHaveAttribute(
      "placeholder",
      "Ask the repo agent, or run a command..."
    );
    expect(screen.getByTestId("attach-file-button")).toBeInTheDocument();
    expect(screen.getByTestId("mic-button")).toBeInTheDocument();
    expect(screen.getByTestId("file-upload-input")).toBeInTheDocument();
    expect(screen.getByTestId("send-button").className).toContain("rounded-full");
  });

  it("renders composer with textarea and handles Korean IME composition safely", () => {
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
    expect(onSend).toHaveBeenCalledWith("안녕하세요", []);
  });

  it("toggles between send and stop buttons based on isRunning prop", () => {
    const onSend = vi.fn();
    const onStop = vi.fn();
    const { rerender } = render(
      <MobileChatComposer onSend={onSend} onStop={onStop} isRunning={false} />
    );

    expect(screen.getByTestId("send-button")).toBeInTheDocument();
    expect(screen.queryByTestId("stop-button")).not.toBeInTheDocument();

    rerender(
      <MobileChatComposer onSend={onSend} onStop={onStop} isRunning={true} />
    );

    expect(screen.queryByTestId("send-button")).not.toBeInTheDocument();
    const stopButton = screen.getByTestId("stop-button");
    expect(stopButton).toBeInTheDocument();

    fireEvent.click(stopButton);
    expect(onStop).toHaveBeenCalledTimes(1);
  });

  it("revokes blob URLs systematically when message with attachments is sent", () => {
    const origCreateObjectURL = URL.createObjectURL;
    const origRevokeObjectURL = URL.revokeObjectURL;
    const mockCreateObjectURL = vi.fn().mockReturnValue("blob:mock");
    const mockRevokeObjectURL = vi.fn();
    URL.createObjectURL = mockCreateObjectURL;
    URL.revokeObjectURL = mockRevokeObjectURL;

    try {
      const onSend = vi.fn();
      render(<MobileChatComposer onSend={onSend} />);

      const fileInput = screen.getByTestId("file-upload-input");
      const testFile = new File(["dummy content"], "test.png", { type: "image/png" });

      fireEvent.change(fileInput, { target: { files: [testFile] } });
      expect(mockCreateObjectURL).toHaveBeenCalled();

      const sendButton = screen.getByTestId("send-button");
      expect(sendButton).not.toBeDisabled();

      fireEvent.click(sendButton);

      expect(onSend).toHaveBeenCalledTimes(1);
      expect(onSend).toHaveBeenCalledWith(
        "",
        expect.arrayContaining([
          expect.objectContaining({ name: "test.png", url: "blob:mock" }),
        ])
      );
      expect(mockRevokeObjectURL).toHaveBeenCalledWith("blob:mock");
    } finally {
      URL.createObjectURL = origCreateObjectURL;
      URL.revokeObjectURL = origRevokeObjectURL;
    }
  });

  it("handles terminal accessory bar key taps correctly", () => {
    const onSend = vi.fn();
    const onStop = vi.fn();
    const { rerender } = render(
      <MobileChatComposer onSend={onSend} onStop={onStop} isRunning={false} />
    );

    expect(screen.getByTestId("terminal-accessory-bar")).toBeInTheDocument();

    // [/clear] -> sends "clear"
    fireEvent.click(screen.getByTestId("accessory-key-clear"));
    expect(onSend).toHaveBeenCalledWith("clear", []);

    // [Ctrl+C] when not running -> sends "\x03"
    fireEvent.click(screen.getByTestId("accessory-key-ctrl-c"));
    expect(onSend).toHaveBeenCalledWith("\x03", []);

    // [Ctrl+C] when isRunning -> calls onStop()
    rerender(<MobileChatComposer onSend={onSend} onStop={onStop} isRunning={true} />);
    fireEvent.click(screen.getByTestId("accessory-key-ctrl-c"));
    expect(onStop).toHaveBeenCalledTimes(1);

    // [ESC] with empty text -> sends "\x1b"
    fireEvent.click(screen.getByTestId("accessory-key-esc"));
    expect(onSend).toHaveBeenCalledWith("\x1b", []);

    // [ESC] with draft -> clears draft
    const textarea = screen.getByTestId("chat-composer-textarea");
    fireEvent.change(textarea, { target: { value: "some command" } });
    expect(textarea).toHaveValue("some command");
    fireEvent.click(screen.getByTestId("accessory-key-esc"));
    expect(textarea).toHaveValue("");

    // [Tab] -> inserts two spaces
    fireEvent.click(screen.getByTestId("accessory-key-tab"));
    expect(textarea).toHaveValue("  ");
  });
});

