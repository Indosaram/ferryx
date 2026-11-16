import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { MobileChatWorkspace } from "../remote/chat/MobileChatWorkspace";
import { loadAppearanceSettings } from "../lib/appearanceSettings";
import { APPEARANCE_SETTINGS_STORAGE_KEY } from "../lib/storageKeys";
import {
  APPEARANCE_QA_STATES, CHAT_QA_STATES, AppearanceQaFixture,
  QaTerminalSurface, chatQaComposerProps, isChatQaState, seedAppearanceQaState,
} from "./chatQaStates";

function renderState(state: string) {
  return render(
    <MobileChatWorkspace showHeader={false} messages={[]} onSendMessage={() => true} {...chatQaComposerProps(state)} />,
  );
}

describe("mobile chat QA states", () => {
  afterEach(() => {
    cleanup();
    window.localStorage.removeItem(APPEARANCE_SETTINGS_STORAGE_KEY);
  });

  it("seeds the production ReferencePromptCard in the message stream for a two-option approval", () => {
    renderState("prompt-approval");
    expect(screen.getByTestId("chat-stream-prompt")).toContainElement(screen.getByTestId("reference-prompt-card"));
    // A two-option approval is the card's own Approve / Decline pair, not the option list.
    const card = screen.getByTestId("reference-prompt-card");
    expect(within(card).getByTestId("approval-accept")).toHaveTextContent("Yes, run it");
    expect(within(card).getByTestId("approval-decline")).toHaveTextContent("No, tell Codex what to do instead");
    expect(screen.queryByTestId("reference-prompt-option-0")).not.toBeInTheDocument();
  });

  it("seeds a multi-select prompt with more than two options", () => {
    renderState("prompt-multi");
    expect(screen.getByTestId("reference-prompt-multi-3")).toBeInTheDocument();
    expect(screen.getByTestId("reference-prompt-submit")).toBeInTheDocument();
  });

  it("seeds a prompt with a typed-answer field", () => {
    renderState("prompt-custom");
    expect(screen.getByTestId("reference-prompt-custom-input")).toBeInTheDocument();
  });

  it("seeds held messages, staged files and a plain-language attach error", () => {
    renderState("held");
    expect(screen.getByTestId("held-message-held-1")).toBeInTheDocument();
    cleanup();

    renderState("attach");
    expect(screen.getByTestId("attachment-preview-att-1")).toBeInTheDocument();
    expect(screen.getByTestId("attachment-preview-att-2")).toBeInTheDocument();
    cleanup();

    renderState("attach-error");
    const error = screen.getByTestId("chat-composer-attach-error");
    expect(error).toHaveTextContent(/^Couldn't attach this file\./);
    expect(error.textContent).not.toMatch(/[A-Z]{2,}_[A-Z]+/);
  });

  it("seeds a disconnected chat whose composer cannot send", () => {
    renderState("offline");
    expect(screen.getByTestId("chat-composer-warning")).toHaveTextContent("The connection to your computer dropped.");
    expect(screen.getByTestId("send-button")).toBeDisabled();
  });

  it("seeds approvals only through the production prompt card, never the legacy per-message card", () => {
    const harness = readFileSync(resolve(__dirname, "MobileChatQaHarness.tsx"), "utf-8");
    expect(harness).not.toMatch(/approvalAction/);
  });

  it.each(APPEARANCE_QA_STATES)("registers and renders the appearance fixture %s", (state) => {
    expect(CHAT_QA_STATES).toContain(state);
    expect(isChatQaState(state)).toBe(true);
    seedAppearanceQaState(state, "dark");
    render(<AppearanceQaFixture state={state} />);
    expect(screen.getByTestId(`qa-${state}`)).toBeInTheDocument();
    if (state === "appearance-preview" || state === "custom-css-active") {
      expect(screen.getByTestId("appearance-chat-preview")).toBeInTheDocument();
    }
    if (state === "theme-import-error") expect(screen.getByRole("alert")).toBeInTheDocument();
    if (state === "remote-appearance-sheet") {
      expect(screen.getByTestId("remote-appearance-sheet")).toBeInTheDocument();
      expect(screen.getByRole("combobox", { name: "Theme" })).toHaveValue("dark");
    }
  });

  it.each(["light", "dark"] as const)("seeds the requested %s base without activating a saved override", (theme) => {
    seedAppearanceQaState("appearance-preview", theme);
    const settings = loadAppearanceSettings();
    expect(settings.theme).toBe(theme);
    expect(settings.activeCustomThemeId).toBeNull();
    expect(settings.customThemes).toHaveLength(1);
    expect(settings.customThemes[0].base).toBe(theme);
  });

  it("retains rejected theme data and falls back to the built-in palette", () => {
    seedAppearanceQaState("theme-import-error", "light");
    render(<AppearanceQaFixture state="theme-import-error" />);
    expect(loadAppearanceSettings().customThemes).toEqual([]);
    expect(loadAppearanceSettings().activeCustomThemeId).toBeNull();
    expect(screen.getByRole("alert").textContent).toContain("version:");
    expect(JSON.parse(window.localStorage.getItem(APPEARANCE_SETTINGS_STORAGE_KEY)!).customThemes[0].version).toBe(2);
  });

  it("keeps the CSS recovery control outside chat and removes the seeded stylesheet", () => {
    seedAppearanceQaState("custom-css-active", "dark");
    render(<AppearanceQaFixture state="custom-css-active" />);
    expect(loadAppearanceSettings().customCss).not.toBe("");
    const recovery = screen.getByRole("button", { name: "Remove custom CSS" });
    expect(recovery.closest("[data-chat-scope]")).toBeNull();
    fireEvent.click(recovery);
    expect(loadAppearanceSettings().customCss).toBe("");
  });

  it("renders a fallback for an unknown appearance state without creating a fixture", () => {
    render(<AppearanceQaFixture state="unknown-appearance-state" />);
    expect(screen.getByTestId("appearance-qa-fallback")).toBeInTheDocument();
    expect(isChatQaState("unknown-appearance-state")).toBe(false);
    expect(chatQaComposerProps("unknown-appearance-state")).toEqual({});
  });

  it("renders the terminal stand-in as a neutral surface with no text", () => {
    render(<QaTerminalSurface />);
    const surface = screen.getByTestId("qa-terminal-placeholder");
    expect(surface).toHaveTextContent("");
    expect(surface).toHaveAttribute("aria-hidden", "true");
  });
});
