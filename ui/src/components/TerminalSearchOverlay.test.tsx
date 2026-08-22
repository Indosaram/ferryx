import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { TerminalInstance } from "../lib/terminalHostManager";
import { TerminalSearchOverlay } from "./TerminalSearchOverlay";

function createMockInstance(): TerminalInstance {
  const terminal = {
    focus: vi.fn(),
    dispose: vi.fn(),
  } as any;

  const searchAddon = {
    findNext: vi.fn().mockReturnValue(true),
    findPrevious: vi.fn().mockReturnValue(true),
    clearDecorations: vi.fn(),
    dispose: vi.fn(),
    onDidChangeResults: vi.fn(() => ({ dispose: vi.fn() })),
  } as any;

  return {
    element: document.createElement("div"),
    terminal,
    fitAddon: { fit: vi.fn() } as any,
    searchAddon,
    disposeWebgl: vi.fn(),
    resizeObserver: { observe: vi.fn(), unobserve: vi.fn(), disconnect: vi.fn() } as any,
    disposables: [],
    session: {
      id: "s1",
      cwd: "/repo",
      workspaceId: "ws1",
      worktree: null,
      backendSessionId: "b1",
      lifecycle: "working",
    },
    active: true,
  };
}

describe("TerminalSearchOverlay", () => {
  afterEach(() => {
    cleanup();
  });
  it("renders search input, navigation buttons, and close button", () => {
    const instance = createMockInstance();
    const onClose = vi.fn();
    render(<TerminalSearchOverlay instance={instance} onClose={onClose} />);

    expect(screen.getByRole("search")).toBeInTheDocument();
    expect(screen.getByTestId("terminal-search-input")).toBeInTheDocument();
    expect(screen.getByLabelText("Previous match")).toBeInTheDocument();
    expect(screen.getByLabelText("Next match")).toBeInTheDocument();
    expect(screen.getByLabelText("Close search")).toBeInTheDocument();
  });

  it("triggers incremental search on typing and next/prev on Enter/Shift+Enter", () => {
    const instance = createMockInstance();
    const onClose = vi.fn();
    render(<TerminalSearchOverlay instance={instance} onClose={onClose} />);

    const input = screen.getByTestId("terminal-search-input");
    fireEvent.change(input, { target: { value: "error" } });

    expect(instance.searchAddon.findNext).toHaveBeenCalledWith("error", { incremental: true });

    fireEvent.keyDown(input, { key: "Enter", shiftKey: false });
    expect(instance.searchAddon.findNext).toHaveBeenCalledWith("error", { incremental: false });

    fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
    expect(instance.searchAddon.findPrevious).toHaveBeenCalledWith("error", { incremental: false });
  });

  it("navigates matches with arrow buttons", () => {
    const instance = createMockInstance();
    const onClose = vi.fn();
    render(<TerminalSearchOverlay instance={instance} onClose={onClose} />);

    const input = screen.getByTestId("terminal-search-input");
    fireEvent.change(input, { target: { value: "build" } });

    fireEvent.click(screen.getByLabelText("Next match"));
    expect(instance.searchAddon.findNext).toHaveBeenCalledWith("build", { incremental: false });

    fireEvent.click(screen.getByLabelText("Previous match"));
    expect(instance.searchAddon.findPrevious).toHaveBeenCalledWith("build", { incremental: false });
  });

  it("closes on Escape and close button, clearing decorations and refocusing terminal", () => {
    const instance = createMockInstance();
    const onClose = vi.fn();
    const { unmount } = render(<TerminalSearchOverlay instance={instance} onClose={onClose} />);

    const input = screen.getByTestId("terminal-search-input");
    fireEvent.keyDown(input, { key: "Escape" });

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(instance.searchAddon.clearDecorations).toHaveBeenCalled();
    expect(instance.terminal.focus).toHaveBeenCalled();

    unmount();
    const onClose2 = vi.fn();
    render(<TerminalSearchOverlay instance={instance} onClose={onClose2} />);

    fireEvent.click(screen.getByLabelText("Close search"));
    expect(onClose2).toHaveBeenCalledTimes(1);
  });
});
