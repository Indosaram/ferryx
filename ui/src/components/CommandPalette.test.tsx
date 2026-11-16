import { fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { loadAppearanceSettings, saveAppearanceSettings, useAppearanceSettings } from "../lib/appearanceSettings";
import { isMacShortcutPlatform, shortcutLabel } from "../lib/shortcuts";
import type { ThemeDocumentV1 } from "../lib/themeDocument";
import type { TerminalTab, Worktree } from "../lib/types";
import { CommandPalette } from "./CommandPalette";

const worktrees: Worktree[] = [
  {
    path: "/repo/main",
    head: "abc123",
    branch: "refs/heads/orca/ws/main",
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
  },
  {
    path: "/repo/feature",
    head: "def456",
    branch: "refs/heads/orca/ws/feature-search",
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
  },
];

const tabs: TerminalTab[] = [
  { id: "tab-main", label: "main", sessionId: "session-main" },
  { id: "tab-feature", label: "feature shell", sessionId: "session-feature" },
];

const customThemes: ThemeDocumentV1[] = [
  { version: 1, id: "ocean", name: "Ocean", base: "dark", tokens: {} },
  { version: 1, id: "sunrise", name: "Sunrise", base: "light", tokens: {} },
];

function AppearancePalette({ onOpen = () => {} }: { onOpen?: () => void }) {
  const appearance = useAppearanceSettings();
  const [open, setOpen] = useState(true);
  return (
    <CommandPalette
      open={open}
      worktrees={worktrees}
      tabs={tabs}
      onSelectWorktree={() => {}}
      onSelectTab={() => {}}
      appearance={{ ...appearance, onOpen }}
      onClose={() => setOpen(false)}
    />
  );
}

describe("CommandPalette", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("filters appearance commands and preserves worktrees and tabs for an empty query", () => {
    saveAppearanceSettings({ customThemes });
    render(<AppearancePalette />);
    expect(screen.getByRole("region", { name: "Worktrees" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Terminal tabs" })).toBeInTheDocument();

    const input = screen.getByPlaceholderText("Search worktrees and terminal tabs");
    fireEvent.change(input, { target: { value: "theme" } });
    const commands = within(screen.getByRole("region", { name: "Commands" }));
    expect(commands.getAllByRole("button").map((button) => button.dataset.commandId)).toEqual([
      "appearance.cycleTheme", "appearance.theme.ocean", "appearance.theme.sunrise",
    ]);
    expect(screen.queryByRole("region", { name: "Worktrees" })).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Terminal tabs" })).not.toBeInTheDocument();

    fireEvent.change(input, { target: { value: "zzz" } });
    expect(screen.queryByRole("region", { name: "Commands" })).not.toBeInTheDocument();
    expect(document.querySelectorAll("[data-command-id]")).toHaveLength(0);
    fireEvent.change(input, { target: { value: "theme" } });
    expect(document.querySelector('[data-command-id="appearance.cycleTheme"]')).toBeInTheDocument();
  });

  it.each([true, false])("cycles the stored built-in theme and closes (saved user themes: %s)", (hasThemes) => {
    saveAppearanceSettings({ theme: "charcoal", customThemes: hasThemes ? customThemes : [], activeCustomThemeId: hasThemes ? "ocean" : null });
    render(<AppearancePalette />);
    const command = document.querySelector('[data-command-id="appearance.cycleTheme"]')!;
    expect(command.querySelector("kbd")).toHaveTextContent(shortcutLabel("appearance.cycleTheme", isMacShortcutPlatform()));
    fireEvent.click(command);
    expect(loadAppearanceSettings()).toMatchObject({ theme: "dark", activeCustomThemeId: null });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("reselects a saved user theme through the store and closes", () => {
    saveAppearanceSettings({ customThemes, theme: "light" });
    render(<AppearancePalette />);
    fireEvent.click(document.querySelector('[data-command-id="appearance.theme.ocean"]')!);
    expect(loadAppearanceSettings()).toMatchObject({ theme: "light", activeCustomThemeId: "ocean" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("opens the supplied appearance surface with a shortcut hint and closes", () => {
    const onOpen = vi.fn();
    render(<AppearancePalette onOpen={onOpen} />);
    const command = document.querySelector('[data-command-id="appearance.openSettings"]')!;
    expect(command.querySelector("kbd")).toHaveTextContent(shortcutLabel("appearance.openSettings", isMacShortcutPlatform()));
    fireEvent.click(command);
    expect(onOpen).toHaveBeenCalledOnce();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("hides appearance commands and shortcut hints when no appearance surface is installed", () => {
    saveAppearanceSettings({ customThemes });
    const { container } = render(
      <CommandPalette open worktrees={worktrees} tabs={tabs} onSelectWorktree={vi.fn()} onSelectTab={vi.fn()} onClose={vi.fn()} />,
    );
    expect(container.querySelector('[data-command-id^="appearance."]')).not.toBeInTheDocument();
    for (const id of ["appearance.cycleTheme", "appearance.openSettings"] as const) {
      const hint = shortcutLabel(id, isMacShortcutPlatform());
      expect(Array.from(container.querySelectorAll("kbd")).some((kbd) => kbd.textContent === hint)).toBe(false);
    }
  });
  it("lists worktrees and terminal tabs and switches the selected item", () => {
    const onSelectWorktree = vi.fn();
    const onSelectTab = vi.fn();
    const onClose = vi.fn();

    render(
      <CommandPalette
        open
        worktrees={worktrees}
        tabs={tabs}
        onSelectWorktree={onSelectWorktree}
        onSelectTab={onSelectTab}
        onClose={onClose}
      />,
    );

    expect(screen.getByRole("dialog", { name: "Command palette" })).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText("Search worktrees and terminal tabs"), { target: { value: "feature" } });
    fireEvent.click(screen.getByRole("button", { name: /feature-search/i }));
    expect(onSelectWorktree).toHaveBeenCalledWith(worktrees[1]);
    expect(onClose).toHaveBeenCalledOnce();

    fireEvent.change(screen.getByPlaceholderText("Search worktrees and terminal tabs"), { target: { value: "shell" } });
    fireEvent.click(screen.getByRole("button", { name: /feature shell/i }));
    expect(onSelectTab).toHaveBeenCalledWith("tab-feature");
  });

  it("closes on Escape", () => {
    const onClose = vi.fn();
    render(
      <CommandPalette open worktrees={worktrees} tabs={tabs} onSelectWorktree={vi.fn()} onSelectTab={vi.fn()} onClose={onClose} />,
    );
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
  });
  it("offers New Window only when the desktop shell provides it", () => {
    const onNewWindow = vi.fn();
    const onClose = vi.fn();
    const first = render(
      <CommandPalette open worktrees={worktrees} tabs={tabs} onSelectWorktree={vi.fn()} onSelectTab={vi.fn()} onClose={vi.fn()} />,
    );
    expect(within(first.container).queryByRole("button", { name: "New Window" })).not.toBeInTheDocument();
    first.unmount();

    const { container } = render(
      <CommandPalette open worktrees={worktrees} tabs={tabs} onSelectWorktree={vi.fn()} onSelectTab={vi.fn()} onNewWindow={onNewWindow} onClose={onClose} />,
    );
    const palette = within(container);
    fireEvent.change(palette.getByPlaceholderText("Search worktrees and terminal tabs"), { target: { value: "window" } });
    const command = palette.getByRole("button", { name: "New Window" });
    expect(command).toHaveAttribute("data-command-id", "window.new");
    fireEvent.click(command);
    expect(onNewWindow).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledOnce();
  });
});
