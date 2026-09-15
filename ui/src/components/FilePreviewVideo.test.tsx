import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { FilePreviewFailure, FilePreviewPayload } from "../lib/filePreviewTypes";
import { FilePreviewVideo } from "./FilePreviewVideo";

const MEDIA_ERR_ABORTED = 1;
const MEDIA_ERR_NETWORK = 2;
const MEDIA_ERR_DECODE = 3;
const MEDIA_ERR_SRC_NOT_SUPPORTED = 4;

function videoPayload(overrides: Partial<FilePreviewPayload> = {}): FilePreviewPayload {
  return {
    handle: "handle-1",
    displayName: "clip.webm",
    kind: "video",
    byteLength: 262_144,
    encoding: null,
    mediaType: "video/webm",
    mediaUrl: "http://127.0.0.1:47531/media/handle-1",
    text: null,
    lineCount: null,
    target: null,
    ...overrides,
  };
}

/** jsdom has no media engine: `load`/`play`/`pause` are unimplemented stubs. */
function stubMediaEngine() {
  return {
    pause: vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {}),
    load: vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {}),
    play: vi
      .spyOn(HTMLMediaElement.prototype, "play")
      .mockImplementation(() => Promise.resolve()),
  };
}

function setMediaError(element: HTMLMediaElement, code: number | null) {
  Object.defineProperty(element, "error", {
    configurable: true,
    value: code === null ? null : { code, message: "" },
  });
}

function videoElement(): HTMLVideoElement {
  return screen.getByTestId("file-preview-video-element") as HTMLVideoElement;
}

let media: ReturnType<typeof stubMediaEngine>;

