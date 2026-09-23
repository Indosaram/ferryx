import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { FilePreviewImage } from "./FilePreviewImage";
import type { FilePreviewPayload } from "../lib/filePreviewTypes";

/**
 * Task 4 renderer tests.
 *
 * Every assertion is driven by real DOM events (`load` / `error` on the actual
 * `img`, pointer events on the viewport). No timers, no sleeps: the browser
 * signals the renderer consumes are exactly the signals these tests dispatch.
 */

afterEach(() => {
  cleanup();
});

function imagePayload(overrides: Partial<FilePreviewPayload> = {}): FilePreviewPayload {
  return {
    handle: "handle-a",
    displayName: "screenshot.png",
    kind: "image",
    byteLength: 24_576,
    encoding: null,
    mediaType: "image/png",
    mediaUrl: "http://127.0.0.1:52341/preview/handle-a",
    text: null,
    lineCount: null,
    target: null,
    ...overrides,
  };
}

type Handlers = {
  onReload: ReturnType<typeof vi.fn>;
  onExternalOpen: ReturnType<typeof vi.fn>;
  onFailure: ReturnType<typeof vi.fn>;
};

function handlers(): Handlers {
  return { onReload: vi.fn(), onExternalOpen: vi.fn(), onFailure: vi.fn() };
}

function renderImage(payload: FilePreviewPayload, generation = 1, spies: Handlers = handlers()) {
  const view = render(
    <FilePreviewImage
      payload={payload}
      generation={generation}
      onReload={spies.onReload}
      onExternalOpen={spies.onExternalOpen}
      onFailure={spies.onFailure}
    />,
  );
  return { ...view, spies };
}

/** jsdom never decodes; publish the intrinsic size the browser would report. */
function loadImage(img: HTMLImageElement, width: number, height: number) {
  Object.defineProperty(img, "naturalWidth", { configurable: true, value: width });
  Object.defineProperty(img, "naturalHeight", { configurable: true, value: height });
  fireEvent.load(img);
}

function viewport(): HTMLElement {
  return screen.getByTestId("file-preview-image-viewport");
}

function drag(target: HTMLElement, from: { x: number; y: number }, to: { x: number; y: number }) {
  fireEvent.pointerDown(target, { pointerId: 7, clientX: from.x, clientY: from.y, button: 0 });
  fireEvent.pointerMove(target, { pointerId: 7, clientX: to.x, clientY: to.y });
  fireEvent.pointerUp(target, { pointerId: 7, clientX: to.x, clientY: to.y });
}

describe("FilePreviewImage source admission", () => {
  it("renders the capability URL for every allowlisted raster type", () => {
    for (const mediaType of ["image/png", "image/jpeg", "image/gif", "image/webp", "image/bmp", "image/x-icon", "image/svg+xml"]) {
      const payload = imagePayload({ mediaType, mediaUrl: `http://127.0.0.1:52341/preview/${mediaType}` });
      const { unmount } = renderImage(payload);
      const img = screen.getByRole("img") as HTMLImageElement;
      expect(img.getAttribute("src")).toBe(payload.mediaUrl);
      expect(img).toHaveAttribute("alt", "screenshot.png");
      unmount();
    }
  });

  it("renders an SVG payload as an img and not a document", () => {
    const spies = handlers();
    renderImage(
      imagePayload({ displayName: "diagram.svg", mediaType: "image/svg+xml", mediaUrl: "http://127.0.0.1:52341/preview/svg" }),
      1,
      spies,
    );

    const img = screen.getByRole("img") as HTMLImageElement;
    expect(img.getAttribute("src")).toBe("http://127.0.0.1:52341/preview/svg");
    expect(document.querySelector("iframe, object, embed")).toBeNull();
    expect(spies.onFailure).not.toHaveBeenCalled();
  });

  it("never creates an img source when metadata is missing", () => {
    const spies = handlers();
    renderImage(imagePayload({ mediaUrl: null }), 1, spies);

    expect(document.querySelector("img")).toBeNull();
    expect(spies.onFailure).toHaveBeenCalledTimes(1);
    expect(spies.onFailure.mock.calls[0][0].reason).toBe("UnsupportedFormat");
  });

  it("reports unsupported metadata exactly once per generation", () => {
    const spies = handlers();
    const { rerender } = renderImage(imagePayload({ mediaType: "image/heic" }), 1, spies);
    rerender(
      <FilePreviewImage
        payload={imagePayload({ mediaType: "image/heic" })}
        generation={1}
        onReload={spies.onReload}
        onExternalOpen={spies.onExternalOpen}
        onFailure={spies.onFailure}
      />,
    );

    expect(spies.onFailure).toHaveBeenCalledTimes(1);
  });
});

