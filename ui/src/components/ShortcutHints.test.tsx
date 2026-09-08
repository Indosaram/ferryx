import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ShortcutHints, type ShortcutHintContext } from "./ShortcutHints";
import { shortcutLabel, useShortcuts } from "../lib/shortcuts";

const context: ShortcutHintContext = {
  tabIds: ["second", "first"],
  closeTabId: "second",
  worktrees: [{ path: "/repo/feature", workspaceId: "project" }, { path: "/repo", workspaceId: "project" }],
};

function Fixture({ isMac = true, onNew = () => undefined }: { isMac?: boolean; onNew?: () => void }) {
  useShortcuts({ "tab.newTerminal": onNew }, { isMac });
  return (
    <>
      <ShortcutHints
        isMac={isMac}
        getContext={() => context}
        enabledActions={["tab.newTerminal", "tab.newBrowser", "browser.back", "settings.toggle", "tab.select1", "tab.select2", "workspace.select1", "workspace.select2"]}
      />
      <button data-shortcut="tab.newTerminal">New</button>
      <button data-shortcut="tab.newBrowser">Browser</button>
      <button data-shortcut="browser.back">Back</button>
      <button data-shortcut="settings.toggle">Settings</button>
      <button data-shortcut="tab.newTerminal" disabled>Disabled</button>
      <div data-shortcut-scope-active="false"><button data-shortcut="tab.newTerminal">Inactive</button></div>
      <button data-tab-dnd-id="first">First tab</button>
      <button data-tab-dnd-id="second">Second tab</button>
      <button data-shortcut-worktree-path="/repo" data-shortcut-workspace-id="project">Root</button>
      <input aria-label="Editor" />
      <div className="terminal-host"><textarea aria-label="Terminal" /></div>
    </>
  );
}

function hold(init: KeyboardEventInit = { key: "Meta", metaKey: true }) {
  fireEvent.keyDown(document.activeElement ?? window, init);
  act(() => vi.advanceTimersByTime(300));
}

function hints() {
  return Array.from(document.querySelectorAll<HTMLElement>("[data-shortcut-hint]"));
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
    x: 100, y: 100, left: 100, top: 100, right: 180, bottom: 128, width: 80, height: 28, toJSON: () => ({}),
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("modifier-held shortcut targets", () => {
  it("reveals existing labels only after the intentional hold, without taking focus", () => {
    render(<Fixture />);
    const sink = screen.getByRole("textbox", { name: "Terminal" });
    sink.focus();
    fireEvent.keyDown(sink, { key: "Meta", metaKey: true });
    act(() => vi.advanceTimersByTime(299));
    expect(hints()).toHaveLength(0);
    act(() => vi.advanceTimersByTime(1));
    expect(hints().map((hint) => hint.textContent)).toContain(shortcutLabel("tab.newTerminal", true));
    expect(document.activeElement).toBe(sink);
    expect(hints().filter((hint) => hint.dataset.shortcutHint === "tab.newTerminal")).toHaveLength(1);
  });

  it("does not flash for a quick shortcut and still executes it once", () => {
    const onNew = vi.fn();
    render(<Fixture onNew={onNew} />);
    fireEvent.keyDown(window, { key: "Meta", metaKey: true });
    fireEvent.keyDown(window, { key: "t", code: "KeyT", metaKey: true });
    act(() => vi.advanceTimersByTime(500));
    expect(onNew).toHaveBeenCalledTimes(1);
    expect(hints()).toHaveLength(0);
  });

  it("refines hints when Shift is added and clears when the last modifier is released", () => {
    render(<Fixture />);
    hold();
    fireEvent.keyDown(window, { key: "Shift", metaKey: true, shiftKey: true });
    expect(hints().map((hint) => hint.dataset.shortcutHint)).toEqual(["tab.newBrowser"]);
    fireEvent.keyUp(window, { key: "Meta", shiftKey: true });
    expect(hints()).toHaveLength(0);
  });

  it("uses the actual tab and workspace order rather than DOM order", () => {
    render(<Fixture />);
    hold({ key: "Control", ctrlKey: true });
    expect(hints().find((hint) => hint.dataset.shortcutHint === "tab.select1")?.dataset.shortcutTarget).toBe("Second tab");
    fireEvent.keyUp(window, { key: "Control" });
    hold();
    expect(hints().find((hint) => hint.dataset.shortcutHint === "workspace.select2")?.dataset.shortcutTarget).toBe("Root");
  });

  it.each([true, false])("shows the existing Alt alias on platform isMac=%s", (isMac) => {
    render(<Fixture isMac={isMac} />);
    hold({ key: "Alt", altKey: true });
    expect(hints().find((hint) => hint.dataset.shortcutHint === "browser.back")?.textContent).toBe(isMac ? "⌥ArrowLeft" : "Alt+ArrowLeft");
  });

  it("does not show shortcuts blocked by ordinary text editing", () => {
    render(<Fixture />);
    screen.getByRole("textbox", { name: "Editor" }).focus();
    hold();
    expect(hints().some((hint) => hint.dataset.shortcutHint === "tab.newTerminal")).toBe(false);
    expect(hints().some((hint) => hint.dataset.shortcutHint === "settings.toggle")).toBe(true);
  });

  it.each(["blur", "compositionstart", "pointerdown", "visibilitychange"])("clears visible hints on %s", (event) => {
    render(<Fixture />);
    hold();
    expect(hints().length).toBeGreaterThan(0);
    fireEvent(event === "visibilitychange" ? document : window, new Event(event));
    expect(hints()).toHaveLength(0);
  });

  it("ignores AltGraph and IME-owned modifier events", () => {
    render(<Fixture />);
    const event = new KeyboardEvent("keydown", { key: "AltGraph", ctrlKey: true, altKey: true });
    Object.defineProperty(event, "getModifierState", { value: (key: string) => key === "AltGraph" });
    fireEvent(window, event);
    fireEvent.keyDown(window, { key: "Meta", metaKey: true, isComposing: true });
    act(() => vi.advanceTimersByTime(500));
    expect(hints()).toHaveLength(0);
  });

  it("does not advertise a workspace chord shadowed by tab selection on Linux", () => {
    render(<Fixture isMac={false} />);
    hold({ key: "Control", ctrlKey: true });
    expect(hints().some((hint) => hint.dataset.shortcutHint === "tab.select2")).toBe(true);
    expect(hints().some((hint) => hint.dataset.shortcutHint === "workspace.select2")).toBe(false);
  });

  it("removes hints when an existing target becomes disabled", async () => {
    render(<Fixture />);
    hold();
    await act(async () => {
      screen.getByRole("button", { name: "New" }).setAttribute("disabled", "");
    });
    expect(hints().some((hint) => hint.dataset.shortcutHint === "tab.newTerminal")).toBe(false);
  });
});
