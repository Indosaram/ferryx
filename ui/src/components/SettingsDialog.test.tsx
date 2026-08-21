import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SHORTCUTS, shortcutLabel } from "../lib/shortcuts";
import { TERMINAL_SETTINGS_STORAGE_KEY } from "../lib/terminalSettings";
import { SettingsDialog } from "./SettingsDialog";

afterEach(cleanup);
beforeEach(() => localStorage.clear());

describe("SettingsDialog", () => {
  it("persists terminal font size and scrollback controls", () => {
    render(<SettingsDialog open onClose={vi.fn()} />);

    fireEvent.change(screen.getByLabelText("Font size"), { target: { value: "17" } });
    fireEvent.change(screen.getByLabelText("Scrollback"), { target: { value: "30000" } });

    expect(JSON.parse(localStorage.getItem(TERMINAL_SETTINGS_STORAGE_KEY)!)).toEqual({ fontSize: 17, scrollback: 30_000 });
  });

  it("shows only functional terminal settings and registry-derived shortcut labels", () => {
    render(<SettingsDialog open onClose={vi.fn()} />);

    expect(screen.getByRole("heading", { name: "Terminal" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Shortcuts" })).toBeInTheDocument();
    expect(screen.queryByText(/browser/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/remote/i)).not.toBeInTheDocument();
    for (const shortcut of SHORTCUTS) {
      expect(screen.getByText(shortcut.title)).toBeInTheDocument();
      expect(screen.getByText(shortcutLabel(shortcut.id))).toBeInTheDocument();
    }
  });
});
