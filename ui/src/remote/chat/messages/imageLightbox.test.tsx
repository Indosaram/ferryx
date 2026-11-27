import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ExpandedImageDialog, ImageActionsProvider, ZoomableImage } from "./imageLightbox";
import { AttachmentList } from "../MobileChatComponents";

afterEach(cleanup);

const DATA_SRC = "data:image/png;base64,AAAA";

describe("image lightbox", () => {
  it("opens the dialog from a thumbnail, zooms, and closes on Escape", () => {
    render(<ZoomableImage src={DATA_SRC} alt="shot" thumbnailTestId="chat-image" />);
    fireEvent.click(screen.getByTestId("chat-image"));
    expect(screen.getByTestId("expanded-image-dialog")).toBeInTheDocument();
    expect(screen.getByTestId("image-zoom-level")).toHaveTextContent("100%");
    fireEvent.click(screen.getByTestId("image-zoom-in"));
    expect(screen.getByTestId("image-zoom-level")).toHaveTextContent("150%");
    fireEvent.click(screen.getByTestId("image-zoom-out"));
    fireEvent.click(screen.getByTestId("image-zoom-out"));
    expect(screen.getByTestId("image-zoom-level")).toHaveTextContent("50%");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("expanded-image-dialog")).not.toBeInTheDocument();
  });

  it("offers save and copy from a right-click on the thumbnail", () => {
    const onSaveImage = vi.fn();
    const onCopyImage = vi.fn();
    render(
      <ImageActionsProvider actions={{ onSaveImage, onCopyImage }}>
        <ZoomableImage src={DATA_SRC} alt="shot" thumbnailTestId="chat-image" />
      </ImageActionsProvider>,
    );
    // The testid names the picture; the button around it owns the long-press path.
    fireEvent.contextMenu(screen.getByTestId("chat-image"));
    expect(screen.getByTestId("image-action-menu")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("image-save"));
    expect(onSaveImage).toHaveBeenCalledWith(DATA_SRC, "shot");
    fireEvent.contextMenu(screen.getByTestId("chat-image"));
    fireEvent.click(screen.getByTestId("image-copy"));
    expect(onCopyImage).toHaveBeenCalledWith(DATA_SRC);
    expect(screen.queryByTestId("image-action-menu")).not.toBeInTheDocument();
  });

  it("falls back to a download anchor when no save handler is wired", () => {
    render(<ExpandedImageDialog src={DATA_SRC} alt="shot" onClose={() => {}} />);
    fireEvent.contextMenu(screen.getByTestId("expanded-image-dialog"));
    const save = screen.getByTestId("image-save");
    expect(save).toBeInTheDocument();
  });

  it("opens the thumbnail menu above the clipping thumbnail box, not inside it", () => {
    // The attachment shelf clips its box, so an absolutely positioned menu inside it is
    // cut off: the menu has to leave the flow.
    render(
      <AttachmentList
        attachments={[{ id: "1", name: "shot.png", type: "image", url: DATA_SRC }]}
      />,
    );
    fireEvent.contextMenu(screen.getByTestId("message-image-thumbnail"));
    const menu = screen.getByTestId("image-action-menu");
    // Positioned against the viewport at the thumbnail, so the clipping shelf cannot cut it.
    expect(menu.style.top).not.toBe("");
    expect(menu.style.left).not.toBe("");
    expect(menu.className).toContain("fixed");
  });

  it("closes the thumbnail menu with Escape, before any dialog is open", () => {
    render(<ZoomableImage src={DATA_SRC} alt="shot" thumbnailTestId="chat-image" />);
    fireEvent.contextMenu(screen.getByTestId("chat-image"));
    expect(screen.getByTestId("image-action-menu")).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("image-action-menu")).not.toBeInTheDocument();
    expect(screen.queryByTestId("expanded-image-dialog")).not.toBeInTheDocument();
  });

  it("opens only the menu on a long press, not the full-screen dialog", () => {
    vi.useFakeTimers();
    try {
      render(<ZoomableImage src={DATA_SRC} alt="shot" thumbnailTestId="chat-image" />);
      const thumbnail = screen.getByTestId("chat-image");
      fireEvent.touchStart(thumbnail);
      act(() => {
        vi.advanceTimersByTime(600);
      });
      expect(screen.getByTestId("image-action-menu")).toBeInTheDocument();
      // The click a lifted finger produces belongs to the long press that already fired.
      fireEvent.click(thumbnail);
      expect(screen.queryByTestId("expanded-image-dialog")).not.toBeInTheDocument();
      expect(screen.getByTestId("image-action-menu")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("still opens the dialog on a plain tap after the menu is closed", () => {
    render(<ZoomableImage src={DATA_SRC} alt="shot" thumbnailTestId="chat-image" />);
    const thumbnail = screen.getByTestId("chat-image");
    fireEvent.contextMenu(thumbnail);
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(thumbnail);
    expect(screen.getByTestId("expanded-image-dialog")).toBeInTheDocument();
  });
});
