import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { MobileChatWorkspace } from "../remote/chat/MobileChatWorkspace";
import { loadAppearanceSettings } from "../lib/appearanceSettings";
import { APPEARANCE_SETTINGS_STORAGE_KEY } from "../lib/storageKeys";
import {
  APPEARANCE_QA_STATES, CHAT_QA_STATES, AppearanceQaFixture,
  QaTerminalSurface, chatQaComposerProps, chatQaMessages, isChatQaState, seedAppearanceQaState,
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

  it("seeds the queued list through the composer's queued rows", () => {
    const props = chatQaComposerProps("queued-list");
    expect(props.queuedMessages).toHaveLength(2);
    render(
      <MobileChatWorkspace showHeader={false} messages={[]} onSendMessage={() => true} {...props} />,
    );
    expect(screen.getAllByTestId(/^queued-follow-up-queued-/)).toHaveLength(2);
  });

  it("seeds chips and permission control as frozen-contract sources", () => {
    const props = chatQaComposerProps("chips");
    expect(props.chipSources?.models).toHaveLength(2);
    expect(props.chipSources?.skills).toHaveLength(1);
    expect(typeof props.chipSources?.threads).toBe("function");
    expect(typeof props.chipSources?.pullRequests).toBe("function");
    expect(props.permissionControl?.current).toBe("supervised");
    expect(props.permissionControl?.modes).toContain("full-access");
  });

  it("seeds the chip menu on an empty draft, and the chip-bearing draft separately", () => {
    // Two states, because the menu and the seeded chips fail for different reasons: the menu
    // needs an empty buffer, the chips need a draft with every link kind already in it.
    expect(chatQaComposerProps("chips-menu").draft).toBeUndefined();
    expect(chatQaComposerProps("chips").draft).toContain("ferryx://chip/model/opus");
    // `chips-typed` keeps the seeded draft: it is the state that pins the typing defect.
    expect(chatQaComposerProps("chips-typed").draft).toBe(chatQaComposerProps("chips").draft);
  });

  it("seeds the citation draft with and without the interaction, from one constant", () => {
    // `citation` types a comment through the pencil; `citation-seeded` mounts the same draft and
    // touches nothing, so the two runs together separate "editing works" from "a seeded comment
    // survives the rich editor".
    const typed = chatQaComposerProps("citation");
    const seeded = chatQaComposerProps("citation-seeded");
    expect(typed.draft).toBe(seeded.draft);
    expect(typed.draft).toContain('"why the mask stays composited"');
    expect(typed.onCiteSelection).toBeTypeOf("function");
    expect(seeded.onCiteSelection).toBeUndefined();
  });

  it("puts a chip-bearing draft in the composer so the chips state is not an empty box", () => {
    const props = chatQaComposerProps("chips");
    const draft = props.draft ?? "";
    // every kind the composer writes: model, pull request, skill, file and a slash command
    expect(draft).toContain("ferryx://chip/model/opus");
    expect(draft).toContain("ferryx://chip/pr/42");
    expect(draft).toContain("ferryx://chip/skill/run-tests");
    expect(draft).toContain("ferryx://chip/file/DESIGN.md");
    expect(draft).toContain("/compact");
  });

  it("seeds a citation chip that already carries its comment", () => {
    renderState("citation");
    const row = screen.getByTestId("composer-citation-0");
    expect(row).toHaveTextContent("why the mask stays composited");
    expect(screen.getByTestId("citation-comment-edit-0")).toBeInTheDocument();
  });

  it("seeds two queued rows, one of them refused", () => {
    renderState("queued-list");
    expect(screen.getByTestId("queued-follow-up-queued-1")).toHaveTextContent("Queued");
    expect(screen.getByTestId("queued-follow-up-queued-2")).toHaveTextContent("Not confirmed");
  });

  it("offers the answer-attach row on a prompt that takes a typed answer", () => {
    renderState("prompt-custom");
    expect(screen.getByTestId("reference-prompt-custom-input")).toBeInTheDocument();
    expect(screen.getByTestId("reference-answer-attach")).toBeInTheDocument();
  });

  it("seeds a multi-select prompt that also accepts a typed answer with attachments", () => {
    renderState("prompt-multi");
    expect(screen.getByTestId("reference-prompt-multi-0")).toBeInTheDocument();
    expect(screen.getByTestId("reference-prompt-custom-input")).toBeInTheDocument();
    expect(screen.getByTestId("reference-answer-attach")).toBeInTheDocument();
  });

  it("seeds the plan card as a rich plan part, not as prose", () => {
    render(
      <MobileChatWorkspace showHeader={false} messages={chatQaMessages("plan-card") ?? []}
        onSendMessage={() => true} {...chatQaComposerProps("plan-card")} />,
    );
    expect(screen.getByTestId("proposed-plan-card")).toBeInTheDocument();
  });

  it.each(["tool-groups-all", "tool-groups-hide-successful", "tool-groups-hide-all", "tool-groups-zen"] as const)(
    "seeds %s with the same turns filtered by the pane preference",
    (state) => {
      const props = chatQaComposerProps(state);
      const messages = chatQaMessages(state);
      expect(messages?.some((message) => (message.toolCalls?.length ?? 0) > 0)).toBe(true);
      render(
        <MobileChatWorkspace showHeader={false} messages={messages ?? []} onSendMessage={() => true} {...props} />,
      );
      const rows = screen.queryAllByTestId("work-row").length;
      if (state === "tool-groups-all") expect(rows).toBe(4);
      if (state === "tool-groups-hide-successful") expect(rows).toBe(2);
      if (state === "tool-groups-hide-all" || state === "tool-groups-zen") {
        // hide-all and zen keep the failed call: a failure is the one row every mode shows
        // (activity-log.md), so the fixture's seeded error is what survives here.
        expect(rows).toBe(1);
        expect(screen.getByTestId("work-row")).toHaveTextContent("Edited MobileChatWorkspace.tsx");
      }
      cleanup();
    },
  );

  it("seeds stream content for the mermaid, lightbox, find-bar and plan-card states", () => {
    for (const state of ["mermaid", "lightbox", "find-bar", "plan-card"] as const) {
      expect(chatQaMessages(state)).not.toBeNull();
    }
    render(
      <MobileChatWorkspace showHeader={false} messages={chatQaMessages("lightbox") ?? []}
        onSendMessage={() => true} {...chatQaComposerProps("lightbox")} />,
    );
    expect(screen.getByAltText("screenshot-390x844.png")).toBeInTheDocument();
  });

  it("renders the terminal stand-in as a neutral surface with no text", () => {
    render(<QaTerminalSurface />);
    const surface = screen.getByTestId("qa-terminal-placeholder");
    expect(surface).toHaveTextContent("");
    expect(surface).toHaveAttribute("aria-hidden", "true");
  });
});
