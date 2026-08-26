import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TerminalSearchOverlay } from "./TerminalSearchOverlay";

const tauriCoreMocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriCoreMocks.invoke,
  isTauri: tauriCoreMocks.isTauri,
}));

describe("TerminalSearchOverlay native session search", () => {
  beforeEach(() => {
    tauriCoreMocks.invoke.mockReset();
    tauriCoreMocks.isTauri.mockReset();
    tauriCoreMocks.isTauri.mockReturnValue(true);
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("renders search input, navigation buttons, and close button", () => {
    const onClose = vi.fn();
    render(<TerminalSearchOverlay sessionId="backend-term-1" onClose={onClose} />);

    expect(screen.getByRole("search")).toBeInTheDocument();
    expect(screen.getByTestId("terminal-search-input")).toBeInTheDocument();
    expect(screen.getByLabelText("Previous match")).toBeInTheDocument();
    expect(screen.getByLabelText("Next match")).toBeInTheDocument();
    expect(screen.getByLabelText("Close search")).toBeInTheDocument();
  });

  it("triggers native search on query input and displays match counter", async () => {
    tauriCoreMocks.invoke.mockResolvedValue([
      [0, 5, 10],
      [1, 2, 7],
    ]);
    const onClose = vi.fn();
    render(<TerminalSearchOverlay sessionId="backend-term-1" onClose={onClose} />);

    const input = screen.getByTestId("terminal-search-input");
    fireEvent.change(input, { target: { value: "error" } });

    await waitFor(() => {
      expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_search", {
        sessionId: "backend-term-1",
        query: "error",
        caseSensitive: false,
      });
    });

    await waitFor(() => {
      expect(screen.getByText("1/2")).toBeInTheDocument();
    });
  });

  it("navigates matches with Enter, Shift+Enter, and arrow buttons", async () => {
    tauriCoreMocks.invoke.mockResolvedValue([
      [0, 5, 10],
      [1, 2, 7],
      [3, 0, 5],
    ]);
    const onClose = vi.fn();
    render(<TerminalSearchOverlay sessionId="backend-term-1" onClose={onClose} />);

    const input = screen.getByTestId("terminal-search-input");
    fireEvent.change(input, { target: { value: "warn" } });

    await waitFor(() => {
      expect(screen.getByText("1/3")).toBeInTheDocument();
    });

    // Next match via Enter
    fireEvent.keyDown(input, { key: "Enter", shiftKey: false });
    expect(screen.getByText("2/3")).toBeInTheDocument();

    // Next match via button
    fireEvent.click(screen.getByLabelText("Next match"));
    expect(screen.getByText("3/3")).toBeInTheDocument();

    // Wrap around to first match
    fireEvent.click(screen.getByLabelText("Next match"));
    expect(screen.getByText("1/3")).toBeInTheDocument();

    // Previous match via Shift+Enter
    fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
    expect(screen.getByText("3/3")).toBeInTheDocument();

    // Previous match via button
    fireEvent.click(screen.getByLabelText("Previous match"));
    expect(screen.getByText("2/3")).toBeInTheDocument();
  });

  it("closes on Escape and close button, triggering onFocusTerminal", () => {
    const onClose = vi.fn();
    const onFocusTerminal = vi.fn();
    const { unmount } = render(
      <TerminalSearchOverlay
        sessionId="backend-term-1"
        onClose={onClose}
        onFocusTerminal={onFocusTerminal}
      />,
    );

    const input = screen.getByTestId("terminal-search-input");
    fireEvent.keyDown(input, { key: "Escape" });

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onFocusTerminal).toHaveBeenCalledTimes(1);

    unmount();
    const onClose2 = vi.fn();
    render(<TerminalSearchOverlay sessionId="backend-term-1" onClose={onClose2} />);

    fireEvent.click(screen.getByLabelText("Close search"));
    expect(onClose2).toHaveBeenCalledTimes(1);
  });
});
