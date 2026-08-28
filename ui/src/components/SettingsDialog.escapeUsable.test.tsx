import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { SettingsDialog } from "./SettingsDialog";

describe("SettingsDialog Escape remains usable", () => {
  afterEach(() => {
    cleanup();
  });

  it("closes when Escape is pressed on a closed Radix select trigger", () => {
    const onClose = vi.fn();
    render(<SettingsDialog open onClose={onClose} />);

    fireEvent.click(within(screen.getByTestId("settings-nav")).getByRole("button", { name: "Appearance" }));

    const trigger = screen
      .getAllByRole("combobox")
      .find((element) => element.getAttribute("aria-expanded") === "false");

    expect(trigger).toBeDefined();
    fireEvent.keyDown(trigger as HTMLElement, { key: "Escape" });

    expect(onClose).toHaveBeenCalled();
  });

  it("closes when Escape is pressed in a prefilled text input the user did not open", () => {
    const onClose = vi.fn();
    render(<SettingsDialog open onClose={onClose} />);

    const nav = screen.getByTestId("settings-nav");
    fireEvent.click(within(nav).getByRole("button", { name: "Terminal" }));

    const input = screen.getByLabelText("Font family") as HTMLInputElement;
    expect(input.value.length).toBeGreaterThan(0);

    fireEvent.keyDown(input, { key: "Escape" });

    expect(onClose).toHaveBeenCalled();
  });
});
