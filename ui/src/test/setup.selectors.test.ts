import { afterEach, describe, expect, it, vi } from "vitest";

afterEach(() => vi.restoreAllMocks());

describe("jsdom top-layer selector delegation", () => {
  it("evaluates modal state without recursively delegating to its own selector engine", () => {
    const element = document.createElement("div");
    const matches = vi.spyOn(Element.prototype, "matches");

    expect(element.matches(":modal")).toBe(false);
    // One outer evaluation and one rejected native delegation per state:
    // :modal, then its :fullscreen fallback. This checks recursion, not speed.
    expect(matches.mock.calls.length).toBeLessThanOrEqual(4);
  });

  it("preserves fullscreen document state and compound selector evaluation", () => {
    const element = document.createElement("div");
    element.className = "selected";
    const descriptor = Object.getOwnPropertyDescriptor(document, "fullscreenElement");
    Object.defineProperty(document, "fullscreenElement", { configurable: true, value: element });
    try {
      expect(element.matches(":fullscreen")).toBe(true);
      expect(element.matches("div.selected:fullscreen")).toBe(true);
      expect(element.matches("span:fullscreen")).toBe(false);
      expect(element.matches(":modal")).toBe(true);
      expect(element.matches(".selected:not(:fullscreen)")).toBe(false);
    } finally {
      if (descriptor) Object.defineProperty(document, "fullscreenElement", descriptor);
      else Reflect.deleteProperty(document, "fullscreenElement");
    }
  });

  it("preserves ordinary matching and invalid-selector errors", () => {
    const element = document.createElement("button");
    element.setAttribute("aria-label", "Default Agent");
    expect(element.matches('button[aria-label="Default Agent"]')).toBe(true);
    expect(element.matches("[hidden]")).toBe(false);
    expect(() => element.matches("[")).toThrow();
  });
});
