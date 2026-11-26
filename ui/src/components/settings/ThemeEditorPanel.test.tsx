import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  _resetNativeOsThemeForTest,
  applyAppearanceSettings,
  DEFAULT_APPEARANCE_SETTINGS,
  loadAppearanceSettings,
} from "../../lib/appearanceSettings";
import { AppearanceSection } from "./AppearanceSection";
import { inspectThemeRoleAtElement } from "./themeInspector";
import { CommandPalette } from "../CommandPalette";

vi.mock("../../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/tauri")>();
  return {
    ...actual,
    setTerminalAppearance: vi.fn().mockResolvedValue(null),
    setTerminalThemePalette: vi.fn().mockResolvedValue(null),
    listEnvironmentThemes: vi.fn().mockResolvedValue(null),
    readEnvironmentThemeBackground: vi.fn().mockResolvedValue(null),
    saveSupervisorTranscriptFile: vi.fn().mockResolvedValue("saved"),
    isTauriRuntime: () => false,
  };
});

describe("T3 Code visual theme editor, inspector, import dialog, and palette picker", () => {
  beforeEach(() => {
    localStorage.clear();
    _resetNativeOsThemeForTest();
    applyAppearanceSettings(DEFAULT_APPEARANCE_SETTINGS);
  });

  afterEach(() => {
    cleanup();
    applyAppearanceSettings(DEFAULT_APPEARANCE_SETTINGS);
    localStorage.clear();
  });

  it("opens visual ThemeEditorPanel, live-previews OKLCH edits, and saves a dual-mode theme with app CSS", () => {
    render(<AppearanceSection />);
    fireEvent.click(screen.getByRole("button", { name: "Create theme" }));
    expect(screen.getByTestId("theme-editor-panel")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Theme name"), { target: { value: "Midnight Berry" } });
    fireEvent.change(screen.getByLabelText("Canvas"), { target: { value: "#191524" } });
    fireEvent.change(screen.getByLabelText("Accent"), { target: { value: "#c084fc" } });
    expect(document.documentElement.style.getPropertyValue("--background")).toBe("#191524");

    fireEvent.click(screen.getByRole("button", { name: /Add light variant/i }));
    fireEvent.click(screen.getByRole("tab", { name: /Custom CSS/i }));
    const cssBox = screen.getByLabelText("Theme custom CSS");
    fireEvent.change(cssBox, { target: { value: '[data-slot="sidebar"] { opacity: 0.95; }' } });

    fireEvent.click(screen.getByRole("button", { name: "Save theme" }));
    const stored = loadAppearanceSettings();
    const saved = stored.customThemes.find((t) => t.name === "Midnight Berry");
    expect(saved).toBeDefined();
    expect(saved?.cssScope).toBe("app");
    expect(saved?.variants?.light).toBeDefined();
    expect(stored.activeCustomThemeId).toBe(saved?.id);
  });

  it("maps app elements to exact theme roles in the live element inspector", () => {
    const sidebar = document.createElement("aside");
    sidebar.setAttribute("data-slot", "sidebar");
    const bubble = document.createElement("div");
    bubble.setAttribute("data-user-message-bubble", "");
    document.body.append(sidebar, bubble);

    expect(inspectThemeRoleAtElement(sidebar)?.primaryRole).toBe("sidebar");
    expect(inspectThemeRoleAtElement(bubble)?.primaryRole).toBe("messageSurface");
    sidebar.remove();
    bubble.remove();
  });

  it("opens CommandPalette in themes mode and selects a curated T3 Code theme", () => {
    const updateSettings = vi.fn();
    const onClose = vi.fn();
    render(
      <CommandPalette
        open
        initialMode="themes"
        worktrees={[]}
        tabs={[]}
        onSelectWorktree={vi.fn()}
        onSelectTab={vi.fn()}
        appearance={{
          settings: DEFAULT_APPEARANCE_SETTINGS,
          updateSettings,
          onOpen: vi.fn(),
        }}
        onClose={onClose}
      />,
    );
    const t3ChatBtn = screen.getByRole("button", { name: /T3 Chat/i });
    fireEvent.mouseEnter(t3ChatBtn);
    expect(document.documentElement.dataset.customTheme).toBe("t3-chat");
    fireEvent.click(t3ChatBtn);
    expect(updateSettings).toHaveBeenCalledWith({ activeCustomThemeId: "t3-chat", themeHalves: undefined });
    expect(onClose).toHaveBeenCalledOnce();
  });
});
