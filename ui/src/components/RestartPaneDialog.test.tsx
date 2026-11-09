import "./worktree-disk-test-dom";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { RestartPaneDialog } from "./RestartPaneDialog";

afterEach(cleanup);

const baseProps = {
  agentName: "Claude",
  canResume: true,
  onCancel: vi.fn(),
  onResume: vi.fn(),
  onFreshShell: vi.fn(),
};

describe("RestartPaneDialog", () => {
  it("offers resume and plain-shell restart for a resumable agent", () => {
    const props = { ...baseProps, onResume: vi.fn(), onFreshShell: vi.fn() };
    const { getByTestId } = render(<RestartPaneDialog {...props} />);

    fireEvent.click(getByTestId("restart-pane-resume"));
    expect(props.onResume).toHaveBeenCalledTimes(1);

    fireEvent.click(getByTestId("restart-pane-shell"));
    expect(props.onFreshShell).toHaveBeenCalledTimes(1);
  });

  it("omits the resume choice when the provider session cannot continue", () => {
    const { queryByTestId, getByTestId } = render(
      <RestartPaneDialog {...baseProps} canResume={false} />,
    );

    expect(queryByTestId("restart-pane-resume")).toBeNull();
    expect(getByTestId("restart-pane-shell")).toBeTruthy();
  });

  it("cancels on Escape and on backdrop click without choosing", () => {
    const onCancel = vi.fn();
    const onResume = vi.fn();
    const onFreshShell = vi.fn();
    const { getByRole } = render(
      <RestartPaneDialog {...baseProps} onCancel={onCancel} onResume={onResume} onFreshShell={onFreshShell} />,
    );

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onCancel).toHaveBeenCalledTimes(1);

    fireEvent.mouseDown(getByRole("dialog").parentElement!);
    expect(onCancel).toHaveBeenCalledTimes(2);
    expect(onResume).not.toHaveBeenCalled();
    expect(onFreshShell).not.toHaveBeenCalled();
  });
});