describe("FilePreviewImage zoom and pan", () => {
  it("starts fitted and reports intrinsic dimensions after load", () => {
    renderImage(imagePayload());
    const img = screen.getByRole("img") as HTMLImageElement;
    loadImage(img, 1600, 900);

    expect(screen.getByTestId("file-preview-image-zoom")).toHaveTextContent("Fit");
    expect(screen.getByTestId("file-preview-image-dimensions")).toHaveTextContent("1600 × 900");
    expect(img.style.width).toBe("");
  });

  it("switches to 100% at the intrinsic pixel size", () => {
    renderImage(imagePayload());
    const img = screen.getByRole("img") as HTMLImageElement;
    loadImage(img, 800, 600);

    fireEvent.click(screen.getByRole("button", { name: "Actual size" }));

    expect(screen.getByTestId("file-preview-image-zoom")).toHaveTextContent("100%");
    expect(img.style.width).toBe("800px");
  });

  it("steps zoom by 10 percentage points and clamps at 10% and 800%", () => {
    renderImage(imagePayload());
    loadImage(screen.getByRole("img") as HTMLImageElement, 400, 400);

    fireEvent.click(screen.getByRole("button", { name: "Actual size" }));
    const zoomIn = screen.getByRole("button", { name: "Zoom in" });
    const zoomOut = screen.getByRole("button", { name: "Zoom out" });
    const readout = screen.getByTestId("file-preview-image-zoom");

    fireEvent.click(zoomIn);
    expect(readout).toHaveTextContent("110%");

    for (let i = 0; i < 100; i += 1) fireEvent.click(zoomIn);
    expect(readout).toHaveTextContent("800%");
    expect(zoomIn).toBeDisabled();

    for (let i = 0; i < 100; i += 1) fireEvent.click(zoomOut);
    expect(readout).toHaveTextContent("10%");
    expect(zoomOut).toBeDisabled();
  });

  it("pans only while zoomed and resets pan and zoom together", () => {
    renderImage(imagePayload());
    loadImage(screen.getByRole("img") as HTMLImageElement, 2000, 1500);

    drag(viewport(), { x: 100, y: 100 }, { x: 160, y: 130 });
    expect(screen.getByTestId("file-preview-image-canvas").style.transform).toBe("");

    fireEvent.click(screen.getByRole("button", { name: "Actual size" }));
    drag(viewport(), { x: 100, y: 100 }, { x: 160, y: 130 });
    expect(screen.getByTestId("file-preview-image-canvas").style.transform).toBe("translate(60px, 30px)");

    fireEvent.click(screen.getByRole("button", { name: "Reset view" }));
    expect(screen.getByTestId("file-preview-image-zoom")).toHaveTextContent("Fit");
    expect(screen.getByTestId("file-preview-image-canvas").style.transform).toBe("");
  });

  it("returns to fit and clears pan when a new generation replaces the image", () => {
    const spies = handlers();
    const { rerender } = renderImage(imagePayload(), 1, spies);
    loadImage(screen.getByRole("img") as HTMLImageElement, 2000, 1500);
    fireEvent.click(screen.getByRole("button", { name: "Actual size" }));
    drag(viewport(), { x: 0, y: 0 }, { x: 40, y: 40 });

    rerender(
      <FilePreviewImage
        payload={imagePayload({ handle: "handle-b", displayName: "b.png", mediaUrl: "http://127.0.0.1:52341/preview/handle-b" })}
        generation={2}
        onReload={spies.onReload}
        onExternalOpen={spies.onExternalOpen}
        onFailure={spies.onFailure}
      />,
    );

    expect(screen.getByTestId("file-preview-image-zoom")).toHaveTextContent("Fit");
    expect(screen.getByTestId("file-preview-image-canvas").style.transform).toBe("");
    expect((screen.getByRole("img") as HTMLImageElement).getAttribute("src")).toBe(
      "http://127.0.0.1:52341/preview/handle-b",
    );
  });
});

