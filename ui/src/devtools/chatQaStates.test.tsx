import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { MobileChatWorkspace } from "../remote/chat/MobileChatWorkspace";
import { QaTerminalSurface, chatQaComposerProps } from "./chatQaStates";

function renderState(state: string) {
  return render(
    <MobileChatWorkspace showHeader={false} messages={[]} onSendMessage={() => true} {...chatQaComposerProps(state)} />,
  );
}

describe("mobile chat QA states", () => {
  afterEach(cleanup);

  it("seeds the production ReferencePromptCard in the message stream for a two-option approval", () => {
    renderState("prompt-approval");
    expect(screen.getByTestId("chat-stream-prompt")).toContainElement(screen.getByTestId("reference-prompt-card"));
    expect(screen.getByTestId("reference-prompt-option-0")).toBeInTheDocument();
    expect(screen.getByTestId("reference-prompt-option-1")).toBeInTheDocument();
    expect(screen.queryByTestId("reference-prompt-option-2")).not.toBeInTheDocument();
  });

  it("seeds a multi-select prompt with more than two options", () => {
    renderState("prompt-multi");
    expect(screen.getByTestId("reference-prompt-multi-3")).toBeInTheDocument();
    expect(screen.getByTestId("reference-prompt-submit")).toBeInTheDocument();
  });

  it("seeds a prompt with a typed-answer field", () => {
    renderState("prompt-custom");
    expect(screen.getByTestId("reference-prompt-custom-input")).toBeInTheDocument();
  });

  it("seeds held messages, staged files and a plain-language attach error", () => {
    renderState("held");
    expect(screen.getByTestId("held-message-held-1")).toBeInTheDocument();
    cleanup();

    renderState("attach");
    expect(screen.getByTestId("attachment-preview-att-1")).toBeInTheDocument();
    expect(screen.getByTestId("attachment-preview-att-2")).toBeInTheDocument();
    cleanup();

    renderState("attach-error");
    const error = screen.getByTestId("chat-composer-attach-error");
    expect(error).toHaveTextContent(/^Couldn't attach this file\./);
    expect(error.textContent).not.toMatch(/[A-Z]{2,}_[A-Z]+/);
  });

  it("seeds a disconnected chat whose composer cannot send", () => {
    renderState("offline");
    expect(screen.getByTestId("chat-composer-warning")).toHaveTextContent("The connection to your computer dropped.");
    expect(screen.getByTestId("send-button")).toBeDisabled();
  });

  it("seeds approvals only through the production prompt card, never the legacy per-message card", () => {
    const harness = readFileSync(resolve(__dirname, "MobileChatQaHarness.tsx"), "utf-8");
    expect(harness).not.toMatch(/approvalAction/);
  });

  it("renders the terminal stand-in as a neutral surface with no text", () => {
    render(<QaTerminalSurface />);
    const surface = screen.getByTestId("qa-terminal-placeholder");
    expect(surface).toHaveTextContent("");
    expect(surface).toHaveAttribute("aria-hidden", "true");
  });
});