beforeEach(() => {
  media = stubMediaEngine();
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("FilePreviewVideo", () => {
  it("renders a native video element with metadata preload and never autoplays", () => {
    render(
      <FilePreviewVideo
        payload={videoPayload()}
        generation={1}
        onReload={vi.fn()}
        onExternalOpen={vi.fn()}
        onFailure={vi.fn()}
      />,
    );

    const video = videoElement();
    expect(video.tagName).toBe("VIDEO");
    expect(video.getAttribute("src")).toBe("http://127.0.0.1:47531/media/handle-1");
    expect(video).toHaveAttribute("controls");
    expect(video.getAttribute("preload")).toBe("metadata");
    expect(video.hasAttribute("autoplay")).toBe(false);
    expect(video.hasAttribute("loop")).toBe(false);
    expect(video.autoplay).toBe(false);
    expect(media.play).not.toHaveBeenCalled();
  });

  it("exposes reload and external-open callbacks", () => {
    const onReload = vi.fn();
    const onExternalOpen = vi.fn();
    render(
      <FilePreviewVideo
        payload={videoPayload()}
        generation={1}
        onReload={onReload}
        onExternalOpen={onExternalOpen}
        onFailure={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /reload/i }));
    fireEvent.click(screen.getByRole("button", { name: /open externally/i }));

    expect(onReload).toHaveBeenCalledTimes(1);
    expect(onExternalOpen).toHaveBeenCalledTimes(1);
  });

  it("surfaces a decode failure as a recoverable UnsupportedFormat error", () => {
    const onFailure = vi.fn();
    const onReload = vi.fn();
    render(
      <FilePreviewVideo
        payload={videoPayload()}
        generation={1}
        onReload={onReload}
        onExternalOpen={vi.fn()}
        onFailure={onFailure}
      />,
    );

    const video = videoElement();
    setMediaError(video, MEDIA_ERR_DECODE);
    fireEvent.error(video);

    expect(onFailure).toHaveBeenCalledTimes(1);
    const failure = onFailure.mock.calls[0][0] as FilePreviewFailure;
    expect(failure.reason).toBe("UnsupportedFormat");
    expect(failure.details?.reason).toBe("UnsupportedFormat");

    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(/clip\.webm/);
    // Recoverable: the element stays mounted and retry is offered in place.
    expect(videoElement()).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /try again/i }));
    expect(onReload).toHaveBeenCalledTimes(1);
  });

  it("reports an unsupported source as UnsupportedFormat and a network error without inventing a reason", () => {
    const onFailure = vi.fn();
    const { rerender } = render(
      <FilePreviewVideo
        payload={videoPayload()}
        generation={1}
        onReload={vi.fn()}
        onExternalOpen={vi.fn()}
        onFailure={onFailure}
      />,
    );

    setMediaError(videoElement(), MEDIA_ERR_SRC_NOT_SUPPORTED);
    fireEvent.error(videoElement());
    expect((onFailure.mock.calls[0][0] as FilePreviewFailure).reason).toBe("UnsupportedFormat");

    rerender(
      <FilePreviewVideo
        payload={videoPayload()}
        generation={2}
        onReload={vi.fn()}
        onExternalOpen={vi.fn()}
        onFailure={onFailure}
      />,
    );
    setMediaError(videoElement(), MEDIA_ERR_NETWORK);
    fireEvent.error(videoElement());

    const networkFailure = onFailure.mock.calls[1][0] as FilePreviewFailure;
    expect(networkFailure.reason).toBeNull();
    expect(networkFailure.message).toMatch(/clip\.webm/);
  });

  it("ignores aborted and reason-less media errors produced by teardown", () => {
    const onFailure = vi.fn();
    render(
      <FilePreviewVideo
        payload={videoPayload()}
        generation={1}
        onReload={vi.fn()}
        onExternalOpen={vi.fn()}
        onFailure={onFailure}
      />,
    );

    const video = videoElement();
    setMediaError(video, MEDIA_ERR_ABORTED);
    fireEvent.error(video);
    setMediaError(video, null);
    fireEvent.error(video);

    expect(onFailure).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("pauses, detaches the source and reloads the element when the preview closes", () => {
    const { unmount } = render(
      <FilePreviewVideo
        payload={videoPayload()}
        generation={1}
        onReload={vi.fn()}
        onExternalOpen={vi.fn()}
        onFailure={vi.fn()}
      />,
    );

    const video = videoElement();
    unmount();

    expect(media.pause).toHaveBeenCalledTimes(1);
    expect(video.hasAttribute("src")).toBe(false);
    expect(media.load).toHaveBeenCalledTimes(1);
  });

  it("tears down the replaced element, mounts a fresh one and drops the stale failure", () => {
    const onFailure = vi.fn();
    const first = videoPayload();
    const props = {
      generation: 1,
      onReload: vi.fn(),
      onExternalOpen: vi.fn(),
      onFailure,
    };
    const { rerender } = render(<FilePreviewVideo payload={first} {...props} />);

    const firstVideo = videoElement();
    setMediaError(firstVideo, MEDIA_ERR_DECODE);
    fireEvent.error(firstVideo);
    expect(screen.getByRole("alert")).toBeInTheDocument();

    const second = videoPayload({
      handle: "handle-2",
      displayName: "second.webm",
      mediaUrl: "http://127.0.0.1:47531/media/handle-2",
    });
    rerender(<FilePreviewVideo payload={second} {...props} generation={2} />);

    const secondVideo = videoElement();
    expect(secondVideo).not.toBe(firstVideo);
    expect(secondVideo.getAttribute("src")).toBe("http://127.0.0.1:47531/media/handle-2");
    expect(firstVideo.hasAttribute("src")).toBe(false);
    expect(firstVideo.isConnected).toBe(false);
    expect(media.pause).toHaveBeenCalledTimes(1);
    expect(media.load).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(onFailure).toHaveBeenCalledTimes(1);
  });

  it("ignores an error event that arrives on a detached element after replacement", () => {
    const onFailure = vi.fn();
    const props = {
      onReload: vi.fn(),
      onExternalOpen: vi.fn(),
      onFailure,
    };
    const { rerender } = render(
      <FilePreviewVideo payload={videoPayload()} generation={1} {...props} />,
    );
    const firstVideo = videoElement();

    rerender(
      <FilePreviewVideo
        payload={videoPayload({ handle: "handle-2", mediaUrl: "http://127.0.0.1:47531/media/handle-2" })}
        generation={2}
        {...props}
      />,
    );

    setMediaError(firstVideo, MEDIA_ERR_DECODE);
    fireEvent.error(firstVideo);

    expect(onFailure).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("reports a payload with no media URL instead of rendering an empty player", () => {
    const onFailure = vi.fn();
    render(
      <FilePreviewVideo
        payload={videoPayload({ mediaUrl: null, mediaType: null })}
        generation={1}
        onReload={vi.fn()}
        onExternalOpen={vi.fn()}
        onFailure={onFailure}
      />,
    );

    expect(screen.queryByTestId("file-preview-video-element")).toBeNull();
    expect(onFailure).toHaveBeenCalledTimes(1);
    expect((onFailure.mock.calls[0][0] as FilePreviewFailure).reason).toBe("UnsupportedFormat");
    expect(screen.getByRole("alert")).toBeInTheDocument();
  });
});