describe("FilePreviewImage failure handling", () => {
  it("shows a recoverable error with retry, reload and external actions", () => {
    const spies = handlers();
    renderImage(imagePayload({ displayName: "corrupt.png" }), 1, spies);
    const img = screen.getByRole("img") as HTMLImageElement;

    fireEvent.error(img);

    expect(screen.getByTestId("file-preview-image-error")).toBeInTheDocument();
    expect(spies.onFailure).toHaveBeenCalledTimes(1);
    expect(spies.onFailure.mock.calls[0][0]).toMatchObject({ reason: null, details: null });

    fireEvent.click(screen.getByRole("button", { name: "Open externally" }));
    expect(spies.onExternalOpen).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "Reload file" }));
    expect(spies.onReload).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(screen.queryByTestId("file-preview-image-error")).toBeNull();
    const retried = screen.getByRole("img") as HTMLImageElement;
    expect(retried.getAttribute("src")).toBe(imagePayload().mediaUrl);
    expect(retried).not.toBe(img);
  });

  it("reports one failure per failed load, not one per render", () => {
    const spies = handlers();
    const payload = imagePayload();
    const { rerender } = renderImage(payload, 1, spies);
    fireEvent.error(screen.getByRole("img") as HTMLImageElement);

    rerender(
      <FilePreviewImage
        payload={payload}
        generation={1}
        onReload={spies.onReload}
        onExternalOpen={spies.onExternalOpen}
        onFailure={spies.onFailure}
      />,
    );

    expect(spies.onFailure).toHaveBeenCalledTimes(1);
  });

  it("ignores a late error from an image the modal already replaced", () => {
    const spies = handlers();
    const { rerender } = renderImage(imagePayload(), 1, spies);
    const stale = screen.getByRole("img") as HTMLImageElement;

    rerender(
      <FilePreviewImage
        payload={imagePayload({ handle: "handle-b", displayName: "b.png", mediaUrl: "http://127.0.0.1:52341/preview/handle-b" })}
        generation={2}
        onReload={spies.onReload}
        onExternalOpen={spies.onExternalOpen}
        onFailure={spies.onFailure}
      />,
    );

    fireEvent.error(stale);

    expect(screen.queryByTestId("file-preview-image-error")).toBeNull();
    expect(spies.onFailure).not.toHaveBeenCalled();
    expect(screen.getByRole("img")).toBeInTheDocument();
  });

  it("ignores a late load from an image the modal already replaced", () => {
    const spies = handlers();
    const { rerender } = renderImage(imagePayload(), 1, spies);
    const stale = screen.getByRole("img") as HTMLImageElement;

    rerender(
      <FilePreviewImage
        payload={imagePayload({ handle: "handle-b", displayName: "b.png", mediaUrl: "http://127.0.0.1:52341/preview/handle-b" })}
        generation={2}
        onReload={spies.onReload}
        onExternalOpen={spies.onExternalOpen}
        onFailure={spies.onFailure}
      />,
    );

    loadImage(stale, 4000, 3000);

    expect(screen.queryByTestId("file-preview-image-dimensions")).toBeNull();
  });
});
