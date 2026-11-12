import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ShortcutCheatSheet } from "./ShortcutCheatSheet";

afterEach(cleanup);

describe("ShortcutCheatSheet", () => {
  it("renders nothing while closed", () => {
    const { queryByTestId } = render(<ShortcutCheatSheet open={false} onClose={vi.fn()} />);

    expect(queryByTestId("shortcut-cheat-sheet")).toBeNull();
  });

  it("filters rows by title and drops groups left with no match", () => {
    const { container } = render(<ShortcutCheatSheet open onClose={vi.fn()} isMac />);

    fireEvent.change(screen.getByPlaceholderText("Search shortcuts"), { target: { value: "copy mode" } });

    expect(container.querySelector('[data-shortcut-id="terminal.copyMode"]')).not.toBeNull();
    expect(container.querySelector('[data-shortcut-id="terminal.splitDown"]')).toBeNull();
    expect(container.querySelector('section[aria-label="Tabs"]')).toBeNull();
  });

  it("closes on Escape", () => {
    const onClose = vi.fn();
    render(<ShortcutCheatSheet open onClose={onClose} />);

    fireEvent.keyDown(window, { key: "Escape" });

    expect(onClose).toHaveBeenCalledOnce();
  });
});
