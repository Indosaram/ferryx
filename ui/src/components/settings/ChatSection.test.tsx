import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

import {
  PANE_CHAT_ANNOUNCE_LEVEL_STORAGE_KEY,
  PANE_CHAT_TOOL_CALL_VISIBILITY_STORAGE_KEY,
  saveAnnounceLevel,
  saveToolCallVisibility,
  loadAnnounceLevel,
  loadToolCallVisibility,
  DEFAULT_ANNOUNCE_LEVEL,
} from "../../lib/paneChatPreferences";
import { ChatSection } from "./ChatSection";

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

describe("Settings -> Chat", () => {
  it("saves the tool-call visibility the user picks under its ferryx key", () => {
    render(<ChatSection />);
    fireEvent.click(screen.getByTestId("pane-chat-tool-call-visibility-hide-all"));

    expect(window.localStorage.getItem(PANE_CHAT_TOOL_CALL_VISIBILITY_STORAGE_KEY)).toBe("hide-all");
    expect(loadToolCallVisibility()).toBe("hide-all");
    expect(screen.getByTestId("pane-chat-tool-call-visibility-hide-all")).toBeChecked();
  });

  it("saves the announcement level and shows the reference's default as checked", () => {
    render(<ChatSection />);
    // An untouched install reads the reference's default, so the control must agree with it.
    expect(screen.getByTestId(`pane-chat-announce-level-${DEFAULT_ANNOUNCE_LEVEL}`)).toBeChecked();

    fireEvent.click(screen.getByTestId("pane-chat-announce-level-status-only"));

    expect(window.localStorage.getItem(PANE_CHAT_ANNOUNCE_LEVEL_STORAGE_KEY)).toBe("status-only");
    expect(loadAnnounceLevel()).toBe("status-only");
    expect(screen.getByTestId("pane-chat-announce-level-status-only")).toBeChecked();
  });

  it("tells a mounted pane the moment a preference changes", () => {
    const listener = vi.fn();
    const stop = vi.fn();
    window.addEventListener("ferryx-pane-chat-preferences", listener);
    saveToolCallVisibility("zen");
    saveAnnounceLevel("everything");
    expect(listener).toHaveBeenCalledTimes(2);
    window.removeEventListener("ferryx-pane-chat-preferences", listener);
    stop();
  });
});
