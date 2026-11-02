import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RemoteBrowserControls } from "./RemoteBrowserControls";
import type { RemoteInputRefusal } from "./RemoteBrowser";

afterEach(() => {
  cleanup();
});

async function runEvalWith(onEval: (script: string) => Promise<unknown>) {
  render(
    <RemoteBrowserControls
      url="https://example.com"
      driverState="driving"
      canPointClick
      pointClickEnabled
      onEval={onEval}
    />,
  );

  await act(async () => {
    fireEvent.click(screen.getByTestId("remote-browser-eval-toggle"));
  });
  await act(async () => {
    fireEvent.change(screen.getByTestId("remote-browser-eval-input"), {
      target: { value: "document.title" },
    });
  });
  await act(async () => {
    fireEvent.click(screen.getByTestId("remote-browser-eval-run-btn"));
  });
  await act(async () => {
    fireEvent.click(screen.getByTestId("remote-browser-eval-confirm-btn"));
  });
}

describe("RemoteBrowserControls eval refusal", () => {
  // RED mutation: restore `msg.includes("approval") || msg.includes("APPROVAL")` in the catch
  // of `handleRunEval` -> "does not refuse on message text alone" fails.
  it("refuses on the structured PERMISSION_DENIED code", async () => {
    const rejection = Object.assign(
      new Error("unsupported operation: eval operation requires explicit driver approval"),
      { code: "PERMISSION_DENIED" },
    );

    await runEvalWith(() => Promise.reject(rejection));

    expect(screen.getByTestId("remote-browser-controls-refusal-banner")).toBeDefined();
    expect(screen.getByTestId("remote-browser-controls-refusal-code").textContent).toBe(
      "PERMISSION_DENIED",
    );
    expect(screen.getByTestId("remote-browser-controls-refusal-reason").textContent).toBe(
      rejection.message,
    );
  });

  it("does not refuse on message text alone", async () => {
    const rejection = Object.assign(new Error("approval of the page failed to execute"), {
      code: "BROWSER_EXECUTION_FAILED",
    });

    await runEvalWith(() => Promise.reject(rejection));

    expect(screen.queryByTestId("remote-browser-controls-refusal-banner")).toBeNull();
    expect(screen.getByTestId("remote-browser-eval-result").textContent).toContain(
      rejection.message,
    );
  });
});

describe("RemoteBrowserControls accessibility", () => {
  it("labels inputs and exposes the driver badge as a status region", () => {
    render(<RemoteBrowserControls url="https://example.com" driverState="driving" />);
    expect(screen.getByLabelText("Page URL")).toBeDefined();
    expect(screen.getByLabelText("Element reference")).toBeDefined();
    expect(screen.getByLabelText("Text to fill")).toBeDefined();
    expect(screen.getByRole("status").textContent).toBe("Driving");
  });
});

describe("RemoteBrowserControls refusals it decides locally", () => {
  // RED mutation: drop the 32 KiB guard in `handleRunEval` -> this refusal never fires and
  // onEval is called.
  it("refuses an oversized eval script before dispatch", async () => {
    const onEval = vi.fn().mockResolvedValue({ ok: true });
    render(
      <RemoteBrowserControls
        url="https://example.com"
        driverState="driving"
        canPointClick
        pointClickEnabled
        onEval={onEval}
      />,
    );

    await act(async () => {
      fireEvent.click(screen.getByTestId("remote-browser-eval-toggle"));
    });
    await act(async () => {
      fireEvent.change(screen.getByTestId("remote-browser-eval-input"), {
        target: { value: "x".repeat(32 * 1024 + 1) },
      });
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId("remote-browser-eval-run-btn"));
    });

    expect(onEval).not.toHaveBeenCalled();
    expect(screen.getByTestId("remote-browser-controls-refusal-code").textContent).toBe(
      "PAYLOAD_TOO_LARGE",
    );
  });
});

