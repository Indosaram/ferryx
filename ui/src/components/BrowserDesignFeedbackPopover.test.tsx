import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BrowserDesignFeedbackPopover } from "./BrowserDesignFeedbackPopover";

afterEach(cleanup);

describe("BrowserDesignFeedbackPopover", () => {
  const snapshot = {
    session_id: "s",
    timestamp_ms: 1,
    screenshot_png_base64: "iVBORw0KGgo=",
    dom_elements: [{ id: "el-1", tag: "button", bounds: [1, 2, 100, 40] as [number, number, number, number] }],
  };
  const targets = [
    { sessionId: "term-1", workspaceId: "ws-1", label: "Terminal 1" },
    { sessionId: "term-2", workspaceId: "ws-1", label: "Terminal 2" },
  ];

  it("renders the thumbnail with a data:image/png;base64 src and shows the element summary text", () => {
    render(
      <BrowserDesignFeedbackPopover
        snapshot={snapshot}
        targets={targets}
        sending={false}
        error={null}
        onSend={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    const img = screen.getByAltText("Selected element capture") as HTMLImageElement;
    expect(img).toBeDefined();
    expect(img.getAttribute("src")).toBe("data:image/png;base64,iVBORw0KGgo=");
    expect(screen.getByText("button#el-1 100x40 at (1, 2)")).toBeDefined();
  });

  it("the Send button is disabled while the memo is empty, becomes enabled after fireEvent.change, and clicking it calls onSend with first target id and memo", () => {
    const onSend = vi.fn();
    render(
      <BrowserDesignFeedbackPopover
        snapshot={snapshot}
        targets={targets}
        sending={false}
        error={null}
        onSend={onSend}
        onCancel={vi.fn()}
      />,
    );

    const sendBtn = screen.getByRole("button", { name: "Send" });
    expect(sendBtn.hasAttribute("disabled")).toBe(true);

    const memoInput = screen.getByLabelText("Design feedback note");
    fireEvent.change(memoInput, { target: { value: "add 8px margin" } });

    expect(sendBtn.hasAttribute("disabled")).toBe(false);

    fireEvent.click(sendBtn);
    expect(onSend).toHaveBeenCalledTimes(1);
    expect(onSend).toHaveBeenCalledWith({ sessionId: "term-1", workspaceId: "ws-1", memo: "add 8px margin" });
  });

  it("choosing another option in the select and sending passes that sessionId instead", () => {
    const onSend = vi.fn();
    render(
      <BrowserDesignFeedbackPopover
        snapshot={snapshot}
        targets={targets}
        sending={false}
        error={null}
        onSend={onSend}
        onCancel={vi.fn()}
      />,
    );

    const targetSelect = screen.getByLabelText("Delivery target");
    fireEvent.change(targetSelect, { target: { value: "term-2" } });

    const memoInput = screen.getByLabelText("Design feedback note");
    fireEvent.change(memoInput, { target: { value: "fix padding" } });

    const sendBtn = screen.getByRole("button", { name: "Send" });
    fireEvent.click(sendBtn);

    expect(onSend).toHaveBeenCalledTimes(1);
    expect(onSend).toHaveBeenCalledWith({ sessionId: "term-2", workspaceId: "ws-1", memo: "fix padding" });
  });

  it("a non-null error prop renders inside role='alert' and onCancel fires when Cancel is clicked", () => {
    const onCancel = vi.fn();
    render(
      <BrowserDesignFeedbackPopover
        snapshot={snapshot}
        targets={targets}
        sending={false}
        error="Failed to deliver feedback"
        onSend={vi.fn()}
        onCancel={onCancel}
      />,
    );

    const alert = screen.getByRole("alert");
    expect(alert).toBeDefined();
    expect(alert.textContent).toBe("Failed to deliver feedback");

    const cancelBtn = screen.getByRole("button", { name: "Cancel" });
    fireEvent.click(cancelBtn);
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("renders status text and keeps Send disabled when targets is empty", () => {
    render(
      <BrowserDesignFeedbackPopover
        snapshot={snapshot}
        targets={[]}
        sending={false}
        error={null}
        onSend={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    const status = screen.getByRole("status");
    expect(status).toBeDefined();
    expect(status.textContent).toBe(
      "No terminal session in this workspace can receive the feedback.",
    );

    const sendBtn = screen.getByRole("button", { name: "Send" });
    expect(sendBtn.hasAttribute("disabled")).toBe(true);

    const memoInput = screen.getByLabelText("Design feedback note");
    fireEvent.change(memoInput, { target: { value: "some text" } });
    expect(sendBtn.hasAttribute("disabled")).toBe(true);
  });
});
