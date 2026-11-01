import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { MobileOverflowSheet } from "./MobileOverflowSheet";

beforeEach(cleanup);
afterEach(cleanup);

describe("MobileOverflowSheet", () => {
  it("renders closed when open is false", () => {
    render(
      <MobileOverflowSheet
        open={false}
        onOpenChange={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewWorktree={vi.fn()}
      />,
    );

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.queryByText("New terminal")).not.toBeInTheDocument();
  });

  it("renders action items when open", () => {
    render(
      <MobileOverflowSheet
        open
        onOpenChange={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewWorktree={vi.fn()}
        onOpenMachines={vi.fn()}
        connectionLabel="Relay"
        buildStamp="v2026.1012.0"
      />,
    );

    expect(screen.getByRole("dialog", { name: "Actions" })).toBeInTheDocument();
    expect(screen.getByText("New terminal")).toBeInTheDocument();
    expect(screen.getByText("New worktree")).toBeInTheDocument();
    expect(screen.getByText("Computers")).toBeInTheDocument();
    expect(screen.queryByText("Sign out")).not.toBeInTheDocument();
    expect(screen.getByText("Relay")).toBeInTheDocument();
    expect(screen.getByText("v2026.1012.0")).toBeInTheDocument();
  });

  it("shows Computers and Sign out together when both exist, with sign out set apart", () => {
    render(
      <MobileOverflowSheet open onOpenChange={vi.fn()} onOpenMachines={vi.fn()} onSignOut={vi.fn()} />,
    );
    expect(screen.getByTestId("overflow-action-machines")).toHaveTextContent("Computers");
    const signOut = screen.getByTestId("overflow-action-sign-out");
    expect(signOut).toHaveClass("text-chat-danger");
    expect(signOut.parentElement).toHaveClass("border-t");
  });

  it("signs out only on a second, explicit press", () => {
    const onSignOut = vi.fn();
    const onOpenChange = vi.fn();
    render(<MobileOverflowSheet open onOpenChange={onOpenChange} onSignOut={onSignOut} />);
    fireEvent.click(screen.getByTestId("overflow-action-sign-out"));
    expect(onSignOut).not.toHaveBeenCalled();
    expect(screen.getByTestId("overflow-action-sign-out")).toHaveTextContent("Tap again to sign out");
    fireEvent.click(screen.getByTestId("overflow-action-sign-out"));
    expect(onSignOut).toHaveBeenCalledTimes(1);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("disables New terminal and New worktree when disabled is true (e.g. pending or no workspaceId)", () => {
    render(
      <MobileOverflowSheet
        open
        onOpenChange={vi.fn()}
        onNewTerminal={vi.fn()}
        onNewWorktree={vi.fn()}
        onOpenMachines={vi.fn()}
        disabled={true}
      />,
    );

    expect(screen.getByTestId("overflow-action-new-terminal")).toBeDisabled();
    expect(screen.getByTestId("overflow-action-new-worktree")).toBeDisabled();
    // Machines is not disabled by the workspace disabled flag
    expect(screen.getByTestId("overflow-action-machines")).not.toBeDisabled();
  });

  it("triggers callbacks and closes sheet on selection", () => {
    const onNewTerminal = vi.fn();
    const onNewWorktree = vi.fn();
    const onOpenMachines = vi.fn();
    const onOpenChange = vi.fn();

    render(
      <MobileOverflowSheet
        open
        onOpenChange={onOpenChange}
        onNewTerminal={onNewTerminal}
        onNewWorktree={onNewWorktree}
        onOpenMachines={onOpenMachines}
      />,
    );

    fireEvent.click(screen.getByText("New terminal"));
    expect(onNewTerminal).toHaveBeenCalledTimes(1);
    expect(onOpenChange).toHaveBeenCalledWith(false);

    fireEvent.click(screen.getByText("New worktree"));
    expect(onNewWorktree).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByText("Computers"));
    expect(onOpenMachines).toHaveBeenCalledTimes(1);
  });
});

