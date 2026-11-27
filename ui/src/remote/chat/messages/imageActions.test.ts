/**
 * Saving and copying a transcript image (t3code activity-log.md "Open an image").
 *
 * The lightbox hands these two actions to its owner. Neither may throw at a caller that cannot
 * honour it: a refused save or a browser without the async clipboard is `false`, so the lightbox
 * stays usable instead of failing on the press.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { copyImage, imageActionHandlers, imageFileName, saveImage } from "./imageActions";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("image file names", () => {
  it("keeps a name that already carries an extension", () => {
    expect(imageFileName("blob:https://host/1234", "screenshot.png")).toBe("screenshot.png");
  });

  it("adds .png to a bare name and falls back to the source's last segment", () => {
    expect(imageFileName("blob:x", "screenshot")).toBe("screenshot.png");
    expect(imageFileName("https://host/a/b/shot.webp?token=1")).toBe("shot.webp");
    expect(imageFileName("blob:https://host/9f8e")).toBe("image.png");
  });
});

describe("saving an image", () => {
  it("downloads the bytes the lightbox shows under the name it was given", () => {
    const clicked: HTMLAnchorElement[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      clicked.push(this);
    });
    expect(saveImage("blob:https://host/9f8e", "diagram")).toBe(true);
    expect(clicked).toHaveLength(1);
    expect(clicked[0].download).toBe("diagram.png");
    expect(clicked[0].href).toContain("blob:https://host/9f8e");
    // The anchor is removed again: a detached download must not linger in the document.
    expect(document.querySelector("a[download]")).toBeNull();
  });
});

describe("copying an image", () => {
  it("puts the fetched bytes on the clipboard as a PNG", async () => {
    const written: unknown[] = [];
    vi.stubGlobal("navigator", {
      clipboard: { write: async (items: unknown[]) => void written.push(...items) },
    });
    vi.stubGlobal("ClipboardItem", class {
      readonly items: Record<string, Blob>;
      constructor(items: Record<string, Blob>) {
        this.items = items;
      }
    });
    // A minimal response: jsdom's `Blob` is not the one the platform `Response` accepts as a
    // body, so the stub answers with the two fields `copyImage` reads.
    const png = new Blob([new Uint8Array([1, 2, 3])], { type: "image/png" });
    vi.stubGlobal("fetch", vi.fn(async () => ({ ok: true, blob: async () => png }) as unknown as Response));

    expect(await copyImage("https://host/shot.png")).toBe(true);
    expect(written).toHaveLength(1);
    expect(Object.keys((written[0] as { items: Record<string, Blob> }).items)).toEqual(["image/png"]);
  });

  it("answers false for a source that will not fetch, instead of throwing", async () => {
    vi.stubGlobal("navigator", { clipboard: { write: async () => {} } });
    vi.stubGlobal("ClipboardItem", class {});
    vi.stubGlobal("fetch", vi.fn(async () => new Response("gone", { status: 404 })));
    expect(await copyImage("https://host/expired.png")).toBe(false);
  });

  it("answers false where the browser has no async clipboard", async () => {
    vi.stubGlobal("navigator", {});
    expect(await copyImage("https://host/shot.png")).toBe(false);
  });

  it("hands the lightbox a pair whose signatures match its props", () => {
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    const handlers = imageActionHandlers();
    // Neither returns a promise: the lightbox treats them as fire-and-forget.
    expect(handlers.onSaveImage("blob:x", "name")).toBeUndefined();
    expect(handlers.onCopyImage("blob:x")).toBeUndefined();
  });
});
