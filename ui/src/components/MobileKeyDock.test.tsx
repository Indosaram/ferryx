import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MobileKeyDock } from "./MobileKeyDock";

describe("MobileKeyDock", () => {
  afterEach(cleanup);

  it("keeps Enter, the arrows, More keys and the Line toggle in fixed grid rows instead of a hidden scroller", () => {
    // Given: a dock with the input-mode toggle
    render(<MobileKeyDock onSendKey={vi.fn()} inputMode="direct" onInputModeChange={vi.fn()} />);

    // Then: no row relies on horizontal overflow, and every primary key is present
    const dock = screen.getByTestId("mobile-key-dock");
    expect(dock.innerHTML).not.toMatch(/overflow-x-auto/);
    for (const name of ["Enter", "Left", "Up", "Down", "Right", "More keys", "Ctrl-C"]) {
      expect(screen.getByRole("button", { name })).toBeInTheDocument();
    }
    expect(screen.getByRole("button", { name: "Line" })).toHaveAttribute("aria-pressed", "false");
  });

  it("moves the navigation row behind More keys when compact", () => {
    // Given: a compact dock
    render(<MobileKeyDock onSendKey={vi.fn()} compact />);
    expect(screen.queryByRole("button", { name: "Enter" })).toBeNull();

    // When: More keys is opened
    fireEvent.click(screen.getByRole("button", { name: "More keys" }));

    // Then: the navigation row is reachable again
    expect(screen.getByRole("button", { name: "Enter" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "More keys" })).toHaveAttribute("aria-pressed", "true");
  });

  it("marks a one-shot modifier with a dot and a locked one with a lock glyph, never a ring", () => {
    // Given: a dock
    vi.spyOn(Date, "now").mockReturnValue(1000);
    render(<MobileKeyDock onSendKey={vi.fn()} />);
    const ctrl = screen.getByRole("button", { name: "Ctrl" });

    // When: Ctrl is tapped once
    fireEvent.click(ctrl);

    // Then: one-shot shows a dot
    expect(ctrl).toHaveAttribute("data-state", "oneshot");
    expect(ctrl.querySelector('[data-testid="dock-modifier-oneshot"]')).not.toBeNull();

    // When: tapped again inside the lock window
    fireEvent.click(ctrl);

    // Then: locked shows a lock glyph and no ring outline
    expect(ctrl).toHaveAttribute("data-state", "locked");
    expect(ctrl.querySelector('[data-testid="dock-modifier-locked"]')).not.toBeNull();
    expect(ctrl.className).not.toMatch(/ring-2/);
    vi.restoreAllMocks();
  });

  it("drops the bottom safe-area padding while the soft keyboard is open", () => {
    // Given / When: the keyboard is open
    const view = render(<MobileKeyDock onSendKey={vi.fn()} keyboardOpen />);

    // Then: no home-indicator gap above the keyboard
    expect(screen.getByTestId("mobile-key-dock").className).not.toMatch(/pb-safe/);
    view.rerender(<MobileKeyDock onSendKey={vi.fn()} />);
    expect(screen.getByTestId("mobile-key-dock").className).toMatch(/pb-safe/);
  });

  it("styles Ctrl-C with the danger ink on a neutral key rather than dark text on a red tint", () => {
    render(<MobileKeyDock onSendKey={vi.fn()} />);
    const ctrlC = screen.getByRole("button", { name: "Ctrl-C" });
    expect(ctrlC.className).toMatch(/text-chat-danger/);
    expect(ctrlC.className).not.toMatch(/destructive/);
  });
});
