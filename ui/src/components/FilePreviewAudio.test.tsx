import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { FilePreviewFailure, FilePreviewPayload } from "../lib/filePreviewTypes";
import { FilePreviewAudio } from "./FilePreviewAudio";

const MEDIA_ERR_ABORTED = 1;
const MEDIA_ERR_NETWORK = 2;
const MEDIA_ERR_DECODE = 3;

function audioPayload(overrides: Partial<FilePreviewPayload> = {}): FilePreviewPayload {
  return {
    handle: "handle-1",
    displayName: "song.mp3",
    kind: "audio",
    byteLength: 4096,
    encoding: null,
    mediaType: "audio/mpeg",
    mediaUrl: "http://127.0.0.1:47531/media/handle-1",
    text: null,
    lineCount: null,
    target: null,
    resolvedPath: null,
    ...overrides,
  };
}

function setMediaError(element: HTMLMediaElement, code: number | null) {
  Object.defineProperty(element, "error", {
    configurable: true,
    value: code === null ? null : { code, message: "" },
  });
}

function audioElement(): HTMLAudioElement {
  return screen.getByTestId("file-preview-audio-element") as HTMLAudioElement;
}

beforeEach(() => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("FilePreviewAudio", () => {
  it("maps MediaError codes like the video renderer and ignores teardown aborts", () => {
    const onFailure = vi.fn();
    render(
      <FilePreviewAudio
        payload={audioPayload()}
        generation={1}
        onReload={vi.fn()}
        onExternalOpen={vi.fn()}
        onFailure={onFailure}
      />,
    );
    const audio = audioElement();

    setMediaError(audio, MEDIA_ERR_ABORTED);
    fireEvent.error(audio);
    setMediaError(audio, null);
    fireEvent.error(audio);
    expect(onFailure).not.toHaveBeenCalled();

    setMediaError(audio, MEDIA_ERR_NETWORK);
    fireEvent.error(audio);
    expect((onFailure.mock.calls[0][0] as FilePreviewFailure).reason).toBeNull();

    setMediaError(audio, MEDIA_ERR_DECODE);
    fireEvent.error(audio);
    expect((onFailure.mock.calls[1][0] as FilePreviewFailure).reason).toBe("UnsupportedFormat");
  });

  it("remounts the element on a new generation with the same URL and ignores the old element's errors", () => {
    const onFailure = vi.fn();
    const props = { payload: audioPayload(), onReload: vi.fn(), onExternalOpen: vi.fn(), onFailure };
    const { rerender } = render(<FilePreviewAudio {...props} generation={1} />);
    const first = audioElement();

    rerender(<FilePreviewAudio {...props} generation={2} />);
    const second = audioElement();
    expect(second).not.toBe(first);
    expect(first.isConnected).toBe(false);

    setMediaError(first, MEDIA_ERR_DECODE);
    fireEvent.error(first);
    expect(onFailure).not.toHaveBeenCalled();
  });
});
