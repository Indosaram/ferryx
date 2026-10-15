import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { useShortcutOverrides } from "../../lib/shortcutOverrides";
import { SHORTCUT_OVERRIDES_STORAGE_KEY } from "../../lib/storageKeys";
import { ShortcutsSection } from "./ShortcutsSection";

function Harness({ isMac }: { isMac: boolean }) {
  const { overrides, setOverride, resetOverrides } = useShortcutOverrides();
  return (
    <ShortcutsSection isMac={isMac} overrides={overrides} onChangeOverride={setOverride} onResetAll={resetOverrides} />
  );
}

function row(id: string): HTMLElement {
  const element = document.querySelector(`[data-shortcut-id="${id}"]`);
  if (!(element instanceof HTMLElement)) throw new Error(`missing row ${id}`);
  return element;
}

function storedOverrides(): Record<string, unknown> | null {
  const raw = localStorage.getItem(SHORTCUT_OVERRIDES_STORAGE_KEY);
  return raw === null ? null : (JSON.parse(raw) as Record<string, unknown>);
}

function openRecorder(title: string): HTMLElement {
  fireEvent.click(screen.getByRole("button", { name: `Edit shortcut for ${title}` }));
  return screen.getByRole("textbox", { name: `Record shortcut for ${title}` });
}

beforeEach(() => {
  localStorage.clear();
});

afterEach(cleanup);

describe("ShortcutsSection recorder", () => {
  it("records a new chord and saves it", () => {
    render(<Harness isMac />);
    const recorder = openRecorder("Close active tab");

    act(() => {
      fireEvent.keyDown(recorder, { key: "y", code: "KeyY", metaKey: true });
    });

    expect(storedOverrides()).toEqual({ "tab.close": { key: "y", mod: true } });
    expect(screen.queryByRole("textbox", { name: "Record shortcut for Close active tab" })).toBeNull();
    expect(within(row("tab.close")).getByText("⌘Y")).toBeInTheDocument();
    expect(within(row("tab.close")).queryByText("⌘W")).toBeNull();
  });

  it("rejects a conflicting chord and names the owner", () => {
    render(<Harness isMac />);
    const recorder = openRecorder("Close active tab");

    act(() => {
      fireEvent.keyDown(recorder, { key: "t", code: "KeyT", metaKey: true });
    });

    expect(within(row("tab.close")).getByRole("alert")).toHaveTextContent("⌘T is already used by New terminal tab.");
    expect(storedOverrides()).toBeNull();
    // Refused, not saved: the recorder stays open for another chord.
    expect(screen.getByRole("textbox", { name: "Record shortcut for Close active tab" })).toBeInTheDocument();

    act(() => {
      fireEvent.keyDown(recorder, { key: "c", code: "KeyC", metaKey: true });
    });
    expect(within(row("tab.close")).getByRole("alert")).toHaveTextContent("⌘C is already used by Copy.");
    expect(storedOverrides()).toBeNull();
  });

  it("unbind and reset restore behavior", () => {
    render(<Harness isMac />);

    fireEvent.click(screen.getByRole("button", { name: "Unbind shortcut for New terminal tab" }));
    expect(storedOverrides()).toEqual({ "tab.newTerminal": null });
    expect(within(row("tab.newTerminal")).getByText("Unbound")).toBeInTheDocument();
    expect(within(row("tab.newTerminal")).queryByText("⌘T")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Reset shortcut for New terminal tab" }));
    expect(storedOverrides()).toBeNull();
    expect(within(row("tab.newTerminal")).getByText("⌘T")).toBeInTheDocument();

    // The freed chord is assignable while unbound, and Reset all restores every default.
    fireEvent.click(screen.getByRole("button", { name: "Unbind shortcut for New terminal tab" }));
    const recorder = openRecorder("Close active tab");
    act(() => {
      fireEvent.keyDown(recorder, { key: "t", code: "KeyT", metaKey: true });
    });
    expect(storedOverrides()).toEqual({ "tab.newTerminal": null, "tab.close": { key: "t", mod: true } });

    fireEvent.click(screen.getByRole("button", { name: "Reset all" }));
    expect(storedOverrides()).toBeNull();
    expect(within(row("tab.newTerminal")).getByText("⌘T")).toBeInTheDocument();
    expect(within(row("tab.close")).getByText("⌘W")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Reset all" })).toBeDisabled();
  });

  it("warns on bare Ctrl+letter off macOS", () => {
    render(<Harness isMac={false} />);
    const recorder = openRecorder("Close active tab");

    act(() => {
      fireEvent.keyDown(recorder, { key: "y", code: "KeyY", ctrlKey: true });
    });

    expect(storedOverrides()).toEqual({ "tab.close": { key: "y", mod: true } });
    expect(within(row("tab.close")).getByText("Ctrl+Y", { selector: "kbd" })).toBeInTheDocument();
    expect(within(row("tab.close")).getByRole("status")).toHaveTextContent(/bare Ctrl\+letter.*goes to the shell/);

    // A chord with another modifier does not warn.
    const again = openRecorder("Close active tab");
    act(() => {
      fireEvent.keyDown(again, { key: "Y", code: "KeyY", ctrlKey: true, shiftKey: true });
    });
    expect(storedOverrides()).toEqual({ "tab.close": { key: "y", mod: true, shift: true } });
    expect(within(row("tab.close")).queryByRole("status")).toBeNull();
  });

  it("ignores IME composition keys while recording", () => {
    render(<Harness isMac />);
    const recorder = openRecorder("Close active tab");

    act(() => {
      fireEvent.keyDown(recorder, { key: "ㅛ", code: "KeyY", metaKey: true, isComposing: true });
      fireEvent.keyDown(recorder, { key: "Process", code: "KeyY", metaKey: true, keyCode: 229 });
      fireEvent.keyDown(recorder, { key: "Dead", code: "KeyE", altKey: true });
      fireEvent.keyDown(recorder, { key: "Meta", code: "MetaLeft", metaKey: true });
    });

    expect(storedOverrides()).toBeNull();
    expect(screen.getByRole("textbox", { name: "Record shortcut for Close active tab" })).toBeInTheDocument();

    // Once composition ends, a Hangul-mode key still records its physical letter.
    act(() => {
      fireEvent.keyDown(recorder, { key: "ㅛ", code: "KeyY", metaKey: true });
    });
    expect(storedOverrides()).toEqual({ "tab.close": { key: "y", mod: true } });
  });
});
