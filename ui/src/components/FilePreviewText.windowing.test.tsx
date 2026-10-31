import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const clipboardMock = vi.hoisted(() => ({ copyTextToClipboard: vi.fn(async (_text: string) => true) }));
vi.mock("../lib/clipboard", () => clipboardMock);

import { FilePreviewText, SOURCE_WINDOW_THRESHOLD } from "./FilePreviewText";
import type { FilePreviewPayload, FilePreviewTextProps } from "../lib/filePreviewTypes";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function payloadOf(over: Partial<FilePreviewPayload> = {}): FilePreviewPayload {
  const text = over.text ?? "hello\n";
  return {
    handle: "h-1",
    displayName: "notes.txt",
    kind: "text",
    byteLength: text.length,
    encoding: "utf-8",
    mediaType: null,
    mediaUrl: null,
    text,
    lineCount: text.split("\n").length,
    target: null,
    resolvedPath: null,
    ...over,
  };
}

function propsOf(over: Partial<FilePreviewTextProps> = {}): FilePreviewTextProps {
  return {
    payload: payloadOf(),
    generation: 1,
    onFailure: vi.fn(),
    target: null,
    sourceMode: false,
    onSourceModeChange: vi.fn(),
    markdown: null,
    ...over,
  } as FilePreviewTextProps;
}

/** `line N` for N in 1..count, joined by newlines (no trailing newline). */
function numberedText(count: number, override: (line: number) => string | null = () => null): string {
  return Array.from({ length: count }, (_, i) => override(i + 1) ?? `line ${i + 1}`).join("\n");
}

function renderedLineNumbers(): number[] {
  return screen
    .getAllByTestId(/^file-preview-line-\d+$/)
    .map((element) => Number(element.getAttribute("data-testid")!.replace("file-preview-line-", "")));
}