describe("RemoteBrowserControls command failures and URL sync", () => {
  // RED mutation: in commandFailureRefusal drop the `details?.code ||` term / use `|| "KEY_NOT_ALLOWED"` again.
  it("keeps the server's structured details for a rejected keypress (F16)", async () => {
    const details = {
      code: "KEY_NOT_ALLOWED",
      reason: "key refused by page allowlist",
      inputClass: "key",
      remediation: "Use an allowlisted key.",
    };
    const onKeypress = vi
      .fn()
      .mockRejectedValue(Object.assign(new Error("refused"), { code: "BROWSER_INPUT_REFUSED", details }));
    render(<RemoteBrowserControls url="https://example.com" driverState="driving" onKeypress={onKeypress} />);
    await act(async () => {
      fireEvent.click(screen.getByTestId("remote-browser-key-Enter"));
    });
    expect(screen.getByTestId("remote-browser-controls-refusal-code").textContent).toBe("KEY_NOT_ALLOWED");
    expect(screen.getByTestId("remote-browser-controls-refusal-reason").textContent).toBe(details.reason);
  });

  // RED mutation: restore `|| "KEY_NOT_ALLOWED"` as the fallback code for a failed keypress.
  it("does not label a non-refusal keypress failure as KEY_NOT_ALLOWED (F16)", async () => {
    const onKeypress = vi.fn().mockRejectedValue(new Error("Request timed out"));
    render(<RemoteBrowserControls url="https://example.com" driverState="driving" onKeypress={onKeypress} />);
    await act(async () => {
      fireEvent.click(screen.getByTestId("remote-browser-key-Enter"));
    });
    expect(screen.getByTestId("remote-browser-controls-refusal-code").textContent).toBe("COMMAND_FAILED");
  });

  // RED mutation: make the url-sync effect unconditional again (`setInputUrl(url)` without the edited check).
  it("does not overwrite a URL being typed when the page url changes (F15)", async () => {
    const { rerender } = render(<RemoteBrowserControls url="https://a.example" driverState="driving" />);
    const input = screen.getByTestId("remote-browser-url-input");
    fireEvent.change(input, { target: { value: "https://typed.example" } });
    rerender(<RemoteBrowserControls url="https://redirect.example" driverState="driving" />);
    expect(input).toHaveValue("https://typed.example");
  });
});

describe("RemoteBrowserControls eval confirmation (desktop)", () => {
  async function typeScript(onEval: () => Promise<unknown>) {
    render(<RemoteBrowserControls url="https://example.com" driverState="driving" phone={false} onEval={onEval} />);
    await act(async () => {
      fireEvent.click(screen.getByTestId("remote-browser-eval-toggle"));
    });
    await act(async () => {
      fireEvent.change(screen.getByTestId("remote-browser-eval-input"), { target: { value: "document.title" } });
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId("remote-browser-eval-run-btn"));
    });
  }

  it("does not run the script until the confirm step is accepted", async () => {
    const onEval = vi.fn().mockResolvedValue("ok");
    await typeScript(onEval);
    expect(onEval).not.toHaveBeenCalled();
    expect(screen.getByTestId("remote-browser-eval-confirm")).toBeDefined();

    await act(async () => {
      fireEvent.click(screen.getByTestId("remote-browser-eval-confirm-btn"));
    });
    expect(onEval).toHaveBeenCalledWith("document.title");
  });

  it("cancelling the confirm step never runs the script", async () => {
    const onEval = vi.fn().mockResolvedValue("ok");
    await typeScript(onEval);
    await act(async () => {
      fireEvent.click(screen.getByTestId("remote-browser-eval-cancel-btn"));
    });
    expect(onEval).not.toHaveBeenCalled();
    expect(screen.queryByTestId("remote-browser-eval-confirm")).toBeNull();
  });

  it("gives the eval textarea a 16px base size so iOS does not zoom", () => {
    render(<RemoteBrowserControls url="" driverState="driving" phone={false} />);
    fireEvent.click(screen.getByTestId("remote-browser-eval-toggle"));
    expect(screen.getByTestId("remote-browser-eval-input").className).toContain("text-base");
  });
});

describe("RemoteBrowserControls phone layout", () => {
  it("keeps one primary row with 44px controls and offers no eval", () => {
    render(<RemoteBrowserControls url="https://example.com" driverState="driving" phone />);
    expect(screen.getByLabelText("Go back").className).toContain("size-11");
    expect(screen.getByLabelText("Reload").className).toContain("size-11");
    expect(screen.getByTestId("remote-browser-url-input").className).toContain("min-h-11");
    expect(screen.getByTestId("remote-browser-release-btn").className).toContain("min-h-11");
    expect(screen.queryByTestId("remote-browser-eval-toggle")).toBeNull();
    expect(screen.queryByTestId("remote-browser-eval-panel")).toBeNull();
  });

  it("moves Ref, Fill, keys and point click behind a Tools button", () => {
    render(<RemoteBrowserControls url="" driverState="driving" phone canPointClick pointClickEnabled />);
    expect(screen.queryByTestId("remote-browser-fill-btn")).toBeNull();
    expect(screen.queryByTestId("remote-browser-key-Enter")).toBeNull();
    expect(screen.queryByTestId("remote-browser-point-click-toggle")).toBeNull();

    fireEvent.click(screen.getByTestId("remote-browser-tools-btn"));

    const sheet = screen.getByRole("dialog", { name: "Browser tools" });
    expect(sheet).toBeDefined();
    expect(screen.getByTestId("remote-browser-fill-btn")).toBeDefined();
    expect(screen.getByTestId("remote-browser-key-Enter")).toBeDefined();
    expect(screen.getByTestId("remote-browser-point-click-toggle")).toBeDefined();
    expect(screen.getByTestId("remote-browser-key-Enter").className).toContain("min-h-11");
    expect(screen.getByTestId("remote-browser-custom-key-input").className).toContain("text-base");
  });
});

