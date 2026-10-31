import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RemoteBrowserControls } from "./RemoteBrowserControls";

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
