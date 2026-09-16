import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from "vitest";

import { copyTextToClipboard } from "./clipboard";

describe("copyTextToClipboard", () => {
  let execCommandSpy: MockInstance;

  beforeEach(() => {
    if (!("execCommand" in document)) {
      Object.defineProperty(document, "execCommand", {
        value: () => false,
        writable: true,
        configurable: true,
      });
    }
    execCommandSpy = vi.spyOn(document, "execCommand");
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    // Clean up any stray textareas if a test failed mid-execution
    document.querySelectorAll("textarea").forEach((el) => el.remove());
  });

  it("resolves via navigator.clipboard.writeText and never calls execCommand without leaving textarea in DOM", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", {
      clipboard: { writeText },
    });

    const result = await copyTextToClipboard("sample text");

    expect(result).toBe(true);
    expect(writeText).toHaveBeenCalledTimes(1);
    expect(writeText).toHaveBeenCalledWith("sample text");
    expect(execCommandSpy).not.toHaveBeenCalled();
    expect(document.querySelector("textarea")).toBeNull();
  });

  it("falls back to document.execCommand('copy') when writeText rejects and removes textarea from DOM", async () => {
    const writeText = vi.fn().mockRejectedValue(new Error("Document focus error"));
    vi.stubGlobal("navigator", {
      clipboard: { writeText },
    });

    let inspectedInsideExecCommand = false;
    execCommandSpy.mockImplementation((command: string) => {
      if (command === "copy") {
        const ta = document.querySelector("textarea") as HTMLTextAreaElement | null;
        expect(ta).not.toBeNull();
        expect(ta?.value).toBe("fallback text");
        expect(ta?.hasAttribute("readonly")).toBe(true);
        expect(ta?.style.position).toBe("fixed");
        expect(ta?.style.opacity).toBe("0");
        inspectedInsideExecCommand = true;
        return true;
      }
      return false;
    });

    const result = await copyTextToClipboard("fallback text");

    expect(inspectedInsideExecCommand).toBe(true);
    expect(result).toBe(true);
    expect(writeText).toHaveBeenCalledTimes(1);
    expect(execCommandSpy).toHaveBeenCalledWith("copy");
    expect(document.querySelector("textarea")).toBeNull();
  });

  it("returns execCommand's result (false) when writeText rejects and execCommand fails", async () => {
    const writeText = vi.fn().mockRejectedValue(new Error("Activation error"));
    vi.stubGlobal("navigator", {
      clipboard: { writeText },
    });
    execCommandSpy.mockReturnValue(false);

    const result = await copyTextToClipboard("reject fallback false");

    expect(result).toBe(false);
    expect(writeText).toHaveBeenCalledTimes(1);
    expect(execCommandSpy).toHaveBeenCalledWith("copy");
    expect(document.querySelector("textarea")).toBeNull();
  });

  it("returns false when navigator.clipboard is undefined and execCommand returns false", async () => {
    vi.stubGlobal("navigator", {});
    execCommandSpy.mockReturnValue(false);

    const result = await copyTextToClipboard("legacy text");

    expect(result).toBe(false);
    expect(execCommandSpy).toHaveBeenCalledWith("copy");
    expect(document.querySelector("textarea")).toBeNull();
  });

  it("returns false without escaping exception when document.execCommand throws", async () => {
    vi.stubGlobal("navigator", {});
    execCommandSpy.mockImplementation(() => {
      throw new Error("execCommand denied");
    });

    const result = await copyTextToClipboard("throwing text");

    expect(result).toBe(false);
    expect(document.querySelector("textarea")).toBeNull();
  });
});
