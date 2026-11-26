import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SshWorkspaceStatus } from "./SshWorkspaceStatus";

afterEach(() => cleanup());

describe("SshWorkspaceStatus", () => {
  it("renders no actions while the connection is still in progress", () => {
    const onRetry = vi.fn();
    render(<SshWorkspaceStatus hostLabel="studio" message="Connecting via SSH..." onRetry={onRetry} />);

    expect(screen.getByTestId("ssh-workspace-status")).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("calls onRetry when Retry connection is clicked on a failed connection", () => {
    const onRetry = vi.fn();
    const onOpenSettings = vi.fn();
    render(
      <SshWorkspaceStatus
        hostLabel="studio"
        message="Opening terminal..."
        error="SSH_CONNECT_FAILED"
        onRetry={onRetry}
        onOpenSettings={onOpenSettings}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Retry connection" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
    expect(onOpenSettings).not.toHaveBeenCalled();
  });

  it("calls onOpenSettings when Open SSH Settings is clicked on a failed connection", () => {
    const onRetry = vi.fn();
    const onOpenSettings = vi.fn();
    render(
      <SshWorkspaceStatus
        hostLabel="studio"
        message="Opening terminal..."
        error="SSH_CONNECT_FAILED"
        onRetry={onRetry}
        onOpenSettings={onOpenSettings}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Open SSH Settings" }));
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
    expect(onRetry).not.toHaveBeenCalled();
  });

  it("hides Open SSH Settings when the caller provides no handler", () => {
    render(
      <SshWorkspaceStatus
        hostLabel="studio"
        message="Opening terminal..."
        error="SSH_CONNECT_FAILED"
        onRetry={vi.fn()}
      />,
    );

    expect(screen.getByRole("button", { name: "Retry connection" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open SSH Settings" })).toBeNull();
  });
});