describe("FilePreviewText windowed source view", () => {
  it("renders far fewer line rows than a 50,000-line document has", () => {
    render(<FilePreviewText {...propsOf({ payload: payloadOf({ text: numberedText(50_000) }) })} />);

    const source = screen.getByTestId("file-preview-source");
    expect(source).toHaveAttribute("data-windowed", "true");
    const mounted = renderedLineNumbers();
    expect(mounted.length).toBeGreaterThan(0);
    expect(mounted.length).toBeLessThan(200);
    expect(mounted[0]).toBe(1);
    expect(screen.getByTestId("file-preview-line-number-1")).toHaveTextContent("1");
    // The bottom spacer stands in for every unmounted line so the scroll height is the full document.
    const spacer = screen.getByTestId("file-preview-window-spacer-bottom");
    expect(parseFloat(spacer.style.height)).toBe((50_000 - mounted.length) * 20);
  });

  it("keeps every line mounted at or below the threshold", () => {
    render(
      <FilePreviewText {...propsOf({ payload: payloadOf({ text: numberedText(SOURCE_WINDOW_THRESHOLD) }) })} />,
    );
    expect(screen.getByTestId("file-preview-source")).not.toHaveAttribute("data-windowed");
    expect(renderedLineNumbers()).toHaveLength(SOURCE_WINDOW_THRESHOLD);
  });

  it("moves the rendered window when the source view scrolls", () => {
    render(<FilePreviewText {...propsOf({ payload: payloadOf({ text: numberedText(50_000) }) })} />);
    const source = screen.getByTestId("file-preview-source");
    expect(screen.queryByTestId("file-preview-line-25001")).toBeNull();

    source.scrollTop = 25_000 * 20;
    fireEvent.scroll(source);

    expect(screen.getByTestId("file-preview-line-25001")).toHaveTextContent("line 25001");
    expect(screen.queryByTestId("file-preview-line-1")).toBeNull();
    const mounted = renderedLineNumbers();
    expect(mounted.length).toBeLessThan(200);
    const top = screen.getByTestId("file-preview-window-spacer-top");
    expect(parseFloat(top.style.height)).toBe((mounted[0]! - 1) * 20);
  });

  it("mounts and scrolls to a target line deep in the file on open", async () => {
    render(
      <FilePreviewText
        {...propsOf({ payload: payloadOf({ text: numberedText(50_000) }), target: { line: 42_000, col: 3 } })}
      />,
    );

    const line = await screen.findByTestId("file-preview-line-42000");
    expect(line).toHaveAttribute("data-target", "true");
    expect(screen.getByTestId("file-preview-caret")).toHaveAttribute("data-offset", "2");
    expect(screen.getByTestId("file-preview-location")).toHaveTextContent("Ln 42000, Col 3");
    // Scrolled by arithmetic: the line sits inside the viewport window, not at the top.
    const source = screen.getByTestId("file-preview-source");
    expect(source.scrollTop).toBeGreaterThan(41_000 * 20);
    expect(source.scrollTop).toBeLessThanOrEqual(41_999 * 20);
    expect(screen.queryByTestId("file-preview-line-1")).toBeNull();
  });

  it("mounts and selects a search match far below the window on next", async () => {
    const text = numberedText(50_000, (n) => (n === 2 || n === 45_000 ? "a needle here" : null));
    render(<FilePreviewText {...propsOf({ payload: payloadOf({ text }) })} />);

    fireEvent.change(screen.getByTestId("file-preview-search-input"), { target: { value: "needle" } });
    expect(screen.getByTestId("file-preview-search-status")).toHaveTextContent("1 of 2");
    expect(window.getSelection()?.toString()).toBe("needle");
    expect(screen.queryByTestId("file-preview-line-45000")).toBeNull();

    fireEvent.click(screen.getByTestId("file-preview-search-next"));

    expect(screen.getByTestId("file-preview-search-status")).toHaveTextContent("2 of 2");
    const match = await screen.findByTestId("file-preview-match-1");
    expect(screen.getByTestId("file-preview-line-45000")).toContainElement(match);
    const selection = window.getSelection();
    expect(selection?.toString()).toBe("needle");
    expect(selection?.anchorNode ? match.contains(selection.anchorNode) : false).toBe(true);
  });

  it("copies the whole document, not the mounted lines", async () => {
    const text = numberedText(50_000);
    render(<FilePreviewText {...propsOf({ payload: payloadOf({ text }) })} />);
    fireEvent.click(screen.getByTestId("file-preview-copy"));
    await waitFor(() => expect(clipboardMock.copyTextToClipboard).toHaveBeenCalledWith(text));
  });

  it("mounts every line while printing and windows again afterwards", () => {
    render(<FilePreviewText {...propsOf({ payload: payloadOf({ text: numberedText(5_000) }) })} />);
    expect(renderedLineNumbers().length).toBeLessThan(200);

    act(() => {
      window.dispatchEvent(new Event("beforeprint"));
    });
    expect(screen.getByTestId("file-preview-source")).not.toHaveAttribute("data-windowed");
    expect(renderedLineNumbers()).toHaveLength(5_000);

    act(() => {
      window.dispatchEvent(new Event("afterprint"));
    });
    expect(renderedLineNumbers().length).toBeLessThan(200);
  });

  it("keeps syntax highlight on lines without a match while searching", () => {
    const text = numberedText(3_000, (n) => (n === 1 ? "export const answer = 42;" : `const v${n} = ${n};`));
    const { container } = render(
      <FilePreviewText {...propsOf({ payload: payloadOf({ text, displayName: "big.ts" }) })} />,
    );

    fireEvent.change(screen.getByTestId("file-preview-search-input"), { target: { value: "answer" } });

    expect(screen.getByTestId("file-preview-match-0")).toHaveTextContent("answer");
    // The matched line renders literal segments; its neighbours keep their highlighted markup.
    expect(screen.getByTestId("file-preview-line-1").querySelector(".hljs-line")).toBeNull();
    expect(screen.getByTestId("file-preview-line-2").querySelector(".hljs-line")).not.toBeNull();
    expect(container.querySelectorAll(".hljs-keyword").length).toBeGreaterThan(10);
  });
});
