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