describe("RemoteBrowserControls claim button", () => {
  it("uses token styling for Take control and claims immediately when nobody else drives", () => {
    const onClaim = vi.fn();
    render(<RemoteBrowserControls driverState="viewing" phone={false} onClaim={onClaim} />);
    const button = screen.getByTestId("remote-browser-claim-btn");
    expect(button.textContent).toBe("Take control");
    expect(button.className).toContain("bg-chat-primary");
    expect(button.className).toContain("text-chat-primary-foreground");
    fireEvent.click(button);
    expect(onClaim).toHaveBeenCalledTimes(1);
  });

  it("explains the consequence and asks for confirmation when someone else is driving", () => {
    const onClaim = vi.fn();
    render(<RemoteBrowserControls driverState="occupied" phone={false} onClaim={onClaim} />);
    expect(screen.getByTestId("remote-browser-occupied-notice").textContent).toContain(
      "Someone else is controlling this browser. Taking control will stop them.",
    );

    fireEvent.click(screen.getByTestId("remote-browser-claim-btn"));
    expect(onClaim).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("remote-browser-claim-cancel-btn"));
    expect(onClaim).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("remote-browser-claim-btn"));
    fireEvent.click(screen.getByTestId("remote-browser-claim-confirm-btn"));
    expect(onClaim).toHaveBeenCalledTimes(1);
  });
});

describe("RemoteBrowserControls inputs and copy", () => {
  it("configures the URL field for typing addresses", () => {
    render(<RemoteBrowserControls url="" driverState="driving" />);
    const input = screen.getByTestId("remote-browser-url-input");
    expect(input).toHaveAttribute("inputmode", "url");
    expect(input).toHaveAttribute("autocapitalize", "none");
    expect(input).toHaveAttribute("autocorrect", "off");
    expect(input).toHaveAttribute("spellcheck", "false");
    expect(input).toHaveAttribute("enterkeyhint", "go");
  });

  it("hides point click when the page cannot take it instead of striking it through", () => {
    const { rerender } = render(<RemoteBrowserControls url="" driverState="driving" phone={false} />);
    expect(screen.queryByText(/point click/i)).toBeNull();
    rerender(
      <RemoteBrowserControls
        url=""
        driverState="driving"
        phone={false}
        canPointClick
        pointClickDisabledReason="refused by page"
      />,
    );
    expect(screen.queryByText(/point click/i)).toBeNull();
  });

  it("labels glyph-free icon buttons for assistive tech", () => {
    render(<RemoteBrowserControls url="" driverState="driving" phone={false} />);
    expect(screen.getByLabelText("Go back")).toBeDefined();
    expect(screen.getByLabelText("Go forward")).toBeDefined();
    expect(screen.getByLabelText("Arrow left")).toBeDefined();
    expect(screen.getByLabelText("Backspace")).toBeDefined();
  });

  it("shows a plain sentence for a refusal and keeps the raw code under details", () => {
    const refusal: RemoteInputRefusal = {
      code: "BROWSER_INVALID_REFERENCE",
      reason: "invalid element reference: reference cannot be empty",
      inputClass: "fill",
    };
    render(<RemoteBrowserControls url="" driverState="driving" phone={false} refusal={refusal} />);
    const banner = screen.getByTestId("remote-browser-controls-refusal-banner");
    expect(screen.getByTestId("remote-browser-controls-refusal-message").textContent).toBe(
      "That page element is no longer there.",
    );
    const details = banner.querySelector("details");
    expect(details).not.toBeNull();
    expect(details?.open).toBe(false);
    expect(screen.getByLabelText("Dismiss notice")).toBeDefined();
  });
});
