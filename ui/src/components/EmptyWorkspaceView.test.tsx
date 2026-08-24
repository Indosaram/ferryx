import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { EmptyWorkspaceView } from "./EmptyWorkspaceView";

afterEach(() => {
  cleanup();
});

describe("EmptyWorkspaceView", () => {
  it("renders empty workspace message and hint", () => {
    render(<EmptyWorkspaceView onNewTerminal={vi.fn()} onNewBrowserTab={vi.fn()} />);

    expect(screen.getByTestId("empty-workspace-view")).toBeInTheDocument();
    expect(screen.getByText("No open tabs")).toBeInTheDocument();
    expect(screen.getByText("Open a terminal or browser tab to get started.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /new terminal/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /new browser tab/i })).toBeInTheDocument();
  });

  it("calls onNewTerminal when New Terminal button is clicked", () => {
    const onNewTerminal = vi.fn();
    render(<EmptyWorkspaceView onNewTerminal={onNewTerminal} onNewBrowserTab={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: /new terminal/i }));
    expect(onNewTerminal).toHaveBeenCalledTimes(1);
  });

  it("calls onNewBrowserTab when New Browser Tab button is clicked", () => {
    const onNewBrowserTab = vi.fn();
    render(<EmptyWorkspaceView onNewTerminal={vi.fn()} onNewBrowserTab={onNewBrowserTab} />);

    fireEvent.click(screen.getByRole("button", { name: /new browser tab/i }));
    expect(onNewBrowserTab).toHaveBeenCalledTimes(1);
  });
});
