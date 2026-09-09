import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { loadBrowserSettings, saveBrowserSettings } from "../lib/browserSettings";
import { openExternalUrl } from "../lib/browserTauri";
import { registerBuiltInBrowserLinkOpener, requestTerminalLinkOpen } from "../lib/linkRouting";
import { TerminalLinkActions } from "./TerminalLinkActions";

vi.mock("../lib/browserTauri", () => ({ openExternalUrl: vi.fn(async () => undefined) }));

const toasts = vi.hoisted(() => ({
  render: null as ((id: string) => ReactNode) | null,
  custom: vi.fn(),
  dismiss: vi.fn(),
}));
vi.mock("./ui/sonner", () => ({
  toast: {
    custom: (renderer: (id: string) => ReactNode) => {
      toasts.render = renderer;
      toasts.custom();
      return "choice";
    },
    dismiss: toasts.dismiss,
  },
}));

const openBuiltIn = vi.fn(async () => undefined);
let unregister: () => void;

beforeEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
  toasts.render = null;
  unregister = registerBuiltInBrowserLinkOpener(openBuiltIn);
});

afterEach(() => {
  cleanup();
  unregister();
  localStorage.clear();
});

async function showChooser() {
  render(<TerminalLinkActions />);
  expect(await requestTerminalLinkOpen("https://example.com/first")).toBe("chooser");
  if (!toasts.render) throw new Error("Expected a link chooser");
  return render(toasts.render("choice"));
}

describe("remember terminal link destination", () => {
  it.each([
    { button: "Web Browser", destination: "external", builtin: false },
    { button: "Built-in Browser", destination: "builtin", builtin: true },
  ])("remembers $destination and bypasses the chooser on the next link", async ({ button, destination, builtin }) => {
    await showChooser();
    expect(screen.getByRole("checkbox")).not.toBeChecked();
    fireEvent.click(screen.getByRole("checkbox"));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: button })));

    expect(loadBrowserSettings()).toMatchObject({
      openLinksInBuiltInBrowser: builtin,
      showTerminalLinkActions: false,
      shiftOpensSystemBrowser: true,
    });
    cleanup();
    render(<TerminalLinkActions />);
    expect(await requestTerminalLinkOpen("https://example.com/next")).toBe(destination);
    expect(toasts.custom).toHaveBeenCalledTimes(1);
    expect(builtin ? openBuiltIn : openExternalUrl).toHaveBeenLastCalledWith("https://example.com/next");

    saveBrowserSettings({ showTerminalLinkActions: true });
    expect(await requestTerminalLinkOpen("https://example.com/choose-again")).toBe("chooser");
    expect(toasts.custom).toHaveBeenCalledTimes(2);
  });

  it("does not remember an unchecked one-time choice", async () => {
    await showChooser();
    const before = loadBrowserSettings();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Web Browser" })));
    expect(loadBrowserSettings()).toEqual(before);
    expect(await requestTerminalLinkOpen("https://example.com/next")).toBe("chooser");
  });

  it("does not save when checked but dismissed", async () => {
    await showChooser();
    const before = loadBrowserSettings();
    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "Close link actions" }));
    expect(loadBrowserSettings()).toEqual(before);
    expect(openBuiltIn).not.toHaveBeenCalled();
    expect(openExternalUrl).not.toHaveBeenCalled();
  });

  it("still opens externally with Shift after remembering the built-in browser", async () => {
    await showChooser();
    fireEvent.click(screen.getByRole("checkbox"));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Built-in Browser" })));
    expect(await requestTerminalLinkOpen("https://example.com/shift", true)).toBe("external");
    expect(openExternalUrl).toHaveBeenCalledExactlyOnceWith("https://example.com/shift");
  });
});
